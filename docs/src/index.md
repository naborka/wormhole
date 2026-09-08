# What is wormhole

Wormhole runs a coding agent in a box. The box is built from one file,
`wormhole.toml`, and the rule is default deny: nothing from your machine
reaches the agent unless that file names it — not a path, not an
environment variable, not a resolver.

The agent runs with its permission prompts switched off, because the box,
not the prompt, is what holds the line. Inside it gets:

- a writable, throwaway copy of the built image — deleted when the box
  exits, so nothing the agent does survives into the next box
- the workspace, read-write, at its real host path — files it writes
  belong to you
- the granted paths, and nothing else
- only the environment variables the manifest declares
- one named DNS resolver, or none
- a kept `$HOME` per box, so logins and history survive between
  boxes while the root stays throwaway

It is built on user, mount, UTS and PID namespaces — no daemon, no root,
no Docker. One Rust binary.

```sh
wormhole init    # once: a working manifest in this workspace
                 # (or one line: role = "github:you/role@<sha>")
wormhole build   # optional: fetch rootfs, verify sha256, install — once
wormhole box     # every time: the agent, in a fresh copy of that image
                 # (and it builds the image first if nothing has)
wormhole         # the panel: running boxes, and `n` to start a new one
```

The box is on the host's network and speaks to the model API for itself.
What it logs in as is yours to pick — `credentials = "none"`, `"copy"`
or `"share"` — and the banner printed at every start names every host
path it was handed, a shared login included.

The rest of the commands:

| | |
|---|---|
| `wormhole help` | the whole tool on one page — start here |
| `wormhole doctor` | eight host probes and a plain verdict — run this first |
| `wormhole ps [--all \| <id\|name>]` / `attach <id\|name>` | what is running or kept, one box with its environment, and a second terminal into it |
| `wormhole stop <id\|name>` | end a running box, from anywhere |
| `wormhole rename <id\|name> <new>` | call a box something else, without starting it |
| `wormhole reset <id\|name>` | keep the box, empty its home |
| `wormhole remove <id\|name>...` | take boxes away for good |
| `wormhole role add <dir\|url@sha>` | install a role from a directory or a pinned commit |
| `wormhole role list\|show\|remove` | what is installed, the whole recipe, and taking a name back |
| `wormhole box --as <name>` | give a box a name to type instead of its id |
| `wormhole box --credentials none\|copy\|share` | which login the agent starts with, for this start |
| `wormhole gc` | what the data home holds, and what of it can go |

Start with [Install](guide/install.md), then the
[Quickstart](guide/quickstart.md). The [manifest reference](guide/manifest.md)
covers every key and [Boxes](guide/boxes.md) covers running several at once.
[Roles](guide/roles.md) make a box recipe shareable across
workspaces, and [credentials](guide/credentials.md) says how the agent
logs in. [What the box can reach](guide/access.md) is the security
model — read it before granting anything. [What changed](changelog.md) is
the plain-words history.

The working documents — [Concept](concept.md), [Plan](plan.md),
[Status](status.md) — are the project's own source of truth, rendered here
unchanged from the repository root.
