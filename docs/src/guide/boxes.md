# Boxes

A box is not a process. It is a kept thing with a name: its own `$HOME`,
its own history, its own logins and whatever its preflight hook installed.
Starting it runs it; stopping it puts it down; starting it again picks it
up where it was.

A folder holds as many boxes as you make.

```sh
wormhole box                 # first time here: makes a box
wormhole box                 # later: back to that same box
wormhole box --new           # another box, its own home, beside the first
wormhole ps --all            # every box this host keeps
wormhole box --new --as api      # another box, called `api`
wormhole box --id a3f9c1e40b2d   # that exact box, whenever (or `--id api`)
wormhole stop a3f9c1e40b2d       # end a running box, from anywhere
wormhole rename api web          # call it something else
wormhole reset api               # keep the box, empty its home
wormhole remove api                  # take it away for good
```

Every command that takes a box takes either form: the id `ps` prints, or
the name you gave it. Which one you typed is decided by the string itself.

## Ids

Every box has a twelve-character id, assigned when it is made and fixed
for as long as it exists. It is the one name wormhole uses: `ps` prints
it, `attach` takes it, `--id` takes it. A box can also be given a name to
type instead — see [Names](#names).

```
$ wormhole ps --all
ID            NAME     ALIAS  STATE         ROLE     AGENT   LAST USED  WORKSPACE
a3f9c1e40b2d  Alphaca  api    running 8213  alphaca  claude  2m         /home/me/proj
7d2e04ab91ff  Alphaca  -      idle          alphaca  claude  3h12m      /home/me/proj
c518bb0d7a34  -        -      idle          -        claude  2d1h       /home/me/notes
```

## Names

An id is derived and cannot be chosen. Give a box a name and type that
instead:

```sh
wormhole box --new --as api
wormhole box --new --as web

wormhole attach api
wormhole stop web
wormhole box --id api        # back into it later
```

Everything that takes a box takes both forms — the id `ps` prints, or the
name you gave it. Which one you typed comes from the string itself: twelve
hex characters is always an id, which is why a name may never be twelve hex
characters.

`--as` on a box that already has a name renames it; leaving it off keeps
the name it had. A name belongs to one box in one workspace, so a name
already taken here is refused rather than silently moved.

```
$ wormhole ps --all
ID            NAME     ALIAS  STATE         ROLE     AGENT   LAST USED  WORKSPACE
a3f9c1e40b2d  Alphaca  api    running 8213  alphaca  claude  2m         /home/me/proj
7d2e04ab91ff  Alphaca  web    idle          alphaca  claude  3h12m      /home/me/proj
```

`NAME` is what the manifest calls the box and several boxes share it;
`ALIAS` is the one that names exactly one.

## What a bare `wormhole box` does

It resumes this workspace's most recently used box for this role. If every
one of them is already running, it makes another.

So typing the same command twice gets you back the same box, and you never
pile up strangers by accident. Making another is something you ask for.

```sh
$ wormhole box              # box: a3f9c1e40b2d (new)
$ wormhole box              # box: a3f9c1e40b2d (resumed)
$ wormhole box --new        # box: 7d2e04ab91ff (new)
```

The role is part of the match. A box's home carries that role's toolchain
and persona, so a `rust` run never resumes a `java` box —
that would be the wrong home wearing the name of continuity.

What is matched is *where the role comes from* — the directory a local one
lives in, the repository a fetched one came from — and never the spelling
you used. `--role alphaca` and `--role ./roles/alphaca` are one role and
one box, and re-pinning a fetched role to a newer commit keeps the box you
were working in. See [roles](roles.md).

## Stopping one

```sh
wormhole stop <id>
```

Ends the box's PID 1; its own `wormhole box` sees that and cleans up. `d`
in the [panel](#from-the-panel) does the same to the row the cursor is on, and
says so — on a box that is not running it says that instead, rather than
redrawing an unchanged screen.

## Renaming one

```sh
wormhole rename <id|name> <new name>
```

Sets what a box answers to instead of twelve hex characters. `--as` does
the same at start; this does it without starting anything, so a box you
named badly in a hurry is not a box you have to boot to fix.

The name belongs to the box's own workspace, and no two boxes there may
share one — a name that already answers for another box is refused, with
that box's id in the message. A name may never be spellable as an id.

Refused while the box is running: the running box's registry entry carries
the name it started under, nothing rewrites that entry in flight, and a
rename that `attach` could not follow would be a rename in name only.

## Resetting one

```sh
wormhole reset <id|name>
```

Empties the box's home and keeps the box. Same id, same name, same
workspace, same role — nothing the agent put there. The next start seeds
the instructions, the first-run answers and the status line again, and the
agent logs in again.

This is the difference between starting over and starting *somewhere
else*. `wormhole box --new` gives you a second box beside the first;
`reset` gives you the first one back, empty.

## Removing one

```sh
wormhole remove <id|name> [<id|name>...]
```

Takes the box away: its home, and with it the history, the logins and
whatever the agent installed. Its snapshot goes too. This cannot be
undone, so it is the one thing wormhole never does on its own — `gc`
removes only what it can *prove* is dead, and a box you are simply
finished with is not something anything can prove.

Several at once, because clearing up after a day's work is what the
command is for. Every name is resolved before any box is removed: a typo
at the end of the list refuses the whole line rather than leaving half of
it done.

Removing and resetting both need the box idle, and both ask the kernel
rather than a list: they take the box's own claim first, so neither can
ever delete a home an agent is writing to. A running box is refused, with
the `wormhole stop` line that comes first.

A home whose record cannot be read is still a box here. Its directory name
carries the id, which is why `--id` can start one — so `remove` takes one too.
A box you can see and cannot get rid of is a state this store does not
have.

## What each box keeps to itself

| Per box | Where |
|---|---|
| `$HOME` — history, logins, toolchain, caches | `~/.local/share/wormhole/homes/<workspace>-<id>/` |
| Its claim, so the same box cannot start twice | `~/.local/share/wormhole/locks/<workspace>-<id>.lock` |
| Its snapshot, and the receipt measured against it | `~/.local/share/wormhole/snapshots/<workspace>-<id>/` |

Two boxes never share any of those. One shared `$HOME` would be two agents
writing one history, one config and one instructions file at the same time;
one shared snapshot would have each box's receipt report the other's edits.

The image is shared, because it is read-only in effect: a box gets a
throwaway copy and deletes it on exit.

## What they do share: the workspace

Boxes in one folder edit the same files, with no coordination between
them. That is the point — two agents working the same tree, like two
people would. It is also the cost, and it is worth knowing exactly where
it bites:

- **Uncommitted edits.** Last write wins, silently. Neither agent can see
  that the other overwrote it.
- **The git index.** `.git/index` is shared *when both boxes work the same
  tree*. Past the `index.lock` collisions, one agent's `git commit -a`
  sweeps in the other's half-finished files and commits them. A worktree
  each ends this — see below.
- **Build directories.** `target/`, `build/` and the like are fine: cargo
  and gradle take file locks, so builds queue instead of corrupting each
  other. Slower, not broken.

If you want the parallelism without the sharing, give each box a tree of
its own. A `git worktree` **inside the workspace** is the cheapest way,
and it is what several boxes in one folder are for:

```sh
git worktree add wt/feature-a
git worktree add wt/feature-b

wormhole box --new     # point one box at wt/feature-a
wormhole box --new     # and another at wt/feature-b
```

Each worktree has its own index, its own `HEAD` and its own branch, so
two agents stop sharing `.git/index` entirely — while still sharing one
object store, which a clone would duplicate.

**Where the worktree lives is the whole question.** A worktree's `.git` is
a line of text — `gitdir: /abs/path/to/main/.git/worktrees/<name>` — and
the box sees only the workspace you started it in. Created *inside* the
workspace, that path is inside the mount and every git command works.
Created *outside* it, the path is not there and git answers `fatal: not a
git repository`.

A clone works too, and is what you want when the trees should share
nothing at all:

```sh
git clone git@github.com:you/proj.git ~/work/proj-b
cd ~/work/proj-b && wormhole box
```

## Only starting the same box twice is refused

```
$ wormhole box --id a3f9c1e40b2d
wormhole: box a3f9c1e40b2d is already running (pid 8213)
```

The claim is an `flock`, held by the kernel for the process's whole life.
Nothing has to be reaped: a box killed at any point stops holding it that
instant. Whether a box is free is asked by *taking* its lock, never by
reading a list, so the answer cannot go stale between the reading and the
start.

To get a second terminal into a box that is already running, attach to it
rather than starting it again:

```sh
wormhole attach a3f9c1e40b2d          # its agent
wormhole attach a3f9c1e40b2d -- sh    # a shell beside it
```

## From the panel

```sh
wormhole
```

The panel lists every box, running and idle, most recently used first.
Enter does the one thing that row allows — join a box that is running,
start one that is not.

| Key | What it does |
|---|---|
| `enter` | join a running box, or start an idle one |
| `n` | another box here, from a manifest or role you pick |
| `d` | stop the running box on this row |
| `x` | remove this box — asks first |
| `r` | reset this box, keeping the box — asks first |
| `y` | the one key that answers a question |
| `q` | quit, or take back a question |

`x` and `r` both take an agent's history, so both ask before they act, and
`y` is the only key that answers. **Every** other key cancels — `q`
included, because `q` is the reflex for "no" and taking the whole panel
down on it would be the one answer nobody meant.

They are the same bodies `wormhole remove` and `wormhole reset` use, so what
`x` does in here and what `remove` does out there can never drift apart.
Neither offers itself on a running box: the row says `d` stops it first.

A key the panel understands but cannot act on where the cursor is says so
under the hints, and the next key clears it. `d` on a box that is not
running is that case: it kills nothing, and it tells you rather than
redrawing a screen that looks unchanged.

Resuming from the panel starts the box in **its own** workspace, not
wherever the panel was opened.

A home the panel cannot read is named under `could not read:`, on the
panel's own screen — nothing prints over the box list while the panel is
up. Such a home has no row to put a cursor on, so it is reached by id from
the command line: `wormhole box --id <id>` starts it and `wormhole remove
<id>` takes it away, both by its directory rather than by a record neither
can read.

## Upgrading

Homes kept before boxes had ids are adopted, not orphaned: an old home is
already named `<workspace>-<twelve hex>`, and that hex is read as the id
of its workspace's first box. History, logins and installed toolchains
carry over with nothing to do.

The same holds for homes kept before a role had an identity. Such a record
carries only the reference somebody typed at the time, so it is matched on
that, exactly as it used to be; the first start under this wormhole writes
the identity into it and it behaves like any other from then on.

## Reclaiming

`wormhole gc` deletes only what it can prove. It reads each home's own
record to decide: a home whose workspace no longer exists is proven dead
and can go; one whose workspace is still there is kept, however long it
has been idle. A lock file whose box is gone is dead too — `remove` leaves
those behind on purpose, because unlinking a lock while holding it would
let another start take a second, different one for the same box.

```sh
wormhole gc                            # report only
wormhole gc --delete                   # remove what is proven dead
wormhole gc --delete --unreferenced    # and what no box here starts from
```

### Images, bases and artifacts

Each of these is named by a digest, and a recipe names every digest the
store keeps on its behalf: the image it builds to, the base rootfs it
starts from, and every artifact it pins. So reading the recipe of every
box on this host turns *"nothing records what this belongs to"* into a
proof, and a version bump no longer leaves a gigabyte behind for good.

Three verdicts, and the difference between them is the whole point:

| Verdict | Means | Taken by |
|---|---|---|
| `dead` | proven finished with | `--delete` |
| `unreferenced` | proven that no box here starts from it | `--delete --unreferenced` |
| `unproven` | a recipe could not be read, so nothing can be proven | nothing |

`unreferenced` is not the same claim as `dead`. A recipe you have built
but never started a box from references nothing that can be counted, so it
reads unreferenced and is correct to keep — which is why a bare `--delete`
leaves it and you have to ask for it by name. The report says how much
more the flag would give back, so the number is never silently missing
from both columns.

One recipe that cannot be read may be the very one that references an
image, so a single unreadable manifest makes the whole answer `unproven`
rather than quietly deleting on a gap in the evidence. The image a
*running* box is using is always kept: with `rootfs = "readonly"` that
image is the box's live root.
