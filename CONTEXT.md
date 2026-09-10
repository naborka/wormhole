# wormhole

Isolation tool for AI coding agents run with all permissions granted. The agent is assumed hostile: it sees one directory and nothing else of the machine, and holds no login you did not hand it. It is on the host's network and speaks to the world for itself.

## Language

One name per thing, and the names this project avoids. The documents, the code and the error messages all use these words in exactly this sense.

### Core nouns

**Workspace**:
The directory wormhole was invoked in, identified by its absolute path. The only host directory a box writes.
_Avoid_: project, repo, working directory

_Kept despite a collision._ Cargo, Terraform CLI, HCP Terraform and VS Code all call something else a workspace, and this repository is itself a Cargo workspace — so inside this tree the word means two things. It stays because it is the right English word for the directory you work in, and because the alternative is Terraform's: it renamed `env` to `workspace` to escape one collision, landed in a worse one, and has been explaining the difference ever since. Where the ambiguity could bite — a document that talks about both — say "Cargo workspace" for the other one.

**Manifest**:
The `wormhole.toml` that is the whole recipe for a box — image, agent, access, environment. A workspace has one; so does a role.
_Avoid_: config, settings, profile

**Box**:
The running sandbox for one workspace. Its root filesystem is a throwaway copy of the image, deleted when the box exits. A workspace holds as many boxes as you make, each with its own id and its own home. Two products of one role are two boxes: the home holds that product's login and history.
_Avoid_: container, sandbox, VM

**Session**:
One terminal attached to a box. The launch holds the first; `wormhole attach` opens more.
_Avoid_: tab, instance, agent run

**Role**:
A directory holding a manifest and whatever that manifest names — an instructions file, a preflight hook — that is not tied to one workspace. Reached by path, by installed name, or by pinned commit; those are three ways to *reach* one role, never three roles. A role may offer more than one product; that does not make it more than one role.
_Avoid_: profile, template, persona (a persona is a file a role ships, not the role)

**Product**:
The vendor CLI a box runs as PID 1: `claude`, `codex`, `grok`. Named by `[agent] run`, or by `--run`. Omit `--run` and a terminal picks; a resume keeps the recorded product. Not the process (that is the agent), not the model, not the role.
_Avoid_: agent (the hostile process in the box), client (an MCP server), runner, backend

**Source**:
Where a role comes from, and therefore which role it is: the canonical directory of one on this machine, the repository URL of one that was fetched. What a box records, together with the product, to decide whether two starts mean one box. Not the commit — that is which *version* — and not the name, which is one of several spellings.
_Avoid_: origin, identity, ref (a ref is what the user types)

**Alias**:
A name you gave something that already has an identity of its own — a box, which has an id; a role, which has a source. Yours, local to this machine, and changeable at any time; the identity under it is none of those. `wormhole box --as api` sets a box's, `wormhole role add <dir> --as java` sets a role's. An alias never looks like the identity it stands in for: a box's may not be twelve hex characters, because that is what an id is.
_Avoid_: name (a manifest has one of those, and several boxes share it), label, tag, handle

**Boundary**:
The namespaces a box runs behind: user, mount, UTS and PID. No network namespace: a box is on the host's network. No daemon, no root, no Docker.
_Avoid_: jail, isolation level

### Images

**Base**:
The rootfs tarball a manifest names by URL, verified against `base_sha256` before anything is extracted. Cached under its digest and shared by every image built from it.
_Avoid_: distro, rootfs image

**Artifact**:
A file a recipe names by URL and digest, fetched **on the host** and placed in the build box read-only. What `base` already is, for everything else a build installs: the digest is the proof, so the box opens no connection and nothing about the transport is trusted. Cached under its own digest, like a base.
_Avoid_: download, asset, dependency, vendored file

**Image**:
The finished read-only tree — base, packages, build lines — cached under a digest of `[image]`. Change a key in that table and the next build builds; change one anywhere else and it does not.
_Avoid_: layer, snapshot, container image

**Home**:
The per-box directory mounted as the box's `$HOME`. The only box state that outlives the box: logins, history, and whatever the agent installed there.
_Avoid_: home volume, state dir

### A box's life

**Key**:
What a box's store entries are named by: its workspace's basename and its id, `proj-a3f9c1e40b2d`. One string behind the home and the lock, so the two can never disagree about which box they are. A key carries the id, which is why a box whose record cannot be read can still be named by one.
_Avoid_: slug, path, dirname

**Claim**:
The `flock` a box holds for as long as something is using it. The only honest answer to "is this box running": a list can go stale between the reading and the acting, and taking the lock cannot. Every command that touches a home takes it first.
_Avoid_: lock (that is the file; the claim is the hold on it), pidfile, mutex

**Stop**:
Ending a box's process. The box survives — its home is untouched and the next start picks it up where it was.
_Avoid_: kill, close, delete

**Reset**:
Emptying a box's home while keeping the box: same id, same alias, same workspace, same role. The difference between starting over and starting somewhere else, which is what a new box would be.
_Avoid_: clear, wipe, reinit

**Remove**:
Taking a box away for good — its home. Never done on wormhole's own initiative: `gc` reclaims only what it can prove, and "finished with" is not something anything can prove.
_Avoid_: delete, destroy, prune

### What `gc` can prove

Four verdicts, and the differences between them are the point. `gc` deletes only what it can prove, so each verdict says exactly what was proven.

**Dead**:
Proven finished with: a box directory whose process is gone, a home whose workspace no longer exists, a lock whose box is gone. What a bare `--delete` takes.

**Live**:
In use, or claimed by something still running. Never taken, however the flags are set.

**Unreferenced**:
Proven that no box on this host starts from it — a digest that no kept box's recipe names. A weaker claim than dead, because a recipe built but never started from references nothing that can be counted, so `--unreferenced` is what asks for it and `--delete` alone leaves it.
_Avoid_: unused, orphaned, stale (each of those claims more than was proven)

**Unproven**:
A recipe could not be read, so nothing about what references it can be shown either way. Reported with its size and never taken. One unreadable manifest makes the whole answer unproven rather than deleting on a gap in the evidence.

### What a box can reach

**Grant**:
A host path that a manifest's `[access] grants` names, mounted into the box. Baseline is empty — the workspace and nothing else — and every grant prints at launch.
_Avoid_: permission, allow rule, exception

**Credential**:
A secret that belongs to the host. It reaches a box only by a credentials mode — `none`, `copy`, `share` — or by an explicit grant; nothing else carries one across.
_Avoid_: token, key, secret (except when quoting a protocol field)

**Credentials mode**:
How a box's agent gets its login, set by `[access] credentials` or `--credentials` on the start. `none`: a clean box home, and the agent's own `/login` inside the box. `copy`: the host's credential files copied into the box home once, where absent; the box refreshes its own copy. `share`: the host's files bound read-write at the same place in the box home, so one login serves both and a refresh in the box lands on the host.
_Avoid_: auth mode, login sharing, credential grant (that is a `grants` line, which is the other route)

**Broker** (_retired_):
The host-side process that once held the credential and handed the box a socket instead. Built, proven, removed on 2026-09-08; the word survives only in the design record.
_Avoid_: using it for anything current

**Declared variable**:
An environment variable named in the manifest's `[env]` or by `--env`. The only kind that reaches a box; an undeclared host variable never crosses. Per variable: `fixed` is the manifest's own value and never moves, `default` fills in when the host has none, `ask` is asked for once, kept host-side, and masked on every screen.
_Avoid_: passed variable, env override

**Baked env**:
The environment a start computes and writes beside the box's pid — the box's own, for its whole life. What `wormhole ps <id>` shows and what every session starts from.
_Avoid_: snapshot, cached env

**Refresh**:
What every attach does to the baked env: a value the attaching shell exports wins, the baked one survives where it exports none, `fixed` never moves. Reaches new sessions only — Linux writes no other process's environment, so PID 1's tree keeps its start values.
_Avoid_: sync, reload

### A role from a repository

**Pin**:
The commit a remote role is installed at. `git` verifies every object against its own hash, so the commit id is the proof and nothing about the transport is trusted. A ref with no pin is refused where it is typed.
_Avoid_: version, tag, revision

**Checkout**:
The fetched commit on disk, under `checkouts/<sha>` in the data home. Named by a digest, never modified, shared by every role pinned to it.
_Avoid_: clone, cache

**Approval**:
The yes given at `wormhole role add`, after the whole recipe has been shown. Per commit and host-wide: a different commit is a different pin with no approval at all. A launch never asks — it refuses and names the `role add` that fixes it.
_Avoid_: trust, consent, permission

### Safety

**Launch assertion**:
A check run before the terminal is handed over. Failure is refusal to start, never a warning and never a quiet downgrade.
_Avoid_: health check, precondition warning

**Banner**:
The last line a start prints, naming what the box got beyond the baseline — a named resolver, a read-only root, host paths bound in. Silent when nothing was, so a line always means something.
_Avoid_: status line, header

**Receipt** (_retired_):
The list of what the agent changed in the workspace, measured against a snapshot taken before the start. Built, then removed on 2026-09-08 with `[runtime] snapshot`; version control is the undo point.
_Avoid_: using it for anything current

**Panel**:
The box list reached by running `wormhole` with no arguments. Starts, joins and stops boxes, and shows the full permission preview before anything starts.
_Avoid_: TUI (that is the whole terminal program), dashboard, control panel
