# One home, several sessions: what the vendors document

Primary-source research into what happens when one agent home (`~/.claude`
and `~/.claude.json`, `~/.codex`, `~/.grok`, `~/.cargo`, `~/.rustup`) is used
from several project directories, one process at a time first and two at
once later. Checked 2026-09-13. Every claim is cited to an official doc
page, the vendor's CHANGELOG, or the tool's own source at a pinned commit.
"Fact." is what the source says. "Inference." is my reasoning from it.

Read alongside [shared-box.md](shared-box.md), which proposed the design,
and [shared-box-prior-art.md](shared-box-prior-art.md) §6 and §8, which
established that all three products key their per-project state to the
cwd path. The open question there was the one guessed at in
[shared-box.md](shared-box.md) "The one real hazard": does a running
Claude Code rewrite `~/.claude.json` on token refresh, and can two writers
lose a refresh. This document settles that and its siblings.

Source pins used below:

| Tree | Commit | Date |
|---|---|---|
| `openai/codex` | `a505c71490885a44979df056284badbfdd75b3fb` | 2026-09-13 |
| `xai-org/grok-build` | `37949780c144e37df692e3d669051a21fec24f20` | 2026-09-09 |
| `rust-lang/cargo` | `7941be6fb416b4cd9666aef7b858dfea25587a8c` | 2026-09-11 |
| `rust-lang/rustup` | `17f3d120587c6f365d4040b69ba029481ced5f97` | 2026-09-13 |
| `anthropics/claude-code` CHANGELOG | top entry `2.1.270` | fetched 2026-09-13 |

Claude Code ships as a minified binary with no public source, so its
section rests on the docs at `code.claude.com`, the CHANGELOG, and
maintainer statements. Codex and grok are open source in Rust and are
read directly.

---

## What the evidence says

1. The login is not in `~/.claude.json`. On Linux Claude Code keeps the
   OAuth login in `~/.claude/.credentials.json` (mode `0600`);
   `~/.claude.json` holds theme, the OAuth *account* record, per-project
   trust, personal MCP servers and UI toggles, and is written on `/config`
   changes and trust answers. The earlier guess that a token refresh
   rewrites `~/.claude.json` is wrong. §1a.
2. Two Claude Code sessions writing `~/.claude.json` at once did lose each
   other's changes until 2.1.259: "Fixed concurrent sessions silently
   reverting each other's `~/.claude.json` changes — workspace trust no
   longer resets". The mechanism of the fix is not documented. §1b.
3. Claude Code's credential refresh is guarded by a cross-process lock
   since at least 2.1.248 ("another Claude Code process held the token
   refresh lock"), after a run of refresh-race fixes from 2.1.118 to
   2.1.211. The vendor treats many sessions on one credential store as a
   supported case it keeps fixing, not as misuse. §1b, §4.
4. Claude Code picks up settings, hooks, skills, MCP servers and plugins
   live through a file watcher; `CLAUDE.md` and `~/.claude/rules/` load at
   launch and again after `/compact` or `/cd`; a binary update takes
   effect "the next time you start Claude Code". §1c, §1d.
5. `IS_SANDBOX` and `CLAUDE_CODE_SANDBOXED` appear nowhere in the docs,
   the env-vars reference, or the CHANGELOG. Their effect on the trust
   dialog is not verified by any primary source; what is known comes from
   reading the minified binary, recorded in
   [agent-autonomy.md](agent-autonomy.md). §1e.
6. Codex writes `auth.json` by truncate-in-place with no file lock, and
   its `AuthManager` never re-reads the file except before a refresh; the
   refresh path re-reads first and adopts a sibling's newer token. Codex
   writes `config.toml` by temp-file-and-rename with no lock, appends
   `history.jsonl` under `flock`, and opens its SQLite state in WAL mode
   with a five-second busy timeout. Nothing in Codex's docs mentions two
   processes on one `CODEX_HOME`; the code plainly expects it. §2.
7. grok is the most careful of the three: `auth.json` is written by
   rename under an `auth.json.lock` `flock` whose comment says an unlinked
   lock "lets two processes spend the same refresh token";
   `config.toml`, `trusted_folders.toml` and `active_sessions.json` are all
   rename-under-flock; a watcher reloads `auth.json` and `config.toml` in
   a running session. §3.
8. Refresh-token rotation is provable only where there is source. Codex
   maps the server error `refresh_token_reused` to "Exhausted" and grok's
   whole refresh design exists because "a sibling process rotated the RT
   out from under us". Anthropic's endpoint is undocumented; the CHANGELOG
   speaks of a "freshly-rotated OAuth token" and a "refresh-token race",
   which is consistent with rotation but is not a statement of it. §4.
9. Cargo locks `$CARGO_HOME` for concurrent builds by design: a
   `.package-cache` download lock and a `.package-cache-mutate` lock that
   is shared during builds, so "multiple cargo processes [can] build
   concurrently without interfering with one another". rustup locks only
   its `settings.toml` and a state file; a toolchain install is a
   rollback transaction, not a cross-process lock. §5.
10. Every cargo invocation in any project reads `$CARGO_HOME/config.toml`,
    where `build.rustc-wrapper`, `build.rustc`, `[alias]`,
    `target.<triple>.runner`, `[env]`, `[paths]`, `[source].replace-with`
    and `[net] git-fetch-with-cli` each change what runs during a build.
    `$CARGO_HOME/bin` is what `cargo install` and rustup put on `PATH`.
    §5b.

---

## 1. Claude Code

### 1a. Which file holds what, and when it is written

<https://code.claude.com/docs/en/authentication>, "Credential management":

> On Linux, credentials are stored in `~/.claude/.credentials.json` with
> file mode `0600`.

> If you've set the `CLAUDE_CONFIG_DIR` environment variable, Claude Code
> keeps the `.credentials.json` file under that directory instead

> Claude Code manages `.credentials.json` through `/login` and `/logout`.

<https://code.claude.com/docs/en/claude-directory>, the `.claude.json` entry:

> Read at session start for your preferences and MCP servers. Claude Code
> writes back to it when you change settings in `/config` or approve trust
> prompts

> Holds state that does not belong in settings.json: theme, OAuth session,
> per-project trust decisions, your personal MCP servers, and UI toggles.

> The `projects` key tracks per-project state like trust-dialog acceptance
> and last-session metrics. Permission rules you approve in-session go to
> `.claude/settings.local.json` instead

Same page, application data table:

> `backups/`: Earlier versions of `~/.claude.json`, copied when Claude Code
> rewrites the file. Claude Code keeps the five newest, plus a copy of any
> version it couldn't parse.

> `sessions/`: holds one small file per running session, used to detect
> concurrent sessions and crashes. It isn't part of the age-based sweep:
> Claude Code removes each file when its session exits and clears crash
> leftovers on the next launch.

> `history.jsonl`: Every prompt you've typed, with timestamp and project
> path.

<https://code.claude.com/docs/en/settings>:

> Claude Code also keeps a fifth file, `~/.claude.json`, that it writes for
> itself; you don't need to edit it. It holds your sign-in session, MCP
> server configurations, per-project state such as trust decisions, and
> the global config keys that `/config` writes for you.

<https://code.claude.com/docs/en/devcontainer>, "Persist authentication":

> Claude Code stores its authentication token, user settings, and session
> history under the `~/.claude` directory. It stores your OAuth account,
> personal MCP servers, and per-project trust in `~/.claude.json`, a
> separate file outside that directory, so mounting a volume at `~/.claude`
> alone doesn't keep you signed in.

**Fact.** The token lives in `.credentials.json`; the *account* record
("OAuth session", "OAuth account") lives in `.claude.json`. The docs name
two write triggers for `.claude.json`: `/config` changes and trust
answers. They name none for a token refresh. Confirmed on this host:
`~/.claude/.credentials.json` is mode `600`, 508 bytes; `~/.claude.json`
is mode `644`, 54.8 KB.

**Fact.** The `sessions/` directory exists to "detect concurrent
sessions". Several sessions on one home is a case the product has code
for.

**Inference.** A token refresh rewrites `.credentials.json`, not
`.claude.json`, so the race named in [shared-box.md](shared-box.md)
between wormhole's seed and a refresh does not exist as described.
The race that does exist is between wormhole's seed and a trust answer or
`/config` change in a running session, both rare and both user-driven.
The "not verified" part: no doc says whether the OAuth *account* fields in
`.claude.json` (`oauthAccount`, which wormhole carries over as a login
field, `manifest.rs` `CLAUDE_STATE.login_fields`) are rewritten on
refresh. The CHANGELOG entries below suggest they are not the refresh
target.

### 1b. What the CHANGELOG says about several instances

<https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md>
(raw: <https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md>),
quoted with the version header each line sits under.

`~/.claude.json` and shared config:

> **2.1.259** Fixed concurrent sessions silently reverting each other's
> `~/.claude.json` changes — workspace trust no longer resets and
> MCP/project state is no longer lost when running many sessions at once

> **2.1.234** Windows: startup no longer stalls on repeated rename retries
> when `~/.claude.json` is read-only

> **2.1.232** Fixed a startup race that could silently unregister a plugin
> marketplace due to concurrent writes to `known_marketplaces.json`

> **2.1.224** Fixed plugin install records being silently corrupted when
> the same plugin is installed in multiple projects

> **2.1.218** Fixed prompt history entries being dropped or duplicated when
> history writes raced or failed

> **2.1.199** Fixed resetting a corrupted config file from the startup
> recovery dialog destroying it unrecoverably — it now backs up the file
> first

> **2.1.141** Fixed `/model` in one session silently changing the
> autocompact threshold in other concurrent sessions

> **2.1.133** Fixed `/effort` in one session unexpectedly changing the
> effort level of other concurrent sessions

> **2.1.86** Fixed statusline showing another session's model when running
> multiple Claude Code instances and using `/model` in one of them

> **2.1.86** Fixed unnecessary config disk writes on every skill invocation
> that could cause performance issues and config corruption on Windows

> **1.0.31** Fixed a bug where ~/.claude.json would get reset when file
> contained invalid JSON

Credentials and token refresh across instances:

> **2.1.248** Fixed being sent to the login screen when another Claude Code
> process held the token refresh lock while the session token had expired;
> the request now fails with a retryable error instead

> **2.1.221** Fixed a rare wake-from-sleep race where two Claude Code
> processes could both refresh the same MCP connector or WIF OAuth token at
> once, forcing re-authentication

> **2.1.211** Fixed parallel Claude Code sessions all logging out
> simultaneously after wake-from-sleep when many sessions share one
> credential store

> **2.1.143** Fixed a corrupt `.credentials.json` with a non-array `scopes`
> value hanging the CLI on startup or silently aborting OAuth token refresh

> **2.1.136** Fixed a rare login loop where a concurrent credential write
> could overwrite a freshly-rotated OAuth token and force re-login

> **2.1.133** Fixed parallel sessions all dead-ending at 401 after a
> refresh-token race wiped shared credentials

> **2.1.129** Fixed OAuth refresh race after wake-from-sleep that could log
> out all running sessions

> **2.1.126** Fixed a rare race where a concurrent credential write could
> clear a valid OAuth refresh token

> **2.1.118** Fixed macOS keychain race where a concurrent MCP token refresh
> could overwrite a freshly-refreshed OAuth token, causing unexpected
> "Please run /login" prompts

> **2.1.118** Fixed credential save crash on Linux/Windows corrupting
> `~/.claude/.credentials.json`

Atomic writes and lock files:

> **2.1.229** Fixed a file-watcher handle leak after atomic file
> replacements

> **2.1.216** Fixed `claude daemon stop --any` potentially terminating an
> unrelated process via a stale legacy daemon lockfile

> **2.1.169** Fixed plugin `.in_use` PID lock files accumulating without
> bound; stale markers from crashed sessions are now swept once per day

**Fact.** Nine entries between 2.1.118 and 2.1.259 fix losses caused by
two or more sessions writing one credential store or one `.claude.json`.
By 2.1.248 there is a named "token refresh lock" that other processes can
hold. By 2.1.259 the `.claude.json` read-modify-write no longer reverts
another session's changes. "Atomic file replacements" and a file watcher
are named in 2.1.229.

**Not verified.** How the 2.1.259 fix works (re-read before write, a
lock, or per-key merge) is not stated anywhere. The binary is minified,
so the mechanism cannot be cited. What can be said is that the vendor
has, in writing, made "many sessions at once" on one home a case it
fixes bugs for.

### 1c. What a running session picks up live

<https://code.claude.com/docs/en/settings>, "When edits take effect":

> Claude Code watches your settings files and reloads them when they
> change, so it applies most edits to the running session without a
> restart, including edits to `permissions`, `hooks`, and credential
> helpers such as `apiKeyHelper`. Claude Code also loads a settings file
> you create mid-session if its folder existed when the session started.

> The reload covers user, project, local, and managed settings, and Claude
> Code runs the `ConfigChange` hook for each settings-file change it
> detects

> Claude Code reads some keys only once, at session start, so an edit to
> one of them doesn't reach the running session. ... `model` ...
> `effortLevel` and `modelSettings`

<https://code.claude.com/docs/en/hooks>:

> Direct edits to hooks in settings files are normally picked up
> automatically by the file watcher.

> Claude Code runs ConfigChange hooks when a settings file, a managed
> policy file, or a skill file changes.

> ConfigChange hooks can block configuration changes from taking effect.
> ... When blocked, the new settings are not applied to the running
> session.

(The hooks page says nothing about a startup snapshot of hook config. The
phrase in the task brief does not appear in the current page.)

<https://code.claude.com/docs/en/memory>:

> CLAUDE.md files ... You write these files in plain text; Claude reads
> them at the start of every session.

> CLAUDE.md and CLAUDE.local.md files in the directory hierarchy above the
> working directory are loaded at launch. Files in subdirectories load on
> demand when Claude reads files in those directories.

> Project-root CLAUDE.md survives compaction: after `/compact`, Claude
> re-reads it from disk and re-injects it into the session.

> Rules without `paths` frontmatter are loaded at launch with the same
> priority as `.claude/CLAUDE.md`.

<https://code.claude.com/docs/en/permissions>, on `/cd`:

> Claude Code keeps the conversation, loads the new directory's
> `CLAUDE.md`, and prompts you to trust the workspace if you haven't
> worked in it before.

<https://code.claude.com/docs/en/skills>:

> Claude Code watches skill directories for file changes, except in bare
> mode. When you add, edit, or remove a skill under `~/.claude/skills/`,
> the project `.claude/skills/`, or a `.claude/skills/` inside an
> `--add-dir` directory, Claude Code picks up the change within the current
> session, without a restart.

with the limit that a top-level skills directory created after start
needs a restart to be watched.

<https://code.claude.com/docs/en/mcp>: `claude mcp add` and `claude mcp
remove` take effect in the running session; plugin MCP servers connect or
disconnect "when the change applies", and `/reload-plugins` reloads
"plugins, skills, agents, hooks, plugin MCP servers, and plugin LSP
servers" (<https://code.claude.com/docs/en/plugins>). Plugin install
summaries can say `Run /reload-plugins to activate.`

**Fact.** Live: settings (all four files), hooks, skills, MCP config,
plugins after `/reload-plugins`. Launch-time: `CLAUDE.md`, rules,
`model`, effort. Re-read at `/compact` and `/cd`: the project `CLAUDE.md`.

### 1d. The native installer and a running instance

<https://code.claude.com/docs/en/setup>:

> Claude Code checks for updates on startup and periodically while running.
> Updates download and install in the background, then take effect the
> next time you start Claude Code.

> On macOS and Linux, the native installer manages the launcher at
> `~/.local/bin/claude` as a symlink into `~/.local/share/claude/versions/`.
> If you replace that launcher with your own script or symlink, auto-update
> and `claude update` leave it in place: new versions still install under
> the `versions/` directory, and your launcher decides which version runs.
> Before v2.1.207, the auto-updater replaced a custom launcher at that path
> with its own symlink on every update.

> With a custom launcher, Claude Code also keeps every installed version on
> disk because it can't tell which version the launcher needs.

> `DISABLE_AUTOUPDATER` only stops the background check; `claude update`
> and `claude install` still work. To block all update paths, including
> manual updates, set `DISABLE_UPDATES` instead.

> On WinGet the upgrade may fail while Claude Code is running because
> Windows locks the executable.

Confirmed on this host: `~/.local/bin/claude ->
/home/nabor/.local/share/claude/versions/2.1.270`.

CHANGELOG, on a running instance across an update:

> **2.1.257** Fixed background sessions left running an older Claude Code
> binary piling up across auto-updates instead of being retired

> **2.1.243** Fixed Claude in Chrome losing its connection to Claude Code
> after an auto-update cleaned up the version it was set up with; the
> native host now launches via the stable `claude` launcher

> **2.1.235** Fixed the prompt footer not showing the "Update installed"
> restart notice after a background auto-update

> **2.1.208** Fixed background-session attach failing permanently ... after
> an update replaced the binary a running `claude agents` process was
> launched from

**Fact.** An update is a new file under `versions/` plus a symlink swap.
The running process keeps its already-mapped binary; the docs promise
the new one only at the next start. Old versions are cleaned up, which
has bitten helpers that held a path to the old file (2.1.243, 2.1.208).

**Inference.** In a shared home, session A's auto-update (or the
preflight's `claude update` for session B) changes which binary session
C's *next* start runs, never what A or B is running now. The one hazard
is the version cleanup removing a file a running session still needs to
re-exec; the vendor fixed two such cases and the docs do not say the
class is closed.

### 1e. `IS_SANDBOX` and `CLAUDE_CODE_SANDBOXED`

<https://code.claude.com/docs/en/env-vars> lists neither variable (the
page was fetched in full and searched). <https://code.claude.com/docs/en/security>,
<https://code.claude.com/docs/en/permissions>,
<https://code.claude.com/docs/en/sandbox-environments> and
<https://code.claude.com/docs/en/devcontainer> do not name them. The
CHANGELOG has no line containing either string (grep over 6679 lines).

What the docs do say about when the trust dialog is skipped
(<https://code.claude.com/docs/en/permissions>):

> Claude Code shows the trust dialog in interactive sessions only. A
> `claude -p` run or an SDK session never shows it

> Outside a repository, Claude Code keys the trust on the directory you
> started it from, and the trust covers any subdirectory of that directory
> apart from a git repository nested inside it

> When you start in your home directory, Claude Code holds the trust for
> the current session only and doesn't write it to disk

**Not verified.** No primary source documents `IS_SANDBOX` or
`CLAUDE_CODE_SANDBOXED`. The reason wormhole sets them is recorded in
[agent-autonomy.md](agent-autonomy.md) from reading the minified binary
(`CLAUDE_CODE_SANDBOXED` short-circuits a trust check; `IS_SANDBOX`
disables the server-forced Bash sandbox). That reading is evidence about
one build, not a vendor commitment.

---

## 2. Codex CLI

Paths are under `codex-rs/` at commit `a505c714`. Raw file URLs use the
prefix `https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/`.

### `auth.json` on refresh

`login/src/auth/storage.rs`, `FileAuthStorage::save`
(<https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/login/src/auth/storage.rs>):

```rust
fn save(&self, auth_dot_json: &AuthDotJson) -> std::io::Result<()> {
    let auth_file = get_auth_file(&self.codex_home);
    ...
    let mut options = OpenOptions::new();
    options.truncate(true).write(true).create(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(auth_file)?;
    file.write_all(json_data.as_bytes())?;
    file.flush()?;
    Ok(())
}
```

**Fact.** Truncate-in-place, no temp file, no rename, no `flock`. A
reader that opens the file between `truncate` and `write_all` sees an
empty or partial file. `pub(super) fn get_auth_file` is
`codex_home.join("auth.json")`.

`login/src/auth/manager.rs`
(<https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/login/src/auth/manager.rs>):

```rust
/// Central manager providing a single source of truth for auth.json derived
/// authentication data. It loads once (or on preference change) and then
/// hands out cloned `CodexAuth` values so the rest of the program has a
/// consistent snapshot.
///
/// External modifications to `auth.json` will NOT be observed until
/// `reload()` is called explicitly.
pub struct AuthManager {
    ...
    refresh_lock: Semaphore,
```

```rust
/// Attempt to refresh the token by first performing a guarded reload from
/// the active auth source. If the loaded token differs from the cached token,
/// we can assume that the source already refreshed it. Otherwise, ask the
/// token authority to refresh.
pub async fn refresh_token(&self) -> Result<(), RefreshTokenError> {
    let _refresh_guard = self.refresh_lock.acquire().await ...
    match self.reload_if_account_id_matches(expected_account_id.as_deref()).await {
        ReloadOutcome::ReloadedChanged => {
            tracing::info!("Skipping token refresh because auth changed after guarded reload.");
            Ok(())
        }
        ReloadOutcome::ReloadedNoChange => self.refresh_token_from_authority_impl().await,
```

and the persist step, `fn persist_tokens`:

```rust
// Persist refreshed tokens into auth storage and update last_refresh.
fn persist_tokens(storage, id_token, access_token, refresh_token) -> io::Result<AuthDotJson> {
    let mut auth_dot_json = storage.load()?.ok_or(...)?;
    ...
    if let Some(refresh_token) = refresh_token {
        tokens.refresh_token = refresh_token;
    }
    auth_dot_json.last_refresh = Some(Utc::now());
    storage.save(&auth_dot_json)?;
```

**Fact.** The refresh lock is an in-process `Semaphore`, not a file lock.
Before calling the authority Codex re-reads `auth.json`; if another
process already rotated the token, it adopts that token and does not
refresh. `persist_tokens` re-reads the file again just before saving, so
the read-modify-write window is the width of one JSON parse and one
in-place write, not a session.

**Inference.** Two Codex processes with the same account can both pass
the guarded reload with the same old token if both refresh inside the
same short window, and both call the authority. The loser gets
`invalid_grant` (see §4). Codex has no on-disk guard against that. It is
the same class of bug Claude Code fixed in 2.1.126 to 2.1.248.

### `config.toml` on trust

`core/src/config/edit.rs`
(<https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/core/src/config/edit.rs>):

```rust
/// Persist edits using a blocking strategy.
pub fn apply_blocking(codex_home: &Path, edits: &[ConfigEdit]) -> anyhow::Result<()> {
    let config_path = codex_home.join(CONFIG_TOML_FILE);
    apply_blocking_to_resolved_file(&config_path, edits)
}

fn apply_blocking_to_resolved_file(resolved_config_file: &Path, edits: &[ConfigEdit]) -> anyhow::Result<()> {
    ...
    let serialized = match write_paths.read_path { Some(path) => std::fs::read_to_string(&path) ... };
    let doc = ... serialized.parse::<DocumentMut>()?;
    let mut document = ConfigDocument::new(doc);
    for edit in edits { mutated |= document.apply(edit)?; }
    if !mutated { return Ok(()); }
    write_atomically(&write_paths.write_path, &document.doc.to_string())
```

with `ConfigEditsBuilder::set_project_trust_level` pushing
`ConfigEdit::SetProjectTrustLevel { path, level }` and
`core/src/config/mod.rs` `set_project_trust_level` wrapping it. The
writer, `utils/path-utils/src/lib.rs`
(<https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/utils/path-utils/src/lib.rs>):

```rust
pub fn write_atomically(write_path: &Path, contents: &str) -> io::Result<()> {
    ...
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(contents.as_bytes())?;
    tmp.persist(write_path)?;
```

**Fact.** Read whole file, edit the TOML document in place (`toml_edit`,
comments kept), write by temp-file-and-rename. No lock. A concurrent
writer between read and rename loses.

### `history.jsonl`

`message-history/src/lib.rs`
(<https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/message-history/src/lib.rs>):

```rust
const HISTORY_FILENAME: &str = "history.jsonl";

/// Append a `text` entry associated with `conversation_id` to the history file.
///
/// Uses advisory file locking (`File::try_lock`) with a retry loop to ensure
/// concurrent writes from multiple TUI processes do not interleave.
pub async fn append_entry(...)
    ...
    options.append(true);
    options.mode(0o600);
    ...
    for _ in 0..MAX_RETRIES {
        match history_file.try_lock() {
            Ok(()) => {
                history_file.seek(SeekFrom::End(0))?;
                history_file.write_all(line.as_bytes())?;
                history_file.flush()?;
                enforce_history_limit(&mut history_file, history_max_bytes)?;
                return Ok(());
```

and `lookup` "acquires a shared advisory file lock via
`File::try_lock_shared`". **Fact.** History is the one Codex file whose
comment names "multiple TUI processes" as a case to handle.

### SQLite state

`state/src/sqlite.rs`
(<https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/state/src/sqlite.rs>):

```rust
const STATE_DB_FILENAME: &str = "state_5.sqlite";
...
pub async fn open_read_write_pool(&self, path: &Path) -> Result<SqlitePool, Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .auto_vacuum(SqliteAutoVacuum::Incremental)
        .busy_timeout(Duration::from_secs(5))
```

alongside `logs_2.sqlite`, `goals_1.sqlite`, `memories_1.sqlite`,
`queue_1.sqlite`, `thread_history_1.sqlite` in the same file. The startup
failure text, `cli/src/state_db_recovery.rs`
(<https://raw.githubusercontent.com/openai/codex/a505c71490885a44979df056284badbfdd75b3fb/codex-rs/cli/src/state_db_recovery.rs>):

```rust
pub(crate) fn print_locked_guidance(startup_error: &LocalStateDbStartupError) {
    eprintln!("Codex couldn't start because another Codex process is using its local data.");
    eprintln!("Quit any other copies of Codex that may still be running, then try again.");
```

**Fact.** WAL mode with a 5 s busy timeout is SQLite's standard
multi-process configuration: readers do not block writers, and a writer
waits up to 5 s for another writer. Codex has a dedicated message for the
case where the wait is exceeded at startup, which is only reachable when
another Codex process holds the database.

### Documentation

The config reference (<https://developers.openai.com/codex/config-reference>,
served from <https://learn.chatgpt.com/docs/config-file/config-reference>)
documents `history.persistence` ("Control whether Codex saves session
transcripts to history.jsonl"), `history.max_bytes`,
`projects.<path>.trust_level`, `cli_auth_credentials_store`
(`file | keyring | auto | ephemeral`) and `sqlite_home` ("Directory where
Codex stores the SQLite-backed state DB"). It says nothing about several
`codex` processes on one `CODEX_HOME`. The repository's `docs/` has no
such text either. **Fact.** Multi-process behaviour is in the code, not
the docs.

---

## 3. grok (xAI Grok Build)

The installer at <https://x.ai/cli/install.sh> downloads a prebuilt
binary into `~/.grok/downloads/grok-<version>-<platform>` and symlinks
`~/.grok/bin/grok` to it ("Use relative symlinks when BIN_DIR and
DOWNLOAD_DIR share a parent"; `ln -sf "$link_target" "$BIN_DIR/grok"`).
<https://x.ai/cli/stable> served `1.0.30` on the check date. The source is
public: <https://github.com/xai-org/grok-build> (Apache-2.0, "External
contributions are not accepted" per its CONTRIBUTING). The user guide
ships inside it at `crates/codegen/xai-grok-pager/docs/user-guide/`. Raw
URLs below use the prefix
`https://raw.githubusercontent.com/xai-org/grok-build/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/`.

### `auth.json` on refresh

`xai-grok-login/src/manager/lock.rs`
(<https://raw.githubusercontent.com/xai-org/grok-build/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-login/src/manager/lock.rs>):

```rust
//! Advisory `auth.json.lock` handling.
//! The lock file is never deleted and a held flock is never broken: an unlinked lock lets two processes spend the same refresh token.
//! Staleness resolves in place on the live lock, via [`flock_wait`].
...
pub const LOCK_FILE_NAME: &str = "auth.json.lock";
```

`xai-grok-login/src/storage.rs`
(<https://raw.githubusercontent.com/xai-org/grok-build/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-login/src/storage.rs>):

```rust
/// Persist `auth.json`, preferring a crash-safe atomic write but falling back to a non-atomic in-place write when the disk is full.
pub(super) fn write_auth_json(auth_file: &Path, auth_store: &AuthStore) -> std::io::Result<()>
...
/// Atomic write: a temp file, then a rename.
fn write_auth_json_atomic(auth_file: &Path, auth_store: &AuthStore) -> std::io::Result<()> {
    // Unique per write (pid and a monotonic seq): two concurrent in-process writers must not share one tmp path
```

`xai-grok-login/src/manager.rs`
(<https://raw.githubusercontent.com/xai-org/grok-build/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-login/src/manager.rs>):

```rust
/// Persist rotated tokens to disk and cache, then spawn `/user` enrichment. Invariants: **Disk write before any network I/O** (else a sibling process can reuse the not-yet-rotated RT and the IdP returns `invalid_grant`).
/// **Caller holds the `auth.json` file lock** (production callers: `refresh_chain` Success arm, `flow::run_auth_flow`).
pub async fn update(self: &Arc<Self>, auth: GrokAuth) -> std::io::Result<GrokAuth>
...
/// `true` when the refresh token on disk is present and differs from the one we actually spent. That means a sibling process rotated the RT while our exchange was in flight.
/// The rejection we just got is then a lost race rather than a revoked session. ...
/// Two hand-rolled copies of this comparison is how the wrong one survived long enough to log a dozen processes out at once.
fn refresh_token_superseded(disk_rt: Option<&str>, spent_rt: &str) -> bool
```

`xai-grok-login/src/manager/refresh_chain.rs`:

```rust
/// `Held` is the live lock, proven before the irreversible IdP call.
/// `Adopted` is a sibling's freshly rotated token; return it without refreshing.
pub(super) enum LockOutcome { Held(AuthFileLock), Adopted(Box<GrokAuth>) }
```

**Fact.** grok holds an `flock` on `auth.json.lock` across the whole
refresh, writes by rename, and re-reads under the lock to adopt a
sibling's newer token before spending its own. The comments name the
multi-process case repeatedly and describe a past incident ("log a dozen
processes out at once").

### `config.toml`

`xai-grok-shell/src/config/mod.rs`
(<https://raw.githubusercontent.com/xai-org/grok-build/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-shell/src/config/mod.rs>):

```rust
/// Locked read-modify-write of `~/.grok/config.toml`: the whole window runs under the config-init
/// flock and lands via atomic replace; unchanged configs skip the write.
fn update_config_toml_locked(grok_home, mutate) -> Result<...> {
    let config_path = grok_home.join("config.toml");
    let _flock = crate::util::config::acquire_init_lock(grok_home)?;
    let content = crate::util::config::read_to_string_or_empty(&config_path)?;
    ...
    crate::util::config::atomic_write_string(&config_path, &toml::to_string_pretty(&config)?)?;
```

and "runs under the config-init flock with an atomic replace like every
config.toml writer". `xai-grok-config/src/fs_atomic.rs` `write_atomically`
is temp-file with `create_new` plus `rename`.

### Folder trust

`xai-grok-workspace/src/folder_trust.rs`
(<https://raw.githubusercontent.com/xai-org/grok-build/37949780c144e37df692e3d669051a21fec24f20/crates/codegen/xai-grok-workspace/src/folder_trust.rs>):

> reads/writes the durable `TrustStore` (`~/.grok/trusted_folders.toml`)

> An explicit `--trust` grant is persisted to the store up front (see
> `grant_folder_trust`), so it is honored here.

> On a stamped build: env > user config > managed > remote > default true.

with `BoolFlag::env("GROK_FOLDER_TRUST")` as the first source, and the
user guide `10-hooks.md`:

> a `--trust` / `/hooks-trust` grant trusts the whole folder for **MCP,
> LSP, hooks, project instructions, and project skills** together, and
> covers subdirectories of the same repository. A nested git checkout
> under that folder is a separate workspace and is not covered.
> Conversely, disabling folder-trust (`GROK_FOLDER_TRUST=0` or
> `[folder_trust] enabled = false`) ungates those surfaces together.

`xai-grok-workspace/src/trust.rs`: `ExclusiveLock::acquire(&path.with_extension("toml.lock"))`
around the store write, and `persist_doc`: "Write `doc` to `path`
atomically (unique temp, fsync, rename) with owner-only (`0600`)
permissions."

**Fact.** Trust is a separate file, `trusted_folders.toml`, written
rename-under-flock. wormhole does not write this file; it passes `--trust`
and `GROK_FOLDER_TRUST=0` (`manifest.rs` `KNOWN_AGENTS`), so nothing of
wormhole's touches it.

### Several sessions, live reload, sessions on disk

`xai-grok-active-sessions/src/lib.rs`:

```rust
//! Tracks open TUI sessions in `~/.grok/active_sessions.json` for crash recovery.
//! A clean exit removes the entry; a crash leaves it behind.
...
const LOCK_FILENAME: &str = "active_sessions.lock";
```

with every mutation under `lock_file.lock_exclusive()` and a temp-file
rename.

`xai-grok-shell/src/config/watcher.rs`:

> Watches `~/.grok/` for `auth.json`, `config.toml`, and
> `models_cache.json` changes, plus any extra paths (project
> `.grok/config.toml`, `.mcp.json`, etc.) provided at startup. ... When
> the agent writes `auth.json` or `config.toml`, the watcher fires and the
> `ConfigReloader` re-reads it.

User guide `02-authentication.md`:

> Grok picks up changes to `~/.grok/auth.json` automatically. If you
> update credentials externally (for example, with a script that writes
> new tokens), Grok uses the new credentials on the next API call without
> a restart.

> Grok stores credentials in `~/.grok/auth.json` and reuses them across
> sessions. Grok refreshes access tokens automatically in the background.

`08-skills.md`: "Grok reloads skills when files change on disk."
`17-sessions.md`: sessions are "stored on disk under `~/.grok/sessions/`"
as `~/.grok/sessions/<encoded-cwd>/<session-id>/`, and the `-s` flag text
says "sequential use is reliable, concurrent same-ID is best-effort".
`14-headless-mode.md` and the public docs
(<https://docs.x.ai/build/cli/headless-scripting>): "Headless sessions
... are stored in `~/.grok/sessions`", "pass `--no-auto-update` ... to skip
background update checks. You can also persistently disable them by
setting `auto_update = false` under the `[cli]` section".

### Update and the running binary

`xai-grok-update/src/auto_update.rs`: the download is streamed "to a temp
file, then rename atomically" into `~/.grok/downloads/`, then
`atomic_symlink_swap` "Creates a temporary symlink next to `link_path`,
then renames it over the old symlink"; `resolve_restart_exe` prefers
`~/.grok/bin/grok` because "`current_exe()` resolves symlinks via
`/proc/self/exe` ... so it returns the old versioned target after a
symlink swap"; the TUI shows a "restart hint" and can `restart_grok()`
via `exec`. **Fact.** Same shape as Claude Code: new file, symlink swap,
running process unchanged until it re-execs.

**Fact, overall.** Of the three products, grok is the only one whose
source names every shared file's multi-process contract and locks each
of them.

---

## 4. Login refresh and token rotation

**Codex (ChatGPT OAuth).** `login/src/auth/manager.rs`:

```rust
fn classify_refresh_token_failure(code, body, is_invalid_grant_bad_request) -> RefreshTokenFailedError {
    let reason = match normalized_code.as_deref() {
        Some("refresh_token_expired") => RefreshTokenFailedReason::Expired,
        Some("refresh_token_reused") => RefreshTokenFailedReason::Exhausted,
        Some("refresh_token_invalidated") => RefreshTokenFailedReason::Revoked,
```

and `struct RefreshResponse { id_token: Option<String>, access_token:
Option<String>, refresh_token: Option<String> }` with `persist_tokens`
overwriting `tokens.refresh_token` when the response carries one.
**Fact.** The server can return a new refresh token on every refresh and
has an error code for reuse of an old one. That is rotation with reuse
detection. Two Codex processes that both spend the same refresh token get
one success and one `refresh_token_reused`, which Codex classes as
permanent ("Exhausted"). The guarded reload in §2 is the mitigation, and
it is a window, not a lock.

**grok (xAI OIDC).** The comments quoted in §3 say it outright: "an
unlinked lock lets two processes spend the same refresh token", "a sibling
process rotated the RT while our exchange was in flight", "the IdP
returns `invalid_grant`". `02-authentication.md`: "Tokens auto-refresh
silently via the stored `refresh_token`", scope `offline_access`.
**Fact.** Rotation, and grok's file lock exists to serialise it across
processes.

**Claude Code (Anthropic OAuth).** No document at `code.claude.com` or
`platform.claude.com` describes the CLI's OAuth refresh grant or whether
the refresh token rotates. The CHANGELOG lines quoted in §1b use the words
"freshly-rotated OAuth token" (2.1.136), "refresh-token race wiped shared
credentials" (2.1.133), "clear a valid OAuth refresh token" (2.1.126),
"the token refresh lock" (2.1.248) and "after the OAuth token expired or
rotated mid-session" (2.1.216). **Inference.** Those words are only
coherent if the stored token changes on refresh and two refreshers can
conflict, so rotation is very likely; but no primary source states it, so
it stays **not verified**. What is verified is that the vendor added a
cross-process refresh lock and fixed the concurrent-write losses.

---

## 5. Cargo and rustup in a shared home

### 5a. Cargo's package cache lock

`src/util/cache_lock.rs` at commit `7941be6f`
(<https://raw.githubusercontent.com/rust-lang/cargo/7941be6fb416b4cd9666aef7b858dfea25587a8c/src/util/cache_lock.rs>):

```rust
//! Support for locking the package and index caches.
//!
//! This implements locking on the package and index caches (source files,
//! `.crate` files, and index caches) to coordinate when multiple cargos are
//! running at the same time.
...
//! * [`CacheLockMode::DownloadExclusive`] -- This is an exclusive lock
//!   acquired while downloading packages and doing resolution.
//! * [`CacheLockMode::Shared`] -- This is a shared lock acquired while a
//!   build is running. In other words, whenever cargo just needs to read from
//!   the cache, it should hold this lock.
//! * [`CacheLockMode::MutateExclusive`] -- This is an exclusive lock acquired
//!   whenever needing to modify existing source files (for example, with
//!   cache garbage collection).
...
//! This is implemented by two separate lock files, the "download" one and the
//! "mutate" one.
...
const CACHE_LOCK_NAME: &str = ".package-cache";
const MUTATE_NAME: &str = ".package-cache-mutate";
```

and on `Shared`:

```rust
/// This allows multiple
/// cargo processes to build concurrently without interfering with one
/// another, while guarding against other cargos using `MutateExclusive`.
```

**Fact.** Concurrent builds in different projects sharing one
`$CARGO_HOME` are a designed case: downloads are serialised by
`.package-cache`, builds take a shared lock, only cache GC takes the
exclusive one. The Cargo book does not describe the lock; the source doc
comment is the reference.

### 5b. What every cargo run reads from `$CARGO_HOME`

<https://doc.rust-lang.org/cargo/guide/cargo-home.html>:

> **config.toml**: Cargo's global configuration file

> **credentials.toml**: Private login credentials from `cargo login`

> **.crates.toml, .crates2.json**: These hidden files contain package
> information of crates installed via `cargo install`. Do NOT edit by hand!

> **bin**: The bin directory contains executables of crates that were
> installed via `cargo install` or `rustup`. To be able to make these
> binaries accessible, add the path of the directory to your `$PATH`
> environment variable.

<https://doc.rust-lang.org/cargo/reference/config.html>:

> If, for example, Cargo were invoked in `/projects/foo/bar/baz`, then the
> following configuration files would be probed for and unified in this
> order: ... `/.cargo/config.toml`, `$CARGO_HOME/config.toml` which
> defaults to ... Unix: `$HOME/.cargo/config.toml`

> If a key is specified in multiple config files, the values will get
> merged together. Numbers, strings, and booleans will use the value in
> the deeper config directory taking precedence over ancestor directories,
> where the home directory is the lowest priority. Arrays will be joined
> together

Keys in that file that change what runs during a build, quoted:

> `build.rustc`: Sets the executable to use for `rustc`.

> `build.rustc-wrapper`: Sets a wrapper to execute instead of `rustc`. The
> first argument passed to the wrapper is the path to the actual executable
> to use

> `build.rustc-workspace-wrapper`: Sets a wrapper to execute instead of
> `rustc`, for workspace members only.

> `build.rustflags`: Extra command-line flags to pass to `rustc`.

> `[alias]`: The `[alias]` table defines CLI command aliases. ... Aliases
> are not allowed to redefine existing built-in commands.

> `target.<triple>.runner`: If a runner is provided, executables for the
> target `<triple>` will be executed by invoking the specified runner with
> the actual executable passed as an argument. This applies to `cargo run`,
> `cargo test` and `cargo bench` commands.

> `[env]`: The `[env]` section allows you to set additional environment
> variables for build scripts, rustc invocations, `cargo run` and
> `cargo build`.

> `[net] git-fetch-with-cli`: If this is `true`, then Cargo will use the
> `git` executable to fetch registry indexes and git dependencies.

> `[paths]`: An array of paths to local packages which are to be used as
> overrides for dependencies.

> `[source.<name>].replace-with`: If set, replace this source with the
> given named source or named registry.

> `[registries.<name>].credential-provider` / `[registry].credential-provider`:
> Specifies the credential provider for the given registry.

> `install.root`: Sets the path to the root directory for installing
> executables for `cargo install`. ... The default if not specified is
> Cargo's home directory

**Fact.** A project that can write `$CARGO_HOME/config.toml` can make
every other project's `cargo build` run an arbitrary wrapper, alias
`cargo test` to anything, route `cargo run` through a runner, replace
crates.io with another source, or point git fetches at its own `git`.
`$CARGO_HOME/bin` is on `PATH` by the vendor's instruction, so a binary
dropped there shadows anything later in `PATH`.

### 5c. rustup and concurrent installs

rustup source at commit `17f3d120`
(<https://raw.githubusercontent.com/rust-lang/rustup/17f3d120587c6f365d4040b69ba029481ced5f97/src/utils/raw.rs>):

```rust
pub(crate) fn write_locked_file(path: &Path, contents: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .read(true).write(true).create(true)
        // Truncation must happen after the exclusive lock is held.
        .truncate(false)
        .open(path)?;
    file.lock()?;
    file.set_len(0)?;
```

Its only callers are `src/settings.rs` (the `settings.toml` write) and
`src/config.rs` (the notification-state file); `read_locked_file` takes
`lock_shared` for the same two files. Toolchain installation goes through
`src/dist/component/transaction.rs`:

```rust
//! A transactional interface to file system operations needed by the
//! installer.
//!
//! Installation or uninstallation of a single component is done
//! within a Transaction, which supports a few simple file system
//! operations. If the Transaction is dropped without committing then
//! it will *attempt* to roll back the transaction.
```

There is no `flock` in `src/dist/`, `src/toolchain/` or `src/install.rs`
(grep at the pinned commit). The rustup book has no page on concurrency;
the only related text is the unstable `RUSTUP_CONCURRENT_DOWNLOADS`
variable (<https://rust-lang.github.io/rustup/environment-variables.html>),
which is about parallel downloads inside one process.

**Fact.** rustup serialises writes to `settings.toml` (default toolchain,
overrides) and nothing else. Two `rustup toolchain install` or `rustup
update` runs on one `RUSTUP_HOME` at once are not coordinated; each is a
rollback transaction on its own. **Inference.** Two boxes installing the
same toolchain at once can each see the other's half-written files and
one may roll back the other's work; wormhole's alphaca image installs the
toolchain in the image, not the home, so this only matters if a role
starts putting toolchains in `~/.rustup`.

---

## What this means for the shared box

**Inference throughout.** Facts are in the sections above.

### (a) Does the vendor design for two sessions in two directories sharing one home?

- **Claude Code: yes, with a bug history.** A `sessions/` directory exists
  "to detect concurrent sessions". Nine CHANGELOG fixes from 2.1.118 to
  2.1.259 are about many sessions on one credential store or one
  `.claude.json`, and the fixes speak of a "token refresh lock". The docs
  never say "supported", but the product has been made to work for it in
  public, release after release. A box on 2.1.259 or later is past the
  documented `.claude.json` reversion bug. The mechanism is unverifiable,
  so a design should not rely on the details, only on the fact that the
  vendor owns the problem.
- **Codex: tolerated, not documented.** Per-cwd trust, per-thread rollout
  files, `flock` on `history.jsonl` "from multiple TUI processes", WAL
  SQLite with a busy timeout and an explicit "another Codex process is
  using its local data" message. The weak spot is `auth.json`: an
  in-place truncate write with no lock and an in-process refresh guard.
  Two Codex sessions refreshing inside one window will spend the same
  refresh token and one becomes "Exhausted". Nothing in Codex's docs
  claims otherwise.
- **grok: yes, explicitly.** Every shared file (`auth.json`,
  `config.toml`, `trusted_folders.toml`, `active_sessions.json`) is
  rename-under-flock and the auth comments describe the multi-process
  refresh race as a solved incident. Sessions are keyed by encoded cwd.

### (b) Which of wormhole's start-time writes could collide with a running CLI

wormhole's seed (`crates/wormhole/src/seed.rs` `seed_agent_config`)
reads each file, merges its entries (`wormhole_core::seed::config_file`),
and replaces the file by write-and-rename (`main.rs` `replace_file`,
"Replaces a file in one step: write beside it, then `rename(2)` over
it"). The window is read-to-rename, a few milliseconds, with no lock.

- **`.claude.json`** (`CLAUDE_STATE`: `hasCompletedOnboarding`,
  `bypassPermissionsModeAccepted`, per-project `hasTrustDialogAccepted`,
  `hasCompletedProjectOnboarding`, `hasClaudeMdExternalIncludesApproved`,
  and the `oauthAccount` login field). Claude Code writes this file on
  `/config` changes and trust answers, and keeps five backups in
  `~/.claude/backups/` on every rewrite. A refresh does *not* write it
  (§1a), so the "lose the refresh and the box is logged out" hazard in
  [shared-box.md](shared-box.md) is retired. What remains: if session A
  answers a trust prompt or `/config` at the instant wormhole seeds for
  session B, one write is lost. The loser is a trust flag or a UI toggle,
  and wormhole's own entries are `settled` (rewritten on every start), so
  a lost seed heals at the next start and a lost vendor write is a single
  re-prompt. Whether Claude Code's 2.1.259 fix also re-reads before its
  own write, and so tolerates a foreign writer, is not stated.
- **`.claude/settings.json`** (`CLAUDE_SETTINGS`:
  `skipDangerousModePermissionPrompt`, `permissions.defaultMode`,
  `enableAllProjectMcpServers`, `switchModelsOnFlag`). Claude Code writes
  user settings from `/config` and `/memory` (`autoMemoryEnabled`), and
  watches the file. A seed while A runs is picked up by A live (§1c),
  which is harmless because the values are the ones A already runs with.
  A `ConfigChange` hook in A fires for wormhole's write.
- **`.claude/.credentials.json`**. wormhole copies it once where absent
  (`credentials = "copy"`) or binds it (`share`); it never merges into it.
  With `share`, the host's own `claude` and the box's `claude` are two
  writers on one file, which is exactly the population the 2.1.118 to
  2.1.248 fixes were for; on a current build that is the vendor's
  supported case, on an old pinned build it is the known race.
- **`.codex/config.toml`** (`CODEX_CONFIG`: per-project `trust_level`,
  `approval_policy`, `sandbox_mode`, `web_search`, `notice.*`). Codex
  writes it by read, `toml_edit`, temp-and-rename, with no lock, on trust
  answers and `codex mcp add`. Two unlocked read-modify-write writers; the
  loser's keys vanish until the next start reseeds them. Codex does
  nothing about a foreign writer. The preflight's own `codex mcp add
  context7` is one such write, run once because it checks `codex mcp get`
  first.
- **`.codex/auth.json`**. wormhole copies or binds, never merges. Codex's
  in-place truncate write means a reader racing it can see a torn file,
  and the guarded reload is per process. Two boxes on one shared
  `auth.json` is the Codex case to avoid, or to accept as "one re-login
  per lost race".
- **`.grok/config.toml`** (`GROK_CONFIG`: `ui.permission_mode`,
  `features.web_fetch`). grok takes its "config-init flock" around every
  `config.toml` write. wormhole does not take that lock, so wormhole's
  seed is the one writer outside grok's protocol. The fix is small: take
  an exclusive `flock` on the same lock file grok uses before seeding, or
  do not seed while a claim on the home is held by another process.
  grok's watcher reloads `config.toml` live, so a seed while A runs
  reaches A.
- **`.grok/auth.json`** and **`trusted_folders.toml`**. wormhole replaces
  the credential file whole (`CredentialWrite::Replace`) and never touches
  the trust store. grok's `auth.json.lock` is not taken by wormhole's
  replace, so a copy-in at start races a refresh in a running session; a
  start that finds the home claimed should skip the credential copy.

The common shape: every collision above is between wormhole's unlocked
rename and a vendor's own write, and every one is avoidable by the rule
[shared-box.md](shared-box.md) already proposed, tightened: when a start
finds the home in use by another process, write only the new workspace's
per-directory keys, do it before the new agent starts, and for grok take
grok's own lock file first. Claude Code and grok would then see the change
live; Codex only at its next start.

### (c) Live or next start, per channel

| Channel | Claude Code | Codex | grok |
|---|---|---|---|
| Settings file (`settings.json` / `config.toml`) | Live, file watcher; `model` and effort at start only (§1c) | Next start; no watcher found in `config/`, and trust is read into the layer stack at load (`core/src/config/mod.rs`); not verified further | Live for `~/.grok/config.toml` (watcher, §3); `pager.toml` "Changes apply on restart" |
| Hooks | Live, "picked up automatically by the file watcher" (§1c) | Not researched here | Live on `r` in the Hooks tab ("Reload all hooks from disk"); otherwise next start |
| Instructions file (`CLAUDE.md` / `AGENTS.md` / `rules/`) | At launch, re-read after `/compact` and on `/cd` (§1c) | Not researched here | Not researched here |
| Skills | Live, watched (§1c) | Not researched here | Live, "Grok reloads skills when files change on disk" |
| MCP config | Live for `claude mcp add/remove`; plugin servers on `/reload-plugins` (§1c) | Next start for `config.toml` edits; not verified further | Live via config hot-reload (`07-mcp-servers.md`) |
| The CLI binary | Next start; running process keeps the old file; version cleanup can remove it (§1d) | Preflight swaps `~/.local/bin/codex` by `mv`; the running process keeps its open inode (kernel semantics, not a vendor statement) | Next start or in-TUI restart; symlink swap (§3) |
| Cargo config (`$CARGO_HOME/config.toml`) | Read by every `cargo` invocation in every project (§5b) | same | same |
| Cargo package cache | Locked; concurrent builds designed for (§5a) | same | same |
| rustup toolchains | Not locked; `settings.toml` is (§5c) | same | same |
| Shell rc files | Read by each new shell only; a running shell never re-reads (POSIX shell semantics, not vendor-specific). grok's installer appends a `# >>> grok installer >>>` block to `~/.bashrc` on every install (install.sh) | same | same |
| Login file | `.credentials.json`: the vendor locks refresh across processes since 2.1.248 (§1b) | `auth.json`: in-place write, in-process lock, guarded reload before refresh (§2) | `auth.json`: flock plus rename plus adopt-sibling (§3); watcher reloads it live |

Three things the table makes plain. First, for Claude Code and grok
"live" is the default, so a shared home is not "two sessions with two
frozen configs": a change made in project A is in project B's running
session within seconds, and B's `ConfigChange` hooks see it. That is a
feature for the operator and a channel for a hostile agent, and it is the
concrete form of the "blast radius" argument in
[shared-box.md](shared-box.md). Second, `$CARGO_HOME/config.toml` is the
widest channel in the table: it is not live, but it is read by every
build in every project with no watcher, no lock and no prompt, and its
keys run programs. A shared `~/.cargo` needs that file to be either
read-only to the agent or absent. Third, the only place where the vendor
gives no help at all is Codex's `auth.json`; every other shared file is
either locked by the vendor, tolerant of a lost write, or healed by the
next start.
