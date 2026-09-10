# The manifest

`wormhole.toml` is the box's whole recipe. Parsing refuses what it does
not understand: an unknown key, another version, a malformed digest or an
unknown agent is an error, never a guess.

The source of truth for these semantics is
[`manifest.rs`](https://github.com/naborka/wormhole/blob/dev/crates/wormhole-core/src/manifest.rs);
this page follows it.

## Shape

Two keys at the top, then one table per concern:

```toml
version = 1
name = "Alphaca"          # optional; what `wormhole ps` calls this box

[image]                   # what gets built — changing anything here rebuilds
[agent]                   # who runs, and what it reads before starting
[access]                  # what the box can reach
[runtime]                 # how the box is made, and what it leaves behind
[limits]                  # what it may use of the machine
[env.NAME]                # one table per environment variable
```

TOML reads a top-level key written *below* a header as part of that
table, so `version` and `name` belong above the first `[table]`. No two
tables share a key spelling, so a misplaced line lands on a field that
does not exist and is refused by name.

### `role` — a manifest that carries no recipe

A workspace can hand its box to a [role](roles.md) instead of describing
one, in which case that line is the whole file:

```toml
version = 1
role = "github:you/alphaca-java@<40 hex characters>"
```

`role` names the recipe *instead of* carrying one, so it may not appear
beside `[image]` — both together is refused rather than merged. An
explicit `--role` still beats it: the flag is you speaking, the file is a
default.

## `[image]` — what gets built

The digest of this table, and nothing else, names the built image. Change
a line here and the next `wormhole build` builds. Change one anywhere else
and it does not.

```toml
[image]
base = "https://example.invalid/rootfs.tar.gz"
base_sha256 = "<64 lowercase hex>"
package_sources = ["http://dl-cdn.alpinelinux.org/alpine/v3.22/main"]
packages = ["bash", "nodejs", "npm", "git", "ca-certificates"]
build = ["npm install -g --offline /tmp/claude-code.tgz"]

[[image.artifact]]
url = "https://registry.npmjs.org/@anthropic-ai/claude-code/-/claude-code-2.1.246.tgz"
sha256 = "<64 lowercase hex>"
into = "/tmp/claude-code.tgz"
```

For a real base and a real digest, run `wormhole init` — the
[quickstart](quickstart.md) prints what it writes. A URL and a digest are
kept in one place in this repository so they cannot go stale in four.

| Key | Meaning |
|---|---|
| `base` | URL of the base rootfs tarball (`file://` works too) |
| `base_sha256` | That tarball's digest, 64 lowercase hex. Verified before extraction — the only thing standing between a URL and arbitrary code in the box |
| `package_sources` | Written to `/etc/apk/repositories` before installing. `http` is fine: `apk` verifies package signatures, and a fresh rootfs has no CA store yet |
| `packages` | `apk add`, once, at build time |
| `build` | Shell lines run in the image after the packages, in order, stopping at the first failure. Each one is echoed before it runs, so the last line printed is the one that failed |
| `[[image.artifact]]` | Files fetched on the host and proved by digest, then placed in the build box — see below |

After `apk add`, the build refuses an image whose CA store the packages
left empty. Alpine's `ca-certificates` trigger discards its own output and
exits zero, so a bundle it failed to write is reported as a successful
install and every later `https` fetch fails with a verify error naming no
cause. The store the packages left behind is the build's business, so the
build is where it is checked.

### `[[image.artifact]]` — fetched on the host, proved by digest

The rule `base` already lives by, applied to everything else a recipe
installs. One table per file:

```toml
[[image.artifact]]
url = "https://static.rust-lang.org/rustup/archive/1.29.0/x86_64-unknown-linux-musl/rustup-init"
sha256 = "9cd3fda5fd293890e36ab271af6a786ee22084b5f6c2b83fd8323cec6f0992c1"
into = "/tmp/rustup-init"
```

| Key | Meaning |
|---|---|
| `url` | Where the bytes come from. Any scheme `curl` speaks, `file://` included |
| `sha256` | Their digest, 64 lowercase hex. A mismatch stops the build before any box starts |
| `into` | Where the file appears in the build box. Under `/tmp`, and nowhere else |

**Wormhole fetches it, not the box.** The host is where your resolver,
your proxy and your trust store already work; what crosses into the build
box is bytes whose digest the recipe already named. So a build that names
all its fetches this way opens no TLS connection of its own, and a network
that intercepts TLS — Cloudflare WARP, a corporate proxy — has nothing to
intercept. It also cannot substitute the bytes: the digest is fixed.

Four rules follow from what an artifact is, and each is refused rather
than worked around:

- **`into` must be under `/tmp`.** That is a tmpfs in the build box, so the
  finished image carries what the artifact *did* and not the artifact.
  Anywhere else is the image itself, where the bind would leave an empty
  file behind. `..` is refused before the prefix is believed.
- **The file is read-only and carries no execute bit.** Bytes off the
  network are not something to run by accident, so running one is a
  deliberate `cp /tmp/tool /tmp/run-tool && chmod +x /tmp/run-tool`.
- **An artifact needs a `build` line.** One nothing could use changes
  nothing about the image, so two byte-identical images would sit under
  two digests — the digest would be saying something untrue.
- **These tables go after every plain key of `[image]`.** TOML reads a
  `build` or `packages` line written below one of them as part of *its*
  table, and it is refused by name.

Fetched files are cached under their own digest, so two recipes naming the
same bytes share one copy and a changed digest can never read the old one.

What a recipe *cannot* pin — the toolchain `rustup` fetches for itself,
whatever `apk` resolves — still terminates TLS inside the build box. That
is what `[access] host_ca` is for, and it now reaches the build box too.

## `[agent]` — who runs

```toml
[agent]
run = "claude"                    # or ["claude", "codex", "grok"]
instructions = "ROLE.md"
preflight = "hooks/preflight.sh"
```

| Key | Meaning |
|---|---|
| `run` | `claude`, `codex`, `grok`. A list: pick on a terminal, or `--run`. Each name is its own box. Absent: no agent. A list cannot carry `model` |
| `model` | Passed the way the product reads a model: `ANTHROPIC_MODEL` in the box for `claude`, `GROK_DEFAULT_MODEL` for `grok`, the `model` key in `.codex/config.toml` for `codex`. Only with a single `run` name |
| `instructions` | A file beside the manifest, appended after the built-in instructions so it wins where they disagree |
| `preflight` | A script beside the manifest, seeded into the box home and run before the agent starts. `WORMHOLE_RUN` is set to this box's product, so one hook can install skills and MCP the way that CLI understands. The agent then replaces the shell. A failing hook stops the box. Non-secret setup only. Secrets belong to `ask`; the login to `[access] credentials` |

## `[access]` — what the box can reach

```toml
[access]
credentials = "none"                     # or "copy", "share"
dns = "1.1.1.1"                          # absent: the host's resolver
host_ca = false
# grants = ["~/some/path"]               # host paths, only when you mean it
```

| Key | Meaning |
|---|---|
| `grants` | Host paths the box may see, bound read-write at their host paths. `~` expands to your home. Symlinks, `..` and workspace overlap are refused |
| `credentials` | How the agent logs in. `"none"` (default): a clean box home, `/login` inside the box. `"copy"`: the host's login files copied into the box home once, where absent; the box refreshes its own copy and the host is never written. `"share"`: the host's login file bound read-write at the same place in the box home, so one login serves both and a refresh in the box lands on the host. `wormhole box --credentials MODE` beats this for one start. See [credentials](credentials.md) |
| `dns` | The resolver the box uses, building and running alike. Absent means the host's own `/etc/resolv.conf`, bound read-only |
| `host_ca` | **Adds** the host's CA bundle to what the box trusts, and points every TLS client at it — most do not read a bundle unless told to, and Node, which the agent is, never reads one at all. Applies to the build box as well. For networks that intercept TLS with their own CA. Off by default — see [what the box can reach](access.md) |

The box is on the host's network and speaks to the API for itself.
Nothing here filters a connection; what `[access]` decides is what of
*yours* the box can read — paths, trust, and the login it acts as. A box
with no `[access]` table holds no login of yours and reaches the
workspace and nothing else.

A manifest from before the broker went that still says `network`,
`broker` or `egress` is refused by name, with `credentials` as the
pointer. A host with no `/etc/resolv.conf` and no `dns` line is refused
too: a box on the host's network with no resolver would fail every
lookup and blame the network.

## `[runtime]` — how the box is made

```toml
[runtime]
rootfs = "copy"      # or "readonly"
```

### `rootfs`

`"copy"`, the default, gives each box a `cp --reflink` copy of the image
and deletes it on exit. On Btrfs or XFS that is free; on ext4 it is a full
physical copy of the image on every start.

`"readonly"` binds the cached image itself instead, with a small tmpfs
seeded from the image over `/etc` and `/var`. There is no copy at all, so
start time stops scaling with image size. The trade is real: the box can no
longer `apk add` anything at run time.

## `[limits]` — what it may use

```toml
[limits]
cpu = "1.5"        # cores' worth of runtime, not a pinning
memory = "512M"    # also "2G", or plain bytes
pids = 256         # the cheap answer to a fork bomb
```

cgroup v2 caps, applied to the box and not to the process reporting on it.
A limit that cannot be applied **stops the box** — a manifest that asked
for a ceiling and silently got none is the exact failure the limit exists
to prevent. Applying them needs a delegated cgroup hierarchy; `wormhole
doctor` says whether your host has one.

## `[env.NAME]` — the environment

The box's environment is exactly the variables the manifest declares.
An undeclared host variable never reaches the box.

```toml
[env.ANTHROPIC_API_KEY]
default = ""              # host's value wins; empty if the host has none

[env.SHELL]
fixed = "/bin/bash"       # the host's value is ignored
```

`default` is for things that describe you (keys, tokens). `fixed` is for
things that describe the box — your host `SHELL` is a path that does not
exist in there.

One more per-variable key: `ask = true` fills it by asking you — once,
ever. The first start that finds an asked variable empty prompts on the
terminal (input masked) and keeps the answer in
`~/.config/wormhole/secrets.toml` (0600), so every later box that
declares the same name — any role, any workspace — already has it. One
`CONTEXT7_API_KEY` typed one time serves all ten of your roles. An asked
value is a secret: it is masked on every screen wormhole prints, and it
never comes from the manifest, so a role you fetch cannot carry one. Off
a terminal nothing can ask: the start says which name is missing, points
at `wormhole secret set NAME`, and goes on without it. `wormhole secret
list | set NAME | remove NAME` manages the store; values never appear in
argv or on any screen.

```toml
[env.CONTEXT7_API_KEY]
ask = true                # asked once, kept for every box
```

`ask` beside `fixed` or `default` is refused: a value that is never
missing could never be asked for. A secret a box holds is a secret that
box's agent can read; the agent's own login is `[access] credentials`'
business, not `[env]`'s.

The terminal is declared for you. `TERM` is only a name: every curses
program turns it into a description by looking it up, and the box carries
only the descriptions its image ships. So on every box start, and on every
`wormhole attach`, wormhole copies your terminal's own compiled description
out of the host's terminfo database into the box home, and your real
`TERM` reaches the box and resolves in it. When there was no description
to copy, the box is told `xterm-256color`. `COLORTERM`, `TERM_PROGRAM`
and `TERM_PROGRAM_VERSION` carry the host's value too. A manifest that
declares any of the four wins.

## `version`

`version = 1`. A manifest claiming any other is refused by name:

```
wormhole: this wormhole reads manifest version 1, not "v1alpha1"
```

The key exists for exactly that case. An *added* key is already caught
without it — unknown keys are refused — but a *renamed* one would surface
as a heap of unknown-field errors instead of "your wormhole is too old".


## What a second run does

`wormhole build` hashes `[image]`, looks for an image under that digest,
and stops there if it exists:

```
$ wormhole build
image ready: ~/.local/share/wormhole/images/8f3c…
```

Nothing is fetched, nothing is installed. The base tarball and every
artifact are cached separately under their own digests, so a changed
package list never refetches them and a changed grant never reinstalls
anything.

`wormhole box` builds the same way when there is no image yet, and says so
before it starts. An image is what a box is made of, so nothing asks you
to go and make one first; `wormhole build` is only how you pay that cost
when it suits you rather than at your next start. Two starts wanting the
same image is fine: the second waits for the first and then uses what it
built. What a box *does* redo on every start:

- copies the image into a throwaway root (free on Btrfs and XFS, a full
  copy on ext4, skipped entirely with `[runtime] rootfs = "readonly"`)
- re-seeds the instructions file, the preflight hook and the Claude Code
  config into the kept home, so a manifest edit or a wormhole upgrade
  takes effect at once

The kept home itself survives — one per box — so history, settings, logins
and installed toolchains carry over every time that box starts again. A
workspace holds as many boxes as you make; see [boxes](boxes.md).

## The three caches

```
~/.local/share/wormhole/bases/<sha256 of tarball>       # extracted rootfs
~/.local/share/wormhole/artifacts/<sha256 of the file>  # one fetched file
~/.local/share/wormhole/images/<sha256 of [image]>      # after packages + build
```

Each is keyed by what it holds, so nothing shares a key with anything
else: a changed package list never refetches the base, and two recipes
naming the same artifact at two addresses share one copy.

Editing `[image]` builds a *new* image beside the old one. The old one
stays until you ask for it: `wormhole gc` reads the recipe of every box on
this host, and an image, base or artifact no box starts from is reported
`unreferenced`. `wormhole gc --delete --unreferenced` gives that space
back — see [Reclaiming](boxes.md#reclaiming).
