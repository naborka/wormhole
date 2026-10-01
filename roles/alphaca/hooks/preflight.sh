#!/bin/sh
# Alphaca preflight. Runs in the box before the agent. The product
# WORMHOLE_RUN names is the one thing that must be here at the end;
# everything else warns and moves on.
set -eu

run="${WORMHOLE_RUN:?wormhole: WORMHOLE_RUN is unset; the start did not bind a product}"
bin="$HOME/.local/bin"
musl="$(uname -m)-unknown-linux-musl"
mkdir -p "$bin"

# The tag behind GitHub's latest-release redirect, without the API's
# rate limit. Empty when it cannot be read.
latest_tag() {
    curl -fsSI --connect-timeout 10 "https://github.com/$1/releases/latest" 2>/dev/null \
        | sed -n 's|^[Ll]ocation: .*/tag/\([^[:space:]]*\).*|\1|p' | tr -d '\r'
}

# What a tool reports, empty when it is not installed.
installed_version() {
    "$1" --version 2>/dev/null | sed -n 's/.* \([0-9][0-9.]*\).*/\1/p'
}

# A release tarball from repo $1 at tag $2 into $4: all of it, or only
# the members named after it.
fetch_release() {
    url="https://github.com/$1/releases/download/$2/$3.tar.gz"
    dir=$4
    shift 4
    curl -fsSL --connect-timeout 10 "$url" | tar -xz -C "$dir" "$@"
}

install_claude() {
    version() { claude --version 2>/dev/null; }
    if command -v claude >/dev/null 2>&1; then
        if claude update </dev/null; then
            echo "[preflight] claude: $(version)"
        else
            echo "[preflight] WARN: claude update failed; keeping $(version)" >&2
        fi
    elif curl -fsSL --connect-timeout 10 https://claude.ai/install.sh | bash \
        && command -v claude >/dev/null 2>&1; then
        echo "[preflight] installed claude $(version)"
    else
        echo "[preflight] ERROR: claude install failed; nothing to run" >&2
        exit 1
    fi
}

# Codex is a tree, not a binary: it spawns codex-code-mode-host from
# beside its own real path, so $bin/codex is a link into the tree.
codex_pkg="$HOME/.local/codex"

# The new tree is built whole before the old one goes, so a failed
# download leaves the working codex alone.
fetch_codex() {
    mkdir -p "$codex_pkg.new" || return 1
    if ! fetch_release openai/codex "$1" "codex-package-$musl" "$codex_pkg.new"; then
        rm -rf "$codex_pkg.new"
        return 1
    fi
    rm -rf "$codex_pkg"
    mv "$codex_pkg.new" "$codex_pkg"
    ln -sfn "$codex_pkg/bin/codex" "$bin/codex"
    codex --version >/dev/null 2>&1
}

install_codex() {
    # A start killed mid-download leaves one; nothing else reclaims it.
    rm -rf "$codex_pkg.new"
    tag=$(latest_tag openai/codex)
    runs=$(installed_version codex)
    # A plain file is the old one-binary install: nothing to spawn
    # beside it, so it goes even when its version matches.
    packaged=$runs
    [ -L "$bin/codex" ] || packaged=

    if [ -n "$tag" ] && [ "${tag#rust-v}" = "$packaged" ]; then
        return
    elif [ -z "$tag" ] && [ -z "$runs" ]; then
        echo "[preflight] ERROR: cannot read codex's latest release and none is installed" >&2
        exit 1
    elif [ -z "$tag" ]; then
        echo "[preflight] WARN: cannot read codex's latest release; keeping $runs" >&2
    elif fetch_codex "$tag"; then
        echo "[preflight] codex $tag"
    elif [ -n "$runs" ]; then
        echo "[preflight] WARN: codex $tag install failed; keeping $runs" >&2
    else
        echo "[preflight] ERROR: codex $tag install failed; nothing to run" >&2
        exit 1
    fi
}

install_grok() {
    latest=$(curl -fsSL --connect-timeout 10 https://x.ai/cli/stable 2>/dev/null | tr -d '[:space:]')
    have=$(grok --version 2>/dev/null | sed -n 's/^grok \([^ ]*\).*/\1/p')
    if [ -z "$latest" ] && [ -z "$have" ]; then
        echo "[preflight] ERROR: cannot read grok's latest version and none is installed" >&2
        exit 1
    elif [ -z "$latest" ]; then
        echo "[preflight] WARN: cannot read grok's latest version; keeping $have" >&2
    elif [ "$latest" != "$have" ]; then
        if curl -fsSL --connect-timeout 10 https://x.ai/cli/install.sh | GROK_BIN_DIR="$bin" bash -s "$latest" \
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
have=$(installed_version rtk)
if [ -z "$tag" ]; then
    echo "[preflight] WARN: cannot read rtk's latest release; keeping ${have:-none}" >&2
elif [ "${tag#v}" != "$have" ]; then
    if fetch_release rtk-ai/rtk "$tag" "rtk-$musl" "$bin" rtk; then
        echo "[preflight] rtk $tag"
    else
        echo "[preflight] WARN: rtk $tag download failed" >&2
    fi
fi

# Everything below only needs doing once per home: skills, rtk's hook,
# context7 and plugins all live in the kept home, and a box resumed in
# another project keeps the same home. So it runs again only when this
# part of the hook changes, or when a warning left it unfinished.
stamp="$HOME/.cache/alphaca/setup-$(sed -n '/^# Everything below only needs doing once/,$p' "$0" | sha256sum | cut -c1-16)"
if [ -f "$stamp" ]; then
    echo "[preflight] setup already done by this hook; delete $stamp to redo it"
    exit 0
fi
setup_ok=1
warn() {
    echo "[preflight] WARN: $*" >&2
    setup_ok=0
}

# Skills, named and pinned. A skill is instructions the agent follows,
# so an unpinned HEAD would change how the box behaves between starts,
# and `--skill '*'` would let a new one in unread. The installer still
# does the placing, so each product's own directory stays its business.
# `-g`, or they land in the workspace.
add_skills() {
    src="$HOME/.cache/alphaca/skills-src"
    rm -rf "$src"
    if ! git init -q "$src" \
        || ! git -C "$src" fetch -q --depth 1 "https://github.com/$1" "$2" \
        || ! git -C "$src" checkout -q FETCH_HEAD; then
        warn "cannot fetch $1 at $2"
    else
        # Unquoted on purpose: $3 is a list, every name [a-z-] only.
        flags=
        for skill in $3; do flags="$flags -s $skill"; done
        # shellcheck disable=SC2086
        if npx -y skills add "$src" $flags -a "$skills_agent" -g --yes </dev/null; then
            echo "[preflight] skills from $1 ($run)"
        else
            warn "skills add $1 failed"
        fi
    fi
    rm -rf "$src"
}

# The hook owns this home's skills, so the set is what it names and
# nothing more: an earlier run's picks go before this run's land.
npx -y skills remove --all -g </dev/null >/dev/null 2>&1 || true

add_skills JuliusBrussee/caveman 3ee70a102609e550bd2e68004bf5990a9341c851 \
    "caveman cavecrew caveman-compress caveman-stats lean-build migration
     safe-refactor surgical-patch verify-and-stop"
add_skills mattpocock/skills c55ee46073ed923f86ce59a5eb3b6d895095d1b7 \
    "code-review codebase-design diagnosing-bugs domain-modeling grilling pr
     prototype research resolving-merge-conflicts tdd wizard writing-for-agents"

# RTK flavour: claude and codex get a PreToolUse rewrite, grok a rules
# file. The text rtk appends to an instructions file is wiped at the next
# start, so ROLE.md carries it instead.
if command -v rtk >/dev/null 2>&1; then
    rtk telemetry disable >/dev/null 2>&1 </dev/null || true
    rtk_ok=0
    case "$run" in
        claude)
            rtk init -g --auto-patch </dev/null && rtk_ok=1
            ;;
        codex)
            # `rtk hook codex` answers nothing; the claude one speaks the
            # same JSON. docs/research/box-tools-jvm.md.
            mkdir -p "$HOME/.codex" \
                && printf '{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"rtk hook claude"}]}]}}\n' \
                    >"$HOME/.codex/hooks.json" \
                && rtk_ok=1
            ;;
        grok)
            # grok reads every *.md in that directory, RTK.md included.
            CODEX_HOME="$HOME/.grok/rules" rtk init -g --codex </dev/null && rtk_ok=1
            ;;
    esac
    if [ "$rtk_ok" -eq 1 ]; then
        echo "[preflight] rtk init done"
    else
        warn "rtk init failed"
    fi
else
    warn "rtk binary not on PATH"
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
            warn "context7 MCP add failed"
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
            warn "context7 MCP add failed"
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
            warn "context7 MCP add failed"
        fi
        ;;
esac

# Claude plugins have no grok or codex analogue. Skip them there and
# say so; rust-analyzer-lsp is the one that is not also a skill repo.
case "$run" in
    claude)
        claude plugin marketplace add anthropics/claude-plugins-official </dev/null \
            || warn "marketplace anthropics/claude-plugins-official failed"
        if claude plugin install rust-analyzer-lsp@claude-plugins-official </dev/null; then
            echo "[preflight] rust-analyzer-lsp plugin installed"
        else
            warn "rust-analyzer-lsp plugin failed"
        fi
        ;;
    *)
        echo "[preflight] skipping Claude plugins; $run has no analogue"
        ;;
esac

if [ "$setup_ok" -eq 1 ]; then
    mkdir -p "$(dirname "$stamp")"
    : >"$stamp"
    echo "[preflight] setup done; later starts skip it until this hook changes"
fi
