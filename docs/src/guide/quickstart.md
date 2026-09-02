# Quickstart

## 1. Write a manifest

```sh
wormhole init
```

`wormhole.toml` in your workspace root is the whole recipe: the image, the
agent, and everything the box may reach. `init` writes a working one —
Alpine, Node, Claude Code, your credential and nothing else of your host:

```toml
{{#include ../../../templates/wormhole.toml}}
```

Every key is in [the manifest reference](manifest.md). Read
[what the box can reach](access.md) before you add a grant.

`init` never writes over a manifest that is already there.

## 2. Build the image (optional)

```sh
wormhole build
```

Fetches the base rootfs, verifies its digest before extracting anything,
installs the packages, runs the build lines. Cached twice: the extracted
rootfs under its tarball digest, the finished image under a digest of
`[image]`. Run it again and it prints `image ready` without fetching or
installing anything. A changed grant never reinstalls; a changed package
list never refetches the rootfs.

Skipping this step is fine: the next `wormhole box` builds the image it
needs and says so. Run it by hand when you would rather wait for the fetch
now than at your next start.

## 3. Run the agent

```sh
wormhole box
```

The agent starts as PID 1 of a fresh copy of the image, in your workspace,
holding your terminal. When it exits, the copy is deleted.

Type it again later and you get the same box back, with its history and
whatever it installed. A folder holds as many boxes as you make — see
[boxes](boxes.md).

```sh
wormhole box -- sh        # your command instead of the agent
wormhole box --new        # another box here, its own home
wormhole box --new --as <name>  # another box, under a name you pick
wormhole box --id <id|name>     # that exact box, whenever
wormhole ps               # running boxes
wormhole ps --all         # every box this host keeps, running or idle
wormhole usage            # the account's usage windows, with resets
wormhole attach <id|name> # second terminal into a running box (its agent)
wormhole attach <id> -- sh
wormhole stop <id|name>   # end a running box from anywhere
wormhole rename <id|name> <new>  # call it something else
wormhole reset <id|name>  # keep the box, empty its home
wormhole remove <id|name>...  # take boxes away for good
wormhole gc               # what the data home holds, and what can go
```

`<id>` is the twelve-character ID `wormhole ps` prints; `<name>` is what
you called the box with `--as`. Every command takes either.

While you talk to the agent, Claude Code's status bar shows what is left
of the account's usage windows — the session window, the weekly one, and
any per-model weekly window that binds sooner:

```
SESSION: 13%  FABLE: 78%  WEEKLY: 47%
```

The numbers are the account's, shared by every box, and are refreshed
from the host about once a minute; the box holds no credential and makes
no call for them. A reading that stops refreshing shows its age —
`(12m ago)` — rather than posing as current. A status line you configured
yourself in the home's `.claude/settings.json` is left alone.

## 4. Or drive it from the panel

```sh
wormhole
```

A live list of every box, running and idle, most recently used first.
`Enter` does the one thing that row allows — join a running box's agent,
or start an idle one again in its own workspace. `d` stops a running box,
and says so under the hints; on a box that is not running it says that
instead of doing nothing. `n` makes another box here: it lists every
launchable source (the workspace's manifest and each installed
[role](roles.md)) and shows a full preview of what the box will see before
anything starts. `x` removes a box and `r` resets one — both ask first,
and `y` is the only key that answers. `q` quits, or takes back a question.

The same things, from anywhere and without the panel:

```sh
wormhole stop <id|name>
wormhole reset <id|name>
wormhole remove <id|name>
```

## 5. Reclaiming disk

```sh
wormhole gc                            # report only
wormhole gc --delete                   # remove what is proven dead
wormhole gc --delete --unreferenced    # and what no box here starts from
```

Wormhole deletes only what it can **prove**. A box directory whose process
is gone, a kept home whose workspace no longer exists and a lock whose box
is gone are all proven *dead*, and `--delete` takes them.

An image, a base or a fetched artifact that no box on this host starts
from is proven *unreferenced* — a weaker claim, because a recipe you have
built but never run a box from references nothing that can be counted. So
`--delete` leaves those and `--unreferenced` is what asks for them; the
report says how much more that flag would give back. One recipe that
cannot be read makes the whole answer *unproven* rather than deleting a
gigabyte on a gap in the evidence.

## Where to next

- [The manifest](manifest.md) — every key, including the boundary ones
- [Boxes](boxes.md) — several in one folder, resumable by id
- [Roles](roles.md) — one recipe, any workspace
- [The broker](broker.md) — the credential off the box for good
- [What the box can reach](access.md) — the security model
