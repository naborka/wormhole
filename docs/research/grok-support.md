# Adding xAI's Grok Build CLI: what the third agent cost

The question: what does a third agent client cost now that the registry
exists, and what does grok need that claude and codex did not?

Answer up front: **one registry entry, and one thing the registry could
not say.** Grok needs no first-run config file at all, so `config` became
optional. Everything else was data.

Unlike the codex research, every claim about grok below was checked
against the release binary itself (`grok 1.0.24 (68e414c661e3)`, the
`linux-x86_64` build behind `https://x.ai/cli/stable`), run locally.
Claims taken from documentation instead are marked as such.

## 1. What grok is

Grok Build is xAI's terminal coding agent: a TUI, a headless mode, and an
ACP server, distributed as one binary named `grok`
([x.ai/news/grok-build-cli](https://x.ai/news/grok-build-cli)).

The binary is **static-pie** — its program headers carry no `PT_INTERP`,
so there is no dynamic loader to satisfy and it runs on Alpine as it
ships. Verified by reading the first 8 KiB of the artifact and parsing
the program-header table; the eleven entries are `PHDR`, four `LOAD`,
`TLS`, `DYNAMIC`, `GNU_RELRO`, `GNU_EH_FRAME`, `GNU_STACK`, `NULL`.

Distribution is a versioned URL, not a GitHub release
(`api.github.com/repos/xai-org/grok-build/releases/latest` is 404):

- `https://x.ai/cli/stable` — the channel pointer, one line, the latest
  version
- `https://x.ai/cli/grok-<version>-linux-x86_64` — 150 MiB, plus `.zst`
  (46 MiB) and `.gz` (61 MiB) beside it
- `https://x.ai/cli/install.sh` picks the smallest decoder the host has,
  falls back to `storage.googleapis.com/grok-build-public-artifacts/cli`,
  and links `$GROK_BIN_DIR/grok` at the binary it keeps in
  `~/.grok/downloads`

## 2. The registry entry, field by field

| Field | Value | Why |
|---|---|---|
| `command` | `grok --always-approve --trust` | Two gates, both answered by the box existing |
| `instructions` | `.grok/rules/AGENTS.md` | The one directory grok reads whatever it starts in |
| `pointer` | `Symlink` | No import syntax |
| `model_env` | `GROK_DEFAULT_MODEL` | Grok reads a variable for it |
| `credential_files` | `.grok/auth.json` | Where `grok login` puts the tokens |
| `config` | `None` | Nothing left for a start to answer |

**`--always-approve`.** The documented name; `--yolo` is its alias
([22-permissions-and-safety.md](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/22-permissions-and-safety.md)).
Both are accepted by the binary. The long name is the one in the docs'
own examples, so it is the one wormhole spells.

**`--trust`.** The gate the codex work has no analogue for. Grok's
project rules load only for a trusted folder: "Startup loading requires
folder trust (`--trust` or an interactive grant)"
([12-project-rules.md](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/12-project-rules.md)).
Without it a box would start with the role's instructions silently
unread. It is a boolean flag — `grok --trust --version` prints the
version rather than consuming it — and it appears in the binary's own
shell completions, though not in `--help`.

**No sandbox flag.** Grok's Landlock/seccomp sandbox is off by default
([18-sandbox.md](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/18-sandbox.md)),
so unlike codex there is nothing to bypass. A box that wanted one could
set `GROK_SANDBOX`, and nothing in wormhole stops it.

**`.grok/rules/AGENTS.md`.** Grok scans `$GROK_HOME/rules/` for every
`*.md`, "regardless of where it starts", before any project file. That is
the only home-level location it always reads — there is no
`$GROK_HOME/AGENTS.md` — so the pointer goes into the rules directory.
Verified end to end: with `AGENTS.md` at the home root and
`.grok/rules/AGENTS.md` a relative symlink to it, `grok inspect` run from
a different directory reports the file as global project instructions.

**`GROK_DEFAULT_MODEL`.** `models.default` in `~/.grok/config.toml` is
also read from this variable
([26-config-reference.md](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/26-config-reference.md)),
and the name is present in the binary. So the manifest's `model` travels
the way claude's does, and nothing needs writing to disk for it. This is
why grok needs no config seeding where codex does.

**`config: None`.** Codex needs `projects."<workspace>".trust_level` and
its model written into `.codex/config.toml`; claude needs four first-run
answers in `.claude.json`. Grok needs neither: trust is a flag and the
model is a variable. Making the field optional is the honest encoding —
the alternative is inventing a config file for an agent that asks for
nothing.

## 3. What changed in wormhole beyond the entry

**`pointer_link` moved into the pure core.** Grok's pointer is the first
one two directories deep, so the `../..` arithmetic that had lived
inside the file-writing code in the binary crate now lives in
`wormhole-core::manifest` with a test for both depths. This is the
project's own rule applied: a decision that needs no kernel does not
belong in the crate that has one.

**Two per-agent facts followed it.** `instructions_pointer` now answers
with the path *and* the pointer, from one registry lookup, so the two
cannot disagree and the binary crate no longer reconciles them with an
`expect`. And `ConfigSeed` names its own file, so `.claude.json` and
`.codex/config.toml` are registry facts like every other path instead of
a match in the file-writing code.

No call site learned a third agent name, which is what the codex work's
registry table was for.

## 4. Concept map

| Claude Code | Codex CLI | Grok Build |
|---|---|---|
| `~/.claude/` | `~/.codex/` (`$CODEX_HOME`) | `~/.grok/` (`$GROK_HOME`) |
| `~/.claude/CLAUDE.md` | `~/.codex/AGENTS.md` | `~/.grok/rules/*.md` |
| `~/.claude/settings.json` + `~/.claude.json` | `~/.codex/config.toml` | `~/.grok/config.toml` |
| `~/.claude/.credentials.json` | `~/.codex/auth.json` | `~/.grok/auth.json` |
| `ANTHROPIC_API_KEY` / `ANTHROPIC_MODEL` | `OPENAI_API_KEY` / config key | `XAI_API_KEY` / `GROK_DEFAULT_MODEL` |
| `--dangerously-skip-permissions` | `--dangerously-bypass-approvals-and-sandbox` | `--always-approve --trust` |
| `claude -p` | `codex exec` | `grok -p` |
| `claude update` | GitHub release tarball | `https://x.ai/cli/install.sh` |

## 5. The role

`roles/alphaca-grok/` is alphaca under grok, and every piece was checked
against the real binary and the real installers:

- **grok itself** comes from the channel pointer and the installer,
  compared against `grok --version` first so a current box downloads
  nothing. `zstd` is in the image only because the installer prefers the
  46 MiB build over the 61 MiB one.
- **Skills** go in with `npx -y skills add <repo> --skill '*' -a grok`.
  The installer knows grok as a target: it writes `~/.grok/skills/<name>`,
  and `grok inspect` then lists the skill. Checked with `rust-skills`.
- **rtk** ships no grok flavour. Its codex flavour is the same rules text
  with no hook patching, and it honours `CODEX_HOME` — so
  `CODEX_HOME=~/.grok/rules rtk init -g --codex` writes `RTK.md` into the
  directory grok reads every `*.md` from. Checked: the `AGENTS.md`
  reference it appends follows wormhole's symlink into the canonical
  file, the symlink survives, and a second run adds no duplicate.
- **context7** is `grok mcp add context7 -e … -- npx -y
  @upstash/context7-mcp`, which writes `[mcp_servers.context7]` into
  `~/.grok/config.toml`. Checked, including that `grok mcp list` exits 0
  when nothing is configured, which is what the "already added?" test
  relies on.
- **Plugins are not used.** Grok has `plugin marketplace add`, and it
  accepts a claude marketplace repository, but it then finds no plugins
  in it: it does not read `.claude-plugin/marketplace.json`. Checked
  against `JuliusBrussee/caveman`. The `skills` installer is the route
  that works, which is why the role uses it.

`XAI_API_KEY` is declared empty, the way the codex role declares
`OPENAI_API_KEY`. An empty one is harmless: with no `auth.json`, this
build answers "Not signed in" identically whether the variable is unset,
empty, or holds a key, so the empty default cannot take a box off its
sign-in path.

Deferred, and named rather than hidden: `credentials = "share"` puts the
host's `auth.json` in reach of the box, the same trade the codex role
makes. Grok's own docs advise against copying `auth.json` between
machines; a shared bind is not a copy between machines, but it is one
token for two processes, and a refresh race between them is possible —
the cost is one `grok login`.

## 6. Pre-attack

- **The version comparison can be wrong on a channel switch.** A box
  pinned to alpha by a hand-run `grok update --alpha` would show a
  version the stable pointer never names, and the preflight would
  reinstall stable over it on every start. Accepted: the role does not
  offer a channel, and reinstalling the version the role means is the
  behaviour the role wants.
- **`--trust` trusts whatever the workspace is.** That is the point of a
  box, and it is the same claim `hasTrustDialogAccepted` makes for claude
  and `trust_level = "trusted"` for codex.
- **The persona is now in three role directories.** The codex research
  named a composable `instructions` list as the eventual fix. It is not
  one: a role is published as its own repository and must be
  self-contained, so a list composes files *within* one role and cannot
  reach another role's `ROLE.md`. Three self-contained roles is three
  copies whatever `instructions` accepts. A real fix would be a role that
  names another role, which is a feature nothing has asked for.
