# wormhole — MVP plan

Companion to `CONCEPT.md`. That document says *what* and *why*; this one says *in what order*, and where to stop and start using it.

Every step below names its failing test first. No production code without a test that demanded it.

---

## Crate layout

Two crates. The only boundary the compiler must enforce is purity, and that needs one crate split, not six.

| Crate | Owns | Touches the OS? |
|---|---|---|
| `wormhole-core` | the five port traits, their in-memory fakes, mount planning, role manifest, grants, credential modes, launch assertions as data, banner rendering | **no** — pure, no syscalls, no I/O |
| `wormhole` | CLI, TUI, wiring, and every port impl as a module: `boundary.rs` (`clone`/`unshare`/`pivot_root`/`mount`/seccomp/Landlock), `rootfs.rs` (registry pull, layer cache, overlay), `term.rs` (PTY master, raw mode, VT screen model), `path.rs` (canonicalisation, symlink resolution) | yes |

Four ports, each defined in `wormhole-core` with an in-memory fake: `Boundary`, `RootFs`, `Terminal`, `PathResolver`. (A fifth, `Broker`, was built and removed — see Step 8.) Every decision in the system is reachable through them without a kernel. Ports are traits, not crates — a module graduates to its own crate only when compile time or reuse demands it, which is a mechanical extraction later, not a design decision now.

**Purity is enforced mechanically:** `wormhole-core`'s `Cargo.toml` carries no `libc`, `nix`, or `tokio` dependency, and a one-line CI check keeps it that way.

**Rule that keeps this honest:** if a test needs a real kernel, the logic under test is in the wrong crate. Move the decision into `wormhole-core` and leave only the syscall in the impl module.

**`PathResolver` exists because the purity claim was false without it.** Validating a grant means canonicalising it and resolving symlinks — that is I/O. So `MountPlan::compute` takes *already-resolved* paths and never touches the filesystem; resolution happens behind the port, and its fake lets a test declare "this path is a symlink to `/etc`" without creating one. Without this split, the very first grant-escape test would need a real filesystem and `wormhole-core` would not be pure.

---

## Step 0 — `wormhole doctor`

**Start here.** No ports needed, immediately runnable, and it closes two open assumptions before any architecture is committed.

Probes, each with its own specific failure message:

- `unshare(CLONE_NEWUSER)` succeeds unprivileged
- `/proc/sys/user/max_user_namespaces` > 0
- `/etc/subuid` and `/etc/subgid` have a range for your user
- Landlock ABI version available
- unprivileged `overlayfs` mount works, else `fuse-overlayfs` present
- cgroup v2 delegation available for resource limits
- workspace filesystem supports reflink (whether a box root copy is free or a full copy)
- the uid-map overlap probe from **assumption #1**, run as a diagnostic rather than asserted

**Tests (pure):** table-driven — a set of fake probe results maps to an expected verdict, exit code, and message set. The probe *execution* is a thin trait; the *verdict* is pure and is what the tests cover.

**Why first:** if userns is restricted on this host, everything after Step 3 is wasted. Find out in an hour, not a fortnight.

---

## Step 1 — mount plan (pure, no kernel)

```rust
MountPlan::compute(workspace: &Path, home: &Path, layers: &[Layer], grants: &[Grant])
    -> Result<Vec<MountOp>, PlanError>
```

This is where default-deny lives. It is pure, so it can be tested exhaustively.

**Tests:**
- workspace appears exactly once, rw, at its **host-identical** path
- no host path other than the workspace and explicit grants appears anywhere in the plan
- `/etc/resolv.conf` is never in the plan — asserted, not assumed
- `/etc/hosts` is synthetic: `127.0.0.1 localhost` plus the box's own hostname. The hostname entry is not cosmetic — Node and JVM startup paths call `gethostbyname(hostname)` and hang or fail without it
- `/etc/passwd`, `/etc/group` synthetic, your user only
- ordering is valid: parents mounted before children, overlay before binds into it
- a grant containing `..`, a grant that is a symlink (any symlink — the refusal names the target so the user grants the real path), or a path overlapping the workspace is **rejected**
- nested and duplicate grants normalize to one mount
- grants default to `ro`; `:rw` is explicit
- property test: for any generated grant set, no planned source path lies outside `workspace ∪ grants`

---

## Step 2 — launch assertions and banner, as pure data

Every assertion from CONCEPT.md §8 exists first as a pure verdict: probe results in, refusal-or-proceed out. The banner renders from the same data. Pulling this ahead of the first real box costs nothing — it is data — and means the ratchet exists before there is anything to regress.

**Tests (pure):**
- each of: digest mismatch · a readable credential path · an unplanned rw host mount · a retained capability · a held lockfile → maps to refusal with its own distinct error
- the banner renders what a start got beyond the baseline — the resolver, a read-only root, the grant count (grants, CA bundle, shared credential) — from data; pure function, snapshot-tested

Step 6 wires these verdicts to real probes; nothing about the decisions changes there.

---

## Step 3 — `Boundary` port + `Namespaces` impl

First real box. Run `/bin/true`, then `/bin/sh -c 'ls -a /'`.

**Integration tests (real kernel, gated by a feature flag so unit tests stay fast):**
- the visible filesystem equals the plan from Step 1 — nothing more
- a file created inside the workspace lands on the host owned by **your** uid, at the host-identical path
- `connect()` to any external address fails; no default route exists
- capability bounding set is empty of `CAP_SYS_RAWIO`, `CAP_SYS_BOOT`, `CAP_SYS_MODULE` and cannot be reacquired
- `/proc` inside shows only processes from this box
- child exit status propagates
- the supervisor creates a **second** namespace set as a sibling, not nested (the §4 shape, verified now while it is cheap)

---

## Step 4 — `RootFs` port + digest-pinned base

`debian:13-slim` by digest → `layers/base-<digest>/`.

**Tests:**
- manifest digest and every layer digest verified before unpack
- a mismatched digest **refuses**, leaves no partial layer behind
- an interrupted download leaves no half-unpacked layer
- a cached layer is reused with the network unreachable
- unpack rejects entries with absolute paths, `..`, or symlinks escaping the layer root

**Decided: own the registry client, ~300 lines.** The "nothing for you to install" claim in CONCEPT.md §12 is load-bearing and an external `skopeo` would contradict it. Manifest fetch, layer fetch, digest verification, tar unpack with path validation. Behind the `RootFs` port either way.

---

## Step 5 — assemble the box

overlay(base) + workspace + `$HOME` + tmpfs + synthetic `/etc`. Run `bash -c 'echo hi'`.

**Tests:**
- overlay lowers stack in declared order; the **run-time** upper is a tmpfs and vanishes on shutdown (distinct from the **build-time** upper, which Step 11 freezes into a role layer)
- `$HOME` content survives two consecutive boots
- `wormhole reset` discards `$HOME` and nothing else
- the lockfile prevents the same box being started twice into one `$HOME`, and lives host-side under `~/.local/share/wormhole`, never inside the box
- cgroup v2 limits are applied — cpu, memory, pids. **CONCEPT.md §10 claims runaway resource use is "enforced"; without this step that claim is false**, and a doc asserting a protection the code does not implement is worse than no claim

---

## Step 6 — wire the launch assertions

The pure verdicts from Step 2 get real probes. Refusal, not a warning; each violation already has its distinct error from Step 2.

**Tests (integration):**
- each Step 2 verdict fires from a real violated precondition
- the banner is emitted on every start that got anything beyond the baseline, including `--role` and non-interactive paths
- launch refusals print to stderr before any PTY exists — no panel is needed for them, by construction

This step is the ratchet. Nothing after it can quietly regress an invariant.

---

## Step 7 — PTY passthrough, foreground

`wormhole -- bash` gives you an interactive shell in the box.

**First moment you can feel the thing.**

**Tests (`Terminal` fake, no real tty):**
- bytes flow both directions; nothing is dropped or reordered
- raw mode is restored on normal exit, on error, and on panic
- window size changes propagate to the PTY
- child exit code becomes the process exit code

---

## Step 8 — broker: model API — built, then removed

**Superseded on 2026-09-08.** The broker was built as planned — streaming relay, proactive refresh under `flock`, atomic write-back, the dummy key stripped host-side, every test below green — and then taken out. Two things sank it. Claude Code opens a long-lived `CONNECT` tunnel the moment it starts and makes its calls beside it, which deadlocked the one-connection-at-a-time in-box forwarder into a silent spinner; and everything the broker could not inject into (bootstrap flags, `/usage`, the MCP registry) answered `401` to the dummy key, so the agent ran half-degraded by design. The wanted thing was an autonomous agent. A box is on the host's network now and speaks to the API for itself; what it logs in with is `[access] credentials = "none" | "copy" | "share"` (or `--credentials` on the start): a clean home and `/login` inside the box, the host's credential files copied in once, or the host's files bound read-write. The test list stays below as the record of what was proven.

<details><summary>The original step, for the record</summary>

Host-side reverse proxy, unix socket into the box, ~100-line in-box forwarder on `127.0.0.1`. Shape already proven by CONCEPT.md spike #14.

Broker errors surface to the agent as HTTP responses in its own TUI — nothing here waits on the control panel.

**Tests, all against a fake upstream:**
- the dummy `x-api-key` is stripped and never forwarded
- the real credential is injected; **no log line, error, or panic message ever contains it**
- the response is **streamed** — assert the client receives the first chunk before the fake upstream has finished writing. This is the test that stops a buffering implementation from shipping
- refresh is **proactive**: when `expiresAt` is within a margin, the broker refreshes *before* forwarding, rather than waiting for a 401
- reactive refresh-on-401 exists as a fallback and is attempted **only before the first response byte is flushed**. Once headers are on the wire a retry is impossible, so a mid-stream auth failure surfaces to the agent — asserted as the documented behaviour, not left to chance
- a refresh failure produces a specific, actionable error rather than a blanket 401 — the agent retries ~7× on 401 and would otherwise burn all of them silently
- the credential file is re-read per request, so a rotation outside the process is picked up
- from inside the box: the broker socket is reachable and nothing else is

**Rotation is the hard part, and it is proven, not assumed** (CONCEPT.md spike #15). Tests for it:

- refresh takes an exclusive `flock` on the credential file, re-reads **under the lock**, and skips the refresh if another writer already renewed it
- the write-back is atomic: temp file, `chmod 600`, `rename`. A killed process never leaves a torn credential
- two brokers started concurrently produce **one** refresh, not two — the second observes the first's result. This is the test that stops a silent re-login bug that would otherwise appear only on a second workspace, hours in
- a rotated refresh token is persisted before the response is served, so a crash after refresh does not lose the new token

</details>

---

## Step 9 — agent layer

`wormhole agent install claude[@version]` — throwaway box, build-time allowlist, npm install, freeze read-only layer keyed by version.

**Tests:**
- the layer is read-only and shared across two different workspaces
- the same version resolves to the same cached layer without network
- the **build** box has exactly the build allowlist; the **working** box has none of it
- nothing installs at box start — there is no runtime hook, by design

---

## Step 10 — run the agent

`wormhole` boots a box and runs Claude Code in it, on the host's network, with the login the `credentials` mode hands it.

### ← FIRST RUNNABLE. Dogfood to answer assumptions, not to switch.

At this point: your project directory and nothing else of your machine, no credential in the box unless a `credentials` mode put one there, the host's network, boundary printed on every launch. **No persona yet** — which means this is worse than the role container you use today, and you will not actually switch here. That is Step 11. Saying otherwise would be optimism dressed as a milestone.

Run it anyway, because these questions decide the rest of the design and only a real run answers them:
- ~~does the absence of DNS break anything you did not predict (assumption #3)~~ — moot: the box has the host's network and resolver, and Claude Code's update and feature-flag checks simply work
- does `debian:13-slim` + npm-installed Claude actually run (assumption #4)
- how badly do you miss `apt-get install` inside a running box — the root is thrown away on exit, so packages belong in the image either way

---

## Step 11 — roles, from a local directory

`--role ~/.config/wormhole/roles/chief`. Manifest parse, `files` materialisation, `setup` layer.

**Tests:**
- strict parse: an unknown field is an **error**, not ignored
- `version` is required; a newer version than this binary understands is refused
- `mode = "overwrite"` re-materialises on every boot, **verified after mounts are in place, both under `$HOME` and under the workspace** — this is the assertion that would have caught spike #12 on day one
- `mode = "seed"` does not clobber an existing file
- a `dst` escaping the box is rejected
- `src` outside `files/` still ships (the `shared/persona.md` case)
- a role layer rebuild is byte-identical
- ~~a role `egress` request beyond the configured ceiling is refused~~ — no egress lists since 2026-09-08; the ceiling idea survives for `grants` only

## Step 11b — roles, from a git commit

`wormhole role add <url>@<sha>`. A role that lives in a repository, pinned to
a commit and never to a branch, fetched by `git` — which verifies every object
against its own hash, so the commit id is the proof and no digest of ours is
kept. The pointer (url, sha) lands in config; the checkout lands in data under
the commit, beside the bases and images it is exactly like.

Fetching and asking happen here, at install. A launch does neither: both need a
person, and blocking a scripted start on a prompt nobody sees is worse than
refusing with the command that fixes it.

**Tests:**
- a ref with no `@<sha>` is refused where it is typed, before any fetch
- the pin splits off the **last** `@`, so `git@host:owner/repo.git@<sha>` parses
- a commit is forty lowercase hex, so one commit is one checkout and one approval
- a name that could climb out of the config directory is refused, not repaired
- a hand-edited pointer naming a branch is refused on the way in
- a repository with no `wormhole.toml` at its root is refused, and leaves no checkout
- a launch whose checkout is missing refuses and names `wormhole role add`
- a launch whose approval is missing refuses **before anything is built**
- a bare remote `--role` with no terminal on stdin refuses rather than fetching
- `role add` over a role a person wrote by hand is refused
- two names for one commit share its checkout and its approval
- the preview renders `[image] base`, `packages` and every `build` line — the confirm's whole point is the part that executes

### ← Roles done. MVP-0 completes at Step 14.

Your persona, your settings, your skills, boxed. Step 12 finishes it: the control panel. (Steps 13 and 14, the `CONNECT` proxy and the read-only git broker, were dropped with the broker — the box is on the host's network.) From Step 11 on it is already better than the role container you use today — same persona, no host path but the project, and a login only where you chose one.

---

## Step 12 — control panel

Deferred to here deliberately. Nothing earlier needed it: launch refusals print to stderr before the PTY exists (Step 6), and API errors reach the agent in its own TUI. (It was once meant to arrive one step before the blocked-host flow of Step 13; that flow went with the broker.)

VT screen model, `Ctrl-\` toggle, grants list.

**No live mount injection.** Mount grants are launch-time only; granting a path means restarting the box. Boots are instant (layers cached), mount revocation was never retroactive anyway (CONCEPT.md §5 — open fds survive `umount`), so a restart is what honest revocation already required. This deletes the only `setns` code in the MVP. Live injection is post-MVP if dogfooding demands it.

**Tests:**
- recorded byte streams → screen model → `contents_formatted()` restores an identical screen (fixture-driven, no tty)
- toggling in and back leaves the agent's screen byte-identical
- the panel renders from core state only; it holds no decision of its own
- the panel lists mount grants and offers no revoke; it says a mount change means restart, rather than implying a live revoke exists
- a role's `grants` request renders as a preview and is refused until approved — **including local roles**, so the approval path is exercised daily rather than first meeting reality when git-sourced roles land

---

## Step 13 — `CONNECT` proxy — dropped

**Dropped on 2026-09-08 with the broker.** The allowlist rules (exact names, one-level wildcards only beside their base, the enumerated GitHub set, IP literals refused) were built and tested, and `wormhole allow`/`deny` changed a running box's list live. None of it exists now: a box is on the host's network, and what bounds it is the workspace, the grants and the login mode. If a host allowlist is ever wanted again it is a netfilter question on the host, not a proxy in front of a routeless box.

---

## Step 14 — git and GitHub API broker, read only — dropped

**Dropped on 2026-09-08 with the broker.** Git and `gh` in the box reach GitHub the way they do on the host, over the host's network, with whatever `GITHUB_TOKEN` the manifest asks for (`[env.GITHUB_TOKEN] ask = true`, kept host-side and handed to the box). Scope that token instead of scoping a proxy: read-only unless you mean it, and only to the repositories the role works on.

---

## After MVP-0 — ordered by what the dogfooding tells you

| Candidate | Unblocks |
|---|---|
| Live mount injection via `setns` | granting a path without restarting the box — only if restart friction proves real |
| A ceiling on what a role may ask for | the last unbuilt half of "requests are never grants" (§7) — a fetched role's confirm is the whole defence today |
| A reaper for `checkouts/`, `bases/` and `images/` | `gc` reclaims kept homes only; all three caches grow forever. One rule for all three, not one for each |
| N sessions with per-session PID/mount/net namespaces | the multi-agent goal (§4) |
| Reflink checkpoint and restore | undo for `rm -rf`, without giving up transparency |
| A guest kernel | the host-kernel row of the §1 table — not planned; would need `/dev/kvm` and libkrun #329 closed |
| Crate extraction (`boundary`, `rootfs`, `term` out of the binary crate) | compile time or reuse, if either ever hurts — mechanical, no design change |

---

## Spikes to run in parallel, independent of the steps

Each is cheap and each removes a decision from the critical path.

**Done, then moot:** broker-side OAuth refresh — proven live, CONCEPT.md spike #15. It rotated the refresh token, which is what put `flock` and atomic write-back into Step 8. With the broker gone the agent refreshes for itself; the rotation result is still why `credentials = "share"` warns that the host agent and a box can race each other for one file.

Remaining:

1. **Assumption #1** — the uid-map overlap test. One `sudo` command. Decides whether §4's namespace separation has an alternative.
2. **Assumption #7** — `vt100` upstream health. Check maintenance, compare `vt100-ctt` and `vt100-psmux`, decide vendor-or-depend before Step 12.
3. **Assumption #4** — `debian:13-slim` + npm Claude Code. A `docker run` and an `npm i -g` proves or kills it in ten minutes.
4. ~~**Assumption #9** — is `anthropic-beta: oauth-2025-04-20` required?~~ Moot without the broker.
5. ~~**Assumption #10** — does `gh` accept a base-URL override cleanly?~~ Moot without the GitHub API broker.

---

## One decision still open

**Agent separation mechanism.** §4 recommends namespace-per-session; assumption #1 may reopen uid-per-agent. Not needed until N sessions land, and Step 3 already verifies sibling-namespace creation, so the shape is proven early and cheaply either way.

