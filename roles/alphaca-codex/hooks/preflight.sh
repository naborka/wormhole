#!/bin/sh
# Alphaca Codex preflight. Runs in the box before the agent starts;
# wormhole has already written ROLE.md into the canonical ~/AGENTS.md and
# symlinked ~/.codex/AGENTS.md at it, so everything here is best-effort
# extras — a failure warns and moves on, it never blocks launch.
set -eu

# Skills -> codex's skills directory, via the `skills` installer. Each on
# its own so one bad source does not take the rest down.
for repo in \
    JuliusBrussee/caveman \
    mattpocock/skills \
    leonardomso/rust-skills
do
    if npx -y skills add "$repo" --skill '*' -a codex --yes </dev/null; then
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
