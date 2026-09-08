# wormhole

**Isolated box for AI coding agents running with all permissions granted.**

Want run agent with `--dangerously-skip-permissions` and not think. `wormhole` make safe: agent see one directory and nothing else of your machine, and only the login you chose to hand it.

One command in the directory you work in. No daemon to install. No container runtime.

---

## 1. The thesis

### Threat model, stated first because everything follow from it

**The agent is assumed hostile.** Not "careless" — hostile. Prompt injection from a file it reads, a poisoned dependency, an untrusted issue body: any of these turn the agent into an attacker with your shell. So wormhole assume it will try to read your tokens, exfiltrate, and escape.

Three consequences, and they are not the ones people expect:

1. **Most of the protection is the filesystem, not the sandbox boundary.** Your other files are protected by not existing in the box. Your logins are protected by not being handed over unless you say so — and once handed over, they are the box's. Both are *userspace* design and work identically behind any boundary. The network is not a protection: the box is on the host's network and reaches what the host reaches (§2).
2. **The boundary's unique contribution is the host kernel.** That is one row in the table below, not the whole table. Naming it honestly is what lets the boundary be staged.
3. **The live workspace mount is a deliberate hole and the biggest one.** §6, risk 1.

| Protection | Provided by | Needs a separate kernel? |
|---|---|---|
| Theft of credentials you did not hand over | absent from the box — unmounted paths do not exist | no |
| Theft of the login you did hand over | **not protected** — `credentials = "none"` hands over nothing | — |
| Exfil / C2 | **not protected** — the box has the host's network (§2) | — |
| Remote repo destruction | only what the login you handed over can reach; nothing else of yours is mounted | no |
| Host filesystem outside the workspace | mount namespace — unmounted paths do not exist | no |
| **Host kernel compromise** | **separate guest kernel** | **yes — only this** |

### Why the boundary is staged, and why that is not a compromise

wormhole define a `Boundary` port. Two implementations:

- **`Namespaces`** — our own `clone` + `unshare` + `pivot_root` + `mount` + seccomp + Landlock. Linux. Ships first.
- **`MicroVM`** — separate guest kernel. Ships second.

Everything above the port — credentials, roles, layers, mount planning, control plane, assertions — is written once and boundary-independent. Ordering is not "container because cheaper." It is: 80% of the design is boundary-independent, so build it against a boundary that runs on this machine today, then add the kernel row.

**Non-negotiable condition.** Every launch prints its live boundary:

```
boundary: namespaces (host kernel SHARED) · egress: host network (host resolver) · workspace: rw · grants: 0
```

A staged boundary that does not say which stage it is in is a lie. The banner is what makes staging honest instead of dishonest.

### The boundary ladder, and where each rung actually sits

| Rung | Escape requires |
|---|---|
| Rootful Docker | one kernel LPE, or a runc CVE (three shipped Nov 2025: CVE-2025-31133, -52565, -52881) |
| **`Namespaces` — userns + own mount/pid ns + seccomp + Landlock** | **kernel LPE reachable from an unprivileged user namespace** |
| gVisor (`runsc`) | Sentry escape, *then* a kernel bug — 5–15× syscall latency, fatal for builds |
| **`MicroVM` — separate guest kernel** | **hypervisor escape** |

Rung 2 is where wormhole starts and it is honestly weaker than rung 4. It is also at least as strong as what agent-sandbox tools ship today at the filesystem, which is where the box's protection lives.

### Docker-in-Docker is not a security mechanism

Nest containers for extra isolation is backwards: inner container, outer container, host **share one kernel**. Two namespaces deep = same wall drawn twice. Classic DinD need `--privileged`, which removes exactly the restrictions that made the namespace a boundary. Mounting `/var/run/docker.sock` is host root in one command.

wormhole never uses a container runtime, so there is no socket to mount and no privileged flag to grant. Docker *inside* the box, when it exists, is a capability the box provides, not a security claim.

---

## 2. Network and credentials

**The box is on the host's network.** User, mount, UTS and PID namespaces; no network namespace. Every route the host has, the box has, and it speaks to the model API — and to everything else — for itself. `/etc/resolv.conf` is the manifest's `dns` address when one is named, otherwise the host's own file bound read-only.

This is a deliberate reversal. An earlier design gave the box no route and put a host-side broker in front of the API and a `CONNECT` proxy in front of everything else (spikes #14 and #15, kept in §13). It was proven and it shipped, and it broke real client behaviour every day: the agent opens several `CONNECT` tunnels at once and holds one for its whole life; it reaches endpoints the broker could not inject a credential into — bootstrap flags, usage, the MCP registry — and each of those degraded or failed in ways that looked like the network. The user of this tool wants autonomous agents, and a boundary that makes the agent unreliable is paid for every hour and defends against a threat the same user has chosen to accept. So the route is open and the question is only which login the box speaks with.

### Three answers, all yours

`[access] credentials` in the manifest, or `--credentials` on a start, which wins:

| Mode | What the box gets | What happens on refresh |
|---|---|---|
| `none` (default) | a clean box home; `/login` inside the box makes whatever login the agent makes | the box's own; the host is never read |
| `copy` | the host's credential files copied into the box home once, where absent | the box refreshes its own copy; the host copy is never written |
| `share` | the host's credential files bound read-write at the same place in the box home | lands on the host; one login, refreshed by whoever runs next |

The files are the agent's own: `~/.claude/.credentials.json` for `claude`, `~/.codex/auth.json` for `codex`. For `claude`, `copy` and `share` also carry the account fields from the host's `.claude.json` into the box's, only where the box has none — a token without its account is half a login.

`share` names one race honestly: the refresh token rotates on every use, so the host agent and a box refreshing the same file can invalidate each other. Cost is one `/login`. Not defended, named.

A copied or shared login is a grant like any other and the banner counts it: it is the one host secret a box can be handed, and the preview screen says which mode it got.

### What is not protected, said plainly

With the route open and a login in the box, nothing in wormhole stops the agent from sending anything it can read to anywhere it can reach, or from acting as the account you handed it with everything that account can do. With `none` the exposure is whatever the agent logs in as, on its own. The protection that remains is the filesystem: the workspace, granted paths and the box's own home exist in the box; nothing else of your machine does (§5). No capability, a disposable root, and a login you chose.

### Builds still run in a throwaway box

Preparing an image needs `registry.npmjs.org` or whatever the recipe installs. Preparation happens in a **throwaway box** on the host's network; its output is frozen read-only. Everything a recipe *can* pin is an `[[image.artifact]]`, fetched and proved on the host and bound in read-only, so a build opens as few connections of its own as the recipe allows.

**Preparation never runs on the host.** `npm i -g` executes arbitrary postinstall scripts. Running those on the machine wormhole exists to protect is self-defeating.

---

## 3. Nouns

Three, and their relationships are enforced, not conventional.

| Noun | Is |
|---|---|
| **Workspace** | The directory you ran `wormhole` in. Identified by its absolute path. Nothing to register. |
| **Box** | The running sandbox for a workspace. Disposable. Holds one or more sessions. |
| **Role** | A directory: one manifest, a `files/` tree, a `setup` list. |

### A workspace is the current directory

No `workspace add`, no registry, no config entry, no `.wormhole.toml`. `cd ~/proj && wormhole` and that directory is the box's world.

**Policy never comes from the repository.** Role selection, extra mounts, and grants live in `~/.config/wormhole`, keyed by workspace path — never in a file inside the project. A repository you clone cannot propose, influence, or widen its own sandbox policy. That is the entire point in a tool whose job is containing code you do not trust.

Cost, real and accepted: a team cannot commit a shared box definition. Repo-local config would fix that and would turn every `git clone` into supply-chain surface defended only by an approval prompt. If it ever ships, the approval UX is designed first, not bolted on.

### N boxes per workspace, N sessions per box

A session is `(agent, PTY, namespace set)`. The session layer is a list. **MVP ships one session.** Multiple agents cooperating in one tree is the goal (§4), and the shape does not foreclose it.

A box is a kept thing with an id, not a process: its own `$HOME`, history, logins and installed tools, resumable whenever. A workspace holds as many as you make.

This reverses an earlier position. Two boxes on one directory *was* forbidden, on the reasoning that a live read-write workspace means interleaved writes, duelling `cargo build` on one `target/`, and competing `git checkout`. Two of those three did not survive contact: cargo and gradle take file locks, so builds queue rather than corrupt. The third did — uncommitted edits are lost silently, and one agent's `git commit -a` sweeps up another's half-written files.

So the cost is real and it is now the operator's to accept, said plainly rather than prevented. Wanting two agents on one tree is a legitimate thing to want, and the alternative — forcing a `git worktree` on everyone — is a policy the tool has no standing to impose. Anyone who wants the separation still has worktrees.

What the lockfile now enforces is narrower and absolute: **one process per box.** Two starts into one `$HOME` would be two agents writing one history, one config and one instructions file at once, which is corruption with no operator upside. Keyed by box id rather than workspace path; the kernel ends the claim with the process, so nothing is ever reaped.

---

## 4. Multiple agents in one box

The goal: several agents, one tree, cooperating through files.

### Separating co-resident agents

If the agent is hostile, co-residency needs a boundary *between* agents, or one injected session owns every other one — it can read their memory, kill them, rewrite their persona mid-run, and spend their credential grants.

**The mechanism is namespaces, not uids.** Each session gets its own PID and mount namespace; all run as the same uid, so every file any of them writes on the host is yours.

- PID namespace — session A cannot see, `ptrace`, or kill B.
- Mount namespace — A cannot *name* B's home, and so not B's login either. Isolation by non-existence, same principle as §5.

Uid-per-agent was considered and is probably impossible: `mappings_overlap()` in `kernel/user_namespace.c` rejects overlapping ranges on both the inside and the outside side of a uid map, so N inside uids give N *different* host uids, of which at most one can be yours — transparency then holds for one agent only. Recorded as open assumption #1 with the exact test, because it was not executed.

**Sibling namespaces, not nested.** The supervisor creates each session's namespace set directly. This is only possible because wormhole owns the sandbox: nesting namespaces inside a runtime-managed container requires `CAP_SYS_ADMIN` or a custom seccomp profile — you would weaken your own boundary in order to isolate agents from each other. Proven, §9 spike #11.

---

## 5. What the box contains

**Default deny.** A path exists in the box only if it is in this table or explicitly granted.

| Path | Mode | Source |
|---|---|---|
| the invocation directory, at its **host-identical path** | rw | bind from host |
| `$HOME` | rw | per-workspace persistent directory under `~/.local/share/wormhole` |
| `/usr`, `/bin`, `/lib*`, `/sbin` | ro | base rootfs layer + role overlay |
| `/tmp`, `/dev/shm`, `/run` | rw | tmpfs, ephemeral |
| `/etc/passwd`, `/etc/group` | ro | synthetic — your user only |
| `/etc/hosts` | ro | synthetic — `127.0.0.1 localhost` and the box's own hostname, nothing else |
| `/etc/resolv.conf` | ro | the manifest's `dns`, or the host's own file |
| `/dev/{null,zero,random,urandom,tty,pts}` | rw | minimal devtmpfs |
| everything else on the host | **absent** | — |

Not mounted, ever, without an explicit grant: your home directory, other repositories, `~/.ssh`, `~/.gitconfig`, `~/.aws`, `~/.claude`, `/etc/shadow`, `/mnt`, `/media`. The one exception is the login `credentials` names (§2), which lands in the box's *own* home and is counted as a grant.

### Host-identical paths

The workspace is mounted at the same absolute path it has on the host. Absolute paths in agent output are copy-pasteable in your own shell; a stack trace names a file you can open. A synthetic `/work` throws that away for nothing. Parent directories are synthetic and empty — `/home/you/` contains only the project, and the project directory contains only the project.

### Extra mounts are grants

`--mount <path>` adds one host path, read-only unless `:rw`. Grants persist in `~/.config/wormhole` keyed by workspace path, and the full active set prints on every launch, so a grant you forgot cannot be silently in effect.

**Mount revocation is provably incomplete.** `umount` does not invalidate already-open file descriptors and Linux offers no way to revoke one. A hostile agent that opened an fd keeps that access after you revoke the mount. Mount grants are therefore one-way within a session; revoking means restarting the box. A shared login is a mount grant and has the same limit; a copied one is the box's own until its home is reset.

---

## 6. Layers, without an image builder

```
base.img    debian:13-slim, pulled by digest, read-only   ──┐
agent layer claude@<version>, shared by every workspace   ──┼─ overlayfs
role layer  role `setup` output, read-only                ──┤
tmpfs upper ephemeral                                     ──┘
$HOME       per-workspace persistent directory  ──── rw
workspace   bind from host                      ──── rw, host-identical path
```

### The base is stock and pinned, and you publish nothing

`debian:13-slim` by digest. Public, no auth, ~30 MB once, byte-identical every boot, and **nothing for you to build, publish, or maintain**. The previous design fetched a wormhole-published `base.img` and carried an accepted risk for it ("a build you did not perform"). **That risk is deleted** — Debian's base is not a build you performed either, but it is a build thousands of people audit, and you are not its supply chain.

A digest mismatch refuses to boot. Cached layers are reused offline; the network is touched only when a digest is absent locally.

### The agent layer is shared and version-keyed

`claude` is ~262 MB installed and is in no base image. It is installed **once ever**, in a throwaway box on the host's network (§2), into a read-only layer keyed by version, shared by every workspace and every role. Boots stay instant. The version is explicit and pinnable. A running box has no route to npm.

### Role layers, no Containerfile parser

Build a role layer: boot the base read-only with a fresh empty upper, run the role's `setup` steps in a throwaway box on the host's network, freeze the upper. That upper *is* the role layer.

`setup` covers everything `RUN` does; `files/` covers `COPY`; `env` and `workdir` are already fields. A Dockerfile *subset* would be worse than either — familiar syntax that silently rejects `COPY --from`, multi-stage, heredocs, `ARG`. Better an obviously smaller thing than a familiar thing that lies.

### The root filesystem is disposable; `$HOME` is not

Root is reassembled from layers every boot, so the role definition always describes reality. `$HOME` persists per box, so conversation history, `~/.cargo`, `~/.npm`, and `claude --continue` survive restarts — and, because it is per box and not per workspace, several boxes in one tree never write over each other's. `wormhole reset` discards it.

---

## 7. Roles

Role = how a box becomes `chief-staff-engineer` instead of a bare shell.

### wormhole does not abstract agent config. It transports it.

MCP servers, skills, plugins, hooks are Claude Code nouns. Codex has `AGENTS.md` and a different shape. Any schema meaning the same thing across all of them contains only the intersection. So the contract is:

```
files→paths · setup · argv · env · workdir
```

A role ships each agent's **native files verbatim to real container paths** and declares how to launch it. wormhole knows nothing about MCP. Agent-agnostic at the launcher; agent-specific in the role.

**Cost, named honestly:** this forfeits validation that a launcher-native schema can do — checking that `plugin@marketplace` actually exists in that marketplace before you ship the role. wormhole cannot do that, ever, because the payload is opaque bytes. Payload errors therefore surface as agent misbehavior at run time, not as validation failure. §11 compensates with a role-payload smoke assertion.

### Layout: one manifest, per-agent sections

```
chief-staff-engineer/
  wormhole.role.toml
  shared/persona.md
  files/
    claude/CLAUDE.md
    claude/settings.json
    claude/skills/tdd/SKILL.md
    codex/AGENTS.md
```

The layered role format below is the target, not what ships. Today's
`wormhole.toml` has no `files/` tree and one agent per box; the handbook's
manifest reference lists its keys.

```toml
version = 1

[role]
name = "Chief Staff Engineer"

setup = [
  "apt-get update && apt-get install -y --no-install-recommends build-essential",
  "rustup toolchain install 1.96 --profile minimal -c clippy,rustfmt",
]

[agents.claude]
argv    = ["claude", "--dangerously-skip-permissions"]
workdir = "{{workspace}}"

  [[agents.claude.files]]
  src  = "shared/persona.md"
  dst  = "$HOME/.wormhole/persona.md"
  mode = "overwrite"

  [[agents.claude.files]]
  src  = "files/claude/CLAUDE.md"        # `@~/.wormhole/persona.md`
  dst  = "$HOME/.claude/CLAUDE.md"
  mode = "overwrite"

  [[agents.claude.files]]
  src  = "files/claude/settings.json"
  dst  = "$HOME/.claude/settings.json"
  mode = "seed"
```

**One manifest, not one per agent.** One box provisions N agents (§4), so one box needs one provisioning spec. Per-agent subdirectories each with their own manifest cannot express "these two agents share this tree."

### `files` is a post-mount materialisation with an explicit destination and precedence

This is the correction of a load-bearing bug in the previous design, kept in §9 as spike #12.

- **Every entry names its `dst` explicitly.** No implicit base path. The old design wrote `files/.claude/CLAUDE.md` and `files/.mcp.json` in one list with two different unstated roots — a field called `files→paths` that omitted the paths.
- **Materialisation happens after mounts, not at layer build.** `$HOME` and the workspace are *mount points*. A file baked into a read-only lower at a path a mount later covers is invisible. In the old design every file in the canonical example — persona, settings, skills, `.mcp.json` — was shadowed. The document's only role example did not work.
- **`mode` is per entry.** `overwrite` re-materialises on every boot: a persona edit upstream reaches you on the next launch, and an agent's edit to its own persona does not survive a restart. `seed` writes only if absent, for files you hand-edit.
- **Anything in the repo can be a source.** `shared/persona.md` sits outside `files/` and still ships, because `src` is a repo path and `dst` is a container path. The old design referenced `../../shared/persona.md` as an include — a repo-relative path used as a container-relative one, pointing at a file that was never transported.

`overwrite` is what "the role definition always describes reality" requires. Seeding role-owned files if-absent produces the failure where an upstream role update never arrives and injected content persists across reboots.

**`~/.claude.json` and session state are never role-owned.** The agent writes them; declaring them would fight it every boot.

### Requests are never grants

`grants`, `credentials`, `cpus`, `memory` are requests. `~/.config/wormhole` holds the effective ceiling.

**A role's request is confirmed by diff, from the first day, including your own local roles.** The panel shows what the role asks for, you approve once, and the decision lands in the workspace config; a changed role produces a new diff. No exemption for local roles — an unconfirmed path would be a second code path that your daily use never exercises, so it would be broken the day git-sourced roles arrive and start using it. Confirming your own role also means you read it the way a stranger would.

A role cannot hand itself your login or claim your RAM — the same hole §3 refuses for workspace definitions, one layer down.

**Not built. Said plainly because it is load-bearing:** nothing bounds what a role asks for. `grants` is checked for shape — absolute, no `..`, not a symlink, no workspace overlap — and then mounted as written. A role asking for `~` gets your whole home read-write if you approve it. The confirm below is the whole of the defence today; the ceiling this section describes is the fix and is not in the code.

A per-grant denylist would be decoration, not a fix: a role forbidden `~/.ssh` asks for `~` and gets it. That is §2's own argument for `GET`-only on the GitHub API, and it cuts the same way here.

### Roles are local directories, or a git commit

`--role ~/.config/wormhole/roles/chief` reads one locally: no fetch, no pin, nothing to approve. `wormhole role add <url>@<sha>` fetches one instead — same directory shape, pinned to a commit and never to a branch.

**The commit is the verification, not a digest wormhole keeps.** `git` checks every object it receives against its own hash, so the pin is proved locally and the transport is not trusted for anything. The alternative considered and rejected was a sha256 of a codeload tarball: byte-exact, but GitHub's archive bytes are not stable, so it is a pin the server can invalidate. A commit id cannot change.

**A fetched role is confirmed before it runs, once per commit, host-wide.** The screen shows what a stranger's role would do — grants, environment, hooks, *and* the base tarball, the package list and every `[image] build` line, which is the part that executes. A pin is the bytes, so approving it in one workspace approves it in all of them; a changed pin is a different commit with no approval, and `role add` over a name shows old sha → new sha and asks again.

**Fetching and asking happen at install, never at launch.** Both need a person. A launch that found either missing would have to block a scripted start on a prompt nobody sees, so it refuses and names the `role add` instead. The single exception is a ref typed straight at `--role` with a terminal on stdin, which *is* that person.

The pointer (url, sha) lives in config, where a person writes things; the checkout lives in data under the commit, beside the bases and images it is exactly like.

---

## 8. Every launch proves the boundary

Before the PTY is handed over, and refusing to start if any assertion fails:

- host prerequisites present — unprivileged `user.max_user_namespaces` non-zero, `unshare(CLONE_NEWUSER)` succeeds, Landlock ABI available
- base rootfs digest matches its pin
- no credential file is readable inside the box beyond the one `credentials` handed over
- the workspace is the only host path mounted rw, plus explicitly granted ones
- capability bounding set dropped and not reacquirable
- the workspace lockfile was acquired

Then the banner (§1), the grant set, and the role name.

A security tool that only checks when asked is checked when it does not matter. A kernel upgrade or a distro userns-policy change must not land silently. Failure is refusal, never degradation — that choice follows directly from assuming the agent is hostile.

---

## 9. Surface

```
wormhole                                  # TUI: pick role, then run
wormhole --role <name>                    # skip the picker; the path tests drive
wormhole --mount <path>[:rw]              # grant an extra host path
wormhole reset                            # discard this workspace's persistent $HOME
wormhole agent install claude[@version]   # build the shared agent layer
wormhole role build <path>                # build a role layer
wormhole doctor                           # host prerequisites, verbosely
```

No subcommand takes a workspace argument. The workspace is where you are.

### The control plane is the TUI, and it stays alive

The TUI is not a launcher that exits. It is the live control plane: **`Ctrl-\` toggles** between the agent and a panel that shows the active grants *while the agent runs*. Grants — the login included — are launch-time only: changing them means restarting the box — which honest revocation already required, since open fds survive `umount` (§5). Live mount injection is post-MVP.

`Ctrl-\` and not `Ctrl-A`: that is tmux's and screen's prefix, and agent users live in tmux. `Ctrl-\` is unclaimed by tmux, screen, vim, and emacs; it normally sends `SIGQUIT`, which we already intercept in raw mode. Nests without configuration.

### Screen restoration is deterministic

wormhole is the PTY master, so it sees every byte the agent writes. Feed that stream to a VT screen model (`vt100` crate); on returning from the panel, write `contents_formatted()` — "escape codes sufficient to reproduce the entire contents of the current terminal state." The crate documents `state_formatted()` for exactly this case: "drawing additional things on top of terminal output, since you need to restore terminal state without the terminal contents necessarily being the same."

The previous design planned to nudge the PTY size by one column so `SIGWINCH` would make the agent redraw itself, and recorded "SIGWINCH reliably forces a full redraw in the target agent TUIs" as an open assumption. **That assumption is deleted.** Restoration no longer depends on how an application reacts to a resize. A `SIGWINCH` is still sent afterwards as belt-and-braces, so an app can correct any imperfection in our model.

This component is also what later gives detach/reattach and multi-agent tabs, so the control plane pulls forward work that §4 needs anyway.

### The TUI holds no logic

Every decision lives in a headless core the TUI renders. `--role X` bypasses the TUI entirely and is the path the test suite drives. A decision that exists only inside a terminal renderer cannot be unit-tested, and this document's testing strategy (§11) depends on it not existing there.

### Layout

```
~/.config/wormhole/config.toml                  # ceilings, defaults
~/.config/wormhole/workspaces/<hash>.toml       # role, grants, keyed by absolute path
~/.local/share/wormhole/
  layers/base-<digest>/                         # read-only
  layers/agent-claude-<version>/                # read-only, shared
  layers/role-<name>-<hash>/                    # read-only
  workspaces/<hash>/{home/, lock}
```

---

## 10. Security model — stated honestly

### What wormhole protects

| Threat | Mechanism | Strength |
|---|---|---|
| Theft of credentials not handed over | they do not exist in the box's mount namespace | strong |
| Theft of the login handed over | **none** — `credentials = "none"` is the only defence, and it hands over nothing | by choice |
| Arbitrary C2 / exfil | **none** — the box has the host's network | by choice |
| Remote repo destruction | bounded by the login handed over; nothing else of yours is reachable | as strong as the account's scope |
| Host filesystem outside the workspace | unmounted paths do not exist in the box's mount namespace | strong; Landlock as a second layer |
| Host root compromise | `Namespaces`: unprivileged userns — **weak**. `MicroVM`: VMX/EPT — hypervisor bug required | **stage-dependent; the banner says which** |
| Runaway resource use | cgroup limits, host-side ceiling | enforced |

### What wormhole does not protect — accepted risks

**1. Your working tree.** The workspace is a live read-write bind mount. No snapshot, no shadow ref, no clone. Chosen deliberately: live co-editing in your own IDE, warm build caches, no sync step. Price: `rm -rf`, `git reset --hard`, and `git clean -xfd` reach your actual repository. **This is the entire remaining local blast radius and it is wide open by design.**

One thing narrows it: with `credentials = "none"` the box holds no login of yours that reaches your remotes, so damage is local. **Recoverable from `origin` only for work that is committed *and pushed*.** Uncommitted edits and untracked files have no copy anywhere and are simply gone. The previous version of this document said "recoverable from `origin`" without that qualifier, which was false.

A pre-session reflink snapshot (`cp -a --reflink=always`, instant on btrfs and xfs, a full copy on ext4) would give a real undo point without giving up transparency. Deliberately not in MVP; the accepted position is loss.

**2. Exfiltration, anywhere.** §2. The route is open by design.

**3. The login you handed over.** `copy` and `share` put your credential where the agent reads it, with every capability the account has — hosted connectors included, which execute server-side and never cross the box at all. If a box must not reach something, the account you hand it must not have it.

**4. `Namespaces` shares the host kernel.** A kernel LPE reachable from an unprivileged user namespace defeats it. This is the stage-1 boundary being honestly weaker than stage 2, and it is why the banner exists.

**5. Guest disk and inode exhaustion through the workspace mount.** Quotas required, portable enforcement unproven — assumption #5.

**6. Open file descriptors survive mount revocation.** §5. Not fixable at the mount layer.

**7. A handed-over login is not revocable from wormhole.** A copied file lives on in the box home until `reset`; a shared one is the host's own. Revoking means logging out at the provider.

**8. `share` races with a host-side agent.** §2. Cost is one `/login`. Accepted, not defended.

### Landlock is a second layer, not a load-bearing one

With our own mount namespace, a path that is not mounted does not exist, so there is nothing for Landlock to deny. It defends against a mount-namespace escape only. Naming it as one of "three layers" when it covers one failure mode is how a risk table starts lying — the previous version of this document made exactly that mistake and corrected it in §6 with a "one layer, not three" note. Same discipline here, stated up front.

---

## 11. Testing

TDD needs seams. Three ports drive the entire core with in-memory fakes:

- **`Boundary`** — create a box, run a process in it, get an fd pair. `Namespaces` and `MicroVM` implement it; the fake implements it for every core test.
- **`RootFs`** — resolve a layer by digest or version. Fake serves fixtures; real pulls and verifies.
- **`Terminal`** — bytes in, bytes out. Fake is a byte vector.

Unit-testable without any kernel: mount-plan computation from cwd plus grants, role manifest parsing and `files` precedence, grant persistence, layer resolution, credential mode decisions, VT screen model and restoration, lockfile semantics.

**The seams exist for testability, not for hypothetical backends.** `Boundary` is the one exception and it is not hypothetical — the microVM implementation is a stated deliverable.

A small set of integration tests boot a real box and assert what only a real kernel can prove:

- only the workspace, explicit grants and the box's home are visible; no host credential beyond the one `credentials` named is readable
- a file written in the box lands on the host owned by your uid, at the host-identical path
- the box reads the host's resolver, or the named one, and nothing else of `/etc`
- capability bounding set is dropped and cannot be reacquired
- `unshare(CLONE_NEWUSER)` from the supervisor produces sibling namespaces per session
- these hold for the **role-build box** as well as the working box, since that is where untrusted `setup` runs
- role `files` with `mode = "overwrite"` are correct **after** mounts, both under `$HOME` and under the workspace — the assertion that would have caught spike #12 on day one
- a role layer rebuild is byte-identical
- the launch assertions of §8 each fail closed when their precondition is violated

---

## 12. Scope

### MVP

Linux. `Namespaces` boundary. Workspace is the cwd. `debian:13-slim` by digest, own registry client, shared agent layer, roles from a local directory or a git commit. Host network, `credentials` in three modes. One session. TUI control plane with `Ctrl-\`, arriving after the first runnable — launch refusals print to stderr before any PTY exists. Launch assertions and banner.

### Explicitly not in MVP

- **`MicroVM` boundary.** Stage 2. Requires `/dev/kvm` (assumption #6) and closing libkrun issue #329 (§13).
- **Multiple sessions.** The shape is preserved (§4); the count is one.
- **Live mount injection.** Mount grants are launch-time; changing them restarts the box. No `setns` in MVP. Revisit only if dogfooding shows real restart friction.
- **A ceiling on what a role may ask for.** §7 describes it; nothing enforces it. A fetched role is confirmed by a person before it runs, and that confirm is the whole defence.
- **Workspace snapshots.** §10, risk 1.
- **Any network policy.** No allowlist, no proxy, no interception. §2 says why.
- **A role registry.** Roles are directories and git URLs; both have shipped. A registry is a distribution problem and stays out.

### Size

No daemon, no container runtime, nothing to install separately, no userspace TCP/IP stack and no broker. The old estimate was 12–18k LOC. Deleting `smoltcp`, NAT, DNS, the image builder, the base-image pipeline, the workspace registry and then the broker removes most of it. `MicroVM` adds it back only for the parts a hypervisor genuinely needs.

The claim that survives is not "one static binary" — it is **no daemon, no external runtime, nothing for you to install**. `vt100` and a registry client (or `skopeo`) are dependencies.

---

## 13. Assumptions

### Open — verify before committing

| # | Assumption | If false |
|---|---|---|
| 1 | A uid map may not point two inside uids at one outside uid | Uid-per-agent becomes available as an alternative to §4's namespace separation. Test: `sudo unshare -U sh -c 'printf "0 1000 1\n1 1000 1\n" > /proc/self/uid_map; echo rc=$?'` — needs root because the parent must hold `CAP_SETUID`, or a different EPERM teaches nothing. |
| 3 | ~~Claude Code functions with no `/etc/resolv.conf` and no route~~ — closed the hard way: it did not, reliably, and §2 gave the box the host's network | — |
| 4 | `debian:13-slim` plus an npm-installed `claude` runs — glibc and Node versions satisfied | The agent layer needs a Node it brings itself, or the base is not slim. |
| 5 | Per-workspace disk and inode quotas are enforceable portably | Exhaustion ships as a documented gap. |
| 6 | This host can run `MicroVM` at all | Stage 2 is undogfoodable here. §8's earlier draft recorded this machine reporting `vmx` and `nested=Y` with **no `/dev/kvm` and no `/dev/vhost-vsock`** — unverified since. |
| 7 | The `vt100` crate is maintained enough to depend on | Vendor it, or use a fork (`vt100-ctt`, `vt100-psmux`), or write the screen model. The API is proven; the upstream's health is not. |
| 8 | A `setup` list is reproducible enough that a cached layer stays valid | Layer cache invalidation becomes a correctness problem, not a performance one. See spike #13 — every unpinned input in a role makes this false. |
| 9 | `anthropic-beta: oauth-2025-04-20` is actually required when injecting an OAuth bearer | Harmless either way — spike #14 sent it unconditionally and got a 200. Worth isolating so the header set is minimal and understood rather than cargo-culted. |
| 10 | ~~`gh` honors a base-URL override cleanly enough to route the GitHub API through the broker~~ — moot: there is no broker, `gh` reaches GitHub directly with the token `[env]` asks for | — |

### Spike record

Each entry keeps only the evidence that produced a decision. The resulting design lives in the section named, not here.

Spikes #14 and #15 below proved the model-API broker and its token refresh; both are kept as record of a component that was later removed. It was removed because its proxy shape broke the agent in daily use — concurrent `CONNECT` tunnels it served one at a time, and endpoints it could not inject a credential into — and because the user chose an autonomous agent over the exfil protection the broker bought (§2).

**#1 — TSI filtering: falsified.** libkrun's whole TSI control surface is `krun_add_vsock(ctx_id, tsi_features)` with two flags. Filtering would require forking the VMM. Led to owning a network stack — which §2 then deleted entirely by removing the box's route. Relevant only to `MicroVM`.

**#2 — macOS/HVF parity: confirmed, then descoped.** Every libkrun device wormhole would need is gated by cargo feature, not `target_os`; only `krun_add_net_tap` is Linux-only and wormhole does not use it. Kept for stage 2. v1 is Linux, so parity is not a v1 constraint.

**#7 — macOS virtio-fs permissions: falsified.** APFS cannot represent Linux ownership, so libkrun's macOS virtio-fs synthesises it via xattrs; host-created files carry none. The failing axis was non-root-vs-root *inside* the guest, and the document's own uid-matching rule put the agent in the broken configuration — **the document proposed the bug.** Host-side ownership is determined by the identity of the virtio-fs server process, not by the guest process uid, so matching uids bought nothing. Relevant to stage 2 only; on Linux `Namespaces`, a userns map gives host-correct ownership directly.

**#8 — host confinement on macOS: falsified.** No `pivot_root` on Darwin; `chroot(2)` requires root; Seatbelt is deprecated and warns on every run; App Sandbox is compatible but needs an app bundle. Confinement cannot be primary defence on macOS. Moot for v1.

**#9 — raw virtio queue reachability: holds.** libkrun puts devices on the MMIO bus, not PCI, so the PCI-BAR-mmap literature is inapplicable. Shipped libkrunfw has `CONFIG_MODULES`, `CONFIG_VFIO`, `CONFIG_UIO`, `CONFIG_PCI` unset; `CONFIG_IO_STRICT_DEVMEM` is set on aarch64 and **not** on x86_64, leaving `mmap` of `/dev/mem` at a virtio-mmio address open there. Closed by dropping `CAP_SYS_RAWIO`, `CAP_SYS_BOOT`, `CAP_SYS_MODULE` from the bounding set. Generalises: two of the three mitigations previously proposed here did not exist — `kernel.modules_disabled=1` is meaningless when `CONFIG_MODULES` is off, and lockdown cannot be enabled without the LSM compiled in. Both were written from generic hardening habit rather than from *this* kernel's config. Stage 2.

**#10 — libkrun issue #329: open, and it is stage 2's blocker.** The virtio-fs server trusts the guest to send filenames: *"we basically trust the guest kernel that when it gives us a filename it is actually a filename and not a path."* A guest embedding `/` or `..` writes **outside the shared directory**, on the host, as the wormhole process user — a host filesystem write primitive that never touches the hypervisor boundary. The fix is to validate filenames, not to contain the consequences: ~20 lines in `passthrough.rs`, upstreamable, and wormhole would already build libkrun from source. **`MicroVM` does not ship before this is closed.** `Namespaces` is unaffected — a bind mount into a mount namespace has no filename-parsing server in the path.

**#11 — nested namespaces inside a runtime-managed container: falsified, and it decided §4.** Observed inside a Docker container with the default seccomp profile:

```
$ unshare -U -r sh -c 'id -u'
unshare: unshare failed: Operation not permitted
```

`unshare(CLONE_NEWUSER)` is denied. So per-session namespaces inside a podman- or Docker-managed box require `--cap-add=SYS_ADMIN` or a custom seccomp profile — **weakening the outer boundary in order to isolate agents from each other inside it.** Owning the sandbox makes sibling namespaces free. This is the strongest single argument for `Namespaces` over shelling out to a runtime, and it is evidence, not preference.

**#12 — `files→paths` was dead on arrival: falsified, and it was load-bearing.** The previous design specified `files→paths` as a build-time `COPY` analogue while the diagram mounted the persistent `$HOME` over `$HOME` and the workspace over the workdir. A mount masks the overlay beneath it, so **every file in the document's canonical role example** — persona, `settings.json`, a skill, `.mcp.json` — was invisible at run time. Two further errors in one line: `shared/persona.md` sat outside `files/` and so was never transported, and `../../shared/persona.md` was a repo-relative path used as a container-relative include.

Root cause: repo layout and container layout were conflated, and the contract field named `files→paths` never defined the base path. Class of bug the structure kept producing: *role config silently absent, no error, the agent runs with default behaviour* — the worst failure mode for a persona, because the agent still starts and simply is not the role. Fixed structurally in §7 (explicit `dst`, post-mount materialisation, per-entry `mode`) and asserted in §11.

**#13 — an authored role's inputs all floated: confirmed the risk in assumption #8.** A real role authored against a container-based agent sandbox was measured with five unpinned inputs: base image tag with no digest (and 31 minor versions stale), `mise install rust@stable`, `cargo install --git <repo>` with no `--rev`, `git clone --depth 1` of a third-party repo **at container start**, and `apt-get install` with no versions. Two rebuilds a week apart produce different boxes and nothing records which. Its validation CI had also failed on every run since the repository's first commit, because two different validators enforced one contract and only the one nobody ran locally checked the missing file.

Consequences taken here: `setup` output is content-addressed and rebuild-verified (§11); the base is digest-pinned (§6); *nothing* installs at box start, because there is no runtime hook — deterministic dependencies have exactly one home, the role layer.

**#14 — the model-API broker: proven end to end, and it resolved two assumptions while creating two requirements.** Run inside a container-based agent sandbox against real infrastructure.

Setup: a ~30-line host-side reverse proxy reading a subscription OAuth credential; Claude Code launched with `ANTHROPIC_BASE_URL=http://127.0.0.1:8897` and `ANTHROPIC_API_KEY=sk-ant-WORMHOLE_DUMMY`; broker strips `x-api-key`, injects `Authorization: Bearer <accessToken>` and `anthropic-beta: …,oauth-2025-04-20`, forwards to `api.anthropic.com`.

```
$ claude -p 'Reply with exactly: BROKER_PROVEN' --model sonnet
BROKER_PROVEN
UPSTREAM 200 for POST /v1/messages
```

What it established:

1. **Claude Code accepts a plaintext loopback base URL.** No TLS at the forwarder, no cert in the box's trust store. Former assumption #2, closed.
2. **It starts and issues requests with no real credential present.** A header probe confirmed the outbound `x-api-key` value *was* the dummy — the real bearer was never sent by the box. The no-secret-in-box premise is not theoretical.
3. **The endpoint surface is small:** `POST /v1/messages?beta=true`, `anthropic-version: 2023-06-01`, `anthropic-beta: claude-code-…`. It retries roughly seven times on a 401, so the broker's error responses must be meaningful rather than blanket-401.
4. **The agent refreshes its own OAuth token, in-box, and the refreshed value does not flow back to the host.** Measured directly: the host-injected snapshot had `expiresAt` 22 hours in the past while the container-local file, written that morning by the agent, was live. Access tokens last about eight hours. This produced assumption #9 and requirement two below.

Requirements it created, both now in §2 and in MVP scope:

- The broker must **stream**; the probe buffered the body, which for interactive SSE would freeze the agent's TUI.
- The broker must **own refresh**, because stripping the credential from the box removes the box's ability to do it.

Cost it exposed: setting `ANTHROPIC_API_KEY` at all disables claude.ai-hosted connectors. Named in §2, not hidden.

**#15 — live OAuth refresh: works, and the refresh token rotates.** Executed against the real token endpoint with a backup taken first.

```
POST https://platform.claude.com/v1/oauth/token
grant_type=refresh_token · client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e

HTTP 200
access_token  changed = True
refresh_token ROTATED = True          ← the finding
expires_in    28800 (exactly 8h)
scope         user:file_upload user:inference user:mcp_servers user:profile user:sessions:claude_code
also returned account, organization, token_uuid, refresh_token_expires_in
```

Endpoint, `client_id`, and the `refresh_token` grant were first located by static inspection of the agent bundle — zero risk, no call — and only then exercised live. Client authentication is not required (`token_endpoint_auth` indicates a public client).

**Rotation is the load-bearing result, and it changed the architecture.** A rotating refresh token admits exactly one writer. Two brokers, or a broker plus a host-side agent, means whoever refreshes second presents a dead token and the chain breaks. So the broker takes an exclusive `flock`, re-reads under it, and writes atomically. Class of bug this closes: *any* concurrent refresh silently costs a re-login, and it would have appeared only under load, hours in, on a second workspace.

**Lesson about the safety measure.** A backup of the credential file was taken before the call and is now worthless for the refresh token — rotation invalidated the old one server-side, so restoring the file would restore an invalid token. The backup only preserved an access token with hours left. A backup protects against *local* corruption; it cannot protect against a server-side state change. Proposing it as the safeguard was theatre, and the honest mitigation was the one that actually mattered: write the new tokens back immediately and atomically.

### What the spike record says about this document

Fifteen spikes. Six falsified something this document had already asserted and defended, three of them in ways that would have shipped either a vulnerability or a role that silently did nothing. One — #14 — proved a core premise correct and simultaneously produced two requirements the document had not thought of, which is the pattern to expect: a spike that only confirms is a spike that was not sharp enough. The remaining open assumptions were written by the same process that produced the falsified ones and have had no more scrutiny. **Treat the table above as a list of likely errors, not a list of formalities.**
