# wormhole — status

Live tracker: what works now, what is being built, what comes next.
Update this file in the same commit as the change it records.
`CONCEPT.md` says what and why, `PLAN.md` says in what order and with which tests. This file only tracks state.
`AGENT.md` holds the default instructions every box hands its agent, baked into the binary and seeded into the box home on every start.

Tests today: **470** (`cargo test`: 369 pure, 101 binary), **31 kernel** (`cargo test -p wormhole --features kernel-tests`, real host only). The 17 in `tests/build.rs` fetch a rootfs through `curl`, so they need one on the host; everything else runs anywhere.

## Done — an agent runs in a box today

**The MVP works.** `wormhole box` starts Claude Code inside an isolated box built
from `wormhole.toml`, with permissions bypassed, and it holds the terminal.

| What | Try it |
|---|---|
| `wormhole init` — a working manifest in this workspace, from the one starter recipe in `templates/wormhole.toml`. Never over one already there | `cargo run -p wormhole -- init` |
| `wormhole box [--new \| --id <id\|name>] [--as <name>]` — the manifest's agent, in a fresh copy of the built image. A folder holds as many boxes as you make: a bare `box` resumes this workspace's most recently used free one for this role and makes another only when every one is busy; `--new` asks for another outright, `--id` names one by its id or by the name `--as` gave it. `--as` names the box this start makes or resumes, so `attach api` and `stop api` work instead of twelve hex characters; a name already on another box here is refused, and one spellable as an id is refused where it is set. Each box keeps its own `$HOME`, claim and snapshot. Prints the boundary banner as its last line before the agent takes the terminal | `cargo run -p wormhole -- box --new` |
| `wormhole box -- <cmd>` — same box, your command instead of the agent | `cargo run -p wormhole -- box -- sh` |
| `wormhole build` — fetch the rootfs by URL, verify its sha256, `apk add`, run `[image] build`; two caches | `cargo run -p wormhole -- build` |
| `wormhole run [--grant <path>]... [--dns <ip>] [--image <dir>] -- <cmd>` — the boundary on its own, no manifest | `cargo run -p wormhole -- run -- sh -c 'ls -a /; echo $$'` |
| `wormhole ps [--all]` — running boxes: id, name, pid, agent, uptime, workspace; reaps dead entries. `--all` lists every box the host keeps, running or idle, most recently used first | `cargo run -p wormhole -- ps --all` |
| `wormhole stop <id\|name>` — end a running box from anywhere: kills its PID 1, and its own `wormhole box` cleans up. The panel’s `d` is the same thing on the selected row. Reports rather than swallows — a stop that could not happen never looks like one that did | `cargo run -p wormhole -- stop a3f9c1e40b2d` |
| `wormhole attach <id\|name> [-- <cmd>]` — a second terminal into a running box. Default is the box's agent (attaching means getting back to the agent); `-- sh` for a shell | `cargo run -p wormhole -- attach a3f9c1e40b2d` |
| `wormhole.toml` naming a role — `role = "<dir\|ref>"` as the whole file, so a project says which role it uses once and nobody types `--role`. Refused beside `[image]`: `role` names the recipe instead of carrying one, never merged with it. A launch still fetches nothing and asks nobody | `printf 'version = 1\nrole = "github:you/r@<sha>"\n' > wormhole.toml` |
| `wormhole box --role <name\|dir\|ref>` — a role's manifest, instructions and preflight from `~/.config/wormhole/roles/<name>/`, or any directory when the argument has a `/`, or a pinned commit when it starts with a transport. An explicit `--role` beats the workspace's own manifest. **Which of the three you type never decides which box you get**: a role is identified by where it comes from — the canonical directory of a local one, the repository URL of a fetched one — so every spelling of one role is one box, and a re-pin keeps the box you were working in | `cargo run -p wormhole -- box --role alphaca` |
| `wormhole role add <dir> [--as <name>]` — name a role you wrote. Symlinked, never copied, so the directory you edit stays the role. Nothing is fetched and nothing is approved: there is no commit here to approve, and a directory can change a second after any answer. Prints the recipe instead | `cargo run -p wormhole -- role add ./roles/alphaca` |
| `wormhole role add <url>@<sha> [--as <name>]` — a role that lives in a git repository, pinned to a commit and never a branch. `git` verifies every object against its own hash, so the commit id is the proof and no digest of ours is kept. Fetches it, shows the whole recipe — grants, env, hooks, **and** the base, the packages and every `[image] build` line — and asks. One approval per commit, host-wide; a changed pin shows old sha → new sha and asks again. The pointer goes to config, the checkout to `~/.local/share/wormhole/checkouts/<sha>`. A bare path counts as a repository only when it carries a pin, which is what tells `/srv/mirror@<sha>` from a folder of files | `cargo run -p wormhole -- role add github:you/role@<sha>` |
| `wormhole role list \| show <name\|dir> \| remove <name>` — what is installed and whether it can start; the whole recipe without installing it; and taking a name back. `remove` unlinks the name and never follows it, so removing an installed local role leaves the directory you work in alone | `cargo run -p wormhole -- role list` |
| `wormhole usage` — what is left of the account's usage windows: session, weekly, and the scoped weekly one that binds first, with reset countdowns. During conversation the same reading shows in Claude Code's status bar as `SESSION: 13%  FABLE: 78%  WEEKLY: 47%`, kept fresh from the host — the box holds no credential and makes no call for it | `cargo run -p wormhole -- usage` |
| bare `wormhole` — the panel: every box, running and idle, most recently used first. Enter does the one thing that row allows — join a running box's agent, or start an idle one again in its own workspace. `n` makes another box here: it picks a source (workspace manifest or an installed role), previews everything the box will see, then starts it; `d` stops the selected box and says so, and on a box that is not running says *that* instead of redrawing an unchanged screen; `x` removes a box and `r` resets one, both asking first with `y` as the only key that answers and every other key — `q` included — cancelling; `q` quits | `cargo run -p wormhole` |
| `wormhole rename <id\|name> <new name>` — what a box answers to besides its id, set without starting it. Scoped to the box's workspace and refused where another box there already answers to it; refused while the box runs, because its registry entry carries the name it started under and nothing rewrites that in flight | `cargo run -p wormhole -- rename a3f9c1e40b2d api` |
| `wormhole reset <id\|name>` — empties a box's home and keeps the box: same id, name, workspace and role, nothing the agent put there. The difference between starting over and starting somewhere else | `cargo run -p wormhole -- reset api` |
| `wormhole remove <id\|name>...` — takes boxes away: the home each kept and the snapshot beside it. Several at once, every name resolved before any box goes, so a typo at the end refuses the line rather than leaving half of it done. A home whose record cannot be read is still a box — its directory name carries the id, which is what `--id` already starts one by | `cargo run -p wormhole -- remove api web` |
| All three take the box's own claim first, so the kernel answers "is this running" rather than a list that can go stale, and no removal can reach a home an agent is writing to. The panel's `x` and `r` call the same bodies | |
| `wormhole gc [--delete [--unreferenced]]` — what the data home holds and what of it can be given back. `--delete` takes what is proven dead: box directories whose process is gone, homes whose workspace no longer exists, locks whose box is gone. `--unreferenced` widens it to images, bases and artifacts no box on this host starts from — proven by reading every kept box's recipe, since a recipe names every digest the store keeps for it. Two claims, kept apart: a recipe built but never run from references nothing countable, so a bare `--delete` leaves it. One unreadable recipe makes the whole answer `unproven` and nothing is taken | `cargo run -p wormhole -- gc --delete --unreferenced` |
| `wormhole broker` — the host-side proxy on a unix socket. Holds the credential, renews it under a lock, strips the box's dummy key and injects the real token, and streams the reply straight back. A box with `[access] broker = true` gets the socket and wormhole's own binary bound read-only under `/run`, starts a forwarder on its loopback, and is pointed at it by `ANTHROPIC_BASE_URL` — so the box holds no credential and needs no route | `cargo run -p wormhole -- broker` |
| `wormhole doctor` — 9 host probes, pure verdict, exit code | `cargo run -p wormhole -- doctor` |
| Purity guard: `wormhole-core` has no OS or I/O dependencies | `cargo test -p wormhole-core purity` |
| The handbook — install, quickstart, the manifest reference (every key, grouped by what it decides), boxes, roles, the broker, the access model, plus these working docs rendered | `mdbook serve docs` (deployed to GitHub Pages by `.github/workflows/docs.yml`) |

What the box gives the agent: user, mount, UTS and PID namespaces; a writable
throwaway root from the image; the workspace read-write at its host path; the
granted paths and nothing else; one named resolver; only the environment the
manifest declares; its own `PATH`; an **empty capability bounding set and
no-new-privs**, so nothing in the box can regain a capability or raise
privilege through any `execve`; `Ctrl-C` that reaches the agent and not
wormhole. The agent is PID 1 and its uid matches yours, so files it writes in the
workspace belong to you. Its `$HOME` is kept per box under
`~/.local/share/wormhole/homes/<workspace>-<box id>`, so history, settings, logins
and installed toolchains survive every restart of that box while the root stays
throwaway. Several boxes may share a workspace; none of them shares a home.

## The manifest

`wormhole.toml` in the workspace root is the whole recipe. See the file in this
repo for a working one. `version` and `name` sit at the top level; everything
else is in a table named after what it decides, and no two tables share a key
spelling — so a line written under the wrong header is refused by name rather
than quietly meaning something else.

| Key | Meaning |
|---|---|
| `version` | `1`. A file from another version is refused, not guessed at |
| `name` | What the box calls itself in `wormhole ps`. Nothing functional hangs off it |
| `[image] base`, `base_sha256` | The base tarball and its digest. Verified before extraction |
| `[image] package_sources` | Written to `/etc/apk/repositories`. `http` is fine — `apk` verifies signatures, and the fresh rootfs has no CA store yet |
| `[image] packages` | `apk add`, once, at build time |
| `[image] build` | Shell lines after the packages, in order, stopping at the first failure |
| `[agent] run` | Which agent runs. Only `claude` is known so far |
| `[agent] model` | Becomes `ANTHROPIC_MODEL` |
| `[agent] instructions` | A file beside the manifest, appended after the built-in `AGENT.md` in the agent's instructions file, so it wins where they disagree |
| `[agent] preflight` | A script beside the manifest, seeded into the box home and run there, then the agent replaces the shell |
| `[access] grants` | Host paths the box may see. `~` expands. Symlinks, `..` and workspace overlap are refused |
| `[access] dns` | The one resolver. Absent means no DNS at all |
| `[access] network` | `"host"` (default) or `"none"`. `"none"` gives the box a network namespace of its own holding only loopback — no route off the machine to take. Not the default yet, because an agent that cannot reach the API cannot work; that flips with the broker |
| `[access] broker` | Reach the API through the host-side broker, holding no credential |
| `[access] host_ca` | **Adds** the host's CA bundle (found per distro, symlinks resolved) to what the box trusts, in the running box and the build box alike — for networks that intercept TLS. Off by default |
| `[[image.artifact]]` | `url`, `sha256`, `into`: a file fetched on the host, proved by digest, bound read-only on the build box's `/tmp`. A build that names all its fetches this way opens no TLS of its own |
| `[runtime] rootfs` | `"copy"` (default) or `"readonly"`. `"readonly"` binds the cached image itself instead of copying it, with a tmpfs seeded from the image over `/etc` and `/var`. No per-box copy at all, so start time stops scaling with image size — and the box can no longer install a package at run time, which is the trade |
| `[runtime] snapshot` | Reflink copy of the workspace before the box starts, and a list of what changed when it exits. Off by default: where reflink is unavailable an honest snapshot is a full copy of your tree on every start, so wormhole refuses rather than pay it silently |
| `[limits] cpu`/`memory`/`pids` | cgroup v2 caps: `"1.5"` cores, `"512M"`, `256`. A limit that cannot be applied stops the box — a manifest that asked for a ceiling and silently got none is the failure the limit exists to prevent |
| `[env.NAME]` | `default` lets the host's value win; `fixed` ignores the host |

```
wormhole build     # optional: fetch, verify, install — once per recipe
wormhole box       # every time: the agent, in a fresh copy of that image
                   # (builds the image first when nothing has)
```

Two caches, so a changed package list never refetches and a changed grant never
reinstalls: the **base** under the tarball's sha256, the **image** under a digest
of `[image]` and nothing else. A second `wormhole build` finds that image and
prints `image ready` without fetching or installing anything. Every box gets its
own `cp --reflink` copy of the image, deleted when it exits, so nothing an agent
does survives into the next box or back into the image.

## Done — the road to that MVP

| # | Step | State |
|---|---|---|
| M1 | The boundary: user, mount, UTS, PID namespaces, PID 1, exit codes | **done** |
| M2 | `--grant <path>` binds a host path into the box | **done** |
| M3 | `/dev/tty`, and `Ctrl-C` reaching only the box | **done** |
| M4 | `--dns <address>`; without it the box has no resolver | **done** |
| N1 | `wormhole.toml` parsed and validated (pure) | **done** |
| N2 | `wormhole build` — fetch, verify sha256, extract, cache | **done** |
| N3 | `wormhole build` part two — `apk add` and the `[image] build` lines as root in a box with nothing of yours mounted | **done** |
| N4 | `wormhole box` — the built image, declared env only, grants, preflight, then the agent | **done**, confirmed on the host |
| N5 | Persistent box home, kept per box; bare `run` stays throwaway | **done**, needs a host run to confirm |
| N6 | Default `AGENT.md` baked in, seeded as the agent's own instructions file (`~/.claude/CLAUDE.md`) on every start; manifest `instructions` appends a workspace file. `name` waits for N7, the first thing that reads it | **done** |
| N6b | First-run prompts pre-accepted: onboarding, bypass warning, workspace trust and theme merged into the home's `.claude.json` — never overwriting what the agent wrote there itself. All four keys read off Claude Code 2.1.228 (`hasCompletedOnboarding`, `bypassPermissionsModeAccepted`, per-project `hasTrustDialogAccepted` + `hasCompletedProjectOnboarding`, `theme`); they all live in `.claude.json`, none in `settings.json` | **done**, needs a host run to confirm the agent reaches a prompt |
| N7 | Box registry: `boxes/<pid>/box.toml` beside the root copy, written at start, removed on exit, dead entries reaped. `wormhole ps` lists them; starting the same box twice is refused | **done** |
| N8 | `wormhole attach <id>`: joins the box's user, UTS, PID and mount namespaces through its PID 1 (host pid kept in `boxes/<id>/init.pid`), runs a shell or command in the workspace. The box gets its own `devpts` and `/dev/ptmx`, so terminal-opening tools work | **done**, needs a host run to confirm (`attach_joins_a_running_box`) |
| N10 | Roles: `wormhole build/box --role <name\|dir>` reads the manifest, instructions and preflight from `~/.config/wormhole/roles/<name>/` or a directory path. An explicit `--role` beats the workspace's own manifest; `instructions` and `hooks.preflight` resolve against the role's directory (the hook is seeded into the box home) | **done** |
| N9 | The panel, first slice: bare `wormhole` lists running boxes live, Enter joins the box's agent (execs `attach`), `n` starts a new box from the current workspace's manifest, `d` kills the box's PID 1, `q` quits. Pure state machine in `wormhole-core::tui`, thin `crossterm` shell around it. The role/grant picker is N9b | **done**, needs a host terminal to confirm |
| N12 | The rest of a box's life: `wormhole rename`, `reset` and `remove`, and `x`/`r`/`y` in the panel. One rule for all three — the box must be idle, proven by taking its own claim rather than by reading a list. `gc` learns to prove a built thing unreferenced by reading every kept box's recipe, and reclaims orphaned locks. Every decision stays pure: `run::parse_*`, `tui::Act` and `tui::Key::from_char`, `gc::Sweep`/`plan`/`built_verdict`/`lock_verdict`, `manifest::referenced_digests`, `paths::key_id`, `home::target`/`find`/`alias_conflict`. The claim is the *type* the verbs act on (`Claimed`), so a fourth verb cannot forget to take it | **done**, 14 lifecycle tests plus the panel driven on a real pty |
| N11 | Usage limits in the conversation: the host polls the endpoint behind Claude Code's own `/usage` (host credential, `curl` config over stdin so no token in argv), caches the reply, and `wormhole box` writes the rendered line into `<box home>/.claude/wormhole-limits` once a minute. A seeded status-line script reads that file; the `statusLine` entry is merged into the home's `.claude/settings.json` and one the user set themselves wins. Parsing and both renderings are pure (`wormhole-core::limits`), including the scoped per-model weekly window that binds before the headline weekly one | **done**, needs a host run to confirm the status bar |

What the first real runs taught, each fixed at the root rather than patched:

- **`proc` through a `pre_exec` closure hid its own errors.** That channel carries
  only an errno, so every failure arrived as a bare `EINVAL`. One fork after the
  unshare made PID 1 do all the privileged work; the failure and the blind spot
  went together.
- **An inherited `PATH` describes the host's filesystem.** Arch has no `/bin`,
  Alpine keeps busybox there, so the box could not find `mkdir`. The box sets its
  own `PATH` now.
- **Binding all of `~/.claude` imports host wiring.** Its `settings.json` hooks
  call `bash`, `shellfirm` and `rtk`, none of which exist in the box. Only the
  credential file is granted.
- **Some variables describe the box, not you.** Host `SHELL` is `/usr/bin/zsh`,
  absent in the box, so `env` gained `value` (fixed) beside `default`.

What a review of the finished code found, each fixed at the root as well:

- **`gc` reused the resolver a *launch* uses, so a listing could fetch and
  prompt.** A box's recorded role may be a pinned ref; resolving one may
  `git fetch` and take the terminal for an approval. A bare `wormhole gc`
  could therefore reach the network, seize the screen and exit before
  printing a line — and its answer depended on whether stdin was a
  terminal, so the same store proved different things to a person and to
  cron. Whether a resolve may fetch is now something the command says
  (`Fetch::IfAsked` / `Fetch::Never`), asked at the edge rather than
  inferred from who is watching.
- **`gc` reaped the box directories it had just measured.** Its reference
  scan called `live_boxes`, which renames a dead box's directory and
  deletes it in a thread — after the recursive size walk, and before
  `--delete` tried to remove a path that was already gone. Each store
  directory is walked exactly once now, and reaping stays with `ps` and
  the panel, which are the two things whose job it is.
- **`gc --delete` used the quiet deleter and printed `removed` anyway.**
  A person who asked for space back was told they had it whether or not
  they did. The loud form is what a person's request takes, and the exit
  code says so.
- **Two booleans decided what the report described and what the loop
  removed.** They had to agree and nothing made them. `gc::plan` partitions
  once; the report prints that plan and the caller deletes from it.

- **A registry entry was written in place, where every peer reads it.**
  `box.toml` was truncated and rewritten while `ps`, the panel and every
  starting box scanned it, and a reader that caught it mid-write took itself
  down with a parse error. Writing beside it and `rename(2)`-ing over is now
  the one body behind every file a peer reads — the same one the usage cache
  already used. An entry that still cannot be parsed names its box and is
  skipped.
- **A box claimed its workspace after copying its root, not before.** The
  one-box-per-workspace check was blind for the whole copy — 0.4s with
  reflink, several seconds without — so two boxes could both pass it. The
  entry is written first now, which is what makes the check true for the box
  starting next.
- **Reaping a dead box needed its entry to be readable.** Liveness comes from
  the directory name, which is the pid, so a box killed before it wrote an
  entry is cleaned up too, and a `.dead` directory whose deletion was
  orphaned by a short-lived `ps` is picked up by the next scan instead of
  leaving a whole root copy on the disk.
- **N boxes asked for one account's usage windows N times a minute.** The
  windows are the account's and the cache is shared, but each box's thread
  fetched on its own timer. The fetch is gated on the shared reading's age:
  six boxes ask the rate-limited endpoint once a minute, not six times.

What a second pass over the review's own output found, each fixed at the root:

- **A box was claimed by a scan, and a scan is never a claim.** Writing
  the registry entry before the root copy narrowed the window from seconds to
  microseconds; it did not close it, because reading the entries and writing
  our own can never be one step. The claim is an `flock` now, held by the
  kernel for the process's whole life. That deletes the failure and the stale
  state together: a box killed at any point leaves nothing to reap and nothing
  a second start can mistake for free. The registry scan for a peer in the same
  workspace is gone, not kept beside it. The claim is per box, so a workspace
  holds as many boxes as you make and only starting the *same* one twice is
  refused.
- **The banner printed on `wormhole box` only.** §1 says *every* launch names
  its boundary stage; a bare `wormhole run` is a launch and printed nothing.
- **`assert_launch` was pure data nothing fed.** Two of its facts are real
  observations today, and both are made: after the pivot the box reads its own
  `/proc/self/mountinfo` and refuses if any read-write mount is not in the
  plan, and after dropping capabilities it reads its own `/proc/self/status`
  and refuses if any is left. The plan was exhaustively tested and was never
  what the box ran behind — the mounts were, and nothing checked they agreed.

What a documentation audit of the finished code found:

- **The banner claimed `egress: none` for a brokered box.** §1's whole point
  is that a boundary which does not say which stage it is in is a lie, and
  the pairing the design aims at — `network = "none"` with `broker = true` —
  was the one case that printed it. The box reaches the model API through
  the broker's socket. Root cause: the banner was assembled from three
  hand-picked fields at each call site, so a new way out could be added
  without the banner being told. It takes the whole `RunArgs` now
  (`Banner::for_run`), which also puts the grant count — grants plus the CA
  bundle — in the pure core where it is tested, instead
  of an untested helper in the binary.

What running the panel on a real terminal found:

- **A reader printed to the terminal the panel was drawing on.** The box
  list is rescanned once a second, and a home with no record in it was
  reported with `eprintln!` from inside that scan — in raw mode, where a
  newline moves down without returning to the left margin. The screen
  filled with a staircase of the same warning, over the box list, forever.
  Root cause: reading the store both returned data and reported on it, so
  every caller inherited the report whether it owned the terminal or not.
  A scan now returns what it could not read (`home::Scan`), the panel
  draws those lines on its own screen, and every other surface prints them
  once. The regression test drives the panel on a real pty and fails on any
  newline with no carriage return in front of it.
- **An empty or relative `XDG_DATA_HOME` scattered the store into whatever
  folder wormhole was run from.** The variable was taken as given. A
  relative store follows the working directory, so a box started in one
  folder would not see the locks, homes or registry of a box started in
  another — the claim would stop being a claim. Unset, empty and relative
  now all take the default under `$HOME`, which is what the XDG spec says
  and what the store's whole purpose needs.

## Next — what to build now

Ordered. Each step ends with something runnable.

| # | Step | Why now |
|---|---|---|
| N9b | **Create from the panel.** First half done: `n` lists the workspace manifest and every installed role, shows a full permission preview (grants, CA trust, dns, env, hooks, image state) before starting. Remaining: editing grants interactively and writing the choices back into a manifest | picker and preview **done**; grant editing open |

The security work the MVP postponed, now nearly all done: the capability
bounding set is stripped and no-new-privs is set (**done**, asserted from the
box's own `/proc/self/status` — verified live at `CapBnd: 0000000000000000`),
a box's claim is an `flock` the kernel holds (**done**), every
read-write mount the box ends up with is checked against the plan (**done**),
`network = "none"` gives the box its own namespace with loopback and no route
(**done**, verified live: an empty route table), and cgroup v2 `cpu`/`memory`/
`pids` caps are applied to the box and not to the process reporting on it
(**done**, needs a host with a delegated hierarchy to confirm), and the broker
now carries bytes: `broker = true` puts the credential on the host's side of the
boundary for good (**done** — verified end to end, with the dummy key and the
box's `Host:` header both stripped and the real token injected host-side).
Pairing it with `network = "none"` is what finally makes the thesis true rather
than planned; making that pair the default is the remaining step. One postponed
item stays sharp: several boxes sharing `~/.claude/.credentials.json` race the
rotating refresh token (CONCEPT.md spike #15 — it admits exactly one writer), so
the broker moves from "next" toward prerequisite as a host runs more boxes.

## Choices worth remembering

**Alpine and a verified tarball, not layers.** Your host cannot mount
unprivileged overlayfs and has no `fuse-overlayfs`, so there is nothing to stack.
One tarball plus `apk` needs neither. `apk` also needs no second uid, so a single
`0 → you` map installs as root — `apt` would need the subuid machinery.

**Installing happens with nothing of yours mounted.** No workspace, no home, no
grants, empty environment. Third-party install scripts run as root over a
filesystem that contains only the image.

**DNS stays default-off.** `CONCEPT.md` §"`/etc/resolv.conf` is absent on purpose"
holds: a box with no resolver fails loudly rather than quietly reaching somewhere.
`dns` is the interim opt-in that lets an agent reach the API before the broker
exists, and it writes one `nameserver` line you chose — your host's resolver setup
never leaks in. It goes away with the broker.

**`apk` over `http` is not a downgrade.** Every index and package is checked
against the signing keys in `/etc/apk/keys`, and a fresh rootfs has no CA store to
do TLS with anyway.

## How `run` is shaped, and why

Three processes. `run` on the host spawns `__boxed`, which unshares user, UTS and
PID, writes the id maps, sets the hostname, and forks. **That fork is the box's
PID 1**: it unshares the mount namespace, applies every mount from the plan
including `proc`, pivots into the new root, and `execvp`s the command. `__boxed`
waits and turns the box's wait status into our exit code.

One process does all the privileged work, because `unshare` grants capabilities to
the calling process only and an `execve` by a non-root user drops them, while
`proc` shows the PID namespace of whoever mounts it.

An earlier shape put the mounts in `__boxed` and smuggled the `proc` mount through
a `pre_exec` closure. It failed every command with a bare `EINVAL` — the only
thing that channel can carry is an errno, so the real message was destroyed. The
fork removed the failure and the blind spot together.

Beyond that, `CONCEPT.md`'s later steps still stand: the OCI registry pull with a
layer cache, the `CONNECT` proxy (without it `apt`, WebFetch and context7 stay
dead in a network-less box), the git and GitHub broker, and the control panel.

## Known gaps, deliberate

| Gap | Closes at |
|---|---|
| Host network is still reachable from the box **by default**. `network = "none"` closes it today, and closing it by default needs the broker: an agent with no route cannot reach the API at all | with the broker |
| The broker serves one connection at a time. An agent makes one request at a time, so nothing has needed more yet — but a concurrent client would queue behind the one in flight | when something needs it |
| The broker's upstream leg is `curl`, so it forks per request and reuses no connection. The host's CA store and TLS stay the host's business, which is the trade | if latency bites |
| A credentials grant carries account-level capabilities the preview cannot show: `~/.claude/.credentials.json` brings every hosted MCP connector on the claude.ai account (JIRA, Slack, ...), executing server-side, invisible to the mount plan and the future broker. Documented in the handbook's access model; the real fix is a separate agent account or detached connectors | open |
| An attached shell carries the attacher's environment, not the box's declared one — that environment lives in PID 1 and nothing can read it back | if it bites; the box's own terminal is authoritative |
| Images are one verified tarball, not digest-pinned layers — your host has no unprivileged overlayfs and no `fuse-overlayfs`, so there is nothing to stack | when overlay is available |
| An image built by `wormhole build` but never started from has no box, so `gc` reads it as `unreferenced` — correct as stated, and still not the same as unwanted. It is why `--unreferenced` is a flag of its own rather than part of `--delete` | by design; revisit if it bites |
| Bare `wormhole run` inherits the host's environment apart from `HOME`, `HOSTNAME` and `PATH`. `wormhole box` passes only what the manifest declares; the build box passes nothing | closed for `box` |
| PID 1 ignores any signal it has no handler for, and reaps no orphans. The agent being PID 1 is a design choice; a reaping init would undo it | revisit if zombies bite |
| A recycled pid can make a dead box's registry entry look alive until its dir is removed | if it bites; the window is one pid wrap |
| Nothing bounds what a role asks for. `grants` is checked for shape and then mounted as written, so a role asking for `~` gets your whole home if you approve it. A fetched role's confirm — which shows every `[image] build` line — is the whole defence | when the `~/.config/wormhole` ceiling of CONCEPT.md §7 is built |
| Nothing reclaims `checkouts/`, `bases/` or `images/`. `gc` handles kept homes only. Fixing checkouts alone would leave `gc` inconsistent in a new way, so all three want one rule | when `gc` learns liveness for all three |
| The panel's `n` picks a role and previews permissions; editing grants interactively is still open | N9b |
| Attaching without a command starts a second agent process in the box; it shares the home, so `claude --continue` semantics apply, but the original agent's terminal stays where the box was started | if it bites |
| The box's root copy uses `cp --reflink=auto`: free on btrfs/XFS, silently a full physical copy on ext4 | if startup latency bites; `--reflink=always` would fail loudly instead |
| `wormhole run` still falls back to the host's `/usr` when given no `--image` | when nothing needs it |
| The usage windows come from an undocumented endpoint (the one behind Claude Code's own `/usage`); it can change or vanish without notice. Every failure degrades to the last reading with its age shown — `(12m ago)` — never to a wrong number. Fetching also needs `curl` on the host | when the broker exists, the same numbers come from response headers on wormhole's own route |
| The status line is seeded for `claude` only; the limits file is written for every box, but no other agent has a standard hook to show it | when a second agent lands |

## Environment note

Kernel-dependent tests run only on the real host (Manjaro): `cargo test -p wormhole --features kernel-tests`. In the dev container `unshare` is blocked by seccomp (`Seccomp: 2`, `CapEff: 0`, `NoNewPrivs: 1`), so no box can be built there at all. Pure tests run everywhere; kernel tests sit behind that feature flag.
