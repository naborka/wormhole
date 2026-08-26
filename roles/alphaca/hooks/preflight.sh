#!/bin/sh
# Alphaca preflight. Runs in the box before the agent starts; wormhole has
# already written ROLE.md into ~/.claude/CLAUDE.md, so everything here is
# best-effort extras — a failure warns and moves on, it never blocks launch.
set -eu

# rust-skills -> Claude skills dir.
skills="$HOME/.claude/skills"
if [ ! -d "$skills/rust-skills" ]; then
    mkdir -p "$skills"
    if git clone --depth 1 https://github.com/leonardomso/rust-skills "$skills/rust-skills" 2>/dev/null; then
        echo "[preflight] installed rust-skills"
    else
        echo "[preflight] WARN: rust-skills clone failed" >&2
    fi
fi

# RTK PreToolUse hook. Telemetry disable first records consent so init skips
# that prompt; --auto-patch skips the settings prompt; </dev/null forces
# non-TTY stdin so nothing can block even if the consent record is missing.
if command -v rtk >/dev/null 2>&1; then
    rtk telemetry disable >/dev/null 2>&1 </dev/null || true
    if rtk init -g --auto-patch </dev/null; then
        echo "[preflight] rtk init done"
    else
        echo "[preflight] WARN: rtk init failed" >&2
    fi
else
    echo "[preflight] WARN: rtk binary not on PATH" >&2
fi

# Plugins. Marketplaces first, then installs; each on its own so one bad
# source does not take the rest down.
for marketplace in JuliusBrussee/caveman mattpocock/skills; do
    claude plugin marketplace add "$marketplace" </dev/null \
        || echo "[preflight] WARN: marketplace $marketplace failed" >&2
done
for plugin in \
    caveman@caveman \
    mattpocock-skills@mattpocock \
    context7@claude-plugins-official \
    rust-analyzer-lsp@claude-plugins-official
do
    claude plugin install "$plugin" </dev/null \
        || echo "[preflight] WARN: plugin $plugin failed" >&2
done
