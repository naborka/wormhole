#!/bin/sh
# Alphaca Codex preflight. Runs in the box before the agent. Codex is the
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

# Codex, latest release, in the kept home. Release tags read rust-vX.Y.Z
# and the tarball holds one static musl binary named after its target.
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

# Skills -> codex's skills directory in the kept home, via the `skills`
# installer; without -g they land in the workspace. Each on its own so
# one bad source does not take the rest down.
for repo in \
    JuliusBrussee/caveman \
    mattpocock/skills \
    leonardomso/rust-skills
do
    if npx -y skills add "$repo" --skill '*' -a codex -g --yes </dev/null; then
        echo "[preflight] installed skills from $repo"
    else
        echo "[preflight] WARN: skills add $repo failed" >&2
    fi
done

# RTK, codex flavour: writes ~/.codex/RTK.md and appends a reference to
# AGENTS.md. The append goes through wormhole's symlink into the canonical
# file, which is rewritten on every start — and this hook runs after that
# rewrite, so the reference is back before the agent reads it. A rules
# file, not a rewrite hook: codex has no PreToolUse, the model follows
# the rules instead. Telemetry disable first records consent so init
# skips that prompt; </dev/null forces non-TTY stdin so nothing blocks.
if command -v rtk >/dev/null 2>&1; then
    rtk telemetry disable >/dev/null 2>&1 </dev/null || true
    if rtk init -g --codex </dev/null; then
        echo "[preflight] rtk init done"
    else
        echo "[preflight] WARN: rtk init failed" >&2
    fi
else
    echo "[preflight] WARN: rtk binary not on PATH" >&2
fi

# context7 as an MCP server in ~/.codex/config.toml. Added once: the
# config is merged, never overwritten, so the entry survives restarts.
if codex mcp get context7 >/dev/null 2>&1; then
    :
elif codex mcp add context7 \
    ${CONTEXT7_API_KEY:+--env CONTEXT7_API_KEY="$CONTEXT7_API_KEY"} \
    -- npx -y @upstash/context7-mcp </dev/null; then
    echo "[preflight] context7 MCP added"
else
    echo "[preflight] WARN: context7 MCP add failed" >&2
fi
