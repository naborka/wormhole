# One box, several workspaces: prior art

Primary-source research into how comparable tools tie a persistent
environment (a container, a sandbox, an agent's `$HOME`) to the host
directories it works on, and what follows for identity and state when one
environment serves several directories. Checked 2026-09-13. Every claim is
cited to an official doc site, man page, or the tool's own source.
"Inference" marks my own reasoning, not a quote from a source.

Read alongside [CONTEXT.md](../../CONTEXT.md). The question under study: what
it would take, and what it would cost, for one box (one home, one role, one
product) to serve several workspaces — for example one `alphaca` Claude Code
box reused across several Rust projects.

---

## What the evidence says

1. Every general-purpose dev container surveyed (distrobox, toolbx) is
   **one environment for every directory**, and gets there the same way:
   the host `$HOME` is bind-mounted at the same path and the host cwd is
   passed to `exec --workdir` on every entry. The container never learns
   which directories it "belongs to".
2. The tools that key an environment to a directory (devcontainer CLI,
   VS Code Dev Containers) key it to **`(local folder, config file)`** via
   container labels, and their docs never describe one container serving
   two unrelated folders. A multi-root workspace is one container because
   the *first* folder's config wins, not because the container can grow.
3. **No container runtime adds a bind mount to a running container.**
   `docker update` and `podman update` change only resource limits,
   health checks, env and restart policy. `podman mount` goes the other
   way (container rootfs onto the host). bwrap and Firejail fix the mount
   list at start; Firejail `--join` re-enters the existing namespaces and
   changes nothing in them.
4. The kernel does allow it, but only through the box's own user
   namespace: `setns` into the box's userns (the host user owns it, so it
   holds all capabilities there), then `setns` into its mount namespace,
   then `mount --bind`. `setns(2)` requires `CAP_SYS_ADMIN` in the
   caller's *own* userns for a mount-namespace join, which is why the userns
   join has to come first.
5. The same route gives a cheaper design: `setns(user)` + `setns(mnt)` +
   `unshare(CLONE_NEWNS)` yields a **private copy of the box's mount list,
   owned by the box's userns**, into which a session can bind a second
   workspace that PID 1 and the other sessions never see. This is the
   exact shape of wormhole's existing "Refresh" rule for env: new sessions
   only, PID 1's tree keeps its start view.
6. Claude Code already models "one home, many projects". Per-project
   state lives under `~/.claude/projects/<cwd with non-alphanumerics
   replaced by ->/`, trust is recorded per path under `projects` in
   `~/.claude.json`, `--continue` is scoped to the cwd, and `--resume <id>`
   searches every project on the machine. Nothing in it keys to the
   container, only to the cwd path.
7. Codex is the same shape: `[projects."<path>"] trust_level` in
   `~/.codex/config.toml`, sessions under `$CODEX_HOME/sessions`,
   `codex resume --last` scoped to the cwd, and an explicit prompt when the
   resume cwd differs from the session's saved cwd.
8. Because wormhole mounts the workspace at its **host-identical path**,
   both products would key a second workspace correctly inside a shared
   home with no translation — the box path *is* the host path.
9. What breaks is wormhole's own identity, not the products': the key is
   `basename-id`, the record holds one workspace, `gc` calls a home "dead"
   when *its* workspace is gone, and the trust seed wormhole writes into
   `.claude.json` / `config.toml` is per path. All four become per-list
   rather than per-value.

---

## 1. distrobox — one container, host home shared, host cwd on every entry

<https://distrobox.it/usage/distrobox-create/> (mirrors
`docs/usage/distrobox-create.md` in the repository)

> the HOME directory of the user, external storage, external usb devices and
> graphical apps (X11/Wayland), and audio.

> --home/-H: select a custom HOME directory for the container. Useful to
> avoid host's home littering with temp files.

> The `--home` flag let's you specify a custom HOME for the container. Note
> that this will NOT prevent the mount of the host's home directory, but
> will ensure that configs and dotfiles will not litter it.

Source of the mounts (the project has been rewritten in Go; default branch
`main`), `pkg/containermanager/providers/podman.go`:

```go
options = append(options, "--volume", fmt.Sprintf("%s:%s%s", containerUserHome, containerUserHome, containermanager.BindPropagation()))
...
options = append(options, "--volume", "/:/run/host/"+containermanager.BindPropagation())
```

and for `--home`:

```go
//	1- override the HOME env variable
//	2- export the DISTROBOX_HOST_HOME env variable pointing to original HOME
options = append(options, "--env", fmt.Sprintf("HOME=%s", containerUserCustomHome))
options = append(options, "--env", fmt.Sprintf("DISTROBOX_HOST_HOME=%s", containerUserHome))
```

<https://raw.githubusercontent.com/89luca89/distrobox/main/pkg/containermanager/providers/podman.go>

Entering. <https://distrobox.it/usage/distrobox-enter/>:

> --no-workdir/-nw: always start the container from container's home directory

The cwd logic, `pkg/containermanager/containermanager.go`:

```go
func GetWorkDir(containerHome string, noWorkDir bool) (string, error) {
	workDir, err := os.Getwd()
	...
	if noWorkDir {
		return containerHome, nil
	}
	...
	if !strings.Contains(workDir, containerHome) {
		return "/run/host" + workDir, nil
	}
	return workDir, nil
}
```

and it is passed to the runtime as `--workdir=<dir>` plus `--env=PWD=<dir>`
on every `exec` (`providers/podman.go`, `providers/docker.go`).
<https://raw.githubusercontent.com/89luca89/distrobox/main/pkg/containermanager/containermanager.go>

**Fact.** One container serves every host directory: a cwd under `$HOME`
is reached at its own path, anything else at `/run/host/<path>`. The
container holds no list of directories; the cwd is recomputed per entry.

**Inference.** This works because the whole host filesystem is already
mounted (`/:/run/host`). distrobox never adds a mount after creation; it
avoids needing to by mounting everything up front. That is the opposite of
wormhole's baseline ("the workspace and nothing else"), so the trick does
not transfer — only the *shape* (cwd passed per session) does.

## 2. toolbx — same model, stated as the product

<https://containers.github.io/toolbox/>

> Toolbx environments have seamless access to the user's home directory,
> the Wayland and X11 sockets, networking (including Avahi and CA
> certificates), removable devices (like USB sticks), systemd journal, SSH
> agent, D-Bus, ulimits, `/dev` and the udev database, etc.

> The host file system can be accessed at `/run/host`.

Source, `src/cmd/create.go`: the home is mounted at its own path and the
host root at `/run/host`, both with `rslave` propagation:

```go
"--volume", "/:/run/host:rslave",
...
"--volume", homeDirMountArg,   // homeDirEvaled + ":" + homeDirEvaled + ":rslave"
```

<https://raw.githubusercontent.com/containers/toolbox/main/src/cmd/create.go>

`src/cmd/run.go` passes the host cwd to `podman exec --workdir`, falling
back to the container home when the cwd does not exist inside:

```go
runFallbackWorkDirs = []string{"" /* $HOME */}
...
"--user", currentUser.Username,
"--workdir", workDir,
```

<https://raw.githubusercontent.com/containers/toolbox/main/src/cmd/run.go>

**Fact.** No per-directory state at all; toolbx is by design a single
environment for all of a user's work. **Inference:** the identity question
does not arise for toolbx because the container is keyed to the *user*,
not to any directory — and because the home is the host's home, there is
no second home to keep in sync.

## 3. VS Code Dev Containers / devcontainer CLI — keyed to `(folder, config)`

Spec. <https://containers.dev/implementors/json_reference/>:

> `workspaceFolder`: Sets the default path that `devcontainer.json`
> supporting services / tools should open when connecting to the
> container.

> `workspaceMount`: Requires `workspaceFolder` be set as well. Overrides
> the default local mount point for the workspace when the container is
> created.

CLI. <https://github.com/devcontainers/cli/blob/main/README.md>:

```
devcontainer up --workspace-folder <path-to-vscode-remote-try-rust>
devcontainer exec --workspace-folder <path-to-vscode-remote-try-rust> cargo run
```

and the documented `docker run` it produces:

```
--mount type=bind,source=/home/node/vscode-remote-try-rust,target=/workspaces/vscode-remote-try-rust -l devcontainer.local_folder=/home/node/vscode-remote-try-rust
```

Source, `src/spec-node/singleContainer.ts`:

```ts
export const hostFolderLabel = 'devcontainer.local_folder';
export const configFileLabel = 'devcontainer.config_file';
...
export async function findDevContainer(params, labels: string[]) {
	const ids = await listContainers(params, true, labels);
	const details = await inspectContainers(params, ids);
	return details.filter(container => container.State.Status !== 'removing')[0];
}
```

<https://raw.githubusercontent.com/devcontainers/cli/main/src/spec-node/singleContainer.ts>

VS Code. <https://code.visualstudio.com/docs/devcontainers/containers>:

> All roots/folders in a multi-root workspace will be opened in the same
> container, regardless of whether there are configuration files at lower
> levels.

<https://code.visualstudio.com/remote/advancedcontainers/connect-multiple-containers>:

> Currently you can only connect to one container per Visual Studio Code
> window.

<https://code.visualstudio.com/remote/advancedcontainers/change-default-source-mount>:
after changing `workspaceMount`/`workspaceFolder` you must run **Rebuild
Container** or **Open Folder in Container** — i.e. the mount set is a
creation-time property.

Attach to a running container
(<https://code.visualstudio.com/docs/devcontainers/attach-container>): the
config for an attached container is stored per **image name** by default,
or per **container name** ("Open Named Configuration File"), with its own
`workspaceFolder`. This is the one VS Code path where a container's
identity is not a folder.

**Fact.** The devcontainer identity is exactly the tuple wormhole already
uses, `(workspace folder, config file)`. A second folder means a second
container, or the same container reached by "attach" with config keyed to
the container rather than the folder. **Inference.** The devcontainer
world does not have a "one container, N folders" mode because its mounts
are frozen at create; the multi-root case is solved by mounting the
*parent* once, not by adding mounts later.

## 4. Docker / Podman — cwd bind-mount convention; mounts fixed at create

<https://docs.docker.com/reference/cli/docker/container/run/>:

> `docker run -v $(pwd):$(pwd) -w $(pwd) -i -t ubuntu pwd`

> -w option runs the command executed inside the directory specified, in
> this example, /path/to/dir/. If the path doesn't exist, Docker creates
> it inside the container.

<https://docs.docker.com/reference/cli/docker/container/exec/>:

> By default `docker exec` command runs in the same working directory set
> when the container was created.

with `--workdir` to override per exec.
<https://docs.podman.io/en/latest/markdown/podman-exec.1.html> documents
the same `--workdir, -w` on `podman exec`.

What can change after create:

- `docker update` — options are `--blkio-weight`, `--cpu-*`,
  `--cpuset-*`, `--memory*`, `--pids-limit`, `--restart`. No mount or
  volume option.
  <https://docs.docker.com/reference/cli/docker/container/update/>
- `podman update` — resource limits, health checks, `--env`/`--unsetenv`,
  `--ulimit`, `--restart`. No mount or volume option; the page describes
  the command as allowing "changes to resource limits and healthchecks".
  <https://docs.podman.io/en/latest/markdown/podman-update.1.html>
- `podman mount` — "Mounts the specified containers' root file system in a
  location which can be accessed from the host, and returns its location."
  Outward, not inward.
  <https://docs.podman.io/en/latest/markdown/podman-mount.1.html>

**Fact.** Neither runtime offers a supported way to add a bind mount to a
running container. **Inference.** The `nsenter -m -t <pid> mount --bind`
route (section 7) is the only one left, and neither runtime's docs
describe using it; I found no primary source that recommends it. Note the
docker convention `-v $(pwd):$(pwd)` — the host-identical path — is the
same choice wormhole makes (`mount_plan.rs`: `source: workspace, target:
workspace`), and it is what makes cwd-keyed tool state portable across
host and box.

## 5. bubblewrap and Firejail — mounts fixed at start; `--join` re-enters

bwrap. <https://man.archlinux.org/man/bwrap.1.en>:

> bwrap is an unprivileged low-level sandboxing tool ... creating a new,
> completely empty, filesystem namespace where the root is on a tmpfs that
> is invisible from the host

> --bind SRC DEST: Bind mount the host path SRC on DEST
> --chdir DIR: Change directory to DIR

Filesystem options "are applied in the order they are given as
arguments." There is no option, and no text, for changing a running
sandbox. **Fact:** the mount set is the argv, once.

Firejail. <https://man.archlinux.org/man/firejail.1.en>:

> --join=name|pid: Join the sandbox identified by name or by PID. By
> default a /bin/bash shell is started after joining the sandbox. If a
> program is specified, the program is run in the sandbox.

For a regular user "all security filters are configured for the new
process the same they are configured in the sandbox"; for root "the
security filters and cpus configurations are not applied".

> --join-filesystem=name|pid: Join the mount namespace of the sandbox
> identified by name or PID. ... Security filters and cpus configurations
> are not applied to the process joining the sandbox.

(root only.)

> --private: Mount new /root and /home/user directories in temporary
> filesystems. All modifications are discarded when the sandbox is closed.

> --private=directory: Use directory as user home. ... --private and
> --private=directory cannot be used together.

> --name=name: ... The name cannot contain only digits, as that is treated
> as a PID in the other options, such as in --join.

**Fact.** `--join` is "attach": it enters the namespaces the sandbox
already has and cannot add to them. `--private=dir` is the analogue of
wormhole's home — a persistent directory presented as `$HOME` — and it is
a start-time argument, so two sandboxes started with the same
`--private=dir` on two cwds is Firejail's version of "one home, several
workspaces". **Inference.** Firejail's `--join`/`--join-filesystem` split
is a data point for section 7: the namespace-join primitive is exposed to
users as an attach, never as a mutation.

## 6. Claude Code and Codex — state keyed to the cwd path, not the container

### Claude Code

Transcripts and project state. <https://code.claude.com/docs/en/sessions>:

> By default, Claude Code stores transcripts as JSONL at
> `~/.claude/projects/<project>/<session-id>.jsonl`, where `<project>` is
> your working directory path with non-alphanumeric characters replaced by
> `-`. For a working directory whose converted name exceeds 200 characters,
> Claude Code truncates the name to 200 characters and appends a hash of
> the full path

Confirmed on this host: `~/.claude/projects/-home-nabor--projects-wormhole`
and `-home-nabor--projects-wormhole-wormhole` exist side by side (the
`_` in `_projects` becomes `-`).

> A session is a saved conversation tied to a project directory.

> `claude --continue`: Reopens the most recent conversation in the current
> directory

> You can run `claude --resume <session-id>` from any directory: Claude Code
> looks for the ID in the current project directory and its git worktrees
> first, then in every other project on this machine

> Claude Code stores sessions per project directory. By default the session
> picker shows: Sessions from the current worktree ... Sessions started
> elsewhere that added the current directory with `/add-dir`. Use `Ctrl+W`
> to widen to all worktrees of the repository or `Ctrl+A` to widen to every
> project on this machine.

> `CLAUDE_CODE_PROJECT_DIR_NAME` ... Claude Code ignores
> `CLAUDE_CODE_PROJECT_DIR_NAME` when `CLAUDE_CONFIG_DIR` is unset ... the
> name doesn't vary with the working directory, so under the default
> `~/.claude` it would merge every project's transcripts and auto memory
> into one directory.

Auto memory. <https://code.claude.com/docs/en/memory>:

> Each project gets its own memory directory at
> `~/.claude/projects/<project>/memory/`. The `<project>` path is derived
> from the git repository, so all worktrees and subdirectories within the
> same repo share one auto memory directory. Outside a git repo, the
> project root is used instead.

Trust. <https://code.claude.com/docs/en/permissions>:

> `permissions.allow` rules and `permissions.additionalDirectories` entries
> in a project's `.claude/settings.json` grant capability, so Claude Code
> applies them only after you accept the workspace trust dialog for that
> folder.

> Outside a repository, Claude Code keys the trust on the directory you
> started it from, and the trust covers any subdirectory of that directory
> apart from a git repository nested inside it

> trust it by hand: set `projects["<path>"].hasTrustDialogAccepted` to
> `true` in `~/.claude.json`, where `<path>` is the repository root, or the
> folder itself outside a repository.

> To move the session to a different primary working directory ... run
> `/cd <path>`. Claude Code keeps the conversation, loads the new
> directory's `CLAUDE.md`, and prompts you to trust the workspace if you
> haven't worked in it before.

<https://code.claude.com/docs/en/security>:

> When you start Claude Code directly in your home directory, trust
> acceptance is held for the current session only and is not written to
> disk, so the prompt reappears on each launch.

Where it all lives. <https://code.claude.com/docs/en/claude-directory>:
`~/.claude.json` "Holds state that does not belong in settings.json:
theme, OAuth session, per-project trust decisions, your personal MCP
servers"; "The `projects` key tracks per-project state like trust-dialog
acceptance"; `history.jsonl` holds "Every prompt you've typed, with
timestamp and project path". `CLAUDE_CONFIG_DIR` relocates all of it.

Settings scoping. <https://code.claude.com/docs/en/settings>: precedence
is managed > `--settings` > `.claude/settings.local.json` >
`.claude/settings.json` > `~/.claude/settings.json`; the project files
are found from the cwd. `--add-dir` "Grants file access; Claude Code
doesn't discover most `.claude/` configuration from these directories"
(<https://code.claude.com/docs/en/cli-reference>).

**Fact.** One `~/.claude` holds any number of projects; the product's own
design assumes it. Every per-project thing (transcripts, auto memory,
trust, local settings) is keyed by the cwd's absolute path or its git
root; nothing is keyed to the machine, container, or home.
**Inference.** Inside a wormhole box the cwd is the host path, so a shared
box home would contain exactly the `projects/` layout the host's own
`~/.claude` would, one entry per workspace, with no collision — and
switching workspace without restarting is `/cd`, which the docs say
re-prompts trust for an unknown path.

### Codex

Config reference (<https://developers.openai.com/codex/config-reference>,
now served from <https://learn.chatgpt.com/docs/config-file/config-reference>):

> `projects.<path>.trust_level`: Mark a project or worktree as trusted or
> untrusted (`"trusted"` | `"untrusted"`). Untrusted projects skip
> project-scoped `.codex/` layers, including project-local config, hooks,
> and rules.

> `history.persistence`: Control whether Codex saves session transcripts
> to history.jsonl.

> `project_root_markers`: List of project root marker filenames; used when
> searching parent directories for the project root.

> `tui.resume_cwd`: Working directory to use when resuming or forking a
> session. When unset, Codex asks you to choose if your current directory
> differs from the session's saved directory.

CLI reference (<https://developers.openai.com/codex/cli/reference>):

> codex resume scopes `--last` to the current working directory unless you
> pass `--all`.

> If the current working directory differs from the session's saved
> directory, Codex asks which directory to use.

Source: sessions live under `$CODEX_HOME/sessions` and
`archived_sessions` (`codex-rs/rollout/src/lib.rs`:
`pub const SESSIONS_SUBDIR: &str = "sessions";`), rollout files are
`rollout-<timestamp>-<thread-id>.jsonl` (`rollout/src/metadata.rs`), each
carrying its `cwd` in the session meta. Trust is written to
`~/.codex/config.toml` as `[projects."/path/to/project"] trust_level =
"trusted"` (`core/src/config/mod.rs`, `set_project_trust_level_inner`),
keyed to the *main* repository root for worktrees
(`git-utils/src/trust.rs`, `resolve_root_git_project_for_trust`). The TUI
prompt text (`tui/src/onboarding/trust_directory.rs`): "Trust this folder?
Codex can read, edit, and run files here ... Your trust decision will be
saved." and "Note: You’re in a subdirectory of a Git project. Trusting
will apply to the repository root".
<https://github.com/openai/codex/tree/main/codex-rs>

**Fact.** Same model as Claude Code: one `~/.codex`, trust per path,
sessions per cwd. Codex additionally makes the cwd-mismatch on resume an
explicit prompt.

## 7. Linux namespaces — can a running box gain a mount?

### Adding a mount from outside

`setns(2)` <https://man7.org/linux/man-pages/man2/setns.2.html>:

> Changing the mount namespace requires that the caller possess both
> CAP_SYS_CHROOT and CAP_SYS_ADMIN capabilities in its own user namespace
> and CAP_SYS_ADMIN in the user namespace that owns the target mount
> namespace.

> A process reassociating itself with a user namespace must have the
> CAP_SYS_ADMIN capability in the target user namespace.

> A multithreaded process may not change user namespace with setns()

`user_namespaces(7)` <https://man7.org/linux/man-pages/man7/user_namespaces.7.html>:

> A process that resides in the parent of the user namespace and whose
> effective user ID matches the owner of the namespace has all capabilities
> in the namespace.

> Holding CAP_SYS_ADMIN within the user namespace that owns a process's
> mount namespace allows that process to create bind mounts and mount the
> following types of filesystems: /proc, /sys, devpts, tmpfs, ramfs,
> mqueue, bpf, overlayfs

Every non-user namespace "is owned by the user namespace in which the
creating process was a member at the time of the creation of the
namespace."

`nsenter(1)` <https://man7.org/linux/man-pages/man1/nsenter.1.html>:
`-U/--user` and `-m/--mount` enter the target's namespaces via
`/proc/pid/ns/*`; "nsenter always sets UID for user namespaces, the
default is 0" (`-S`/`-G` override).

**Fact, assembled.** An unprivileged host process (the wormhole user, in
the initial userns) cannot `setns` straight into the box's mount
namespace: it lacks `CAP_SYS_ADMIN` in its *own* userns. It can `setns`
into the box's **user** namespace, because it is the owner and so holds
all capabilities there. After that its "own user namespace" is the box's,
where it has full capabilities, so `setns(mnt)` and then `mount(MS_BIND)`
are both permitted. So `nsenter -U -m -t <pid> mount --bind …` is
possible from the host, in that order, single-threaded.

**Inference.** It is possible but ugly: the joined process carries the
box's full capability set, bypasses whatever seccomp filter the box's
own processes were started under (a filter is per-process, inherited on
fork, not attached to the namespace — see
[hardening-sources.md](hardening-sources.md) §0), and mutates a mount
list the running agent is using. No tool in sections 1–5 does this.

### One home, several mount namespaces

`unshare(2)` <https://man7.org/linux/man-pages/man2/unshare.2.html>:

> CLONE_NEWNS: Unshare the mount namespace, so that the calling process
> has a private copy of its namespace which is not shared with any other
> process. ... Use of CLONE_NEWNS requires the CAP_SYS_ADMIN capability.

`mount_namespaces(7)` <https://man7.org/linux/man-pages/man7/mount_namespaces.7.html>:

> unshare(2) with the CLONE_NEWNS flag: the mount list of the new namespace
> is a copy of the mount list in the caller's previous mount namespace.

> Each mount namespace has an owner user namespace. ... If the new
> namespace and the namespace from which the mount list was copied are
> owned by different user namespaces, then the new mount namespace is
> considered less privileged.

> When creating a less privileged mount namespace, shared mounts are
> reduced to slave mounts. ... Mounts that come as a single unit from a
> more privileged mount namespace are locked together and may not be
> separated in a less privileged mount namespace.

`unshare(1)` <https://man7.org/linux/man-pages/man1/unshare.1.html>:

> unshare since util-linux version 2.27 automatically sets propagation to
> private in a new mount namespace to make sure that the new namespace is
> really unshared.

**Fact, assembled.** After `setns(user)` then `setns(mnt)` into the box,
`unshare(CLONE_NEWNS)` gives the session a mount namespace that (a) is a
copy of the box's mount list, (b) is owned by the box's userns because
the caller is now a member of it, so it is *not* less privileged relative
to the box — the box's locked-mount rules do not tighten further — and
(c) with propagation private, takes bind mounts that no other namespace
sees. **Inference.** This is the "one home, several mount namespaces"
pattern: PID 1 keeps the mount view it started with; a session that needs
a second workspace binds it into its own namespace only. It mirrors the
`Refresh` rule in CONTEXT.md exactly (new sessions only; PID 1's tree
keeps its start values). The cost is the same as above: the session
process must be launched from inside the box's userns with its
capabilities and seccomp reapplied by wormhole, not inherited.

## 8. What this means for wormhole

Facts about the current code, for the record:

- The key is the workspace basename plus the id
  (`crates/wormhole-core/src/paths.rs`, `box_key`), and "Several boxes may
  share a workspace; none of them shares a home" (`home_dir`).
- The workspace is bound at its host-identical path
  (`crates/wormhole-core/src/mount_plan.rs`, `MountOp::Bind { source:
  workspace, target: workspace }`).
- `gc`'s "dead" verdict includes "a home whose workspace no longer exists"
  (CONTEXT.md).
- wormhole seeds the products' first-run/trust answers into `.claude.json`
  and `config.toml` per workspace ([codex-support.md](codex-support.md)).

**Inference, from the sources above.**

1. The products need nothing. Claude Code and Codex are built for one
   home holding N cwd-keyed projects, and the host-identical mount means
   the box's `~/.claude/projects/<escaped path>` entries are the same
   names the host would use. `--continue` stays per-cwd; `--resume <id>`
   already crosses projects.
2. The mount is the hard part, and there are only three honest answers:
   (a) mount every workspace at start and restart the box to add one —
   the devcontainer answer, and the only one any surveyed tool takes;
   (b) per-session mount namespaces (section 7), which keep PID 1's view
   frozen and give each attach its own bind — consistent with `Refresh`,
   but it moves capability handling into the attach path; (c) mutate the
   box's mount namespace from outside — possible, unprecedented, and it
   changes what a running hostile agent can reach without it restarting.
3. Identity has to become a list. The key's basename half stops meaning
   anything once a box has two workspaces; the record's single workspace
   becomes a set; the `gc` "dead" proof becomes "every workspace gone";
   the trust seed is written once per workspace, not once per box; and
   the claim stays one flock per home, which is right — two workspaces
   attached to one running box are two sessions, not two boxes.
4. The thing no surveyed tool answers: what a box's *boundary* means when
   it holds two workspaces. With distrobox/toolbx there is no boundary to
   speak of. With devcontainers there is one folder. wormhole's baseline is
   "the workspace and nothing else"; a second workspace is a grant in all
   but name, and an agent in either can write the other. That is a policy
   choice, not a mechanism gap, and it is the one this research cannot
   settle.
