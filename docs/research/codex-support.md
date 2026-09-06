# Adding OpenAI Codex CLI support: one role per client, one driver layer in wormhole

The question: when Codex CLI joins Claude Code, is the right shape a new
role that uses `codex` with the same setup, or one role that can run
different agent clients?

Answer up front: **both, at different layers — and the code already says
so.** The *client driver* belongs in wormhole, client-agnostic; the *role*
names exactly one client, so Codex support is a new `KnownAgent` entry
plus a new role directory. What must change is not the role model but the
scattering of Claude-specific behavior outside the driver seam wormhole
already has.

Evidence is `file:line` in this repository, or a URL. Paths are relative
to the `wormhole/` crate root. Note on sources: direct fetch of
github.com/openai/codex and developers.openai.com was blocked from this
box (no DNS route, and WebFetch refused both domains), so external claims
were gathered through search excerpts of those primary pages; each claim
cites the primary page it came from and should be re-verified against it
before implementation pins versions or flags.

## 1. What wormhole is, in one paragraph

`wormhole box` builds an image from a role's `wormhole.toml` (pinned base
tarball, apk packages, digest-proved artifacts, build lines), starts an
isolated box with a kept per-box home, seeds the agent's instructions and
config into that home, and runs the agent as PID 1 with permissions
bypassed — "the box, not the prompt, is what holds the line"
(`crates/wormhole-core/src/manifest.rs:246-247`). A role is a directory:
`wormhole.toml` + `ROLE.md` + `hooks/preflight.sh`
(`docs/src/guide/roles.md:8-13`). Box identity is workspace + role
source, and the kept home carries the agent's logins and history
(`crates/wormhole-core/src/home.rs:38-54`).

## 2. Where the client already is a parameter

Wormhole has an agent-driver seam today. A manifest names its client in
`[agent] run` (`crates/wormhole-core/src/manifest.rs:98-119`), and a
static registry defines what a client means:

- `KnownAgent { name, command, instructions }` —
  `crates/wormhole-core/src/manifest.rs:243-257`. The only entry is
  `claude`, command `["claude", "--dangerously-skip-permissions"]`,
  instructions target `.claude/CLAUDE.md`.
- Unknown names are refused at parse
  (`crates/wormhole-core/src/manifest.rs:455-458`), the launch and attach
  commands come from the registry
  (`crates/wormhole-core/src/manifest.rs:481-496`), and instruction
  seeding asks the registry where to write
  (`crates/wormhole-core/src/manifest.rs:498-502`,
  `crates/wormhole/src/main.rs:2355-2372`).

So "client-agnostic roles" already exist in the only sense that is
coherent here: the role *file format* is client-agnostic, and the client
is one line in it (`roles/wormhole-alphaca-java/wormhole.toml:144-145`).

## 3. Where Claude leaks outside that seam

This is the structural finding. Claude-specific behavior is gated by
string comparison at each call site instead of living in the registry, so
adding a second agent means hunting every hidden gate:

| What | Where | Gate |
|---|---|---|
| `agent.model` baked as `ANTHROPIC_MODEL` | `crates/wormhole-core/src/manifest.rs:646-647` | none — happens for *any* agent |
| First-run answers seeded into `.claude.json` | `crates/wormhole/src/main.rs:2409-2426` | `run == Some("claude")` |
| Status line script + `.claude/settings.json` merge | `crates/wormhole/src/main.rs:2428-2449`, `:25` | `run == Some("claude")` |
| Usage windows: undocumented Anthropic endpoint, `claude-code/…` User-Agent, host `~/.claude/.credentials.json` | `crates/wormhole/src/usage.rs:20-35` | none (spawned per box) |
| Broker upstream fixed to `https://api.anthropic.com` | `crates/wormhole/src/broker.rs:26` | n/a — the broker *is* Anthropic-shaped |
| Broker credential: host `~/.claude/.credentials.json`, `claudeAiOauth` JSON, `anthropic-beta: oauth-2025-04-20` header | `crates/wormhole/src/broker.rs:74`, `crates/wormhole-core/src/broker.rs:487`, `:90` | n/a |
| Brokered box env: `ANTHROPIC_BASE_URL` + dummy key | `crates/wormhole-core/src/manifest.rs:660-667` | `brokers(manifest)` only |

The `ANTHROPIC_MODEL` line is an outright bug under a second agent: a
role saying `run = "codex"` with `model = "gpt-5.1-codex"` would export
`ANTHROPIC_MODEL` and Codex would ignore it. The rest is correctly gated
but wrongly *placed* — each gate is a `== "claude"` scattered where the
registry should answer.

## 4. Codex CLI, the facts that matter to wormhole

Each claim cites the primary page it was drawn from.

**Install.** `npm install -g @openai/codex` or `brew install codex`
([github.com/openai/codex](https://github.com/openai/codex)). Codex is a
Rust binary; releases publish `codex-x86_64-unknown-linux-musl.tar.gz`
under tags like `rust-v0.153.3`, and GNU-linux artifacts were dropped in
favor of musl (openai/codex PR #19445, via
[github.com/openai/codex/releases](https://github.com/openai/codex/releases)).
A single static musl binary fits wormhole's Alpine boxes and
`[[image.artifact]]` digest pinning *better* than Claude Code does — no
npm, no platform-dependency tarball pair like
`roles/wormhole-alphaca-java/wormhole.toml:115-127` needs.

**Invocation.** `codex` is the interactive TUI; `codex exec "…"` is the
non-interactive form for scripts/CI
([developers.openai.com/codex/cli/reference](https://developers.openai.com/codex/cli/reference)).
Model comes from `--model`, `-c model="…"`, or `config.toml`; there is no
`ANTHROPIC_MODEL`-style env var.

**Permissions.** Two orthogonal settings, `sandbox_mode`
(`read-only` / `workspace-write` / `danger-full-access`) and
`approval_policy` (`untrusted` / `on-request` / `never`), with flags
`--sandbox`, `--full-auto`, and
`--dangerously-bypass-approvals-and-sandbox` (alias `--yolo`) which the
docs say to use only inside an isolated runner
([developers.openai.com/codex/agent-approvals-security](https://developers.openai.com/codex/agent-approvals-security),
[cli/reference](https://developers.openai.com/codex/cli/reference)).
Wormhole's box *is* that isolated runner, so
`--dangerously-bypass-approvals-and-sandbox` is the exact analogue of
`--dangerously-skip-permissions`
(`crates/wormhole-core/src/manifest.rs:246-248`). It is also technically
necessary: Codex's own Linux sandbox (Landlock/seccomp, bwrap in release
assets) cannot be assumed to nest inside wormhole's namespaces.

**Config.** State lives under `$CODEX_HOME`, default `~/.codex`; main
config is `~/.codex/config.toml` (TOML, keys like `model`,
`approval_policy`, `sandbox_mode`, `mcp_servers`, named `profiles`)
([developers.openai.com/codex/config-basic](https://developers.openai.com/codex/config-basic),
[config-reference](https://developers.openai.com/codex/config-reference)).
Project-local `.codex/config.toml` layers exist but load only for trusted
projects, and trust is recorded in the user config — the analogue of the
first-run/trust answers wormhole seeds into `.claude.json`
(`crates/wormhole/src/main.rs:2409-2412`).

**Instructions.** `AGENTS.md`, not `CLAUDE.md`. Global scope:
`$CODEX_HOME/AGENTS.md` (with `AGENTS.override.md` taking precedence),
loaded first; then project-root-to-cwd `AGENTS.md` files, concatenated,
closer-wins, capped at 32 KiB
([developers.openai.com/codex/guides/agents-md.md](https://developers.openai.com/codex/guides/agents-md.md)).
So the seeding target for a codex `KnownAgent` is `.codex/AGENTS.md` —
the same mechanism as `.claude/CLAUDE.md`, and the *content* needs no
translation: both are plain markdown personas. `compose_instructions`
(`crates/wormhole-core/src/manifest.rs:504-516`) works unchanged.

**Auth.** Default is ChatGPT sign-in: `codex login` opens a browser flow
(local callback on `localhost:1455`) and caches OAuth tokens
(`access_token`, `refresh_token`, `id_token`, `account_id`) in plaintext
`$CODEX_HOME/auth.json`; the file is host-portable — copy it to a
headless machine and codex works; tokens auto-refresh in use
([github.com/openai/codex/blob/main/docs/authentication.md](https://github.com/openai/codex/blob/main/docs/authentication.md),
[developers.openai.com/codex/auth](https://developers.openai.com/codex/auth)).
Alternative: an API key (usage-billed, recommended for CI). There is also
`codex login --with-access-token` fed on stdin, and
`cli_auth_credentials_store = "file" | "keyring"`.

**MCP.** Supported, STDIO and HTTP transports, configured under
`mcp_servers` in `config.toml`
([developers.openai.com/codex/config-reference](https://developers.openai.com/codex/config-reference)).

## 5. Concept map

| Claude Code | Codex CLI |
|---|---|
| `~/.claude/` | `~/.codex/` (`$CODEX_HOME`) |
| `~/.claude/CLAUDE.md` (global instructions) | `~/.codex/AGENTS.md` |
| project `CLAUDE.md` | project `AGENTS.md` |
| `~/.claude/settings.json` + `~/.claude.json` | `~/.codex/config.toml` |
| `~/.claude/.credentials.json` (`claudeAiOauth`) | `~/.codex/auth.json` (`tokens`) |
| `ANTHROPIC_API_KEY` / `ANTHROPIC_MODEL` / `ANTHROPIC_BASE_URL` | `OPENAI_API_KEY` / `--model` or `model` key / `model_providers.*.base_url` |
| `--dangerously-skip-permissions` | `--dangerously-bypass-approvals-and-sandbox` |
| permission modes | `sandbox_mode` × `approval_policy` |
| `claude -p` (non-interactive) | `codex exec` |
| npm `@anthropic-ai/claude-code` + musl platform pkg | npm `@openai/codex`, or one musl release tarball |

## 6. The design question, answered against the code

### Why not one role that runs either client

A "client-agnostic role" would mean the client is chosen at `wormhole
box` time, not in the role. Everything about the actual role files argues
against it:

1. **The image is client-specific.** In the java role, the agent binary
   arrives as digest-pinned artifacts and an offline npm install
   (`roles/wormhole-alphaca-java/wormhole.toml:58-61,115-127`). A
   dual-client role must bake both clients into one image, and its recipe
   churns whenever *either* client releases. Approval is per recipe
   commit (`docs/src/guide/roles.md:184-189`) — every Codex bump would
   re-ask users who only run Claude.
2. **Access is client-specific.** The role grants exactly
   `~/.claude/.credentials.json`
   (`roles/wormhole-alphaca-java/wormhole.toml:153-155`) — deliberately
   the file, not the directory. A dual role grants both credential files
   to every box, doubling what a compromised box can exfiltrate for no
   gain. That is the wrong direction for a tool whose whole point is
   "everything unnamed here is invisible" (`wormhole.toml:151`).
3. **Env and model are client-specific.** `ANTHROPIC_API_KEY`,
   `CAVEMAN_DEFAULT_MODE`, `model = "claude-fable-5"`
   (`roles/wormhole-alphaca-java/wormhole.toml:144-178`) have Codex
   counterparts with different names and different mechanisms.
4. **It is the override system wormhole refuses on principle.** A
   manifest that both names a role and carries a recipe is refused, "not
   merged — merging is where a key like this turns into an override
   system nobody can predict" (`docs/src/guide/roles.md:64-67`,
   `crates/wormhole-core/src/manifest.rs:389-391`). A launch-time
   `--agent` switch is that same key wearing a flag.
5. **Box identity gains nothing.** A box is workspace + role source
   (`docs/src/guide/roles.md:74-94`, `crates/wormhole-core/src/home.rs`).
   Two clients sharing one home is not continuity — their state dirs,
   histories and logins are disjoint anyway. Two roles → two boxes is
   already the honest model.

What *is* client-neutral in a role: the persona text (plain markdown, no
translation), the preflight hook, most packages, the access shape. That
is real, but it is one file and some TOML — and role duplication is a
cost the project has already accepted with open eyes: "ROLE.md repeats
alphaca's persona because a manifest names one instructions file and
wormhole cannot compose two"
(`roles/wormhole-alphaca-java/wormhole.toml:12-13`). A codex role makes
that a third copy; the fix, if wanted, is letting `[agent] instructions`
take a list, which is orthogonal to Codex.

### Why not "just a new role" either

A role alone cannot do it: `run = "codex"` is refused at parse
(`crates/wormhole-core/src/manifest.rs:455-458`,
`ManifestError::UnknownAgent`). The client driver lives in wormhole, and
that is where the work is.

### Recommendation

**One role per client; one client-agnostic driver layer inside wormhole;
fix the seam before adding the second entry.**

1. **Widen `KnownAgent` first (the root-cause fix).** Today's struct
   holds `command` and `instructions`
   (`crates/wormhole-core/src/manifest.rs:243-251`); everything else
   Claude-shaped is gated by `== Some("claude")` at call sites
   (`crates/wormhole/src/main.rs:2413,2433`) or not gated at all
   (`ANTHROPIC_MODEL`, `crates/wormhole-core/src/manifest.rs:646-647`).
   Move into the registry, per agent: how `model` is passed (env var
   name vs an argv suffix vs a config key), which home-config seeding
   runs, which credential file the host-side features read, whether
   the status-line/usage feed applies. Then adding an agent is adding
   one entry, and a forgotten gate is impossible by construction.
2. **Add the `codex` entry.** Command
   `["codex", "--dangerously-bypass-approvals-and-sandbox"]`;
   instructions target `.codex/AGENTS.md`; model passed as
   `["--model", <model>]` (or a seeded `model` key); config seeding =
   merge into `~/.codex/config.toml` (workspace trust, and whatever
   first-run answers a headless box needs); no usage feed.
3. **Codex role v1 skips the broker.** The broker is Anthropic-shaped
   end to end: fixed upstream (`crates/wormhole/src/broker.rs:26`),
   `claudeAiOauth` parsing (`crates/wormhole-core/src/broker.rs:487`),
   Anthropic OAuth beta header (`:90`), `ANTHROPIC_BASE_URL` injection
   (`crates/wormhole-core/src/manifest.rs:660-667`). A codex role starts
   the way the java role handles credentials — grant the one file,
   `~/.codex/auth.json`, with `dns` set
   (cf. `roles/wormhole-alphaca-java/wormhole.toml:153-159`) — accepting
   that the token sits in the box, exactly as the Claude roles accept it
   for `.credentials.json`. A brokered Codex leg (host-side auth.json,
   token refresh against OpenAI's endpoints, `model_providers` base-URL
   override pointing at the forwarder) is real follow-up work, deferred
   here, and it slots into the same per-agent registry from step 1.
4. **Ship the role as its own repository** with `wormhole.toml` at the
   root, per the publishing rule (`docs/src/guide/roles.md:300-303`):
   same Alpine base, the release musl tarball as one
   `[[image.artifact]]` extracted to `/usr/local/bin/codex` (mirroring
   the rtk line, `roles/wormhole-alphaca-java/wormhole.toml:67`),
   `[agent] run = "codex"`, the same `ROLE.md` text,
   `[env.OPENAI_API_KEY] default = ""`.

## 7. Implemented (2026-09-06)

The recommendation above was carried out, with two decisions made on top
of it:

1. **`KnownAgent` widened** (`crates/wormhole-core/src/manifest.rs`):
   per-agent `pointer` (how the instructions path delivers the canonical
   file), `model_env` (`ANTHROPIC_MODEL` for claude, `None` for codex —
   the model lands in `.codex/config.toml` instead, because codex reads
   no env var for one and a `--model` flag would be lost by `attach`,
   which knows the agent name but not the manifest), `brokered_api` (the
   base-URL/dummy-key redirect pair; `None` for codex), `usage`
   (Anthropic usage feed applicability), `config` (which first-run config
   a start seeds). Every `== "claude"` gate outside the registry is gone;
   the `ANTHROPIC_MODEL`-for-any-agent bug is fixed and pinned by
   `a_codex_model_is_not_exported_as_an_anthropic_variable`.
2. **Canonical `AGENTS.md`** — beyond the original recommendation, per
   the project owner: the composed instructions land once, in
   `~/AGENTS.md` in the box home. The path each agent reads gets a
   pointer: `.claude/CLAUDE.md` holds exactly `@~/AGENTS.md` (bare
   `@AGENTS.md` would resolve beside CLAUDE.md, inside `.claude/`), and
   `.codex/AGENTS.md` is a relative symlink — codex has no import syntax
   ([developers.openai.com/codex/guides/agents-md](https://developers.openai.com/codex/guides/agents-md)).
   A future agent adds one `Pointer` value, not a second copy of the text.
3. **Codex first-run seeding** (`crates/wormhole-core/src/seed.rs`
   `codex_config`): merges `projects."<workspace>".trust_level =
   "trusted"` (codex asks per directory even under the bypass flag on
   some releases — openai/codex#14345) and the manifest's `model` into
   `.codex/config.toml`, preserving everything the agent wrote.
4. **Usage feed and status line** gated on `manifest::usage_feed` — they
   poll an Anthropic endpoint with the host's claude credential, so a
   codex box (and an agentless box, which previously spawned the thread
   too) no longer runs them.
5. **Broker left two-legged, on purpose** (`crates/wormhole/src/broker.rs`
   header): the `CONNECT` tunnel is provider-neutral and serves codex; the
   credential-injection leg is the Anthropic adapter and only an agent
   whose entry names a `brokered_api` is pointed at it. A second injection
   adapter (host-side `auth.json`, OpenAI refresh) remains deferred — one
   adapter is a hypothetical seam, and building it here was unverifiable
   offline.
6. **Role** at `roles/alphaca-codex/`: same persona, codex musl tarball
   as one artifact, credential granted as the one file
   `~/.codex/auth.json`, egress for the OpenAI hosts. The artifact digest
   is a placeholder — this box had no route to fetch it; the manifest
   header carries the exact pin command, and the build refuses until it
   is real. Full tooling parity with alphaca, delivered the way codex
   reads each piece (the original draft dropped all of it on the wrong
   belief that it was claude-only): the Rust toolchain and `gh` as image
   parts; rtk via `rtk init -g --codex` — verified against the pinned
   rtk 0.45.0 locally: the flag exists, the AGENTS.md append follows
   wormhole's symlink into the canonical file, and a second run adds no
   duplicate — a rules-file integration, not a rewrite hook, since codex
   has no PreToolUse; the caveman, mattpocock and rust skills installed
   by a codex preflight through the `skills` installer (`-a codex`);
   context7 as an MCP entry added with `codex mcp add`. Claude's
   rust-analyzer-lsp plugin has no codex equivalent; rust-analyzer the
   binary ships in the toolchain. Unverifiable from this box, so
   best-effort with warnings in the preflight: the `skills` installer's
   exact flags, `codex mcp add` syntax, and whether a SessionStart hook
   from the caveman plugin passes codex's hook-trust gate
   non-interactively — caveman's mode rides `CAVEMAN_DEFAULT_MODE`
   regardless.

Verified: 427 workspace lib tests pass; integration expectations updated
to the canonical file; clippy clean. Re-verify against live OpenAI docs
before pinning the release tag: the trust key name and the egress host
list came from search excerpts, not a direct fetch.

Pre-attack on the original recommendation: (a) step 1 is a refactor the feature
does not strictly need — you *could* add a second string gate everywhere;
that choice is how the current scatter happened, and a third agent would
pay for it again. (b) Persona text is now maintained in three role repos;
accepted above as the project's standing trade, with the composable
`instructions` list named as the eventual fix. (c) The v1 credential
grant is weaker isolation than the broker gives Claude; that gap exists
today for `ANTHROPIC_API_KEY`-style setups too, and it is named as
deferred, not solved.
