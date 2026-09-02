# Claude Code updates, and real terminal attach

Primary-source research for two open wormhole questions:

1. **Updates.** What Claude Code's auto-updater actually does, where each
   install method writes on disk, and exactly which knob turns it off.
2. **Attach.** Prior art for making `wormhole attach` a *real* attach — the
   same live terminal view of the already-running process — rather than
   `docker exec` semantics.

Every claim below is followed by the URL of the source that owns it. Where a
source contradicts what we believed locally, the contradiction is called out
in bold. Where no primary source exists, it says **no primary source found**
rather than guessing.

Research done 2026-09-02 against the live docs. Claude Code's documentation
moved host during this research: `docs.claude.com/en/docs/claude-code/*` now
issues a `301 Moved Permanently` to `code.claude.com/docs/en/*`. All doc URLs
below are the current ones. Each page is also served as raw Markdown by
appending `.md` to the path, which is how most of the exact strings here were
pulled.

---

## What the evidence says

**Updates**

1. The env var is `DISABLE_AUTOUPDATER=1`. It stops only the *background*
   check; `claude update` and `claude install` still work. `DISABLE_UPDATES=1`
   is the strict one that blocks everything.
2. There is **no `autoUpdates` boolean settings key and no `autoUpdaterStatus`
   key** in the current settings reference. The only update-related settings
   keys today are `autoUpdatesChannel`, `minimumVersion`,
   `requiredMinimumVersion`, and `requiredMaximumVersion`. Auto-updates are
   turned off through `env` → `DISABLE_AUTOUPDATER`, not through a dedicated key.
3. **The npm package no longer ships a JavaScript bundle.** As of v2.1.198 it
   installs the *same native binary* as the standalone installer, pulled in as
   a per-platform optional dependency and linked into place by a postinstall
   script. This changes what our box image actually contains.
4. Native installs write to `~/.local/share/claude/versions/` with a symlink
   at `~/.local/bin/claude` — **both inside `$HOME`, which wormhole persists**.
   npm-global installs write into the npm global directory, which in our box
   is on the throwaway root filesystem.
5. Anthropic publishes a **GPG-signed `manifest.json` per release carrying
   SHA-256 hex checksums** for every platform binary, plus plain-text channel
   pointer files. This is a better answer to "bump the pin" than the npm
   registry, whose `dist.integrity` is sha512-base64 and whose `dist.shasum`
   is SHA-1 hex — neither is sha256 hex.
6. `~/.claude/.last-update-result.json` is **undocumented**. It appears only in
   a user-filed issue on the official tracker, which quotes the identical JSON
   shape we see locally.

**Attach**

7. `docker attach` and `docker exec` differ exactly as we assumed: attach
   joins the *existing* PID 1 process's streams, exec spawns a *new* process.
   Multiple attach clients are explicitly supported: output is fanned out
   through a broadcaster, stdin is a single shared pipe, and there is **no size
   arbitration at all** — every client independently answers `SIGWINCH` by
   calling the resize API, so the last resize wins.
8. The tiny attach tools converge on one shape: a server process that owns a
   PTY master, a unix socket, and raw byte relay with no terminal emulation.
   dtach admits multiple clients with different sizes are broken and offers
   `^L` as the manual fix; abduco fixes it with a documented priority rule
   (most-recently-connected non-read-only client controls the size).
9. tmux is the only one that *solves* the conflict rather than picking a
   winner: `window-size` takes `largest`, `smallest`, `manual`, or `latest`,
   and `attach-session -d` detaches every other client so the question does
   not arise.
10. **Claude Code already has a first-party attach.** `claude attach <id>`
    attaches to a background session run by a supervisor daemon, with each
    session's terminal in "its own host process". Detach is `←` on an empty
    prompt or `Ctrl+Z`. Attached sessions are forced into fullscreen mode
    "because a background session has no terminal scrollback to append to" —
    which is the same constraint any attach design hits.
11. Running two `claude` processes on one session is documented and is
    documented as *lossy*: "If you resume the same session in two terminals
    without forking, messages from both interleave into one transcript."
12. `vt100::Screen::contents_formatted()` and `state_formatted()` exist and do
    what a screen-restore design needs. They return the **visible** screen
    only; scrollback is reachable but only by moving an offset and reading
    repeatedly, not in one call.

---

# Part 1 — Claude Code auto-update

## 1.1 The officially supported install methods today

The install page presents four first-class methods in tabs, with the native
install marked **(Recommended)** — npm is no longer the headline:

- **Native install (Recommended)**: `curl -fsSL https://claude.ai/install.sh | bash`
  on macOS, Linux and WSL; `irm https://claude.ai/install.ps1 | iex` on Windows
  PowerShell; `curl -fsSL https://claude.ai/install.cmd -o install.cmd && install.cmd && del install.cmd`
  on Windows CMD.
- **Homebrew**: `brew install --cask claude-code`. Two casks exist:
  "`claude-code` tracks the stable release channel, which is typically about a
  week behind and skips releases with major regressions. `claude-code@latest`
  tracks the latest channel and receives new versions as soon as they ship."
- **WinGet**: `winget install Anthropic.ClaudeCode`.
- **Linux package managers**: signed apt, dnf and apk repositories at
  `https://downloads.claude.ai/claude-code/{apt,rpm,apk}/{stable,latest}`.

<https://code.claude.com/docs/en/setup>

npm is documented separately under "Advanced installation options":

> You can also install Claude Code as a global npm package. As of v2.1.198, the
> npm package requires Node.js 22 or later.

<https://code.claude.com/docs/en/setup#install-with-npm>

### 1.1.1 The npm package is a native-binary wrapper, not a JS bundle

**This contradicts the mental model our box image was probably built on.** The
docs are explicit:

> The npm package installs the same native binary as the standalone installer.
> npm pulls the binary in through a per-platform optional dependency such as
> `@anthropic-ai/claude-code-darwin-arm64`, and a postinstall step links it
> into place. The installed `claude` binary does not itself invoke Node.

<https://code.claude.com/docs/en/setup#install-with-npm>

Supported platforms are listed as `darwin-arm64`, `darwin-x64`, `linux-x64`,
`linux-arm64`, `linux-x64-musl`, `linux-arm64-musl`, `win32-x64`, and
`win32-arm64`, and "Your package manager must allow optional dependencies."
(same URL)

Confirmed live against the registry: `@anthropic-ai/claude-code@2.1.258`
declares `engines: {"node":">=22.0.0"}` and eight `optionalDependencies`, one
per platform, each pinned to the exact same version string. Retrieved with
`Accept: application/vnd.npm.install-v1+json` from
<https://registry.npmjs.org/@anthropic-ai/claude-code>.

The failure mode is documented, and its message is worth knowing because it is
what a broken box image would print:

> ```
> Error: claude native binary not installed.
>
> Either postinstall did not run (--ignore-scripts, some pnpm configs)
> or the platform-native optional dependency was not downloaded
> (--omit=optional).
>
> Run the postinstall manually (adjust path for local vs global install):
>   node node_modules/@anthropic-ai/claude-code/install.cjs
>
> Or reinstall without --ignore-scripts / --omit=optional.
> ```

<https://code.claude.com/docs/en/troubleshoot-install#native-binary-not-found-after-npm-install>

That page also notes there is no fallback: "The native binary is delivered only
as an optional dependency, so there is no JavaScript fallback if it is skipped,
and running `install.cjs` again can't place a binary that was never downloaded."
(same URL)

## 1.2 What the auto-updater does, per install method

The general behaviour:

> Claude Code checks for updates on startup and periodically while running.
> Updates download and install in the background, then take effect the next
> time you start Claude Code.
>
> Run `claude doctor` to see the result of the most recent update attempt.

<https://code.claude.com/docs/en/setup#auto-updates>

Which methods auto-update at all:

| Method | Auto-updates? | Source |
|---|---|---|
| Native installer | Yes. "Native installations automatically update in the background to keep you on the latest version." | <https://code.claude.com/docs/en/setup#install-claude-code> |
| npm global | Yes (implied by the npm-specific failure path below) | <https://code.claude.com/docs/en/setup#auto-updates> |
| Homebrew | No. "Homebrew installations do not auto-update." | <https://code.claude.com/docs/en/setup#install-claude-code> |
| WinGet | No. "WinGet installations do not auto-update." | same |
| apt / dnf / apk | No. "Package manager installations do not auto-update through Claude Code; updates arrive through your normal system upgrade workflow." | <https://code.claude.com/docs/en/setup#install-with-linux-package-managers> |

Homebrew and WinGet can opt in:

> To have Claude Code run the upgrade command for you on Homebrew or WinGet,
> set `CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE` to `1`. Claude Code then runs
> the upgrade in the background when a new version is available and shows a
> restart prompt on success. […] apt, dnf, and apk continue to require a manual
> upgrade because those commands need elevated privileges.

<https://code.claude.com/docs/en/setup#auto-updates>

### 1.2.1 Exactly where a native install writes — inside `$HOME`

> On macOS and Linux, the native installer manages the launcher at
> `~/.local/bin/claude` as a symlink into `~/.local/share/claude/versions/`.

<https://code.claude.com/docs/en/setup#auto-updates>

Confirmed twice more. From the uninstall instructions:

> ```bash
> rm -f ~/.local/bin/claude
> rm -rf ~/.local/share/claude
> ```

<https://code.claude.com/docs/en/setup#uninstall-claude-code>

And from the checksum-verification instructions: "To verify an installed native
binary instead, run the command against `~/.local/share/claude/versions/VERSION`,
replacing VERSION with the release you set in Step 2."
<https://code.claude.com/docs/en/setup#binary-integrity-and-code-signing>

There is a documented custom-launcher escape hatch, added in v2.1.207:

> If you replace that launcher with your own script or symlink, auto-update and
> `claude update` leave it in place: new versions still install under the
> `versions/` directory, and your launcher decides which version runs. Before
> v2.1.207, the auto-updater replaced a custom launcher at that path with its
> own symlink on every update.
>
> With a custom launcher, Claude Code also keeps every installed version on disk
> because it can't tell which version the launcher needs. `claude doctor`
> reports a launcher that the native installer didn't create.

<https://code.claude.com/docs/en/setup#auto-updates>

**This is directly relevant to wormhole's split filesystem.** Both native
install paths sit under `$HOME`, which the box persists. An npm-global install
at prefix `/usr/local` sits on the throwaway root.

### 1.2.2 Exactly where an npm global install writes — no exact path documented

The docs never give the literal path an npm-global auto-update writes to. They
refer to it only as "the npm global directory":

> If an npm global install can't auto-update because the npm global directory
> isn't writable, Claude Code shows a one-time notice at startup, and
> `claude doctor` lists the available fixes.

<https://code.claude.com/docs/en/setup#auto-updates>

The troubleshooting page uses `npm root -g` as the way to find it, and the
package directory is `$(npm root -g)/@anthropic-ai/claude-code`:

> ```bash
> rm -rf "$(npm root -g)/@anthropic-ai/claude-code"
> ```

<https://code.claude.com/docs/en/troubleshoot-install#npm-enotempty-during-update-or-reinstall>

**No primary source found** for the exact file the npm-global updater
rewrites, or whether it rewrites the package directory versus the `bin` shim.
The three locations a `claude` binary can come from are documented, though:

> `~/.local/bin/claude` is the native installer, `~/.claude/local/` is a legacy
> local npm install created by older versions of Claude Code, and the npm
> global list shows a `-g` install

<https://code.claude.com/docs/en/troubleshoot-install#check-for-conflicting-installations>

### 1.2.3 `installMethod` and `.last-update-result.json` are undocumented

Neither `installMethod` in `~/.claude.json` nor `~/.claude/.last-update-result.json`
appears anywhere in the official documentation. I checked:

- the full `~/.claude.json` "Global config settings" section, which enumerates
  `autoConnectIde`, `autoInstallIdeExtension`, `diffTool`,
  `externalEditorContext`, and two removed keys — and nothing about installs:
  <https://code.claude.com/docs/en/settings-reference#global-config-settings>
- the complete `~/.claude` directory inventory, which lists transcripts,
  `history.jsonl`, `backups/`, `remote-settings.json`, `daemon.log`,
  `daemon/roster.json`, `jobs/<id>/state.json` — and no update-result file:
  <https://code.claude.com/docs/en/claude-directory>

**No primary source found** for either name. The only corroboration is a
user-filed bug on the official tracker, which quotes the identical JSON shape:

> `.last-update-result.json` at time of failure:
> `{"timestamp":"2026-07-29T14:33:52.420Z","path":"native","outcome":"failed","status":"install_failed","version_from":"2.1.220","version_to":null,"error_code":null}`

<https://github.com/anthropics/claude-code/issues/82408>

That issue is a user report, not documentation, but it is useful evidence: the
`path` field there reads `"native"` where ours reads `"npm-global"`, which
strongly suggests `path` records which updater path ran. The same issue reports
the status-line string on failure as `auto-update failed - run claude doctor`,
and complains that `claude doctor` "only compares installed vs. latest version,
not the update-attempt failure state in `~/.claude/.last-update-result.json`".
Treat both strings as unverified against a first-party source.

## 1.3 Turning the auto-updater off — the exact names

### `DISABLE_AUTOUPDATER` — stops the background check only

The setup page gives the canonical recipe, in the `env` block of a settings
file:

> Set `DISABLE_AUTOUPDATER` to `"1"` in the `env` key of your `settings.json` file:
>
> ```json
> {
>   "env": {
>     "DISABLE_AUTOUPDATER": "1"
>   }
> }
> ```
>
> `DISABLE_AUTOUPDATER` only stops the background check; `claude update` and
> `claude install` still work. To block all update paths, including manual
> updates, set `DISABLE_UPDATES` instead. Use this when you distribute Claude
> Code through your own channels and need users to stay on the version you
> provide.

<https://code.claude.com/docs/en/setup#disable-auto-updates>

The env-var reference row, verbatim:

> | `DISABLE_AUTOUPDATER` | Set to `1` to disable automatic background updates. Manual `claude update` still works. Use `DISABLE_UPDATES` to block both |

<https://code.claude.com/docs/en/env-vars>

### `DISABLE_UPDATES` — blocks everything

> | `DISABLE_UPDATES` | Set to `1` to block all updates including manual `claude update` and `claude install`. Stricter than `DISABLE_AUTOUPDATER`. Use when distributing Claude Code through your own channels and users should not self-update |

<https://code.claude.com/docs/en/env-vars>

### The plugin-updater carve-out

Disabling the main updater does not necessarily stop plugin updates, and there
is a variable that deliberately re-enables them:

> | `FORCE_AUTOUPDATE_PLUGINS` | Set to `1` to force plugin auto-updates even when the main auto-updater is disabled via `DISABLE_AUTOUPDATER` |

<https://code.claude.com/docs/en/env-vars>

Separately, marketplace auto-update is its own mechanism: an `autoUpdate`
Boolean per marketplace, where "`claude-plugins-official` and most other
official Anthropic marketplaces default to `true`, and third-party marketplaces
default to `false`."
<https://code.claude.com/docs/en/settings-reference> (the `extraKnownMarketplaces`
entry)

### There is no `autoUpdates` settings key, and nothing is marked deprecated

**This contradicts the premise in the brief.** I searched the complete settings
reference — every key, its full entry, and the deprecation notices — for
`autoUpdates` as a standalone key and for `autoUpdaterStatus`. Neither exists.
The page *does* carry explicit deprecation notices for other keys
(`disableArtifact` → `enableArtifact`, `includeCoAuthoredBy` → `attribution`,
`ignorePatterns` → permission deny rules, and removal notices for
`permissionExplainerEnabled` and `teammateDefaultModel`), so the absence is not
because the page omits deprecations.

<https://code.claude.com/docs/en/settings-reference#all-settings>

The `autoUpdatesChannel` entry closes the loop, pointing at the env var rather
than at a sibling key:

> To turn auto-updates off entirely, set `DISABLE_AUTOUPDATER` in `env`.

<https://code.claude.com/docs/en/settings-reference#autoupdateschannel>

Note that `env` values in a settings file win over the shell:

> When the same variable is set in both your shell and a settings file `env`
> block, the settings file value applies. Claude Code writes each `env` entry
> into the process environment, replacing the value inherited from the shell.

<https://code.claude.com/docs/en/env-vars#precedence>

`CLAUDE_CODE_AUTO_UPDATE` does not appear in the env-var reference. **No primary
source found** for that name.

## 1.4 Pinning to an exact version

There are four distinct mechanisms, and only one of them is a hard pin.

### Install-time exact version (native installer)

> To install a specific version number:
>
> ```bash
> curl -fsSL https://claude.ai/install.sh | bash -s 2.1.89
> ```
>
> To confirm which version installed, run `claude --version`: the command prints
> the exact version you passed, such as `2.1.89 (Claude Code)`.

The installer also accepts a channel name, and "The channel you choose at
install time becomes your default for auto-updates."

<https://code.claude.com/docs/en/setup#install-a-specific-version>

### Install-time exact version (npm)

Documented in the dev-container guidance rather than the setup page:

> The Dev Container Feature always installs the latest Claude Code release. To
> pin a specific Claude Code version for reproducible builds, install it from
> your Dockerfile with `npm install -g @anthropic-ai/claude-code@X.Y.Z` instead
> of using the feature, and set `DISABLE_AUTOUPDATER` as shown above.

<https://code.claude.com/docs/en/devcontainer#enforce-organization-policy>

The same page gives the container env recipe verbatim:

> ```json
> "containerEnv": {
>   "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
>   "DISABLE_AUTOUPDATER": "1"
> }
> ```

(same URL)

Note the npm upgrade caveat, which matters if a bump command ever shells out to
npm:

> To upgrade an npm installation, run `npm install -g @anthropic-ai/claude-code@latest`.
> Avoid `npm update -g`, which respects the semver range from the original
> install and may not move you to the newest release.

<https://code.claude.com/docs/en/setup#install-with-npm>

And the `sudo` warning: "Do NOT use `sudo npm install -g` as this can lead to
permission issues and security risks." (same URL)

### `autoUpdatesChannel` — a channel, not a pin

- `"latest"` (default): "receive new features as soon as they're released"
- `"stable"`: "use a version that is typically about one week old, skipping
  releases with major regressions"

Set via `/config` → **Auto-update channel**, or in `settings.json`. Homebrew
ignores it — the cask name picks the channel instead.

<https://code.claude.com/docs/en/setup#configure-release-channel>,
<https://code.claude.com/docs/en/settings-reference#autoupdateschannel>

### `minimumVersion` — a floor on updates only

> The `minimumVersion` setting establishes a floor. Background auto-updates and
> `claude update` refuse to install any version below this value […] The
> `minimumVersion` pin only constrains updates. To make Claude Code refuse to
> start outside a version range, use the managed settings
> `requiredMinimumVersion` and `requiredMaximumVersion` instead. Updates also
> respect the `requiredMaximumVersion` ceiling.

<https://code.claude.com/docs/en/setup#pin-a-minimum-version>

### `requiredMinimumVersion` / `requiredMaximumVersion` — managed-only, block startup

Both are `Managed` scope: "Claude Code gives no warning when it ignores the key
elsewhere." `requiredMaximumVersion` sets "the newest Claude Code version your
organization allows to start. When the running version is newer, Claude Code
exits at startup". `requiredMinimumVersion` is the mirror image. Both need
v2.1.163 or later, and both keep `claude update`, `claude install` and
`claude doctor` working "so users can recover".

<https://code.claude.com/docs/en/settings-reference#requiredmaximumversion>,
<https://code.claude.com/docs/en/settings-reference#requiredminimumversion>

**Combining `requiredMinimumVersion` and `requiredMaximumVersion` at the same
value is the closest documented thing to a hard exact-version pin**, and it is
enforced at startup, not at update time. It requires a managed settings file
(`/etc/claude-code/managed-settings.json` on Linux —
<https://code.claude.com/docs/en/devcontainer#enforce-organization-policy>).

## 1.5 What happens when an update fails

### npm global directory not writable

> If an npm global install can't auto-update because the npm global directory
> isn't writable, Claude Code shows a one-time notice at startup, and
> `claude doctor` lists the available fixes.

<https://code.claude.com/docs/en/setup#auto-updates>

The linked remediation section is short and does not quote the notice text:

> If the native installer fails with permission errors, the target directory may
> not be writable. See Check directory permissions.
>
> If you previously installed with npm and are hitting npm-specific permission
> errors, switch to the native installer:
> `curl -fsSL https://claude.ai/install.sh | bash`

<https://code.claude.com/docs/en/troubleshoot-install#permission-errors-during-installation>

**No primary source found** for the literal text of the one-time startup notice,
for what happens on a read-only root filesystem specifically, or for whether the
notice repeats on every launch. The docs say "one-time notice", which suggests
it is suppressed after the first showing, but the docs do not say where that
suppression state is recorded. The user-filed issue at
<https://github.com/anthropics/claude-code/issues/82408> reports the opposite
behaviour for the *status line* ("Stale 'auto-update failed' status message is
misleading and can't be cleared") — again, a bug report, not documentation.

### Download failures

These *are* documented with exact strings:

> The connection to the download server closed while `claude install`,
> `claude update`, or the automatic updater was fetching the Claude Code binary,
> and the retries didn't recover. Claude Code retries the download when the
> connection drops, the transfer stalls, or the downloaded file fails its
> checksum, up to three attempts in total. A completed HTTP error, such as a
> 404, isn't retried because the server already answered.
>
> ```
> The connection dropped while downloading the update (attempt 3/3: aborted). Check your network — proxies sometimes cut off large downloads.
> ```
>
> The text in parentheses names which attempt failed and the underlying network
> error. `claude update` precedes the message with `Error: Failed to install
> native update` on stderr.
>
> A download that stays connected but doesn't finish within 10 minutes fails
> with `Download timed out: exceeded the total deadline` instead.

<https://code.claude.com/docs/en/errors#the-connection-dropped-while-downloading-the-update>

Also documented: the installer being OOM-killed prints

> ```
> Installation was killed before it could finish (exit code 137). This usually means the system ran out of memory.
> Claude Code needs roughly 512MB of free memory to install. Free up memory, then run this script again.
> ```

and "Claude Code needs roughly 512MB of free memory to install."
<https://code.claude.com/docs/en/errors#installation-was-killed-before-it-could-finish>

### `claude doctor` and `claude update`

`claude doctor`:

> `claude doctor` prints read-only installation and settings diagnostics without
> starting a session, including install health, settings-file validation errors,
> and any warnings with suggested fixes.

<https://code.claude.com/docs/en/setup#verify-your-installation>

There is an in-session `/doctor` too: "run `/doctor` inside Claude Code for an
automated check of your installation, settings, extensions, and context usage;
it proposes fixes it can apply after you confirm. If `claude` won't start at
all, run `claude doctor` from your shell instead."
<https://code.claude.com/docs/en/troubleshooting>

`claude update`, with its exact output strings:

> To apply an update immediately without waiting for the next background check,
> run:
>
> ```bash
> claude update
> ```
>
> When an update installs, the command reports `Successfully updated from <old
> version> to version <new version>`. If you're already on the newest version,
> it reports `Claude Code is up to date (<version>)`. Installs managed by
> Homebrew, WinGet, or apk report `Claude is up to date!` instead.

<https://code.claude.com/docs/en/setup#update-manually>

One known hang, fixed in v2.1.214, worth knowing in a box where shell config
paths might be odd:

> `claude update` and `claude doctor` scan your shell configuration files for an
> outdated `claude` alias: `~/.zshrc`, `~/.bashrc`, and `~/.config/fish/config.fish`,
> plus on macOS the first of `~/.bash_profile`, `~/.bash_login`, or `~/.profile`
> that exists. If you set `ZDOTDIR`, the Zsh file is `$ZDOTDIR/.zshrc` instead.
> When one of those paths is a directory, Claude Code skips it and both commands
> complete normally. Before v2.1.214, a directory at one of those paths made both
> commands hang […] `claude update` hung right after printing `Checking for updates`.

<https://code.claude.com/docs/en/troubleshoot-install#claude-update-or-claude-doctor-hangs>

### The supervisor restarts itself across updates

Relevant because the supervisor's state lives in the persisted `$HOME` while the
binary it points at may not:

> **After an auto-update**: the supervisor restarts itself onto the new version
> and moves idle sessions over in the background. Sessions that are working,
> waiting on you, or attached aren't interrupted.

and

> The command also warns when the running supervisor is on a different version
> than the `claude` you invoked, which happens after an update the supervisor
> hasn't restarted into yet. The warning shows both versions and tells you to
> run `claude daemon stop --any` to pick up the new version.

<https://code.claude.com/docs/en/agent-view#the-supervisor-process>

Supervisor state paths, all under the persisted `$HOME`:

| Path | Contents |
|---|---|
| `~/.claude/daemon.log` | Supervisor log |
| `~/.claude/daemon/roster.json` | List of running background sessions, used to reconnect after a restart |
| `~/.claude/jobs/<id>/state.json` | Per-session state shown in agent view |
| `~/.claude/jobs/<id>/tmp/` | Per-session scratch directory |

<https://code.claude.com/docs/en/agent-view#where-state-is-stored>

Also documented there: "If you set `CLAUDE_CONFIG_DIR`, the supervisor uses that
directory instead of `~/.claude` and runs as a separate instance with its own
sessions." (same URL)

## 1.6 Container-relevant environment variables — and which ones do *not* touch updates

The one variable that actually covers updates as part of a broader sweep:

> | `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Set to any non-empty value, such as `1`, to disable nonessential network traffic: auto-updates, telemetry, error reporting, the `/feedback` command, Claude-drafted feedback, release notes, gateway model discovery refreshes, and availability checks such as the fast mode check. It also stops the background runs of plugin `command` sources, which are local commands rather than network traffic, because they can trigger dependency installs. **Setting it to `0` or `false` still disables this traffic**, unlike most on/off variables; unset the variable to allow it again. Also disables feature-flag fetching, which makes Remote Control and the other features that need feature-flag fetching unavailable. Official plugin marketplace auto-install isn't covered; disable it with `CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL` |

<https://code.claude.com/docs/en/env-vars>

The rest, verbatim, with an explicit note on whether each affects updates:

| Variable | Documented effect | Affects updates? |
|---|---|---|
| `DISABLE_TELEMETRY` | "Set to any non-empty value, such as `1`, to opt out of telemetry. **Setting it to `0` or `false` still opts out** […] Telemetry events don't include user data like code, file paths, or bash commands. Also disables feature-flag fetching with the same effect as `DISABLE_GROWTHBOOK`" | **No.** Updates are not mentioned. |
| `DISABLE_ERROR_REPORTING` | "Set to any non-empty value, such as `1`, to opt out of error reporting. **Setting it to `0` or `false` still opts out**, unlike most on/off variables; unset the variable to turn error reporting back on" | **No.** |
| `DISABLE_BUG_COMMAND` | Documented only as a legacy alias: the current name is `DISABLE_FEEDBACK_COMMAND`, which "Set to `1` to disable the `/feedback` command and Claude-drafted feedback. Also disables `/bug` and `/share`, which report through the same path; before v2.1.212 they were aliases of `/feedback` […] **The older name `DISABLE_BUG_COMMAND` is also accepted**" | **No.** |
| `CLAUDE_CODE_DISABLE_TERMINAL_TITLE` | "Set to `1` to disable automatic terminal title updates based on conversation context. In Agent SDK and `claude -p` sessions, this also skips the background small/fast-model request that generates the session title" | **No.** The word "updates" here means terminal-title updates, not software updates. |
| `DISABLE_COST_WARNINGS` | "Set to `1` to disable cost warning messages" | **No.** |
| `DO_NOT_TRACK` | "Set to `1` to opt out of telemetry, with the same effect as `DISABLE_TELEMETRY` […] Claude Code reads this variable as a standard boolean, so `0` leaves telemetry on, and honors it as the cross-tool convention recognized by many developer CLIs" | **No.** |
| `DISABLE_GROWTHBOOK` | "Set to `1` or `true` to disable GrowthBook feature-flag fetching and use code defaults for every flag […] Setting it to `0` or `false` leaves fetching on. Telemetry event logging stays on unless `DISABLE_TELEMETRY` is also set" | **No.** |
| `CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE` | "Set to `1` to let Claude Code run your package manager's upgrade command in the background when a new version is available. Applies to Homebrew and WinGet installations." | **Yes**, but only Homebrew/WinGet, and it *enables* rather than disables. |
| `FORCE_AUTOUPDATE_PLUGINS` | "Set to `1` to force plugin auto-updates even when the main auto-updater is disabled via `DISABLE_AUTOUPDATER`" | **Yes**, in the wrong direction. |

<https://code.claude.com/docs/en/env-vars>

`DISABLE_NON_ESSENTIAL_MODEL_CALLS` and `CLAUDE_CODE_DISABLE_TERMINAL_QUERIES`
do not appear in the current env-var reference. **No primary source found** for
either name.

Two behavioural quirks worth carrying into any config we write, because they
invert normal boolean handling. The reference calls them out explicitly:

> Some variables read only whether you set them at all, so any non-empty value
> including `0` turns the behavior on, and you turn the behavior off by
> unsetting the variable or setting it to an empty value. These variables work
> that way:
>
> * `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`
> * `DISABLE_TELEMETRY`
> * `DISABLE_ERROR_REPORTING`
> * `CLAUDE_CODE_TMUX_TRUECOLOR`
> * `FALLBACK_FOR_ALL_PRIMARY_MODELS`
> * `IS_DEMO`

<https://code.claude.com/docs/en/env-vars#variables>

And the cost of the broad sweep: `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`,
`DISABLE_TELEMETRY`, `DO_NOT_TRACK` and `DISABLE_GROWTHBOOK` each disable
feature-flag fetching, which the docs say makes Remote Control unavailable:

> **Feature-flag evaluation**: `DISABLE_TELEMETRY`, `DO_NOT_TRACK`,
> `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, and `DISABLE_GROWTHBOOK` each
> disable the feature-flag evaluation that Remote Control availability depends
> on. Unset the variable wherever it's set, in your shell environment or in the
> `env` block of a `settings.json` file, to use Remote Control.

<https://code.claude.com/docs/en/remote-control#requirements>

### Which hosts the updater contacts

Useful for an egress allowlist, and for confirming which registry each install
method uses:

| Host | Documented use |
|---|---|
| `downloads.claude.ai` | "Plugin executable downloads; native installer, native auto-updater, and update version checks" |
| `storage.googleapis.com` | "Native installer and native auto-updater on versions prior to 2.1.116" |
| `registry.npmjs.org` | "Plugin installs […] and the package registry for npm and bun installs of Claude Code itself" |
| `formulae.brew.sh` | "Update version checks on Homebrew installs. Other install methods don't contact this host" |
| `raw.githubusercontent.com` | "Changelog feed for `/release-notes`. In interactive sessions, Claude Code also fetches it in the background at startup when its cached changelog doesn't yet cover the running version, such as the first start after an update" |

<https://code.claude.com/docs/en/network-config#network-access-requirements>

The same page notes: "If you install Claude Code through npm or manage your own
binary distribution, end users don't need the native installer and auto-updater
uses of `downloads.claude.ai`, but npm and bun installs need their package
registry, `registry.npmjs.org`".

## 1.7 Resolving "latest version + hash" without executing anything

Two independent options exist. **The Anthropic-published one is strictly better
for our purpose because it is sha256 hex and GPG-signed.**

### Option A — Anthropic's signed release manifest (sha256 hex)

> Each release publishes a `manifest.json` containing SHA256 checksums for every
> platform binary. The manifest is signed with an Anthropic GPG key, so verifying
> the signature on the manifest transitively verifies every binary it lists.

<https://code.claude.com/docs/en/setup#binary-integrity-and-code-signing>

The documented URL shape, quoted from the verification steps:

> ```bash
> REPO=https://downloads.claude.ai/claude-code-releases
> VERSION=2.1.89
> curl -fsSLO "$REPO/$VERSION/manifest.json"
> curl -fsSLO "$REPO/$VERSION/manifest.json.sig"
> ```

(same URL)

Verified live. `GET https://downloads.claude.ai/claude-code-releases/2.1.258/manifest.json`
returns 1778 bytes with top-level keys `version`, `manifestSignatureEnforcement`,
`commit`, `buildDate`, `platforms`, `sdkCompat`, and a `platforms` map over
`darwin-arm64`, `darwin-x64`, `linux-arm64`, `linux-x64`, `linux-arm64-musl`,
`linux-x64-musl`, `win32-x64`, `win32-arm64`. The `linux-x64` entry is:

```json
{
  "binary": "claude",
  "checksum": "704f1334ac65d3e89e1c6c1d7663293ad786a6166afdb71b5075337df630f976",
  "size": 215473560
}
```

That `checksum` is 64 hex characters — **SHA-256 hex, which is exactly what our
manifest pins**. The detached signature at
`https://downloads.claude.ai/claude-code-releases/2.1.258/manifest.json.sig`
exists (confirmed with a HEAD request).

The signing key and its fingerprint are published at fixed URLs:

> ```bash
> curl -fsSL https://downloads.claude.ai/keys/claude-code.asc | gpg --import
> gpg --fingerprint security@anthropic.com
> ```
>
> Confirm the output includes this fingerprint:
>
> ```
> 31DD DE24 DDFA B679 F42D  7BD2 BAA9 29FF 1A7E CACE
> ```
>
> A valid result reports `Good signature from "Anthropic Claude Code Release
> Signing <security@anthropic.com>"`.

<https://code.claude.com/docs/en/setup#verify-the-manifest-signature>

Caveat, documented: "Manifest signatures are available for releases from
`2.1.89` onward. Earlier releases publish checksums in `manifest.json` without a
detached signature." (same URL)

There are also plain-text channel pointer files. These are not documented on the
setup page, but they exist and were verified live:

- `https://downloads.claude.ai/claude-code-releases/latest` → `2.1.258`
- `https://downloads.claude.ai/claude-code-releases/stable` → `2.1.236`

**No primary source found** documenting those two pointer URLs; they are
inferred from the documented `$REPO` base and confirmed empirically. Treat their
stability as unguaranteed.

The apt/dnf/apk repositories are separately signed, with a published apk key
checksum: "Verify the downloaded key with `sha256sum /etc/apk/keys/claude-code.rsa.pub`,
which should report `395759c1f7449ef4cdef305a42e820f3c766d6090d142634ebdb049f113168b6`."
<https://code.claude.com/docs/en/setup#install-with-linux-package-managers>

### Option B — the npm registry JSON API

The registry does expose everything needed without executing code, but **not in
sha256 hex**. npm's own registry documentation defines the fields:

> The `dist` object is generated by npm and may be relied upon. Each dist object
> has at least two fields:
>
> - `shasum`: the SHA-1 sum of the tarball
> - `integrity`: since Apr 2017, string in the format `<hashAlgorithm>-<base64-hash>`,
>   refer the Subresource Integrity and cacache package for more

<https://github.com/npm/registry/blob/main/docs/responses/package-metadata.md#dist>

The same page documents the abbreviated form, which is the cheap one to fetch:

> To request an _abbreviated_ document with only the fields required to support
> installation, set the `Accept` header in your request to the following string:
> `application/vnd.npm.install-v1+json`

(same URL). It adds: "For some packages in the registry, the full metadata is
over 10MB uncompressed. If the information you wish to use for a package is
present in the abbreviated version, you should prefer it over the full version."

Verified live against `https://registry.npmjs.org/@anthropic-ai/claude-code`
with that `Accept` header:

```
dist-tags: {"stable":"2.1.236","latest":"2.1.258","next":"2.1.258"}
dist.integrity: sha512-Zis1AYrHuCcK4V1tXJUkzJdklFsTvvqIcj7gk4K8lyEeJOW99ZoQH/E+WxugMqDEO7xncYZ41gydxlkSTmj/2Q==
dist.shasum:    55f84789ed34ca70702c043e7bcba88dde2daea3
dist.tarball:   https://registry.npmjs.org/@anthropic-ai/claude-code/-/claude-code-2.1.258.tgz
dist keys:      shasum, tarball, fileCount, integrity, signatures, unpackedSize
```

**Confirming the concern in the brief: `dist.integrity` is `sha512-` followed by
Base64, not sha256 hex, and `dist.shasum` is a 40-hex-character SHA-1, not
SHA-256.** Neither field is directly comparable to a sha256-hex pin. The
`<hashAlgorithm>-<base64-hash>` format comes from Subresource Integrity, whose
`hash-expression` grammar is defined in the W3C spec:
<https://www.w3.org/TR/SRI/>

Two further notes. The registry's `dist-tags` carry `stable` and `latest` that
match the `downloads.claude.ai` pointer files exactly (2.1.236 / 2.1.258 at time
of writing), so either source answers "what is the latest version". And the
`dist.signatures` field is npm's own ECDSA registry signature, documented at
<https://docs.npmjs.com/verifying-signatures-and-provenance-for-your-packages>
— a different trust root from Anthropic's GPG key.

Finally, note that the npm tarball is a *wrapper*: the real binary comes from
the per-platform optional dependency package (§1.1.1), so a hash of the
`@anthropic-ai/claude-code` tarball does not cover the binary that actually
runs. Hashing `@anthropic-ai/claude-code-linux-x64` would, or the
`downloads.claude.ai` manifest checksum would directly.

## 1.8 Where the docs disagree with our local picture

Restating the contradictions in one place, loudly:

1. **`autoUpdates` is not a settings key.** The brief asked whether it is
   deprecated in favour of something else. It is not in the current reference at
   all, alongside no `autoUpdaterStatus`. The documented off-switch is
   `env` → `DISABLE_AUTOUPDATER`, with `DISABLE_UPDATES` as the strict form.
   <https://code.claude.com/docs/en/settings-reference#all-settings>
2. **npm-global is no longer a JS install.** Since v2.1.198 it delivers the same
   native binary as the standalone installer, through platform optional
   dependencies plus a postinstall link step.
   <https://code.claude.com/docs/en/setup#install-with-npm>
3. **The native installer writes entirely inside `$HOME`**
   (`~/.local/share/claude/versions/` plus a `~/.local/bin/claude` symlink),
   which is the part of a wormhole box that *persists*. The npm prefix
   `/usr/local` is on the throwaway root.
   <https://code.claude.com/docs/en/setup#auto-updates>
4. **`installMethod: "global"` and `.last-update-result.json` are undocumented.**
   Our reading of them is inference from the file contents plus one user-filed
   issue, not from Anthropic documentation.
5. **The native installer is now the recommended method**, and npm has been
   demoted to "Advanced installation options".
   <https://code.claude.com/docs/en/setup#install-claude-code>

---

# Part 2 — Real terminal attach

Framing, for orientation. `wormhole attach` currently `setns()`es into the
box's user/uts/pid/mnt namespaces, forks, and execs the agent again — a second,
independent process. `wormhole box` allocates no PTY; the agent inherits the
launching terminal's stdio directly. The question is what a *real* attach —
the same live view of the already-running process — costs, and what everyone
who has built one had to decide.

## 2.1 `docker attach` versus `docker exec`

### The semantic difference, from the CLI reference

`docker attach`:

> Use `docker attach` to attach your terminal's standard input, output, and
> error (or any combination of the three) to a running container using the
> container's ID or name. This lets you view its output or control it
> interactively, as though the commands were running directly in your terminal.
>
> > **Note**
> > The `attach` command displays the output of the container's `ENTRYPOINT` and
> > `CMD` process. This can appear as if the attach command is hung when in fact
> > the process may simply not be writing any output at that time.

<https://docs.docker.com/reference/cli/docker/container/attach/>
(raw Markdown: <https://docs.docker.com/reference/cli/docker/container/attach.md>)

`docker exec`, by contrast:

> The `docker exec` command runs a new command in a running container.
>
> The command you specify with `docker exec` only runs while the container's
> primary process (`PID 1`) is running, and it isn't restarted if the container
> is restarted.
>
> The command runs in the default working directory of the container.

<https://docs.docker.com/reference/cli/docker/container/exec/>
(raw Markdown: <https://docs.docker.com/reference/cli/docker/container/exec.md>)

**That is precisely the distinction the brief draws.** Attach joins the streams
of the process that already exists; exec creates a second one that merely shares
the container's namespaces — which is what `wormhole attach` does today.

### Multiple attached clients — explicitly supported, output broadcast, stdin shared

The docs state it plainly:

> You can attach to the same contained process multiple times simultaneously,
> from different sessions on the Docker host.

<https://docs.docker.com/reference/cli/docker/container/attach.md>

The implementation is in moby's stream config, whose own comment describes the
fan-out:

> ```go
> // Config holds information about I/O streams managed together.
> //
> // config.StdinPipe returns a WriteCloser which can be used to feed data
> // to the standard input of the streamConfig's active process.
> // config.StdoutPipe and streamConfig.StderrPipe each return a ReadCloser
> // which can be used to retrieve the standard output (and error) generated
> // by the container's active process. The output (and error) are actually
> // copied and delivered to all StdoutPipe and StderrPipe consumers, using
> // a kind of "broadcaster".
> ```

and:

> ```go
> // NewConfig creates a stream config and initializes
> // the standard err and standard out to new unbuffered broadcasters.
> ```

<https://github.com/moby/moby/blob/master/daemon/internal/stream/streams.go>

Stdin is *not* broadcast — it is one pipe that every attached client writes
into:

> ```go
> // NewInputPipes creates new pipes for both standard inputs, Stdin and StdinPipe.
> func (c *Config) NewInputPipes() {
> 	c.stdin, c.stdinPipe = io.Pipe()
> }
> ```

(same URL). So with N attached clients: output is copied N ways, input from all
N is interleaved into one stream. There is no notion of a "primary" client.

Docker also documents its buffering:

> While a client is connected to container's `stdio` using `docker attach`,
> Docker uses a ~1MB memory buffer to maximize the throughput of the
> application. Once this buffer is full, the speed of the API connection is
> affected, and so this impacts the output process' writing speed. This is
> similar to other applications like SSH.

<https://docs.docker.com/reference/cli/docker/container/attach.md>

### Terminal resize is a separate API call, with no arbitration

Resizing is not part of the attach stream. The Docker CLI registers a `SIGWINCH`
handler per client and calls the resize endpoint:

> ```go
> // resizeTTYTo resizes TTY to specific height and width.
> func resizeTTYTo(ctx context.Context, apiClient resizeClient, id string, height, width uint, isExec bool) error {
> 	...
> 	_, err = apiClient.ExecResize(...)   // or apiClient.ContainerResize(...)
> ```
>
> ```go
> // initTtySize is to init the TTYs size to the same as the window, if there is an error, it will retry 10 times.
> ```
>
> ```go
> // MonitorTtySize updates the container tty size when the terminal tty changes size
> func MonitorTtySize(ctx context.Context, cli command.Cli, id string, isExec bool) error {
> 	initTtySize(ctx, cli, id, isExec, resizeTty)
> 	...
> 	gosignal.Notify(sigchan, signal.SIGWINCH)
> ```

<https://github.com/docker/cli/blob/master/cli/command/container/tty.go>

**Consequence: with multiple `docker attach` clients, whichever client last
resized wins, and every attaching client resizes on attach.** There is no
`largest`/`smallest` policy anywhere in the stack. This is the naive design that
tmux explicitly rejected (§2.3).

### `--detach-keys`

Per-command flag, with a config-file default:

> `--detach-keys` — Override the key sequence for detaching a container

<https://docs.docker.com/reference/cli/docker/container/attach.md> (also on
`docker exec`, <https://docs.docker.com/reference/cli/docker/container/exec/>)

The default and the format come from the top-level CLI reference:

> If the container was run with `-i` and `-t`, you can detach from a container
> and leave it running using the `CTRL-p CTRL-q` key sequence.

> The format of the `<sequence>` is a comma-separated list of either a letter
> [a-Z], or the `ctrl-` combined with any of the following:
>
> * `a-z` (a single lowercase alpha character)
> * `@` (at sign)
> * `[` (left bracket)
> * `\\` (two backward slashes)
> * `_` (underscore)
> * `^` (caret)

<https://docs.docker.com/reference/cli/docker/>

A persistent default goes in `~/.docker/config.json` as the `detachKeys`
property, e.g. `"detachKeys": "ctrl-e,e"` (same URL).

Note the sharp edge Docker documents: without `--detach-keys` or `CTRL-p CTRL-q`,
`CTRL-c` is proxied through as `SIGINT` because `--sig-proxy` defaults to
`true`:

> To stop a container, use `CTRL-c`. This key sequence sends `SIGKILL` to the
> container. If `--sig-proxy` is true (the default), `CTRL-c` sends a `SIGINT`
> to the container.

<https://docs.docker.com/reference/cli/docker/container/attach.md>

### Who owns the PTY, below Docker

runc's terminal documentation is the authoritative description, and it is worth
reading in full because it names the exact mechanism a host-side supervisor
needs.

In "new terminal" mode (`terminal: true` in `config.json`):

> In new terminal mode, `runc` will create a brand-new "console" (or more
> precisely, a new pseudo-terminal using the container's namespaced
> `/dev/pts/ptmx`) for your contained process to use as its `stdio`.
>
> When you start a process in new terminal mode, `runc` will do the following:
>
> 1. Create a new pseudo-terminal.
> 2. Pass the slave end to the container's primary process as its `stdio`.
> 3. Send the master end to a process to interact with the `stdio` for the
>    container's primary process.

<https://github.com/opencontainers/runc/blob/main/docs/terminals.md#new-terminal>

Two details from that section matter for any design that emulates it:

> **NOTE**: In new terminal mode, all three `stdio` file descriptors are the
> same underlying file. The reason for this is to match how a shell's `stdio`
> looks to a process (as well as remove race condition issues with having to
> deal with multiple master pseudo-terminal file descriptors). However this
> means that it is not really possible to uniquely distinguish between `stdout`
> and `stderr` from the caller's perspective.

and

> It should be noted that since a new pseudo-terminal is being used for
> communication with the container, some strange properties of pseudo-terminals
> might surprise you. For instance, by default, all new pseudo-terminals
> translate the byte `'\n'` to the sequence `'\r\n'` on both `stdout` and
> `stderr`.

(same URL)

How the master fd escapes the container — this is the whole trick:

> The way this problem is resolved is through the use of Unix domain sockets.
> There is a feature of Unix sockets called `SCM_RIGHTS` which allows a file
> descriptor to be sent through a Unix socket to a completely separate process
> (which can then use that file descriptor as though they opened it). When using
> `runc` in detached new terminal mode, this is how a user gets access to the
> pseudo-terminal's master file descriptor.
>
> To this end, there is a new option (which is required if you want to use `runc`
> in detached new terminal mode): `--console-socket`. This option takes the path
> to a Unix domain socket which `runc` will connect to and send the
> pseudo-terminal master file descriptor down. The general process for getting
> the pseudo-terminal master is as follows:
>
> 1. Create a Unix domain socket at some path, `$socket_path`.
> 2. Call `runc run` or `runc create` with the argument `--console-socket $socket_path`.
> 3. Using `recvmsg(2)` retrieve the file descriptor sent using `SCM_RIGHTS` by `runc`.
> 4. Now the manager can interact with the `stdio` of the container, using the
>    retrieved pseudo-terminal master.
>
> After `runc` exits, the only process with a copy of the pseudo-terminal master
> file descriptor is whoever read the file descriptor from the socket.

<https://github.com/opencontainers/runc/blob/main/docs/terminals.md#detached-new-terminal>

runc also carries a warning directly relevant to a sandbox that pivots root:

> **Be very careful when passing file descriptors to a container process.** Due
> to some Linux kernel (mis)features, a container with access to certain types of
> file descriptors (such as `O_PATH` descriptors) outside of the container's root
> file system can use these to break out of the container's pivoted mount
> namespace. This has resulted in CVEs in the past.

(same URL, referencing <https://nvd.nist.gov/vuln/detail/CVE-2016-9962>)

And the pass-through mode's own hazard, which is why containers use a fresh PTY
rather than handing the host terminal in:

> **You must be incredibly careful when using detached pass-through (especially
> in a shell).** The reason for this is that by using detached pass-through you
> are passing the host's terminal to the container […] `TIOCSTI` to fake input
> into the **host** shell

(same URL, "Detached Pass-Through")

**The design summary from runc: the PTY is created using the container's
namespaced `/dev/pts/ptmx`, the slave becomes the container process's stdio, and
the master is handed out of the container over a unix socket with `SCM_RIGHTS`
to a host-side supervisor that owns it for the container's lifetime.**

## 2.2 dtach and abduco

Two minimal implementations of exactly the shape wormhole would need. Both are
"a server process holding a PTY master, a unix domain socket, raw byte relay".

### dtach

Design, from the man page:

> `dtach` is a program that emulates the detach feature of screen. It is designed
> to be transparent and un-intrusive; it avoids interpreting the input and output
> between attached terminals and the program under its control. Consequently, it
> works best with full-screen applications such as emacs.

> Sessions are represented by Unix-domain sockets in the filesystem. No other
> permission checking other than the filesystem access checks is performed.
> `dtach` creates a master process that monitors the session socket, the program,
> and any attached terminals.

> `dtach` avoids interpreting the communication stream between the program and
> the attached terminals; it instead relies on the ability of the attached
> terminals to manage the screen.

<https://github.com/crigler/dtach/blob/master/dtach.1>

Modes: `-a` attach, `-A` attach-or-create, `-c` create-and-attach, `-n` create
detached (daemonizing), `-N` create detached in the foreground, `-p` pipe stdin
into a session. (same URL)

On attach:

> **-a** Attach to an existing session. `dtach` attaches itself to the session
> specified by `<socket>`. After the attach is completed, the window size of the
> current terminal is sent to the master process, and a redraw is also requested.

(same URL)

Detach key and its disable flag:

> **-e `<char>`** Sets the detach character to `<char>`. When the detach character
> is pressed, `dtach` detaches itself from the current session and exits. The
> process running in the session is unaffected by the detach. By default, the
> detach character is set to `^\` (Ctrl-\\).
>
> **-E** Disables the detach character. `dtach` does not try to scan input from
> the terminal for a detach character. The only way to detach from the session is
> then by sending the attaching process an appropriate signal.

(same URL)

Redraw strategy — this is dtach's answer to "how does a newly attached client get
a screen", and it is notably *not* screen replay:

> **-r `<method>`** Sets the redraw method to `<method>`. The valid methods are
> `none`, `ctrl_l`, or `winch`.
>
> `none` disables redrawing completely, `ctrl_l` sends a Ctrl L character to the
> program if the terminal is in character-at-a-time and no-echo mode, and `winch`
> forces a WINCH signal to be sent to the program.
>
> When creating a new session, the specified method is used as the default redraw
> method for the session. If not specified, the `ctrl_l` method is used.

(same URL)

**Multiple clients: dtach admits this is broken and offers no arbitration.** From
the README:

> dtach is able to attach to the same session multiple times, though you will
> likely encounter problems if your terminals have different window sizes.
> Pressing ^L (Ctrl-L) will reset the window size of the program to match the
> current terminal.

<https://github.com/crigler/dtach/blob/master/README>

The README also states the implementation dependency, relevant to a box that
pivots root: "dtach may need access to various devices in the filesystem depending
on what `forkpty` does. For example, dtach on Linux usually needs access to
`/dev/ptmx` and `/dev/pts`." (same URL)

Also worth noting, a race dtach had to fix in 0.8: "When using `dtach -A` or
`dtach -c`, the master will now wait until the client attaches before trying to
read from the program being executed. This avoids a race condition when the
program prints something and exits before the client can attach itself."
(same URL)

### abduco

Same shape, cleaner statement of it:

> `abduco` disassociates a given application from its controlling terminal,
> thereby providing roughly the same session attach/detach support as `screen(1)`,
> `tmux(1)`, or `dtach(1)`.
>
> A session comprises of an `abduco` server process which spawns a user command in
> its own pseudo terminal (see `pty(7)`). Each session is given a name represented
> by a unix domain socket (see `unix(7)`) stored in the local file system. `abduco`
> clients can connect to it and their standard input output streams are relayed to
> the command supervised by the server.

And the key limitation, stated up front:

> `abduco` operates on the raw I/O byte stream without interpreting any terminal
> escape sequences. As a consequence **the terminal state is not preserved across
> sessions**. If this functionality is desired, it should be provided by another
> utility such as `dvtm(1)`.

<https://github.com/martanne/abduco/blob/master/abduco.1>

**abduco is the one that actually specifies a multi-client size policy.** From
the man page:

> **-l** Attach with the lowest priority, meaning this client will be the last to
> control the size.

> **SIGWINCH** — Whenever the primary client resizes its terminal the server
> process will deliver a `SIGWINCH` signal to the supervised process.

(same URL)

And spelled out in the README's "Improvements over dtach":

> **better resize handling** on shared sessions, resize request are only processed
> if they are initiated by the most recently connected, non read only client.

<https://github.com/martanne/abduco/blob/master/README.md>

So abduco's rule is: **most-recently-connected, non-read-only client owns the
size**; `-l` opts out; `-r` makes a client read-only:

> **-r** Read-only session, user input is ignored.

<https://github.com/martanne/abduco/blob/master/abduco.1>

The README is careful that read-only is a convenience, not a security boundary:

> Note that this is not a security feature, but only a convenient way to avoid
> accidental keyboard input.

<https://github.com/martanne/abduco/blob/master/README.md>

Other abduco features relevant to a box supervisor: session listing with a
connected-client marker (`*` = at least one client connected, `+` = command
terminated with no client connected), preserved exit status reported on
reattach, `SIGUSR1` to recreate a deleted socket, `SIGTERM` to detach a client,
and socket directories chosen from `$ABDUCO_SOCKET_DIR/abduco`, `$HOME/.abduco`,
`$TMPDIR/abduco/$USER`, `/tmp/abduco/$USER` (first that succeeds), with `$ABDUCO_SESSION`
and `$ABDUCO_SOCKET` exported to the supervised command.
<https://github.com/martanne/abduco/blob/master/abduco.1>

Default detach key is the same as dtach's: `Ctrl+\`, changed with `-e`. (same URL)

## 2.3 tmux and screen — how conflicting client sizes are resolved

tmux is the only tool surveyed that treats the multi-client size conflict as a
policy question with named answers. From the OpenBSD man page (the canonical
one; `tmux.1` in the tmux repo is the same file):

> **window-size** `largest | smallest | manual | latest`
>
> Configure how tmux determines the window size. If set to `largest`, the size of
> the largest attached session is used; if `smallest`, the size of the smallest.
> If `manual`, the size of a new window is set from the `default-size` option and
> windows are resized automatically. With `latest`, tmux uses the size of the
> client that had the most recent activity. See also the `resize-window` command
> and the `aggressive-resize` option.

<https://man.openbsd.org/tmux.1> (source:
<https://github.com/tmux/tmux/blob/master/tmux.1>)

Note `latest` is *most recent activity*, not most recently connected — a
different rule from abduco's.

`aggressive-resize` narrows the scope of that computation:

> **aggressive-resize** `[on | off]`
>
> Aggressively resize the chosen window. This means that tmux will resize the
> window to the size of the smallest or largest session (see the `window-size`
> option) for which it is the current window, rather than the session to which it
> is attached. The window may resize when the current window is changed on another
> session; this option is good for full-screen programs which support `SIGWINCH`
> and poor for interactive programs such as shells.

(same URL)

`attach-session -d` is the "make the conflict go away" escape hatch:

> **attach-session** `[-dErx] [-c working-directory] [-f flags] [-t target-session]`
> (alias: `attach`)
>
> If run from outside tmux, attach to `target-session` in the current terminal.
> […] If `-d` is specified, any other clients attached to the session are
> detached. If `-x` is given, send `SIGHUP` to the parent process of the client
> as well as detaching the client, typically causing it to exit.

(same URL)

tmux also has per-client flags that map almost exactly onto the abduco
`-l` / `-r` pair, set with `-f`:

> **ignore-size** — the client does not affect the size of other clients
>
> **read-only** — the client is read-only
>
> **no-detach-on-destroy** — do not detach the client when the session it is
> attached to is destroyed if there are any other sessions
>
> A leading `!` turns a flag off if the client is already attached.
>
> `-r` is an alias for `-f read-only,ignore-size`. When a client is read-only,
> only keys bound to the `detach-client` or `switch-client` commands have any
> effect.

(same URL)

**Summary of the three multi-client policies:**

| Tool | Size policy with N clients | Read-only client | Force-single-client |
|---|---|---|---|
| Docker | None. Every client resizes on attach and on `SIGWINCH`; last wins. | No | No |
| dtach | None. Documented as broken with differing sizes; `^L` re-syncs manually. | No | No |
| abduco | Most-recently-connected, non-read-only client owns the size. `-l` opts out. | `-r` | No |
| tmux | `window-size` = `largest` / `smallest` / `manual` / `latest`, plus `aggressive-resize`. | `-f read-only` / `-r` | `attach-session -d` |

**No primary source found** for GNU screen's equivalent policy; I did not locate
an authoritative screen man page section on multi-display size arbitration
during this research. screen does have `-x` (multi-display attach) and the
`defnonblock`/`fit`/`maxwin` family, but I have not verified any of it against
the manual, so it is not asserted here.

## 2.4 The mechanics a host-side PTY master needs

### Getting a pair

`posix_openpt(3)` is the standardised entry point, per `pty(7)`:

> An unused UNIX 98 pseudoterminal master is opened by calling `posix_openpt(3)`.
> (This function opens the master clone device, `/dev/ptmx`; see `pts(4)`.) After
> performing any program-specific initializations, changing the ownership and
> permissions of the slave device using `grantpt(3)`, and unlocking the slave
> using `unlockpt(3)`), the corresponding slave device can be opened by passing
> the name returned by `ptsname(3)` in a call to `open(2)`.

<https://man7.org/linux/man-pages/man7/pty.7.html>

`pts(4)` on the kernel side:

> When a process opens `/dev/ptmx`, it gets a file descriptor for a
> pseudoterminal master and a pseudoterminal slave device is created in the
> `/dev/pts` directory. Each file descriptor obtained by opening `/dev/ptmx` is
> an independent pseudoterminal master with its own associated slave, whose path
> can be found by passing the file descriptor to `ptsname(3)`.

> The Linux support for the above (known as UNIX 98 pseudoterminal naming) is
> done using the devpts filesystem, which should be mounted on `/dev/pts`.

<https://man7.org/linux/man-pages/man4/pts.4.html>

`openpty(3)` is the convenience wrapper, and it takes the initial window size:

> The `openpty()` function finds an available pseudoterminal and returns file
> descriptors for the master and slave in `amaster` and `aslave`. If `name` is
> not NULL, the filename of the slave is returned in `name`. If `termp` is not
> NULL, the terminal parameters of the slave will be set to the values in
> `termp`. If `winp` is not NULL, the window size of the slave will be set to
> the values in `winp`.

<https://man7.org/linux/man-pages/man3/openpty.3.html>

Note it is in `libutil` ("System utilities library (libutil, -lutil)"), not libc.
(same URL)

### Making the slave the controlling terminal

`setsid(2)` first:

> `setsid()` creates a new session if the calling process is not a process group
> leader. The calling process is the leader of the new session (i.e., its session
> ID is made the same as its process ID). The calling process also becomes the
> process group leader of a new process group in the session […]
>
> **Initially, the new session has no controlling terminal.** For details of how a
> session acquires a controlling terminal, see `credentials(7)`.

<https://man7.org/linux/man-pages/man2/setsid.2.html>

`credentials(7)` gives the acquisition rule:

> All of the processes in a session share a controlling terminal. The controlling
> terminal is established when the session leader first opens a terminal (unless
> the `O_NOCTTY` flag is specified when calling `open(2)`). **A terminal may be
> the controlling terminal of at most one session.**

<https://man7.org/linux/man-pages/man7/credentials.7.html>

That "first opens a terminal" path is not usable for an *inherited* fd, which is
why `TIOCSCTTY` exists:

> **TIOCSCTTY** — Make the given terminal the controlling terminal of the calling
> process. The calling process must be a session leader and not have a
> controlling terminal already. For this case, `arg` should be specified as zero.
>
> If this terminal is already the controlling terminal of a different session
> group, then the ioctl fails with `EPERM`, unless the caller has the
> `CAP_SYS_ADMIN` capability and `arg` equals 1, in which case the terminal is
> stolen, and all processes that had it as controlling terminal lose it.
>
> **TIOCNOTTY** — If the given terminal was the controlling terminal of the
> calling process, give up this controlling terminal. If the process was session
> leader, then send `SIGHUP` and `SIGCONT` to the foreground process group and all
> processes in the current session lose their controlling terminal.

<https://man7.org/linux/man-pages/man2/TIOCSCTTY.2const.html>

Note: `tty_ioctl(4)` has been split; individual ioctls now have their own
`2const` pages. The index page is
<https://man7.org/linux/man-pages/man4/tty_ioctl.4.html>, which lists
`TIOCSCTTY(2const)`, `TIOCNOTTY(2const)`, `TIOCGWINSZ(2const)` and
`TIOCSWINSZ(2const)` among others.

`login_tty(3)` bundles the whole dance into one call:

> The `login_tty()` function prepares for a login on the terminal referred to by
> the file descriptor `fd` (which may be a real terminal device, or the slave of
> a pseudoterminal as returned by `openpty()`) by creating a new session, making
> `fd` the controlling terminal for the calling process, setting `fd` to be the
> standard input, output, and error streams of the current process, and closing
> `fd`.

<https://man7.org/linux/man-pages/man3/login_tty.3.html>

And `forkpty(3)` bundles even that: "The `forkpty()` function combines
`openpty()`, `fork(2)`, and `login_tty()` to create a new process operating in a
pseudoterminal." (same URL)

### Does an inherited slave fd survive a mount-namespace change / `pivot_root`?

**No primary source found that makes this combined statement directly.** No man
page I located says "a PTY slave file descriptor continues to work after the
holder changes mount namespace or pivots root". What exists is adjacent evidence
that constrains the answer:

1. **A file descriptor is a reference to an open file description, not to a
   path.** `open(2)`:

   > A call to `open()` creates a new open file description, an entry in the
   > system-wide table of open files. […] A file descriptor is a reference to an
   > open file description; **this reference is unaffected if path is subsequently
   > removed or modified to refer to a different file.**

   and

   > a child process created via `fork(2)` inherits duplicates of its parent's
   > file descriptors, and those duplicates refer to the same open file
   > descriptions.

   <https://man7.org/linux/man-pages/man2/open.2.html>

2. **Mount namespaces isolate the *list of mounts*, i.e. path resolution.**
   `mount_namespaces(7)`:

   > Mount namespaces provide isolation of the list of mounts seen by the
   > processes in each namespace instance. Thus, the processes in each of the
   > mount namespace instances will see distinct single-directory hierarchies.

   <https://man7.org/linux/man-pages/man7/mount_namespaces.7.html>

3. **devpts instances are independent of each other.** The kernel documentation:

   > Each mount of the devpts filesystem is now distinct such that ptys and their
   > indices allocated in one mount are independent from ptys and their indices
   > in all other mounts.
   >
   > All mounts of the devpts filesystem now create a `/dev/pts/ptmx` node with
   > permissions 0000.
   >
   > To retain backwards compatibility the a ptmx device node (aka any node
   > created with `mknod name c 5 2`) when opened will look for an instance of
   > devpts under the name `pts` in the same directory as the ptmx device node.

   <https://docs.kernel.org/filesystems/devpts.html>

   Also documented there: `kernel.pty.max = 4096` global limit,
   `kernel.pty.reserve = 1024` "reserved for filesystems mounted from the initial
   mount namespace", `kernel.pty.nr` current count, and a per-instance
   `max=<count>` mount option.

4. **runc's design implies the answer indirectly**: runc deliberately creates the
   pty from "the container's namespaced `/dev/pts/ptmx`" and exports the *master*
   fd across the namespace boundary over `SCM_RIGHTS` rather than importing a
   host slave into the container.
   <https://github.com/opencontainers/runc/blob/main/docs/terminals.md#new-terminal>

5. **reptyr's implementation shows that *path*-based reopening breaks across
   namespaces.** reptyr makes the *target* open the pty, executing `openat` in
   the traced child's own context:

   > ```c
   >     child_fd = do_syscall(&child, openat,
   >                           -1, scratch_page, O_RDWR | O_NOCTTY,
   >                           0, 0, 0);
   > ```

   <https://github.com/nelhage/reptyr/blob/master/attach.c>

   The pty path string is memcpy'd into the child and resolved by the child, so
   it must exist in the child's mount namespace.

Reading 1–3 together, the fd itself is a reference to an open file description
and nothing about a mount-namespace change invalidates it. But that is inference
from general fd semantics, **not a documented statement about PTYs across
`pivot_root`**, and it is stated here as inference. What is documented and
matters is 3: `/dev/pts` inside a new mount namespace is a *different devpts
instance*, so the *name* of a host pty will not resolve inside the box, and
`ptsname()` results are not portable across the boundary.

### Window size

`TIOCGWINSZ` / `TIOCSWINSZ` are the ioctls; they are listed under "Get and set
window size" in the tty ioctl index:
<https://man7.org/linux/man-pages/man4/tty_ioctl.4.html>

`openpty()` takes an initial `struct winsize *winp`:
<https://man7.org/linux/man-pages/man3/openpty.3.html>

The `SIGWINCH`-on-resize contract is what every attach tool relies on; abduco
states it explicitly ("Whenever the primary client resizes its terminal the
server process will deliver a `SIGWINCH` signal to the supervised process" —
<https://github.com/martanne/abduco/blob/master/abduco.1>), and dtach's `winch`
redraw method exploits it ("forces a WINCH signal to be sent to the program" —
<https://github.com/crigler/dtach/blob/master/dtach.1>).

## 2.5 reptyr — retroactively moving a running process to a new terminal

### What it does

> reptyr is a utility for taking an existing running program and attaching it to
> a new terminal.

> "reptyr PID" will grab the process with id PID and attach it to your current
> terminal.
>
> After attaching, the process will take input from and write output to the new
> terminal, including ^C and ^Z. (Unfortunately, if you background it, you will
> still have to run "bg" or "fg" in the old terminal. This is likely impossible to
> fix in a reasonable way without patching your shell.)

<https://github.com/nelhage/reptyr/blob/master/README.md>

### How it works

> `reptyr` works by attaching to the target program using `ptrace(2)`, redirecting
> relevant file descriptors, and changing the program's controlling terminal (See
> `tty(4)`). It is this last detail that makes `reptyr` work much better than
> alternatives such as `retty(1)`.

<https://github.com/nelhage/reptyr/blob/master/reptyr.1>

> The main thing that reptyr does that no one else does is that it actually
> changes the controlling terminal of the process you are attaching.

<https://github.com/nelhage/reptyr/blob/master/README.md>

The source shows the exact sequence, and it is the `setsid` + `TIOCSCTTY` dance
from §2.4 performed *inside the traced process*:

```c
    child_fd = do_syscall(&child, openat,
                          -1, scratch_page, O_RDWR | O_NOCTTY, 0, 0, 0);
    ...
    err = do_syscall(&child, getsid, 0, 0, 0, 0, 0, 0);
    if (err != child.pid) {
        debug("Target is not a session leader, attempting to setsid.");
        err = do_setsid(&child);
    } else {
        do_syscall(&child, ioctl, child_tty_fds[0], TIOCNOTTY, 0, 0, 0, 0);
    }
    ...
    err = do_syscall(&child, ioctl, child_fd, TIOCSCTTY, 1, 0, 0, 0);
    if (err != 0) { /* Seems to be returning >0 for error */
        error("Unable to set controlling terminal: %s", strerror(err));
```

<https://github.com/nelhage/reptyr/blob/master/attach.c>

Note the branch: if the target **is already a session leader**, reptyr must first
`TIOCNOTTY` (give up the old controlling terminal) before `TIOCSCTTY` can
succeed — matching the `TIOCSCTTY` man page's "must be a session leader and not
have a controlling terminal already". If the target is *not* a session leader, it
calls `setsid()` on it, which moves the process out of its original session.
Also note `TIOCSCTTY` is called with `arg == 1`, the steal variant, which the man
page says requires `CAP_SYS_ADMIN` when the terminal already belongs to another
session.

### Documented limitations

**ptrace and Yama:**

> `reptyr` depends on the `ptrace` system call to attach to the remote program. On
> Ubuntu Maverick and higher, this ability is disabled by default for security
> reasons. You can enable it temporarily by doing
>
> ```
> # echo 0 > /proc/sys/kernel/yama/ptrace_scope
> ```
>
> as root, or permanently by editing the file `/etc/sysctl.d/10-ptrace.conf`

<https://github.com/nelhage/reptyr/blob/master/reptyr.1> and
<https://github.com/nelhage/reptyr/blob/master/README.md>

The source's own diagnostic confirms this is the first thing it blames on
`EPERM`:

```c
    fprintf(stderr, "The kernel denied permission while attaching. If your uid matches\n");
    fprintf(stderr, "the target's, check the value of /proc/sys/kernel/yama/ptrace_scope.\n");
    fprintf(stderr, "For more information, see /etc/sysctl.d/10-ptrace.conf\n");
```

<https://github.com/nelhage/reptyr/blob/master/platform/linux/linux.c>

**Portability and architecture:**

> reptyr supports Linux and FreeBSD. Not all functionality is currently available
> on FreeBSD. (Notably, FreeBSD doesn't support `reptyr -T` at this time.
>
> `reptyr` uses ptrace to attach to the target and control it at the syscall
> level, so it is highly dependent on details of the syscall API, available
> syscalls, and terminal `ioctl()`s. A port to other operating systems may be
> technically feasible, but requires significant low-level knowledge of the
> relevant platform […]
>
> reptyr works on i386, x86_64, and ARM.

<https://github.com/nelhage/reptyr/blob/master/README.md>

**Known bugs, verbatim from the man page:**

> When attaching to some curses programs, they will not redraw the screen right
> away, and a `^L` or similar will be needed to force a redraw.
>
> Similarly, after attaching to certain programs, the old terminal will be left in
> an odd state, and a `clear` or even `reset` may be required before the old
> terminal is usable again.
>
> Attaching to rtorrent (and probably some other apps) doesn't work right
> (rtorrent stops accepting input) (The problem is that rtorrent is using epoll to
> poll stdin, and we don't update the internal reference that the epoll fd has to
> the old tty).
>
> **Attaching to a process with children doesn't work right.** This should be
> possible to fix -- I just need to ptrace each child individually and do the same
> games to it.
>
> Attaching a `less(1)` process doesn't work if you have a `.lessfilter` file, as
> `less` leaves around a zombie child in this case.

<https://github.com/nelhage/reptyr/blob/master/reptyr.1>

**"Attaching to a process with children doesn't work right" is the killer for an
agent process that spawns subprocesses.**

**TTY-stealing mode (`-T`)**, which avoids ptracing the target:

> Use an alternate mode of attaching, "TTY-stealing". In this mode, `reptyr` will
> not `ptrace(2)` the target process, but will attempt to discover the terminal
> emulator for that process' pty, and steal the master end of the pty. This mode
> is more reliable and flexible in many circumstances (for instance, it can attach
> all processes on a tty, rather than just a single process). However, as a
> downside, children of `sshd(8)` cannot be attached via `-T` unless `reptyr` is
> run as root.

<https://github.com/nelhage/reptyr/blob/master/reptyr.1>

The heuristic `-T` relies on is fragile, and the source says so:

```c
// Find the PID of the terminal emulator for `target's terminal.
//
// We assume that the terminal emulator is the parent of the session
// leader. This is true in most cases, although in principle you can
// construct situations where it is false. We should fail safe later
// on if this turns out to be wrong, however.
```

<https://github.com/nelhage/reptyr/blob/master/platform/linux/linux.c>

**Containers: no primary source found.** Neither the README, the man page, nor
the source I read mentions namespaces or containers. The mechanism implies
constraints (ptrace across a user namespace boundary, and the `openat` of the pty
path executed in the target's own mount namespace — §2.4 item 5), but reptyr's
own documentation does not discuss it. Do not cite reptyr as evidence either way
about containers.

Other useful modes for a design that needs a spare pty:

> **-l, -L [COMMAND [ARGS]]** — Instead of attaching to a new process, create a
> new pty pair, proxy the master end to the current terminal, and then print the
> name of the slave pty. […] If `-L` is used instead of `-l`, then fds 0-2 of the
> child will also be redirected to point to the slave, and the child will be run
> in a fresh session with the slave as its controlling terminal.
>
> **-s** — By default, reptyr will move any file descriptors in the target that
> were connected to the target's controlling terminal to point to the new
> terminal. The `-s` option will cause reptyr to unconditionally attach file
> descriptors 0, 1, and 2 in the target, even if the target has no controlling
> terminal or they are not connected to a terminal.

<https://github.com/nelhage/reptyr/blob/master/reptyr.1>

## 2.6 Claude Code's own attach / session-sharing story

**This turned out to be much richer than the brief assumed. Claude Code ships a
first-party attach with a supervisor daemon.**

### `claude attach <id>` and the background-session supervisor

The CLI reference lists it:

> | `claude attach <id>` | Attach to a background session in this terminal | `claude attach 7c5dcf5d` |

<https://code.claude.com/docs/en/cli-reference>

The architecture:

> The supervisor is a background service that runs your background sessions so
> they keep working after you close agent view or your terminal. Claude Code
> starts it the first time you background a session or open agent view, and you
> don't need to manage it yourself.
>
> Each session is its own Claude Code process under the supervisor

<https://code.claude.com/docs/en/agent-view#the-supervisor-process>

And critically, there is a distinct PTY-owning layer:

> The supervisor runs each background session's terminal in **its own host
> process**. When that process dies or stops responding, Claude Code shows the
> reason and offers a restart; in both cases the conversation is saved and the
> restart resumes it.

<https://code.claude.com/docs/en/agent-view#the-terminal-host-died-or-the-session-stopped-responding>

Failure handling for that host process, which reads exactly like a PTY-master
supervisor:

> On Linux and WSL, the supervisor checks each host process every few seconds,
> whether or not you open the session, and marks the session failed when the
> process has exited but its connection to the supervisor never closed.
>
> * In agent view, the row shows `terminal host process died — press Enter to
>   restart`.
> * From the shell, `claude attach <id>` restarts a session already marked failed.

(same URL)

### Attach and detach semantics

> Press `Enter` or `→` on a selected row to attach. Agent view is replaced by the
> full interactive session. When you attach, Claude posts a short recap of what
> happened while you were away.

> Press `←` on an empty prompt, or run `/exit`, to detach and return to agent
> view, whether you opened the session from agent view or with `claude attach <id>`
> from your shell.

> `Ctrl+Z` also detaches but goes back to where you started instead: agent view if
> you attached from there, or your shell if you ran `claude attach`. Use `Ctrl+Z`
> when a dialog has focus and isn't responding to `←`.

<https://code.claude.com/docs/en/agent-view#attach-to-a-session>

**The scrollback constraint is stated outright, and it is the same one any attach
design hits:**

> Attached sessions always render in fullscreen mode, regardless of your `tui`
> setting, **because a background session has no terminal scrollback to append
> to**. Scroll with `PgUp`, `PgDn`, or the mouse wheel, and press `Ctrl+O` for
> transcript mode. Your terminal's native scroll and tmux copy mode show only the
> current viewport, the same as when you run any fullscreen application.

<https://code.claude.com/docs/en/agent-view#attach-to-a-session>

Resize during attach is handled, per the changelog for v2.1.210:

> `claude attach` waits while the background service is starting or reconnecting
> instead of failing with a `job not found` or `still starting` error, reports a
> session that finished during the attach as exited, and **applies a terminal
> resize made during a slow attach when the attach completes**.

<https://code.claude.com/docs/en/agent-view>

**No primary source found** for whether two `claude attach` clients can be
attached to the same background session simultaneously, or what the size policy
would be if they were. The docs describe attach only in the singular.

Related commands: `claude agents` (opens agent view), `claude rm <id>`,
`claude stop <id>`, `claude logs`, `claude daemon status`, `claude daemon stop [--any]`,
and `claude --bg "<task>"` to dispatch. `--bg` combined with `-p`/`--print` is
rejected:

> ```
> --bg and --print conflict: --print never starts the interactive session that `claude agents` attaches to, so the job would be unattachable. The prompt is the positional — drop --print: `claude --bg '<task>'`.
> ```

<https://code.claude.com/docs/en/errors#conflict-between-bg-and-print>

Agent view can be disabled entirely:

> To turn off background agents and agent view entirely, set the `disableAgentView`
> setting to `true` or set the `CLAUDE_CODE_DISABLE_AGENT_VIEW` environment
> variable. Administrators can enforce this through managed settings.

<https://code.claude.com/docs/en/agent-view#turn-off-agent-view>

### `--resume` / `--continue`, and whether two processes on one session is safe

The flags:

> | `--continue`, `-c` | Load the most recent conversation in the current directory, skipping background sessions, sessions created with `claude -p` or the Agent SDK, and sessions whose first prompt was `/loop`. `claude -p --continue` includes `-p`, SDK, and `/loop` sessions. Includes sessions that added this directory with `/add-dir` |

> | `--resume`, `-r` | Resume a specific session by ID or name, or show an interactive picker to choose a session. […] When you pass a session ID, Claude Code searches the current project directory and its git worktrees, then every other project on this machine. |

> | `--fork-session` | When resuming, create a new session ID instead of reusing the original (use with `--resume` or `--continue`) |

<https://code.claude.com/docs/en/cli-reference>

**The concurrency answer, stated once and clearly:**

> If you resume the same session in two terminals without forking, **messages from
> both interleave into one transcript.**

<https://code.claude.com/docs/en/sessions#branch-a-session>

That is the only statement I found on the subject. It is a description of the
outcome, not an endorsement or a prohibition. It is not called unsafe, and it is
not called safe; it is called interleaving. `--fork-session` is the documented
way to avoid it.

Session files live where we expected:

| Path under `~/.claude/` | Contents |
|---|---|
| `projects/<project>/<session>.jsonl` | "Full conversation transcript: every message, tool call, and tool result" |
| `projects/<project>/<session>.orphaned-<timestamp>-<suffix>.jsonl`, `projects/<project>/<session>.jsonl.superseded-<timestamp>` | "A previous transcript for the session that Claude Code set aside instead of overwriting or deleting it" |
| `projects/<project>/<session>/subagents/` | Subagent conversation transcripts |
| `projects/<project>/<session>/tool-results/` | "Large tool outputs spilled to separate files" |

<https://code.claude.com/docs/en/claude-directory#cleaned-up-automatically>

The existence of `.superseded-<timestamp>` and `.orphaned-…` files is indirect
evidence that concurrent writers are anticipated and handled by setting a
transcript aside rather than by locking. **No primary source found** describing a
lock file or any concurrency-control mechanism on transcripts.

### Remote Control — a documented multi-viewer mode without a second agent

This is the closest thing Claude Code has to "a second viewer of one running
agent", and it is first-party:

> Remote Control connects claude.ai/code or the Claude app for iOS and Android to
> a Claude Code session running on your machine. […] When you start a Remote
> Control session on your machine, Claude keeps running locally the entire time,
> so your code execution and filesystem access stay on your machine.

> * **Work from both surfaces at once**: the conversation and the progress of
>   subagents and dynamic workflows stay in sync across all connected devices, so
>   you can send messages from your terminal, browser, and phone interchangeably.

> * **Survive interruptions**: if your laptop sleeps or your network drops, Claude
>   Code reconnects automatically when your machine comes back online. While the
>   connection is rebuilding, Claude Code queues messages, permission prompts, and
>   status updates from subagents and workflows, and delivers them once the
>   connection recovers.

<https://code.claude.com/docs/en/remote-control>

Three invocation modes:

- `claude remote-control` — "The process stays running in your terminal in server
  mode, waiting for remote connections." Server mode has `--spawn session`
  ("single-session mode. Serves exactly one session and rejects additional
  connections"), `--capacity <N>` (default 32), `--permission-mode`, and
  `--sandbox` / `--no-sandbox`.
- `claude --remote-control` (alias `--rc`) — "This gives you a full interactive
  session in your terminal that you can also control from claude.ai or the Claude
  app. Unlike `claude remote-control` (server mode), you can type messages locally
  while the session is also available remotely."
- `/remote-control` (alias `/rc`) from inside a running session — "This starts a
  Remote Control session that carries over your current conversation history."

<https://code.claude.com/docs/en/remote-control#start-a-remote-control-session>

**Important constraint for a sandbox**: Remote Control is unavailable under
several of the exact variables a locked-down box would set:

> **Feature-flag evaluation**: `DISABLE_TELEMETRY`, `DO_NOT_TRACK`,
> `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, and `DISABLE_GROWTHBOOK` each
> disable the feature-flag evaluation that Remote Control availability depends on.

and

> **API endpoint**: not available in any of these configurations: You use Amazon
> Bedrock, Google Cloud's Agent Platform, or Microsoft Foundry. You point
> `ANTHROPIC_BASE_URL` at a host other than `api.anthropic.com` […] You sign in
> through an enterprise Claude apps gateway.

and

> **Subscription**: available on Pro, Max, Team, and Enterprise plans. **API keys
> are not supported.**

<https://code.claude.com/docs/en/remote-control#requirements>

### Headless / SDK mode

`claude -p` / `--print` is the non-interactive mode
(<https://code.claude.com/docs/en/headless>), and the Agent SDK is documented
separately including a hosting guide covering "subprocess architecture, session
persistence, scaling, observability, and multi-tenant isolation"
(<https://code.claude.com/docs/en/agent-sdk/hosting>). Note the error above:
`--print` "never starts the interactive session that `claude agents` attaches
to", so headless mode is explicitly *not* a path to a second viewer of an
interactive session.

Also relevant: `--teleport` — "Resume a web session in your local terminal"
(<https://code.claude.com/docs/en/cli-reference>), and
`--no-session-persistence` — "Disable session persistence so sessions are not
saved to disk and cannot be resumed. Print mode only." (same URL).

## 2.7 The `vt100` Rust crate

The crate exists, is maintained, and is widely used: `vt100` 0.16.2, last
published 2025-07-12, repository <https://github.com/doy/vt100-rust>, ~3.68M
recent downloads (from <https://crates.io/api/v1/crates/vt100>).

`Parser` is the entry point:

> A parser for terminal output which produces an in-memory representation of the
> terminal contents.

> ```rust
> /// Creates a new terminal parser of the given size and with the given
> /// amount of scrollback.
> pub fn new(rows: u16, cols: u16, scrollback_len: usize) -> Self
> ```

<https://github.com/doy/vt100-rust/blob/main/src/parser.rs>,
<https://docs.rs/vt100/latest/vt100/struct.Parser.html>

**`contents_formatted()` exists and does exactly what a "restore the screen for a
newly attached client" design needs:**

> ```rust
> /// Returns the formatted visible contents of the terminal.
> ///
> /// Formatting information will be included inline as terminal escape
> /// codes. The result will be suitable for feeding directly to a raw
> /// terminal parser, and will result in the same visual output.
> #[must_use]
> pub fn contents_formatted(&self) -> Vec<u8>
> ```

**`state_formatted()` exists and is the fuller version**, adding input modes
(bracketed paste, application cursor keys, mouse mode, and so on):

> ```rust
> /// Return escape codes sufficient to reproduce the entire contents of the
> /// current terminal state. This is a convenience wrapper around
> /// [`contents_formatted`](Self::contents_formatted) and
> /// [`input_mode_formatted`](Self::input_mode_formatted).
> #[must_use]
> pub fn state_formatted(&self) -> Vec<u8>
> ```

<https://github.com/doy/vt100-rust/blob/main/src/screen.rs>,
<https://docs.rs/vt100/latest/vt100/struct.Screen.html>

There is also a diff form, aimed at exactly the "client already has a state"
case:

> ```rust
> /// `self.contents_diff(prev)` should be equivalent to the result of
> /// rendering `self.contents_formatted()`. This is primarily useful when
> /// you already have a terminal parser whose state is described by `prev`,
> /// since the diff will likely require less memory and cause less
> /// flickering than redrawing the entire screen contents.
> #[must_use]
> pub fn contents_diff(&self, prev: &Self) -> Vec<u8>
> ```

(same URL). `state_diff` is the corresponding pairing for `state_formatted`.

### Scrollback: reachable, but not replayable in one call

The word "visible" in the `contents_formatted` doc is load-bearing. Scrollback is
accessed by moving an offset:

> ```rust
> /// Scrolls to the given position in the scrollback.
> ///
> /// This position indicates the offset from the top of the screen, and
> /// should be `0` to put the normal screen in view.
> ///
> /// This affects the return values of methods called on the screen: for
> /// instance, `screen.cell(0, 0)` will return the top left corner of the
> /// screen after taking the scrollback offset into account.
> ///
> /// The value given will be clamped to the actual size of the scrollback.
> pub fn set_scrollback(&mut self, rows: usize)
> ```

and its reader:

> ```rust
> /// Returns the current position in the scrollback.
> ///
> /// This position indicates the offset from the top of the screen, and is
> /// `0` when the normal screen is in view.
> ```

<https://github.com/doy/vt100-rust/blob/main/src/screen.rs>

There is also a per-row accessor for partial redraws:

> ```rust
> /// You are responsible for positioning the cursor before printing each
> /// row, and the final cursor position after displaying each row is
> /// unspecified.
> pub fn rows_formatted(&self, start: u16, width: u16) -> impl Iterator<Item = Vec<u8>>
> ```

(same URL)

**Answering the brief's question directly: `contents_formatted()` / `state_formatted()`
give a newly attached client the current visible screen with correct attributes
and modes in one call. They do not replay scrollback.** To hand a mid-session
client any history you would have to walk `set_scrollback()` backwards and
collect rows, and the amount available is bounded by the `scrollback_len` passed
to `Parser::new`. `set_scrollback` mutates the `Screen`, so doing that on a
shared parser is not free of side effects.

This is the same tradeoff dtach and abduco chose to *not* pay: abduco says
outright "the terminal state is not preserved across sessions", and dtach's
answer is to send `^L` or a `SIGWINCH` and let the program redraw itself. tmux
and Claude Code's `claude attach` both pay it, and both end up forcing fullscreen
/ alternate-screen semantics as a result.

---

## Appendix — sources used

**Claude Code documentation** (all under <https://code.claude.com/docs/en/>; add
`.md` to any path for raw Markdown)

- `setup` — install methods, auto-updates, paths, channels, pinning, manifest verification
- `settings` — settings precedence and files
- `settings-reference` — every settings key, `~/.claude.json` global config keys
- `env-vars` — the complete environment variable table
- `troubleshoot-install` — install/update failure paths, conflicting installations
- `troubleshooting` — `/doctor` vs `claude doctor`
- `errors` — exact error strings for download and install failures
- `claude-directory` — the `~/.claude` file inventory
- `cli-reference` — `claude attach`, `--resume`, `--continue`, `--fork-session`
- `sessions` — resuming, branching, the two-terminals interleaving statement
- `agent-view` — background sessions, the supervisor, the terminal host process, attach/detach
- `remote-control` — first-party multi-surface control
- `devcontainer` — container env recipe, npm version pinning
- `network-config` — hosts the updater contacts
- `llms.txt` — the documentation index

**Anthropic release artifacts**

- <https://downloads.claude.ai/claude-code-releases/2.1.258/manifest.json> (+ `.sig`)
- <https://downloads.claude.ai/keys/claude-code.asc>

**npm**

- <https://registry.npmjs.org/@anthropic-ai/claude-code>
- <https://github.com/npm/registry/blob/main/docs/responses/package-metadata.md>
- <https://www.w3.org/TR/SRI/>

**GitHub issue tracker (user reports, not documentation)**

- <https://github.com/anthropics/claude-code/issues/82408>

**Docker / moby / runc**

- <https://docs.docker.com/reference/cli/docker/container/attach/>
- <https://docs.docker.com/reference/cli/docker/container/exec/>
- <https://docs.docker.com/reference/cli/docker/>
- <https://github.com/moby/moby/blob/master/daemon/internal/stream/streams.go>
- <https://github.com/moby/moby/blob/master/daemon/attach.go>
- <https://github.com/docker/cli/blob/master/cli/command/container/tty.go>
- <https://github.com/opencontainers/runc/blob/main/docs/terminals.md>

**Attach tools**

- <https://github.com/crigler/dtach/blob/master/dtach.1>
- <https://github.com/crigler/dtach/blob/master/README>
- <https://github.com/martanne/abduco/blob/master/abduco.1>
- <https://github.com/martanne/abduco/blob/master/README.md>
- <https://man.openbsd.org/tmux.1> / <https://github.com/tmux/tmux/blob/master/tmux.1>
- <https://github.com/nelhage/reptyr/blob/master/README.md>
- <https://github.com/nelhage/reptyr/blob/master/reptyr.1>
- <https://github.com/nelhage/reptyr/blob/master/attach.c>
- <https://github.com/nelhage/reptyr/blob/master/platform/linux/linux.c>

**Man pages and kernel docs**

- <https://man7.org/linux/man-pages/man7/pty.7.html>
- <https://man7.org/linux/man-pages/man4/pts.4.html>
- <https://man7.org/linux/man-pages/man3/openpty.3.html>
- <https://man7.org/linux/man-pages/man3/login_tty.3.html>
- <https://man7.org/linux/man-pages/man2/setsid.2.html>
- <https://man7.org/linux/man-pages/man2/TIOCSCTTY.2const.html>
- <https://man7.org/linux/man-pages/man4/tty_ioctl.4.html>
- <https://man7.org/linux/man-pages/man7/credentials.7.html>
- <https://man7.org/linux/man-pages/man2/open.2.html>
- <https://man7.org/linux/man-pages/man7/mount_namespaces.7.html>
- <https://docs.kernel.org/filesystems/devpts.html>

**Rust crates**

- <https://docs.rs/vt100/latest/vt100/struct.Screen.html>
- <https://docs.rs/vt100/latest/vt100/struct.Parser.html>
- <https://github.com/doy/vt100-rust/blob/main/src/screen.rs>
- <https://github.com/doy/vt100-rust/blob/main/src/parser.rs>
