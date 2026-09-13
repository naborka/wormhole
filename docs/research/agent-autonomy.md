# Agent autonomy: what still stops an agent in a box, and what answers it

The question: a box is the boundary, so the CLI inside should never stop to
ask permission, never sandbox itself, never wait on a first-run or trust
dialog, and never be moved to a weaker model. The launch flags
(`claude --dangerously-skip-permissions`, `codex
--dangerously-bypass-approvals-and-sandbox`, `grok --always-approve
--trust`) were all wormhole passed. What was still left, and what answers
it?

Answer up front: **the flags reach only the process wormhole starts.** A
resumed session, a background worker, a CLI typed in an attached shell and a
mode toggled in a past session all read the CLI's own config files, and
wormhole wrote none of them except Claude's `.claude.json` and Codex's
trust. Each `KnownAgent` now carries its config files and env
(`crates/wormhole-core/src/manifest.rs`), so every role gets the same
answers without writing a line.

Checked 2026-09-12 against Claude Code 2.1.269, and on 2026-09-13 against
2.1.270 (installed binaries, grepped with `rg -a -o`), and the 2.1.246 and
2.1.259 npm packages roles pin; Codex
0.154.0 (source at tag `rust-v0.154.0`, npm binary); Grok 1.0.30 (release
binary) and `xai-org/grok-build` at `37949780`. Code references for Codex
are relative to `codex-rs/`, for Grok to `crates/codegen/`. "Inference"
marks reasoning, not a source.

## The trigger: a safeguard flag moved a box to an older model

A box session logged:

> Opus 5 (1M context)'s safeguards flagged this message. [...] Switched to
> Opus 4.8. Details: `[cyber]`

(system message subtype `model_refusal_fallback`, scope `session`.)

The flag is Anthropic's real-time safeguard, decided on the server. No flag,
setting or variable in the client changes it, and wormhole does not try.
The client decides only what happens next:

| Configuration | Main thread | Subagents | Source |
|---|---|---|---|
| `switchModelsOnFlag` unset or `true` (default) | whole session switched, stays switched until `/model` | that response switched | settings schema: "When safeguards flag a message, automatically switch to a different model to keep chatting. When off, your session will pause instead." |
| `switchModelsOnFlag: false` | a dialog with no timeout in the local terminal | **still switch**: the subagent branch returns before the setting is read | binary `lFe()`: `if(!e.isMainThread)return"subagent"` before `go("switchModelsOnFlag")` |
| `CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK` set | the turn ends with the refusal | the refusal returns to the parent, which keeps its model | binary `gM(){return!a.CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK&&!Dte()}` |

Chosen: the variable, set in every claude box, plus `switchModelsOnFlag =
false` written where the file has none, because the variable is
undocumented and the key is what holds if a build stops reading it. Cost: a
flagged turn stops instead of carrying on with an older model. A role takes
it back with `[env.CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK] fixed = ""`.

What reduces the flags themselves is Anthropic's Cyber Verification
Program, applied for by an organization admin; it lifts "High Risk Dual
use" blocks for that organization, never "Prohibited use"
(<https://support.claude.com/en/articles/14604842-real-time-cyber-safeguards-on-claude>).
The binary's own notice: "Apply to the Cyber Verification Program to
reduce these interruptions." OpenAI's equivalent for Codex is Trusted
Access for Cyber (<https://chatgpt.com/cyber>). No client handling or
program was found for Grok.

## What every box now gets

### Claude Code

| Where | Key | What it removes | Evidence |
|---|---|---|---|
| `.claude/settings.json` | `skipDangerousModePermissionPrompt = true` | the bypass warning dialog, and a background worker silently downgraded to default mode | 2.1.269 `O(e)`: "Permission mode downgraded to default — bypass requires accepting the disclaimer interactively first", reads user or policy settings only; 2.1.259 `het()` the same |
| `.claude/settings.json` | `permissions.defaultMode = "bypassPermissions"` | a resumed session, an agent-view dispatch or a `claude` typed by hand starting in default mode | permission-modes docs; ignored from project files since 2.1.257 |
| `.claude/settings.json` | `enableAllProjectMcpServers = true` | the approval dialog for a workspace `.mcp.json` outside bypass | mcp docs |
| `.claude/settings.json` | `switchModelsOnFlag = false`, only where unset | see above | settings schema |
| `.claude.json` | `bypassPermissionsModeAccepted = true` | the same bypass gate on older builds; 2.1.269 migrates it into settings on start | 2.1.269 migration writes `skipDangerousModePermissionPrompt` and deletes the key |
| `.claude.json` | `hasCompletedOnboarding`, `theme` (only where unset) | onboarding | N6b |
| `.claude.json` | `projects[ws].hasTrustDialogAccepted`, `hasCompletedProjectOnboarding` | workspace trust, which bypass does not skip | binary `Xge()` |
| `.claude.json` | `projects[ws].hasClaudeMdExternalIncludesApproved` + `WarningShown` | a dialog for a `CLAUDE.md` importing outside the workspace; unanswered, the imports are silently not loaded | binary dialog gate; both keys needed |
| `.claude.json` | `hasAcknowledgedCostThreshold = true` | the cost-threshold dialog | binary |
| `.claude.json` | `hasSeenAutoDefaultNudge = true` | a one-key offer under the prompt to make auto mode the default, shown because user settings carry a `defaultMode` that is not auto; accepted, a classifier gates the box | 2.1.270 `run(u)`: `o.hasSeenAutoDefaultNudge` checked, then `fe("userSettings")?.permissions?.defaultMode` not `"auto"`; accept writes `defaultMode: "auto"`. Key present in 2.1.246 and 2.1.259 |
| `.claude.json` | `hasSeenEffortMediumNudge = true`, `hasSeenEffortMediumNudgeByModel` `claude-opus-5` and `claude-fable-5-1` = `true` | a one-key offer to drop effort to medium, from `high` on Opus 5 and from `high`, `xhigh` or `max` on Fable 5.1 | 2.1.270 `zQ={"claude-opus-5":{from:["high"]},"claude-fable-5-1":{from:["high","xhigh","max"]}}`, `Wst="medium"`, `qeo()` reads both keys; 2.1.259 reads the flat key only. A later build that names another model needs its key added |
| env | `CLAUDE_CODE_RETRY_WATCHDOG=1` | a run ending on 429/529 or a usage limit instead of waiting | env-vars docs: "Set to `1` for unattended sessions" |
| env | `IS_SANDBOX=1` | a server feature flag forcing Claude's own Bash sandbox on | binary `DTe(){if(!Vue().disableNoSandbox)return!1;return!md()&&!He(a.IS_SANDBOX)&&!Eb.getIsBubblewrapSandbox()}` |
| env | `CLAUDE_CODE_SANDBOXED=1` | the trust dialog for a nested repository the seeded trust does not name | binary `Xge(){if(a.CLAUDE_CODE_SANDBOXED)return!0;...}` |

`IS_SANDBOX` also skips an early "overloaded" failure; retries stay bounded
by the retry count. Every key and variable above is present in 2.1.246.

### Codex

| Where | Key | What it removes |
|---|---|---|
| `.codex/config.toml` | `projects."<ws>".trust_level = "trusted"` | the trust screen, which the flag does not skip |
| `.codex/config.toml` | `approval_policy = "never"`, `sandbox_mode = "danger-full-access"` | approvals and Codex's own sandbox on `codex resume`/`fork` without the flag and app-server threads |
| `.codex/config.toml` | `web_search = "live"` | the `cached` default, an index rather than the web, outside the full-access profile |
| `.codex/config.toml` | `notice.hide_rate_limit_model_nudge = true` | a popup offering a cheaper model near the rate limit |

### Grok

| Where | Key | What it removes |
|---|---|---|
| `.grok/config.toml` | `ui.permission_mode = "always-approve"` | ask mode for `grok agent`, leader-spawned sessions and any start without the flag; the TUI saves a toggled mode to this key, so it is set back each start |
| `.grok/config.toml` | `features.web_fetch = true` | the fetch tool, off by default |
| env | `GROK_FOLDER_TRUST=0` | folder trust for a nested repository or a moved workspace, which `--trust` does not grant; documented in `xai-grok-pager/docs/user-guide/10-hooks.md:81` |

## Read back by the real CLIs

The files a start writes, produced by `manifest::config_writes` and
`seed::config_file`, were put in scratch homes and read by the CLIs
themselves:

- claude 2.1.269, `-p --output-format stream-json`: `permissionMode` was
  `bypassPermissions` with the flag, without it, and without it as a
  background worker (`CLAUDE_CODE_SESSION_KIND=bg`). The legacy key was
  migrated; trust and include approvals stayed.
- codex 0.154.0, `exec --strict-config` without the flag: the header read
  `approval: never` and `sandbox: danger-full-access`.
- grok 1.0.30, `inspect`: the user config loaded, "Project trusted: yes",
  no unrecognized key reported.

## What no setting removes

- **Vendor safeguards.** Server-side for all three. See the trigger above.
- **Claude's critical-path removal prompt.** `rm -rf` on the workspace, a
  parent of it, the home or `/` prompts even in bypass; only a
  `PermissionRequest` hook can answer it. Not answered: the workspace is the
  one thing the box does not protect.
- **A repository's own Claude settings.** A checked-in
  `.claude/settings.json` can set `permissions.disableBypassPermissionsMode`,
  which "takes precedence over --dangerously-skip-permissions", or ask rules,
  or `sandbox.enabled`. `--setting-sources user` ignores such files, but it
  also drops that repository's `.mcp.json` servers and hooks (tested: servers
  gone even with `enableAllProjectMcpServers`). Not passed; a repository that
  restricts its agents says so on purpose.
- **Claude's usage-credit consent.** Past a plan's Fable allowance the
  session pauses on "continue on usage credits or switch models"
  (`fable_overage_consent_prompt`, result `consent`, `switch_default` or
  `cancelled`). Neither answer is the box's to give: one spends money
  outside it, the other is the weaker model this work exists to avoid.
- **Questions the model chooses to ask.** `AskUserQuestion`, plan approval
  (Claude, and Grok's `exit_plan_mode`, which always-approve never answers:
  `tool_calls.rs:1810-1826`), Codex's `request_user_input` in Plan mode.
  Removing those tools removes something the agent can do; they stay.
- **Codex's `rm -f` block.** Under `approval_policy = "never"`, any `rm`
  with `-f` is forbidden with full access; an allow rule in
  `$CODEX_HOME/rules` did not lift it in a live run. The only other setting
  turns it into a prompt.
- **Codex's model-migration prompt.** An interactive start on a model the
  catalog marks for upgrade waits for an answer
  (`tui/src/app/startup_prompts.rs:269-358`). The answer is kept as
  `notice.model_migrations."<current>" = "<target>"`, and both names come
  from the model catalog the server sends at run time, so no fixed seed
  can hold it. It appears only on an outdated model; the role preflight
  keeps codex current.
- **`codex exec` outside a git repository** without the flag:
  `exec/src/lib.rs:964-970` checks `get_git_repo_root` and nothing in the
  config.
- **A trusted Codex project config** outranks the home file on paths
  without the flag; only `-c` on the command line beats it.
- **Usage limits and plan quotas.** Server-side.

## Considered and not set

- Maximum effort (`CLAUDE_CODE_EFFORT_LEVEL=max`, Codex
  `model_reasoning_effort`): how hard the model thinks is the role's
  choice, not a question the box answers. The variable is the wrong place
  for it anyway: while it is set, `/effort` is refused for the session
  ("CLAUDE_CODE_EFFORT_LEVEL=... overrides this session").
- The launch-effort pin. On Fable 5, Opus 4.7 and 4.8, effort is held at
  the model's own default and a settings `effortLevel` is ignored until
  `/effort` runs in an interactive terminal (`unpinFable5LaunchEffort` and
  its siblings in `.claude.json`; 2.1.270 `ww()`). Pinned or not, a home
  with no `effortLevel` gets the same default, and no role sets one.
- Forcing every subagent onto the main model
  (`CLAUDE_CODE_SUBAGENT_MODEL_FORCE`). In 2.1.270 the built-in Explore and
  Plan agents already inherit it (`model:"inherit"`). Forcing would also
  drop a `model` that a plugin agent or the Agent tool names (`bUn()`),
  which can be a stronger model than the main one.
- `CLAUDE_CODE_NO_MODEL_FALLBACK`: a superset of the refusal variable that
  can also make compaction unavailable, bringing back the context-limit stop.
- Turning auto-updaters off: the role preflights update every start, and a
  server-side minimum version stops an outdated binary from starting at all.
- Feedback surveys, tips, IDE dialogs: they do not block, or never appear in
  a box.
- Grok `[claude_compat] imported`: a Grok box home has no `~/.claude`
  (ADR 0004).
