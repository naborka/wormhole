#!/bin/sh
# Alphaca Grok preflight. Runs in the box before the agent. Grok is the
# one thing that must be here at the end; everything else warns and moves on.
set -eu

bin="$HOME/.local/bin"
mkdir -p "$bin"

# The tag behind GitHub's latest-release redirect, without the API's
# rate limit. Empty when it cannot be read.
latest_tag() {
    curl -fsSI "https://github.com/$1/releases/latest" 2>/dev/null \
        | sed -n 's|^[Ll]ocation: .*/tag/\([^[:space:]]*\).*|\1|p' | tr -d '\r'
}

# Grok, latest stable, in the kept home. The channel pointer names the
# version in one small request, so a box that is already current downloads
# nothing. The installer links ~/.local/bin/grok at the binary it keeps in
# ~/.grok/downloads, and ~/.local/bin is first on the box's PATH.
latest=$(curl -fsSL https://x.ai/cli/stable 2>/dev/null | tr -d '[:space:]')
have=$(grok --version 2>/dev/null | sed -n 's/^grok \([^ ]*\).*/\1/p')
if [ -z "$latest" ] && [ -z "$have" ]; then
    echo "[preflight] ERROR: cannot read grok's latest version and none is installed" >&2
    exit 1
elif [ -z "$latest" ]; then
    echo "[preflight] WARN: cannot read grok's latest version; keeping $have" >&2
elif [ "$latest" != "$have" ]; then
    # A `curl | bash` whose curl failed feeds bash an empty script and
    # exits 0, so the binary itself is what says the install happened.
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

# Skills -> grok's skills directory in the kept home, via the `skills`
# installer; without -g they land in the workspace. Each on its own so
# one bad source does not take the rest down.
for repo in \
    JuliusBrussee/caveman \
    mattpocock/skills \
    leonardomso/rust-skills
do
    if npx -y skills add "$repo" --skill '*' -a grok -g --yes </dev/null; then
        echo "[preflight] installed skills from $repo"
    else
        echo "[preflight] WARN: skills add $repo failed" >&2
    fi
done

# RTK as a rules file: grok has no rewrite hook, so the model follows the
# rules instead. rtk ships no grok flavour; the codex one is the same text
# with no hook patching, and CODEX_HOME aims it at the directory grok
# reads every `*.md` from. Telemetry disable first records consent so
# init skips that prompt.
if command -v rtk >/dev/null 2>&1; then
    rtk telemetry disable >/dev/null 2>&1 </dev/null || true
    if CODEX_HOME="$HOME/.grok/rules" rtk init -g --codex </dev/null; then
        echo "[preflight] rtk init done"
    else
        echo "[preflight] WARN: rtk init failed" >&2
    fi
else
    echo "[preflight] WARN: rtk binary not on PATH" >&2
fi

# context7 as an MCP server in ~/.grok/config.toml. Added once: grok
# merges that file, so the entry survives restarts.
if grok mcp list 2>/dev/null | grep -q '^ *context7:'; then
    :
elif grok mcp add context7 \
    ${CONTEXT7_API_KEY:+-e CONTEXT7_API_KEY="$CONTEXT7_API_KEY"} \
    -- npx -y @upstash/context7-mcp </dev/null; then
    echo "[preflight] context7 MCP added"
else
    echo "[preflight] WARN: context7 MCP add failed" >&2
fi
