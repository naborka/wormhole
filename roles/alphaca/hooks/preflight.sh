#!/bin/sh
# Alphaca preflight. Runs in the box before the agent. The product
# WORMHOLE_RUN names is the one thing that must be here at the end;
# everything else warns and moves on.
set -eu

run="${WORMHOLE_RUN:?wormhole: WORMHOLE_RUN is unset; the start did not bind a product}"
bin="$HOME/.local/bin"
mkdir -p "$bin"

# The tag behind GitHub's latest-release redirect, without the API's
# rate limit. Empty when it cannot be read.
latest_tag() {
    curl -fsSI "https://github.com/$1/releases/latest" 2>/dev/null \
        | sed -n 's|^[Ll]ocation: .*/tag/\([^[:space:]]*\).*|\1|p' | tr -d '\r'
}

install_claude() {
    version() { claude --version 2>/dev/null; }
    if command -v claude >/dev/null 2>&1; then
        if claude update </dev/null; then
            echo "[preflight] claude: $(version)"
        else
            echo "[preflight] WARN: claude update failed; keeping $(version)" >&2
        fi
    elif curl -fsSL https://claude.ai/install.sh | bash \
        && command -v claude >/dev/null 2>&1; then
        echo "[preflight] installed claude $(version)"
    else
        echo "[preflight] ERROR: claude install failed; nothing to run" >&2
        exit 1
    fi
}

install_codex() {
    tag=$(latest_tag openai/codex)
    have=$(codex --version 2>/dev/null | sed -n 's/.* \([0-9][0-9.]*\).*/\1/p')
    if [ -z "$tag" ] && [ -z "$have" ]; then
        echo "[preflight] ERROR: cannot read codex's latest release and none is installed" >&2
        exit 1
    elif [ -z "$tag" ]; then
        echo "[preflight] WARN: cannot read codex's latest release; keeping $have" >&2
    elif [ "${tag#rust-v}" != "$have" ]; then
        target="$(uname -m)-unknown-linux-musl"
        url="https://github.com/openai/codex/releases/download/$tag/codex-$target.tar.gz"
        if curl -fsSL "$url" | tar -xz -C "$bin" "codex-$target" \
            && mv "$bin/codex-$target" "$bin/codex"; then
            echo "[preflight] codex $tag"
        elif [ -n "$have" ]; then
            echo "[preflight] WARN: codex $tag download failed; keeping $have" >&2
        else
            echo "[preflight] ERROR: codex $tag download failed; nothing to run" >&2
            exit 1
        fi
    fi
}

install_grok() {
    latest=$(curl -fsSL https://x.ai/cli/stable 2>/dev/null | tr -d '[:space:]')
    have=$(grok --version 2>/dev/null | sed -n 's/^grok \([^ ]*\).*/\1/p')
    if [ -z "$latest" ] && [ -z "$have" ]; then
        echo "[preflight] ERROR: cannot read grok's latest version and none is installed" >&2
        exit 1
    elif [ -z "$latest" ]; then
        echo "[preflight] WARN: cannot read grok's latest version; keeping $have" >&2
    elif [ "$latest" != "$have" ]; then
        if curl -fsSL https://x.ai/cli/install.sh | GROK_BIN_DIR="$bin" bash -s "$latest" \
            && grok --version >/dev/null 2>&1; then
            echo "[preflight] grok $latest"
        elif [ -n "$have" ]; then
            echo "[preflight] WARN: grok $latest install failed; keeping $have" >&2
        else
            echo "[preflight] ERROR: grok $latest install failed; nothing to run" >&2
            exit 1
        fi
    fi
}

# The `skills` installer has its own name for each product.
case "$run" in
    claude) install_claude; skills_agent=claude-code ;;
    codex) install_codex; skills_agent=codex ;;
    grok) install_grok; skills_agent=grok ;;
    *)
        echo "[preflight] ERROR: unknown product $run" >&2
        exit 1
        ;;
esac

# rtk, latest release, same place.
tag=$(latest_tag rtk-ai/rtk)
have=$(rtk --version 2>/dev/null | sed -n 's/.* \([0-9][0-9.]*\).*/\1/p')
if [ -z "$tag" ]; then
    echo "[preflight] WARN: cannot read rtk's latest release; keeping ${have:-none}" >&2
elif [ "${tag#v}" != "$have" ]; then
    url="https://github.com/rtk-ai/rtk/releases/download/$tag/rtk-$(uname -m)-unknown-linux-musl.tar.gz"
    if curl -fsSL "$url" | tar -xz -C "$bin" rtk; then
        echo "[preflight] rtk $tag"
    else
        echo "[preflight] WARN: rtk $tag download failed" >&2
    fi
fi

# Skills via the installer, into this product's directory in the kept
# home; without -g they land in the workspace. Each repo on its own so
# one bad source does not take the rest down.
for repo in \
    JuliusBrussee/caveman \
    mattpocock/skills \
    leonardomso/rust-skills
do
    if npx -y skills add "$repo" --skill '*' -a "$skills_agent" -g --yes </dev/null; then
        echo "[preflight] installed skills from $repo ($run)"
    else
        echo "[preflight] WARN: skills add $repo failed" >&2
    fi
done

# RTK flavour: claude gets a PreToolUse rewrite; the others get a rules
# file. grok has no rewrite hook, so CODEX_HOME aims the same text at
# the directory it reads every *.md from.
if command -v rtk >/dev/null 2>&1; then
    rtk telemetry disable >/dev/null 2>&1 </dev/null || true
    rtk_ok=0
    case "$run" in
        claude)
            rtk init -g --auto-patch </dev/null && rtk_ok=1
            ;;
        codex)
            rtk init -g --codex </dev/null && rtk_ok=1
            ;;
        grok)
            CODEX_HOME="$HOME/.grok/rules" rtk init -g --codex </dev/null && rtk_ok=1
            ;;
    esac
    if [ "$rtk_ok" -eq 1 ]; then
        echo "[preflight] rtk init done"
    else
        echo "[preflight] WARN: rtk init failed" >&2
    fi
else
    echo "[preflight] WARN: rtk binary not on PATH" >&2
fi

# context7 as an MCP server in this product's own config. Added once:
# each CLI merges, never overwrites.
case "$run" in
    claude)
        if claude mcp list 2>/dev/null | grep -q context7; then
            :
        elif claude mcp add context7 \
            ${CONTEXT7_API_KEY:+--env CONTEXT7_API_KEY="$CONTEXT7_API_KEY"} \
            -- npx -y @upstash/context7-mcp </dev/null; then
            echo "[preflight] context7 MCP added"
        else
            echo "[preflight] WARN: context7 MCP add failed" >&2
        fi
        ;;
    codex)
        if codex mcp get context7 >/dev/null 2>&1; then
            :
        elif codex mcp add context7 \
            ${CONTEXT7_API_KEY:+--env CONTEXT7_API_KEY="$CONTEXT7_API_KEY"} \
            -- npx -y @upstash/context7-mcp </dev/null; then
            echo "[preflight] context7 MCP added"
        else
            echo "[preflight] WARN: context7 MCP add failed" >&2
        fi
        ;;
    grok)
        if grok mcp list 2>/dev/null | grep -q '^ *context7:'; then
            :
        elif grok mcp add context7 \
            ${CONTEXT7_API_KEY:+-e CONTEXT7_API_KEY="$CONTEXT7_API_KEY"} \
            -- npx -y @upstash/context7-mcp </dev/null; then
            echo "[preflight] context7 MCP added"
        else
            echo "[preflight] WARN: context7 MCP add failed" >&2
        fi
        ;;
esac

# Claude plugins have no grok or codex analogue. Skip them there and
# say so; rust-analyzer-lsp is the one that is not also a skill repo.
case "$run" in
    claude)
        for marketplace in anthropics/claude-plugins-official; do
            claude plugin marketplace add "$marketplace" </dev/null \
                || echo "[preflight] WARN: marketplace $marketplace failed" >&2
        done
        if claude plugin install rust-analyzer-lsp@claude-plugins-official </dev/null; then
            echo "[preflight] rust-analyzer-lsp plugin installed"
        else
            echo "[preflight] WARN: rust-analyzer-lsp plugin failed" >&2
        fi
        ;;
    *)
        echo "[preflight] skipping Claude plugins; $run has no analogue"
        ;;
esac
