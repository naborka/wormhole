# Role vs agent: who owns the payload

The question: how should wormhole model the relationship between a ROLE
(persona, toolchain, skills, instructions) and an AGENT (claude, codex,
grok, gemini, cursor, aider, …)? Is it better to (A) bind one role to one
agent, (B) make roles agent-agnostic with per-agent adapters/manifests,
(C) put agent manifests inside the launcher, or (D) something else?

Answer up front: **D — a three-layer split that A, B, and C each already
own a piece of, and that none of them may swallow.** The launcher owns a
thin table of *launch facts* (argv, instruction pointer, credential
files, first-run seed). The role owns the *payload* as opaque bytes plus
an opaque hook that calls the agent's own CLI. A published role names
exactly one agent for as long as a box runs one session. One manifest
with `[agents.X]` sections is the shape for N agents in one box, and not
before.

CONCEPT.md §7 is right about the contract (`files→paths · setup · argv ·
env · workdir`) and wrong if it is read as "ship that layout now" or as
"the role stops naming an agent." The user's worry is right: moving
MCP/skills/plugins *into the tool* would break preflight, because those
are not facts a launcher can know.

Evidence is `file:line` in this repository, or a URL of the vendor that
owns the claim. Paths are relative to the `wormhole/` crate root.

This note is research. It does not change code.

---

## What the evidence says

1. There is no shared schema for MCP, skills, plugins, or hooks that a
   launcher could validate. The intersection is: markdown instructions,
   a `SKILL.md` folder, and "an MCP server is a command or a URL."
   Everything past that is per-client bytes.
2. AGENTS.md is the one instruction file that is actually a standard:
   plain markdown, no required fields, closest file wins. Claude Code
   still reads `CLAUDE.md` and imports AGENTS.md with `@`. That is the
   whole of "persona is portable."
3. Skills have a second standard (`SKILL.md` + YAML `name`/`description`).
   Discovery *paths* are not standard. Plugins and hooks have none.
4. Every client already ships the installer for its own payload:
   `claude plugin install`, `codex mcp add`, `grok mcp add`,
   `gemini extensions install`. A launcher that reimplements those is a
   lagging copy of a CLI it does not control.
5. Dev Containers already solved this exact split: one spec, per-product
   `customizations` namespaces the orchestrator does not parse.
   Kubernetes RuntimeClass is the other half: the workload names a
   runtime, it does not describe one.
6. Wormhole today is A at the role boundary and C-thin in
   `KnownAgent`. CONCEPT.md §7 is B as a *target layout*, with the
   launcher still agent-agnostic. Those two answers are for two
   different questions (one session now vs N agents later). Mixing them
   is the bug.

---

# FACTS

## 1. What wormhole is today

A role is a directory: `wormhole.toml` + whatever that manifest names —
an instructions file, a preflight hook
(`docs/src/guide/roles.md:8-13`, `CONTEXT.md:29-31`). CONTEXT.md
forbids calling the role a persona: "a persona is a file a role ships,
not the role."

`[agent] run` names exactly one known client
(`crates/wormhole-core/src/manifest.rs:101-119`). Unknown names are
refused at parse (`ManifestError::UnknownAgent`,
`crates/wormhole-core/src/manifest.rs:365`). The three shipped roles
are the same persona under three clients:

| Role | `run` | Preflight installs |
|---|---|---|
| `roles/alphaca/` | `claude` | `claude` binary, rtk, git-cloned skills, `claude plugin marketplace add` / `plugin install` |
| `roles/alphaca-codex/` | `codex` | `codex` musl tarball, rtk, `npx skills add -a codex`, `codex mcp add` |
| `roles/alphaca-grok/` | `grok` | grok installer, rtk aimed at `~/.grok/rules`, `npx skills add -a grok`, `grok mcp add` |

Persona text is copied, not composed: "ROLE.md repeats alphaca's because
a manifest names one instructions file"
(`roles/alphaca-codex/wormhole.toml:8-9`,
`roles/alphaca-grok/wormhole.toml:8-9`).

The launcher already has a client-driver seam. `KnownAgent` is the
whole of what wormhole knows about a client
(`crates/wormhole-core/src/manifest.rs:274-297`):

```
name · command · instructions path · pointer · model_env
credential_files · credential_write · config seed
```

Three entries, claude / codex / grok
(`crates/wormhole-core/src/manifest.rs:299-343`). The comment on the
struct is the project's own rule: "Everything agent-specific lives
here; a call site that compares `agent.run` to a string instead of
asking this table is the bug this table exists to prevent."

Preflight is a role script, seeded into `$HOME/.wormhole/preflight` and
run inside the box before the agent replaces the shell as PID 1
(`crates/wormhole-core/src/manifest.rs:114-119,197-199,646-666`).
Wormhole does not parse it. A failing hook refuses to start the agent.

Box identity is workspace + role source (`CONTEXT.md:33-35`). One
process per box (`CONCEPT.md:129`). MVP ships one session
(`CONCEPT.md:121`). Several agents cooperating in one tree is a goal,
not a ship (`CONCEPT.md:133-148`).

The agent is assumed hostile (`CONCEPT.md:14-15`, `CONTEXT.md:3`).
Policy never comes from the repository (`CONCEPT.md:115`). A fetched
role is confirmed by a person; the payload is otherwise opaque
(`CONCEPT.md:227-229,312`).

## 2. What CONCEPT.md §7 already proposes

> wormhole does not abstract agent config. It transports it.
> (`CONCEPT.md:219`)

> MCP servers, skills, plugins, hooks are Claude Code nouns. Codex has
> `AGENTS.md` and a different shape. Any schema meaning the same thing
> across all of them contains only the intersection. So the contract is:
> `files→paths · setup · argv · env · workdir`
> (`CONCEPT.md:221-225`)

> A role ships each agent's **native files verbatim to real container
> paths** and declares how to launch it. wormhole knows nothing about
> MCP. Agent-agnostic at the launcher; agent-specific in the role.
> (`CONCEPT.md:227`)

> **Cost, named honestly:** this forfeits validation that a
> launcher-native schema can do — checking that `plugin@marketplace`
> actually exists in that marketplace before you ship the role.
> wormhole cannot do that, ever, because the payload is opaque bytes.
> (`CONCEPT.md:229`)

Layout: one manifest, per-agent sections `[agents.claude]`,
`[agents.codex]`, shared persona, native files under `files/claude/`
and `files/codex/` (`CONCEPT.md:231-277`). "One box provisions N
agents (§4), so one box needs one provisioning spec"
(`CONCEPT.md:279`).

`files` is post-mount materialisation with an explicit `dst` and a
per-entry `mode` (`overwrite` vs `seed`). That is the correction of
spike #12: baking role files into a read-only layer that `$HOME` later
covers made every file in the canonical example invisible
(`CONCEPT.md:281-290,512-520`). Class of bug: *role config silently
absent, no error, the agent runs with default behaviour.*

§7 is labelled "the target, not what ships. Today's `wormhole.toml` has
no `files/` tree and one agent per box" (`CONCEPT.md:244-246`).

A second CONCEPT claim is already false of the tree: spike #13 said
"*nothing* installs at box start, because there is no runtime hook —
deterministic dependencies have exactly one home, the role layer"
(`CONCEPT.md:524`). The tree has `[agent] preflight` and three roles
that install the agent binary, skills, plugins and MCP at every start
(`roles/alphaca/hooks/preflight.sh`,
`roles/alphaca-codex/hooks/preflight.sh`,
`roles/alphaca-grok/hooks/preflight.sh`). The hook exists because those
things live in the kept `$HOME`, not in the image.

## 3. Prior research in this repo, already decided and still true

**Codex research** (`docs/research/codex-support.md`): one role per
client; one client-agnostic driver layer inside wormhole. Five reasons
a single role that runs either client is the wrong identity
(`codex-support.md:167-201`):

1. The image is client-specific. A dual-client recipe churns when
   *either* client releases; approval is per recipe commit.
2. Access is client-specific. A dual role grants both credential files
   to every box.
3. Env and model are client-specific.
4. A launch-time `--agent` switch is the override system the manifest
   refuses (`docs/src/guide/roles.md:64-67`).
5. Two clients sharing one home is not continuity — state dirs,
   histories and logins are disjoint.

What *is* client-neutral: the persona markdown, most packages, the
access *shape*. That is one file and some TOML. Role duplication was
accepted: a manifest names one instructions file
(`codex-support.md:203-211`).

**Grok research** (`docs/research/grok-support.md`): a third agent cost
one registry entry plus making `config` optional. Skills go in through
`npx skills add … -a grok`; context7 through `grok mcp add`; rtk is
aimed at `~/.grok/rules` because grok has no rewrite hook. Plugins
were *not* used: grok 1.0.24 accepted a Claude marketplace repository
and then found no plugins in it, because it did not read
`.claude-plugin/marketplace.json` (`grok-support.md:146-150`).
Credential `share` is a per-agent fact: grok saves a login by rename,
a bind is a mount point, rename returns `EBUSY`
(`grok-support.md:158-184`, `CredentialWrite::Replace` at
`crates/wormhole-core/src/manifest.rs:264-271`).

The persona is now in three role directories. A composable
`instructions` list cannot reach another role's `ROLE.md`; a role is
published as its own repository (`grok-support.md:196-203`).

## 4. Per-client: what is a role fact, what is an agent fact

"Role-specific" here means: the *content* a persona wants (conventions,
toolchain, which MCP, which skills). "Agent-specific" means: the *file,
path, CLI, or flag* that delivers that content to this client.

### Instructions (persona)

| Client | File the client reads | Global / user | Project | Import / compose |
|---|---|---|---|---|
| Claude Code | `CLAUDE.md`, not `AGENTS.md` | `~/.claude/CLAUDE.md` | `./CLAUDE.md` or `./.claude/CLAUDE.md` | `@path` import, max 4 hops. Official recipe for AGENTS.md: a `CLAUDE.md` whose first line is `@AGENTS.md` ([code.claude.com/docs/en/memory](https://code.claude.com/docs/en/memory)) |
| Codex CLI | `AGENTS.md` / `AGENTS.override.md` | `~/.codex/AGENTS.md` (`$CODEX_HOME`) | repo-root-to-cwd walk, one file per directory, closer later, default 32 KiB cap | no import syntax; fallback names via `project_doc_fallback_filenames` ([learn.chatgpt.com/docs/agent-configuration/agents-md](https://learn.chatgpt.com/docs/agent-configuration/agents-md)) |
| Grok Build | `AGENTS.md`, `Agents.md`, `AGENT.md`, `CLAUDE.md`, `Claude.md`, `CLAUDE.local.md`, plus every `*.md` in `.grok/rules/` (and `.claude/rules/`, `.cursor/rules/` for compatibility) | `~/.grok/` and `~/.grok/rules/` | cwd-to-root walk; startup loading requires `--trust` | no size cap stated ([docs.x.ai/build/features/project-rules](https://docs.x.ai/build/features/project-rules), [docs.x.ai/build/features/skills-plugins-marketplaces](https://docs.x.ai/build/features/skills-plugins-marketplaces)) |
| Cursor | `AGENTS.md` (plain markdown) *or* `.cursor/rules/*.mdc` (frontmatter) | User Rules in the dashboard | project root and nested `AGENTS.md`; `.cursor/rules` | nested AGENTS.md combined, more specific wins ([cursor.com/docs/context/rules](https://cursor.com/docs/context/rules)) |
| Gemini CLI | `GEMINI.md` by default | `~/.gemini/GEMINI.md` | workspace + parents; JIT on tool access | `@file.md` imports; `context.fileName` can be `["AGENTS.md", …]` ([geminicli.com/docs/cli/gemini-md](https://geminicli.com/docs/cli/gemini-md/)) |
| Aider | whatever you `--read` | n/a | n/a | configure `read: AGENTS.md` in `.aider.conf.yml` ([aider.chat/docs/usage/conventions.html](https://aider.chat/docs/usage/conventions.html), [agents.md FAQ](https://agents.md)) |

AGENTS.md the *standard* is: a markdown file at the repo root (and
nested), no required fields, no schema. Closest file to the edited
path wins; user chat overrides everything. Stewarded by the Agentic AI
Foundation under the Linux Foundation
([agents.md](https://agents.md), [github.com/agentsmd/agents.md](https://github.com/agentsmd/agents.md)).
Claude Code is the documented exception: it reads CLAUDE.md.

Wormhole already exploits the portable half: composed instructions land
once as `~/AGENTS.md` in the box home; `.claude/CLAUDE.md` holds
`@~/AGENTS.md`; `.codex/AGENTS.md` and `.grok/rules/AGENTS.md` are
symlinks (`crates/wormhole-core/src/manifest.rs:304-338`,
`docs/research/codex-support.md:279-286`).

### Skills

The *file* is converging. The *path* is not.

Agent Skills spec: a directory with `SKILL.md`; YAML frontmatter must
include `name` and `description`; optional `scripts/`, `references/`,
`assets/`. Progressive disclosure: metadata at startup, body on
activation ([agentskills.io/specification](https://agentskills.io/specification),
[agentskills.io/home](https://agentskills.io/home)). Originally
Anthropic, now an open standard. Codex "build[s] on the open agent
skills standard" ([learn.chatgpt.com/docs/build-skills](https://learn.chatgpt.com/docs/build-skills)).
Gemini CLI is "based on the Agent Skills open standard"
([github.com/google-gemini/gemini-cli/blob/main/docs/cli/skills.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/skills.md)).

| Client | Discovery paths | How a role installs them today |
|---|---|---|
| Claude Code | `~/.claude/skills/`, `.claude/skills/`, plugin `skills/`, managed | git clone into `~/.claude/skills`, or `claude plugin install` (`roles/alphaca/hooks/preflight.sh:44-81`) |
| Codex CLI | `$CWD/.agents/skills` up to repo root; `$HOME/.agents/skills`; `/etc/codex/skills`; bundled | `npx -y skills add <repo> --skill '*' -a codex` (`roles/alphaca-codex/hooks/preflight.sh:53-65`); `$skill-installer` is Codex's own ([learn.chatgpt.com/docs/build-skills](https://learn.chatgpt.com/docs/build-skills)) |
| Grok Build | `./.grok/skills/` (walked up), `~/.grok/skills/`, plugin `skills/`, `[skills] paths`, plus `~/.agents/skills/` | `npx -y skills add <repo> --skill '*' -a grok` (`roles/alphaca-grok/hooks/preflight.sh:55-67`) |
| Cursor | `.cursor/skills/`, `.agents/skills/`, `.claude/skills/`, `.codex/skills/`, and the `~/` of each | UI / marketplace; also reads Claude and Codex skill dirs ([cursor.com/docs/context/skills](https://cursor.com/docs/context/skills)) |
| Gemini CLI | `~/.gemini/skills/` or `~/.agents/skills/`; `.gemini/skills/` or `.agents/skills/`; extension-bundled | `gemini skills install <git-url> --consent` ([gemini-cli skills.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/skills.md)) |
| Aider | none | n/a |

`.agents/skills/` is the one path several clients have agreed to
(Codex, Gemini, Cursor, Grok). It is still a *path convention*, not a
launcher schema.

Grok ignores extra SKILL.md keys including `model`, `effort`,
`license`, `compatibility`
([docs.x.ai/build/features/skills-plugins-marketplaces](https://docs.x.ai/build/features/skills-plugins-marketplaces)).
Claude Code skills can register hooks in frontmatter
([code.claude.com/docs/en/skills](https://code.claude.com/docs/en/skills)).
Codex skills can declare MCP dependencies in `agents/openai.yaml`
([learn.chatgpt.com/docs/build-skills](https://learn.chatgpt.com/docs/build-skills)).
Those extras are agent-specific. Copying a SKILL.md body is portable;
copying the extras is not.

### MCP

MCP is a protocol ([modelcontextprotocol.io](https://modelcontextprotocol.io/introduction)).
It is not a config file. Every client stores servers in a different
place, with a different CLI, and a different trust story.

| Client | Config file | CLI | Trust / approval |
|---|---|---|---|
| Claude Code | project `.mcp.json`; user/local in `~/.claude.json`; plugin `.mcp.json` | `claude mcp add --transport http\|stdio …` scopes `local` / `project` / `user` | project `.mcp.json` prompts in interactive sessions; skipped in `-p` / SDK / `bypassPermissions`. Workspace trust gates committed approvals ([code.claude.com/docs/en/mcp](https://code.claude.com/docs/en/mcp)) |
| Codex CLI | `[mcp_servers.<name>]` in `~/.codex/config.toml` or project `.codex/config.toml` (trusted projects only) | `codex mcp add` | project config loads only when the project is trusted ([developers.openai.com/codex/mcp](https://developers.openai.com/codex/mcp), [config-reference](https://developers.openai.com/codex/config-reference)) |
| Grok Build | `[mcp_servers.<name>]` in `~/.grok/config.toml`; project `.grok/config.toml` is limited to MCP, plugins, permission rules | `grok mcp add` / `grok mcp add --transport http` | project MCP is in the project file; see settings scopes ([docs.x.ai/build/settings](https://docs.x.ai/build/settings), [docs.x.ai/developers/docs-mcp](https://docs.x.ai/developers/docs-mcp)) |
| Cursor | `.cursor/mcp.json` (project), `~/.cursor/mcp.json` (user) | `agent mcp enable/disable`; editor Customize | tool approval; enterprise allowlist ([cursor.com/docs/context/mcp](https://cursor.com/docs/context/mcp)) |
| Gemini CLI | `mcpServers` in `~/.gemini/settings.json` or `.gemini/settings.json`; also inside `gemini-extension.json` | edit JSON; extensions bundle servers | settings.json wins over an extension of the same name ([geminicli.com/docs/extensions](https://geminicli.com/docs/extensions)) |
| Aider | none in the conventions docs | n/a | n/a |

A stdio server is "a command and args." An HTTP server is "a URL." That
is the intersection. Header names, env expansion (`${VAR}` vs
`${VAR:-default}` vs `${env:NAME}`), OAuth, scopes, `timeout` keys,
and whether a missing `type` on a URL entry is a stdio server (Claude
Code: yes, and it is a misconfiguration it now errors on) are
client-specific ([code.claude.com/docs/en/mcp](https://code.claude.com/docs/en/mcp)).

Alphaca's three preflights all add context7, three different ways:

```
claude plugin install context7@claude-plugins-official
codex mcp add context7 -- npx -y @upstash/context7-mcp
grok mcp add context7 -- npx -y @upstash/context7-mcp
```

(`roles/alphaca/hooks/preflight.sh:73-80`,
`roles/alphaca-codex/hooks/preflight.sh:85-95`,
`roles/alphaca-grok/hooks/preflight.sh:85-95`). Same MCP server, three
native CLIs. The role, not the launcher, knows which.

### Plugins, extensions, marketplaces

No shared format.

| Client | Package | Manifest | Install |
|---|---|---|---|
| Claude Code | plugin: skills, agents, hooks, MCP, LSP, monitors | `.claude-plugin/plugin.json`; components at plugin root, *not* inside `.claude-plugin/` | `claude plugin marketplace add`; `claude plugin install name@marketplace`; `--plugin-dir` for local ([code.claude.com/docs/en/plugins](https://code.claude.com/docs/en/plugins)) |
| Codex | plugin: skills, apps, MCP | Codex/ChatGPT plugin directory | `/plugins` in TUI; `[plugins."name@mp"]` in config.toml ([developers.openai.com/codex/plugins](https://developers.openai.com/codex/plugins), [learn.chatgpt.com/docs/build-skills](https://learn.chatgpt.com/docs/build-skills)) |
| Grok Build | plugin: skills, agents, hooks, MCP, LSP | loaded from `./.grok/plugins/`, `~/.grok/plugins/`, marketplaces | `grok plugin install <name> --trust`; `grok plugin marketplace add` ([docs.x.ai/build/features/skills-plugins-marketplaces](https://docs.x.ai/build/features/skills-plugins-marketplaces), [x.ai/news/grok-plugin-marketplace](https://x.ai/news/grok-plugin-marketplace)) |
| Gemini CLI | extension | `gemini-extension.json` (`name`, `version`, `mcpServers`, `contextFileName`, `excludeTools`) | `gemini extensions install <github-url>` ([geminicli.com/docs/extensions](https://geminicli.com/docs/extensions)) |
| Cursor | Cursor plugin *or* "Agent Plugins" open standard | `.cursor-plugin/plugin.json` (rules, agents, commands, hooks, MCP) *or* `plugin.json` at root (skills + MCP only) | marketplace / Customize ([cursor.com/docs/reference/plugins](https://cursor.com/docs/reference/plugins)) |

Grok's current docs claim "fully compatible with Claude Code with zero
configuration" and that it "automatically reads Claude Code
marketplaces, plugins, skills, MCPs, agents, hooks, and instruction
files" ([docs.x.ai/build/features/skills-plugins-marketplaces](https://docs.x.ai/build/features/skills-plugins-marketplaces)).
The grok role research, against grok 1.0.24, found the opposite for
marketplaces: `plugin marketplace add` accepted
`JuliusBrussee/caveman` and then found no plugins, because it did not
read `.claude-plugin/marketplace.json` (`grok-support.md:146-150`).
Both can be true at different dates. That is the point: compatibility
claims move. A launcher that encoded "Claude marketplace JSON" as a
grok fact would have been wrong in 1.0.24 and might be redundant now.
The role's preflight, calling grok's own CLI, is what tracks the
binary.

### Hooks

Hooks are shell (or HTTP) at a lifecycle event. Event names, JSON
stdin, matcher language, and where the file lives are per client.

| Client | Where | Events (sample) |
|---|---|---|
| Claude Code | `~/.claude/settings.json`, `.claude/settings.json`, plugin `hooks/hooks.json`, skill/agent frontmatter | `PreToolUse`, `PostToolUse`, `Stop`, `InstructionsLoaded`, … ([code.claude.com/docs/en/hooks](https://code.claude.com/docs/en/hooks), [plugins](https://code.claude.com/docs/en/plugins)) |
| Codex | Codex hooks (developer docs) | not the same set; the alphaca-codex role treats rtk as a rules file because "codex has no PreToolUse" (`roles/alphaca-codex/wormhole.toml:6-7`, `hooks/preflight.sh:67-72`) |
| Grok Build | `~/.grok/hooks/`, project `.grok/hooks/` (requires `/hooks-trust`), plugins | tool and session lifecycle; plugin hooks get `GROK_PLUGIN_ROOT` ([docs.x.ai/build/features/skills-plugins-marketplaces](https://docs.x.ai/build/features/skills-plugins-marketplaces)) |
| Gemini CLI | extension `hooks/hooks.json`; `hooks` in settings.json | intercept CLI behaviour ([github.com/google-gemini/gemini-cli/blob/main/docs/extensions/reference.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/extensions/reference.md)) |
| Cursor | plugin `hooks/hooks.json` | bundled with plugins ([cursor.com/docs/reference/plugins](https://cursor.com/docs/reference/plugins)) |
| Aider | none | n/a |

rtk is the exhibit. One tool, three deliveries, because the hook
surface is not shared:

- claude: `rtk init -g --auto-patch` (PreToolUse rewrite)
  (`roles/alphaca/hooks/preflight.sh:58`)
- codex: `rtk init -g --codex` (rules file, append to AGENTS.md)
  (`roles/alphaca-codex/hooks/preflight.sh:76`)
- grok: `CODEX_HOME=$HOME/.grok/rules rtk init -g --codex` (same
  rules file, aimed at the directory grok reads every `*.md` from)
  (`roles/alphaca-grok/hooks/preflight.sh:76`)

A launcher that "knew about rtk" would have to know all three. The
role's hook already does.

### Launch flags, config seed, credentials

These are the facts `KnownAgent` already holds, and they are not
payload:

| Client | Bypass / trust flags | First-run file | Credential file | How it saves a login |
|---|---|---|---|---|
| claude | `--dangerously-skip-permissions` | `.claude.json` (onboarding, bypass warning, trust) | `.claude/.credentials.json` | InPlace (assumed) |
| codex | `--dangerously-bypass-approvals-and-sandbox` | `.codex/config.toml` (`projects."<ws>".trust_level`, model) | `.codex/auth.json` | InPlace (verified) |
| grok | `--always-approve --trust` | none | `.grok/auth.json` | Replace (verified; `share` is `EBUSY`) |

(`crates/wormhole-core/src/manifest.rs:239-343`). Model: env var
(`ANTHROPIC_MODEL`, `GROK_DEFAULT_MODEL`) or a config key (codex).
Codex docs reserve the bypass flag for an isolated runner, which the
box is ([developers.openai.com/codex/agent-approvals-security](https://developers.openai.com/codex/agent-approvals-security)
via `codex-support.md:103-113`). Grok project rules do not load
without `--trust` ([github.com/xai-org/grok-build/…/12-project-rules.md](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/12-project-rules.md)
via `grok-support.md:55-62`).

Settings files (`~/.claude/settings.json`, `~/.codex/config.toml`,
`~/.grok/config.toml`, `~/.gemini/settings.json`) are agent-owned.
CONCEPT.md: " `~/.claude.json` and session state are never
role-owned. The agent writes them; declaring them would fight it every
boot" (`CONCEPT.md:292`). `mode = "seed"` is the exception for files
the human edits.

### Binary install

Agent-specific, and currently done in preflight into the kept home, not
in the image:

- claude: `curl https://claude.ai/install.sh | bash` then `claude update`
  (`roles/alphaca/hooks/preflight.sh:12-26`)
- codex: GitHub musl tarball `codex-$target.tar.gz`
  (`roles/alphaca-codex/hooks/preflight.sh:16-37`)
- grok: `https://x.ai/cli/install.sh` / `https://x.ai/cli/stable`
  (`roles/alphaca-grok/hooks/preflight.sh:16-39`)

CONCEPT.md §6 wanted a shared, version-keyed, read-only *agent layer*
installed once (`CONCEPT.md:199-201`). The tree does not have that
layer. The hook is what exists.

## 5. The common intersection, stated as a set

Portable without translation:

- Plain markdown persona (AGENTS.md body). Delivery path and pointer
  type are per-agent, already in `KnownAgent`.
- A `SKILL.md` directory that only uses `name` + `description` + body.
  Extra frontmatter and companion files (`agents/openai.yaml`, skill
  hooks) are not.
- The *idea* of an MCP server: a name, and either a command+args or a
  URL. The file that records it, the CLI that writes it, env-expansion
  syntax, and the trust prompt are not.
- Packages in the image, a POSIX `setup`/`preflight` script, argv, env,
  workdir.

Not portable, and not a launcher schema:

- Plugin / marketplace / extension manifests.
- Hook event tables and JSON contracts.
- First-run trust dialogs and permission flags (already registry).
- Credential file layout and write semantics (already registry).
- Settings keys (`permissions`, `approval_policy`, `sandbox_mode`,
  `enabledPlugins`, `mcpServers` vs `mcp_servers`).
- How each client names "the same" MCP server when it arrives via a
  plugin (`mcp__plugin_<plugin>_<server>__<tool>` in Claude Code
  ([code.claude.com/docs/en/mcp](https://code.claude.com/docs/en/mcp))
  vs a TOML table key in Codex/Grok).

CONCEPT.md §7's sentence is exact: "Any schema meaning the same thing
across all of them contains only the intersection"
(`CONCEPT.md:221-222`). The intersection is too small to be an MCP
schema, a skills schema, or a plugins schema. It is a transport
schema.

## 6. Analogies that actually match

### Dev Container Features — the closest "role vs runtime"

A Feature is `devcontainer-feature.json` + `install.sh`, executed as
root at image build ([containers.dev/implementors/features](https://containers.dev/implementors/features/)).
The Feature does not name a container runtime. Product-specific IDE
config lives under `customizations`: "each namespace under
`customizations` is treated as a separate set of properties." The
orchestrator unions arrays and replaces values; it does not interpret
the namespace. That is CONCEPT §7's `files→paths` plus opaque
per-agent sections, under another name.

Features are also the warning. `install.sh` runs as root with no
approval prompt; `:latest` is implicit; digests exist for identity, not
trust (`docs/research/role-ux-prior-art.md:788-809`). Wormhole's
commit-keyed role approval is the stronger half of that split and
should stay.

Instance identity in the devcontainer CLI is
`(workspace folder, config file)` — the same tuple as wormhole's
`(workspace, role)` (`role-ux-prior-art.md:45-46`,
`role-ux-review.md:516-523`). The config file is the recipe, not the
runtime.

### Kubernetes RuntimeClass — workload vs how it runs

A Pod names a `runtimeClassName`. The RuntimeClass object has two
significant fields: a name and a `handler` that identifies a CRI
configuration on the node
([kubernetes.io/docs/concepts/containers/runtime-class](https://kubernetes.io/docs/concepts/containers/runtime-class/)).
The workload spec does not describe runc vs gVisor vs Kata. The
cluster admin owns runtimes; the app owner owns the Pod.

Mapping: role ≈ Pod spec (image, env, mounts, command). Agent ≈
RuntimeClass (how the process is actually started). Wormhole's
`KnownAgent` *is* the RuntimeClass table. Putting MCP/skills into
`KnownAgent` would be putting a Sidecar spec into the CRI handler.

### Nix package vs profile

A Nix profile is "a set of packages that can be installed and upgraded
independently from each other. Nix profiles are versioned, allowing
them to be rolled back easily"
([nix.dev/manual/nix/stable/command-ref/new-cli/nix3-profile](https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-profile.html)).
The package is a derivation; the profile is a composed user
environment, a tree of symlinks.

Mapping: the agent binary (and rtk, rustup) are packages. The box
`$HOME` is the profile: logins, skills, MCP config, history. A role is
closer to a profile *recipe* than to a package. Nix does not make
`nix` itself understand VS Code extensions; `install.sh`-like
activation is in the package. Wormhole should not make `wormhole`
understand `claude plugin`.

## 7. What a launcher can safely own vs what must stay opaque

Safe to own in wormhole (small, stable, per-agent, already
`KnownAgent` or about to be):

- argv / bypass flags
- where the agent reads instructions, and how the pointer is spelled
  (import vs symlink)
- how `model` is passed (env vs config key)
- credential file paths and `CredentialWrite`
- which first-run file a start may seed, and only the keys a headless
  box cannot answer (trust, onboarding)
- `files→paths` as *transport*: `src`, `dst`, `mode`. No interpretation
  of the bytes
- running an opaque `setup` / `preflight` and refusing to start if it
  fails
- env and workdir, which are already manifest fields

Must stay opaque bytes, in the role, installed by the agent's own CLI
or dropped at a native path:

- MCP server entries
- plugin / marketplace / extension state
- skills beyond "here is a directory, put it at `$HOME/.…/skills/X`"
- hooks
- `settings.json` / `config.toml` keys other than the first-run seed
- session transcripts, auto-memory, auth tokens the agent refreshes

A launcher that grows a `[mcp]` table, a `[plugins]` table, or a
skills-installer of its own is C, and it is the thing the user is
right to fear. It would duplicate `claude mcp add` / `codex mcp add` /
`grok mcp add`, lag every CLI change, and still not validate a
marketplace that only the agent can see (`CONCEPT.md:229`).

Dropping a native file at a native path (`files→paths`) is transport.
Calling `codex mcp add` from preflight is also transport: the role
owns the command, the agent owns the file it writes. Both are B's
adapter. Neither is C.

---

# RECOMMENDATIONS

## 8. The four models against wormhole's constraints

Constraints, from the project, not from taste:

| Constraint | Source |
|---|---|
| Agent is hostile | `CONCEPT.md:14-15`, `CONTEXT.md:3` |
| Payload is opaque; wormhole does not abstract agent config | `CONCEPT.md:219-229` |
| Preflight / setup is how skills, plugins, MCP actually land | `manifest.rs:114-119`; three `hooks/preflight.sh` |
| One session per box is the MVP | `CONCEPT.md:121` |
| N agents in one box is a later goal, sibling namespaces | `CONCEPT.md:133-148` |
| A role is published as its own repository, self-contained | `docs/src/guide/roles.md:15-16`, `grok-support.md:196-203` |
| Box identity is workspace + role source, one process per box | `CONTEXT.md:33-35`, `CONCEPT.md:129` |
| Launch-time override of the recipe is refused | `docs/src/guide/roles.md:64-67` |
| Image, credentials, env, model are client-specific | `codex-support.md:172-191` |

### A — bind one role to one agent

This is what ships. `run = "claude"` is a field, not a flag. Three
alphaca directories.

Holds: hostile-agent isolation (one credential file granted, not
three); opaque payload (preflight stays a script); one-session MVP
(the role names the one process that will be PID 1); box identity
(switching client is a new role, so a new home, which is honest
because the homes are disjoint).

Breaks later: N agents in one box. One role cannot provision two
clients if the field is singular. Persona is copied N times; that is
already named as a standing cost, and a list of `instructions` files
*inside one role* is the fix that does not require B
(`codex-support.md:207-211`). A list still cannot reach another
role's `ROLE.md`.

A is not a workaround. For one session it is the identity that matches
the image, the credential, the preflight, and the home.

### B — roles agent-agnostic, per-agent adapters in the role

This is CONCEPT.md §7. One directory, shared persona, `[agents.claude]`
/ `[agents.codex]` with `files`, `argv`, `env`, `workdir`. Launcher
transports; role is specific.

Holds: opaque payload, if "adapter" means native files plus native
CLI in `setup`, not a wormhole-shaped MCP schema. N agents later: one
box, one provisioning spec (`CONCEPT.md:279`). Persona is one file.

Breaks now: one-session MVP. A role that names two agents still bakes
one image (`codex-support.md:173-179`). Approval is per recipe, so a
codex bump re-asks every claude-only user of that role. A dual role
that *grants* both credential files doubles exfil surface
(`codex-support.md:180-186`). A launch-time `--agent` pick is the
override system the manifest refuses. Two clients in one `$HOME` is
the corruption `CONCEPT.md:129` forbids (two agents writing one
history) unless N sessions with sibling namespaces have shipped — and
they have not.

B's layout (shared persona, per-agent native files, explicit `dst`) is
still the right *internal* shape for a single-agent role that wants
to stop copying ROLE.md, and it is the right shape the day one box
runs two sessions. It is not the right *published-role identity*
while a box runs one process.

### C — agent manifests inside the launcher

`KnownAgent` is C-thin and it is correct: launch facts a start must
get right or the agent is not the role (trust flags, instruction
pointer, credential write). Widening it to MCP/skills/plugins is
C-fat.

C-fat fails the opaque-payload rule by construction
(`CONCEPT.md:229`). It fails preflight: those scripts exist *because*
the agent's CLI is the only API that writes a config the agent will
read. It fails hostility: a launcher that "sets up MCP" has to know
secrets and endpoints the box was meant to fetch for itself. It fails
N-agents-later: every new client is a wormhole release, not a role
commit. The grok marketplace episode (`grok-support.md:146-150` vs
current grok docs claiming Claude compatibility) is what C-fat looks
like when the vendor moves.

The user worry names C-fat. Do not do it.

### D — the split

Three layers, each already in the tree or in CONCEPT, none allowed to
absorb the others:

1. **Launcher / C-thin.** `KnownAgent` stays the RuntimeClass table:
   argv, instruction path and pointer, model plumbing, credential
   files and write mode, first-run seed. Adding a client is adding a
   row. No MCP. No skills. No plugins. No hooks. The comment on the
   struct stays the law (`manifest.rs:274-276`).

2. **Published role / A, until N sessions ship.** A role names one
   `run`. Image, grants, env, preflight, and home match that client.
   `alphaca`, `alphaca-codex`, `alphaca-grok` remain three roles. The
   persona may be one file *inside* a role (an `instructions` list, or
   `files→paths` from `shared/persona.md` into the client's native
   path). It may not be a second role.

3. **In-role payload / B's contract, not B's identity.** The role
   ships native files to native destinations (`src`/`dst`/`mode`,
   post-mount — spike #12 is load-bearing, `CONCEPT.md:512-520`) and
   an opaque hook that calls `claude plugin install`, `codex mcp add`,
   `npx skills add -a grok`, `rtk init --codex`, whatever the client
   actually understands this week. Wormhole copies bytes and runs the
   hook. It does not learn what a marketplace is.

When §4 ships (sibling namespaces, N sessions in one box), layer 2
becomes B's identity: one role, one image, N `[agents.X]` blocks, N
homes, N credential files, one tree. Not before. The transport
contract does not have to wait; the identity does.

Preflight stays. CONCEPT's "no runtime hook" (`CONCEPT.md:524`) is
already false of the tree, and it should stay false for anything the
agent writes into `$HOME` (MCP, plugins, logins, skill installs that
the client's CLI records). Image/`setup` is for the toolchain that
can be frozen. `$HOME`/preflight is for the payload that cannot.
Mixing those is how spike #12 happened in reverse: baking MCP config
into a layer `$HOME` then covers.

## 9. Direct answers to the worry and to §7

**"Tools, skills, plugins are set up in preflight; moving agents into
the tool may break that."** Correct, if "into the tool" means C-fat.
The agent *client* is already in the tool as a `KnownAgent` row. That
row does not install context7. The hook does. Keep it that way. A
future `files→paths` that drops a `SKILL.md` at `~/.codex/skills/…`
is extra transport, not a replacement for `codex mcp add`.

**§7's one-manifest-N-agents.** Right contract, wrong moment for the
identity. One box does not provision N agents while MVP is one
session. Shipping `[agents.claude]` beside `[agents.codex]` in a role
that `run`s one of them is dead weight in the image and in the
approval diff. When a box has two sessions it becomes the spec; the
launcher still does not learn MCP.

**Persona duplication.** Real, and not solved by B's identity. Solved
by one markdown file and per-agent pointers, which the tree already
does at `~/AGENTS.md`. An `instructions` list inside one role is the
remaining step and does not require a second agent.

## 10. Pre-attack

- **D has three moving parts and looks like a compromise.** It is not
  averaging A/B/C. A, B, and C each describe one layer that already
  exists. The failure mode is letting any layer eat the others: A
  forever (persona copied, N-agents blocked), B now (image and
  credentials lie), C-fat (preflight dies, schema lags).
- **`files→paths` without the hook cannot replace preflight.** Plugins
  and MCP are often not files a role can vendor: they are CLI
  transactions (`claude plugin install`, OAuth, marketplace pins). A
  role that only drops `.mcp.json` still has to match the client's
  current schema and trust rules (Claude Code: workspace trust, skipped
  prompts in `-p`). The hook calling the CLI is the adapter that
  survives that. Transport of files is for SKILL.md trees and CLAUDE.md
  wrappers.
- **A `setup` at image-build and a `preflight` at start are two
  different times.** CONCEPT conflates them as `setup`. The tree split
  them, correctly, because `$HOME` is a mount. Do not fold preflight
  back into the image to make §7 look shipped.
- **Grok's Claude-compatibility claim may make some adapters thinner
  over time.** That is a role change (stop using `-a grok`, start
  dropping files in `.claude/` and letting grok read them), not a
  launcher change. Verify against the binary, as `grok-support.md` did.
  Docs and binaries have already disagreed once.
- **Cursor and Gemini as first-class `run` values are not free.**
  Cursor is an editor with a CLI (`agent`); Gemini CLI's public docs
  now say it was replaced by Antigravity CLI on 2026-06-18
  ([geminicli.com/docs/cli/gemini-md](https://geminicli.com/docs/cli/gemini-md/)).
  Each new row is still a `KnownAgent` plus a role, under A. Do not
  wait for B to add one.
- **Hostile agent vs opaque hook.** Preflight runs as the box user, on
  the host network, before the agent starts. It is part of the role
  the human approved. It is not a hole in the threat model *beyond*
  the role itself. Making wormhole interpret the hook so it could
  "validate MCP" would be a hole: the launcher would be parsing
  attacker-shaped config. Leave it opaque.
- **One box, N agents, one image** still grants every credential the
  role names to a tree those agents share, unless sibling mount
  namespaces hide each home (`CONCEPT.md:140-144`). B's identity
  without §4's namespaces is one injected session owning the others'
  logins. That is why B waits on §4, not on a TOML bikeshed.
