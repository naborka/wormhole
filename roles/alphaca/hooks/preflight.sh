#!/bin/sh
# Alphaca preflight. Runs in the box before the agent. Claude Code is the
# one thing that must be here at the end; everything else warns and moves on.
set -eu

bin="$HOME/.local/bin"
mkdir -p "$bin"
version() { claude --version 2>/dev/null; }

# Claude Code, latest, in the kept home. The native installer and
# `claude update` both land in ~/.local/bin, first on the box's PATH.
if command -v claude >/dev/null 2>&1; then
    if claude update </dev/null; then
        echo "[preflight] claude: $(version)"
    else
        echo "[preflight] WARN: claude update failed; keeping $(version)" >&2
    fi
elif curl -fsSL https://claude.ai/install.sh | bash \
    && command -v claude >/dev/null 2>&1; then
    # A `curl | bash` whose curl failed feeds bash an empty script and
    # exits 0, so the binary itself is what says the install happened.
    echo "[preflight] installed claude $(version)"
else
    echo "[preflight] ERROR: claude install failed; nothing to run" >&2
    exit 1
fi

# rtk, latest release, same place. The redirect names the tag without
# the API's rate limit.
latest=$(curl -fsSI https://github.com/rtk-ai/rtk/releases/latest 2>/dev/null \
    | sed -n 's|^[Ll]ocation: .*/tag/\([^[:space:]]*\).*|\1|p')
have=$(rtk --version 2>/dev/null | sed -n 's/.* \([0-9][0-9.]*\).*/\1/p')
if [ -z "$latest" ]; then
    echo "[preflight] WARN: cannot read rtk's latest release; keeping ${have:-none}" >&2
elif [ "${latest#v}" != "$have" ]; then
    url="https://github.com/rtk-ai/rtk/releases/download/$latest/rtk-$(uname -m)-unknown-linux-musl.tar.gz"
    if curl -fsSL "$url" | tar -xz -C "$bin" rtk; then
        echo "[preflight] rtk $latest"
    else
        echo "[preflight] WARN: rtk $latest download failed" >&2
    fi
fi

skills="$HOME/.claude/skills"
if [ ! -d "$skills/rust-skills" ]; then
    mkdir -p "$skills"
    if git clone --depth 1 https://github.com/leonardomso/rust-skills "$skills/rust-skills" 2>/dev/null; then
        echo "[preflight] installed rust-skills"
    else
        echo "[preflight] WARN: rust-skills clone failed" >&2
    fi
fi

# Telemetry disable records consent so `rtk init` skips that prompt;
# </dev/null keeps every prompt from blocking.
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

# The official marketplace is registered on claude's first interactive
# start, which a fresh home has not had; add it here or its plugins fail.
for marketplace in anthropics/claude-plugins-official JuliusBrussee/caveman mattpocock/skills; do
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
