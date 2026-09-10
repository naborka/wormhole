# Roles

A role is a box recipe that is not tied to one workspace: persona, image,
and a hook that sets up tools. Point wormhole at it from any workspace.

```
my-role/
  wormhole.toml       # the recipe: image, agent, access, env
  ROLE.md             # persona, named by `[agent] instructions`
  hooks/preflight.sh  # runtime setup, named by `[agent] preflight`
```

This repository ships `alphaca`. It can run as claude, codex, or grok.

## Role vs product

| | |
|---|---|
| **Role** | Who it is. One directory. |
| **Product** | Which CLI: `claude`, `codex`, or `grok`. |

Same role, three boxes — own home, login, history.

```sh
wormhole box --role alphaca            # pick which CLI
wormhole box --role alphaca --run grok # skip the pick
```

A kept box resumes the CLI it already runs. Off a terminal, a new box
needs `--run`.

## What the hook installs

Before the agent starts, `hooks/preflight.sh` runs *inside the box*. It
reads `WORMHOLE_RUN` and sets up only that product:

1. the CLI binary (claude / codex / grok)
2. skills: `npx skills add <repo> -a $RUN` (caveman, mattpocock, rust-skills)
3. MCP: `claude|codex|grok mcp add context7`
4. rtk, aimed at that product

Claude plugins (for example rust-analyzer-lsp) have no grok/codex
analogue. The hook installs them only for claude and prints that it
skipped them otherwise.

Nothing of this is a wormhole schema. The hook calls each CLI's own
commands. A failing required install stops the box; the rest warn and
continue.

## The three ways to name one

`--role` takes exactly one of these, and tells them apart in this order:

| What you type | What it means |
|---|---|
| `--role github:you/role@<sha>`, `--role https://…@<sha>` | a pinned commit in a git repository |
| `--role ./my-role`, `--role /abs/path` | that directory, right there |
| `--role alphaca` | a role installed under `~/.config/wormhole/roles/` |

A transport is checked first, because every remote form also contains a
`/` and the path rule would otherwise swallow it. Anything with no
transport and a `/` is a path; anything else is an installed name.

A bare word is **never** read as a directory in the folder you are in. If
one of that name is sitting there, the refusal says so and gives you both
commands:

```
$ wormhole box --role alphaca
wormhole: no role alphaca in ~/.config/wormhole/roles
  there is a role at ./alphaca — start it with `--role ./alphaca`,
  or name it with `wormhole role add ./alphaca`
```

An explicit `--role` beats the workspace's own `wormhole.toml`: the flag
is you speaking, the file is a default.

## A workspace that names its role

A project can say which role it uses, in one line, and then nobody types
`--role` at all:

```toml
# wormhole.toml — the whole file
version = 1
role = "github:you/alphaca-java@<40 hex characters>"
```

```sh
wormhole box     # that role, for you and for everyone who clones this
```

The pin lives in version control and moves in a pull request, rather than
in whichever shell history happens to be right.

**Not inheritance.** `role` names the recipe *instead of* carrying one. A
manifest with `role` and `[image]` together is refused, not merged —
merging is where a key like this turns into an override system nobody can
predict.

**A launch still fetches nothing and asks nobody.** A fresh clone whose
manifest names a pin this host has not approved refuses and names the
`wormhole role add` that fixes it, exactly as typing that ref at `--role`
does. Cloning a repository causes nothing to be fetched and nothing to run.

### Three ways to name one role, not three roles

Which of the three you type changes nothing about *which box you get*. A
role is identified by where it comes from — the directory a local one
lives in, the repository a fetched one came from — so all of these resume
the same box, with the same home, history and toolchain:

```sh
wormhole box --role alphaca          # installed by that name
wormhole box --role ./roles/alphaca  # the same directory, spelled out
wormhole box --role ~/work/alphaca   # the same directory again
```

The commit is which *version* of a fetched role you are on, not which role
it is, so re-pinning to a newer commit keeps the box you were working in
rather than starting a stranger.

Two different roles stay two boxes. That is the whole reason a role is
part of a box's identity: a box's home carries that role's toolchain and
persona, and resuming across roles would be the wrong home wearing the
name of continuity.

See [role vs product](#role-vs-product) for `--run`.

## Managing what is installed

```sh
wormhole role add <dir|ref>  [--as <name>]   # install one, from either kind of source
wormhole role list                           # what is installed, where it points, whether it can start
wormhole role show <name|dir>                # the whole recipe, without installing
wormhole role remove <name>                  # take the name back
```

`role list` is the only way to see what is installed without opening the
panel:

```
$ wormhole role list
NAME     FROM                                        STATE
alphaca  /home/me/work/alphaca                       ready
java     https://github.com/you/role at 1b2c3d4e5f60  ready
old      https://github.com/you/x at 9f8e7d6c5b4a     needs `role add` to fetch
```

`role show` puts up the same screen `role add` asks with — grants, CA
trust, DNS, environment, hooks, the base tarball and its digest, every
artifact the build is handed with its URL and digest, the package list and
every build line — and installs nothing. It is a dry run
for `--role`, and the only way to read an approval screen again once you
have answered it.

`role remove` unlinks the name and never follows it: removing a local role
you installed takes the name back and leaves the directory you work in
exactly where it was.

## From a git repository

```sh
wormhole role add github:you/your-role@<40 hex characters>
```

wormhole fetches that commit, shows you everything the role asks for, and
asks before anything is kept. Then it is a role like any other — the same
name, the same panel entry, the same `--role`:

```sh
wormhole box --role your-role
```

The name defaults to the repository's; `--as <name>` overrides it:

```sh
wormhole role add github:you/wormhole-role-java@<sha> --as java
```

To move a role to a newer commit, run `role add` again with the new sha.
wormhole prints the old commit and the new one, shows the fresh recipe,
and asks again — a re-pin is where a role's grants and build shell can
change under a name you already trust.

To remove one:

```sh
wormhole role remove your-role
```

### The commit is the pin, and there is no other kind

A ref with no `@<sha>` is refused where you type it. `git` verifies every
object it receives against its own hash, so the commit id is the proof —
nothing about the transport is trusted, and the same forty characters mean
the same bytes forever.

| Form | |
|---|---|
| `github:owner/repo@<sha>` | shorthand for the `https://` form |
| `https://host/owner/repo@<sha>` | |
| `git@host:owner/repo@<sha>`, `ssh://…@<sha>` | over SSH |
| `file:///path/to/repo@<sha>` | a repository on this machine |

wormhole runs `git` and asks for no credentials of its own, so a **private**
repository needs a form your `git` can already authenticate — usually
`git@host:owner/repo`, since `github:` expands to `https://` and a plain
https fetch of a private repository has nothing to authenticate with.

### What you are approving

A role's `[image] build` is arbitrary shell that runs on your machine when
its image is built. Before anything is kept, wormhole shows you the whole
recipe — grants, CA trust, DNS, environment, hooks, the base tarball and
its digest, the package list, and every build line — and waits.

Approval is per commit, and host-wide. Approve `you/role@<sha>` once and it
is approved in every workspace on this machine, because a commit names the
same bytes wherever it is used. A different commit is a different pin with
no approval at all.

**What approval does not bound:** nothing limits what a role may ask for.
A role that asks for `~` gets your whole home, read-write, if you approve
it. The preview is what stands there — read it. See
[what the box can reach](access.md).

### Where it lands, and what a launch will not do

The pointer — the URL and the commit — is written to
`~/.config/wormhole/roles/<name>/source.toml`, which is small enough to
read and diff. The fetched commit itself goes under
`~/.local/share/wormhole/checkouts/<sha>`, beside the images and bases it
is exactly like: named by a digest, never modified, shared by every role
pinned to it.

**A launch never fetches and never asks.** If the checkout or the approval
is missing, `wormhole box --role <name>` refuses and names the `role add`
that fixes it. That is what lets a role from a repository be used from a
script or a scheduled run: by the time anything launches, the network and
the human decision are already behind it.

The one exception is a ref typed straight at `--role`, with no install:

```sh
wormhole box --role github:you/role@<sha>
```

That is a person present at a terminal, so it fetches and asks. With no
terminal on stdin it refuses like everything else — a question nobody can
see is never treated as answered.

## From a folder on this machine

**Straight from the directory, nothing installed.** Any argument with a `/`
and no transport in front of it is the role's directory itself. This is
how you try one out, and how you work on one you are writing:

```sh
wormhole box --role ./roles/alphaca
wormhole box --role ~/work/my-role
```

Edits to that directory take effect on the next start — there is no copy
and no cache of the files, so the directory is always what runs.

**Installed by name.** Give it a name once and type the name after that:

```sh
wormhole role add ./roles/alphaca
wormhole box --role alphaca
```

`role add` on a directory links it rather than copying it, so the
directory you edit stays the role — a copy would leave the name quietly
running whatever the recipe said on the day you installed it. The name
defaults to the directory's; `--as <name>` overrides it.

Nothing is fetched and nothing is approved. There is no commit here to
approve, and a directory can change a second after any answer, so a gate
would be a promise wormhole cannot keep. `role add` prints the recipe
instead, and `wormhole role show` prints it again whenever you want it.

Either way, the first start builds the role's image. `wormhole build
--role alphaca` does the same thing ahead of time, if you would rather
wait for it then than at the start of a session.

An installed role shows up in the panel (`wormhole`, then `n`) alongside
the workspace's own manifest. If `run` lists several products, `n` asks
which CLI next. Then the permission preview. A `--role ./path` does not
show in that list — it was never installed. One `role add` puts it there.

A role is also part of a box's identity, and it is *where the role comes
from* that counts, never how you spelled it. See
[three ways to name one role](#three-ways-to-name-one-role-not-three-roles)
above, and [boxes](boxes.md).

## How a role's files reach the box

The role directory itself is never mounted into the box. Its two files
travel by seeding into the box's kept home before start:

- `[agent] instructions` is composed after the built-in instructions into
  one canonical `AGENTS.md` at the box home root; the path the agent
  actually reads gets a pointer to it — `.claude/CLAUDE.md` holding
  `@~/AGENTS.md`, or a symlink at `.codex/AGENTS.md` and
  `.grok/rules/AGENTS.md` — so the text exists once however many agents
  learn to read it
- `[agent] preflight` is copied to `.wormhole/preflight` in the box home
  and run from there, fresh on every start, with `WORMHOLE_RUN` set to the
  product this box is. Removed when the manifest stops naming one, so
  nothing stale survives a recipe change

A hook the manifest names but the role does not carry stops the launch on
the host, with the path in the error.

## Writing a shareable role

Keep host specifics out. The tools that make that possible:

- `~` in `[access] grants` expands to whoever's home is running it
- `[[image.artifact]]` for everything the build installs from the network:
  a URL and a `sha256`, fetched on *their* host and proved before their
  build box starts. A role written this way builds on a network that
  intercepts TLS, because its build opens no TLS connection — and it
  installs the same bytes on every machine, which a `wget` in
  `[image] build` could not promise
- `[access] host_ca = true` for what a recipe cannot pin — the toolchain
  `rustup` fetches for itself, whatever `cargo` reaches at run time —
  instead of granting a CA bundle path; wormhole finds the host's bundle
  wherever its distro keeps it
- `default`-style env for the user's own keys; `fixed`-style only for
  paths inside the box

What a shared role still means on someone else's machine: every grant and
trust in it is granted from *their* host, to a box running on *their*
machine. The [access model](access.md) explains what each grant carries.

To publish one, put the role at the repository root — `wormhole.toml`
beside `ROLE.md` and `hooks/` — and tell people the commit. A repository
with no `wormhole.toml` at its root is refused at the fetch, so a role in a
subdirectory is not installable; give it a repository of its own.
