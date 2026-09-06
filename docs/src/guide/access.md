# What the box can reach

The rule is default deny. The agent inside a box gets exactly what the
manifest names, and the panel's preview screen shows all of it before a
box starts. This page is what each line of that preview actually means —
including the one thing a mount plan cannot show.

## The boundary

A box is user, mount, UTS and PID namespaces — and, when the manifest asks
for one, a network namespace too. Inside:

- **Root filesystem** — a writable, throwaway copy of the image. Deleted
  on exit; nothing the agent does survives into the next box or back into
  the image. With `[runtime] rootfs = "readonly"` the image is bound
  instead of copied and cannot be written at all, which is stricter, not
  looser.
- **Workspace** — read-write, at its real host path. This is the point of
  the tool: the agent works on your code. Files it writes belong to you,
  because the box's uid is yours.
- **Grants** — each named host path, bound at its host path. A grant must
  be the real path: symlinks are refused (a symlink could quietly retarget
  what you granted), `..` is refused, overlap with the workspace is
  refused.
- **Home** — a kept directory per box under
  `~/.local/share/wormhole/homes/`, mounted as the box's `$HOME`. Logins
  and history survive between boxes. Wormhole also writes into it: the
  agent's instructions and, once a minute, `.claude/wormhole-limits` —
  the account's usage windows, rendered on the host so the box needs no
  credential and no extra network reach to show them.
- **Environment** — only declared variables. An undeclared host variable
  never reaches the box.
- **DNS** — the one named resolver, or none.

Four things hold that are not mounts, and none of them can be turned off:

- **No capability, and none regainable.** The box starts with an empty
  capability bounding set and `no_new_privs`. Nothing in it can regain a
  capability or raise privilege through any `execve` — a setuid binary in
  the image is just a file.
- **The box checks its own mounts.** After pivoting, PID 1 reads its own
  `/proc/self/mountinfo` and refuses to start if any read-write mount is
  not in the plan. The plan being exhaustively tested was never proof that
  the box ran behind it; this is.
- **The box checks its own capabilities.** After dropping them it reads its
  own `/proc/self/status` and refuses if any is left.
- **One process per box.** An `flock` the kernel holds for the process's
  whole life, so the same box can never be started twice into one home. A
  box killed at any point leaves nothing stale behind and nothing a second
  start can mistake for free. A workspace may hold as many *different*
  boxes as you make — see [boxes](boxes.md).

## What it can dial

By default: nothing it was not given. The box gets a network namespace
of its own holding only loopback — there is **no route** off the
machine, rather than a filter in front of one — and reaches out through
the broker alone: the model API with no credential in the box, and the
manifest's `egress` hosts over `CONNECT`, resolved and dialed host-side.
See [the broker](broker.md).

```toml
[access]
network = "host"     # the opt-out: every route the host has
```

With `network = "host"` the box can open connections wherever the host
can; the resolver line and default-deny grants bound what it can *find*
and *read*, not what it can *dial*. Every launch prints which of these
it got:

```
boundary: namespaces (host kernel SHARED) · egress: model api + 5 allowed hosts via the broker · workspace: rw · grants: 0
```

That line is not decoration. A staged boundary that does not say which
stage it is in is a lie, so it is the last thing printed before the agent
takes the terminal — and it names every way out the box actually has.

## What it can use of the machine

`[limits]` caps cpu, memory and process count through cgroup v2. Absent
means the kernel's default, which is no limit. A limit that cannot be
applied stops the box rather than being skipped, because a manifest that
asked for a ceiling and silently got none is the failure the limit exists
to prevent.

## What it changed

The workspace is the box's whole remaining local blast radius.
`[runtime] snapshot = true` takes a reflink copy before the box starts and
prints what changed on the way out — the difference between trusting a
report and having one.

## TLS trust and `host_ca`

The image's CA store decides which TLS the box believes. On networks that
intercept TLS with their own CA — Cloudflare WARP, a corporate proxy — the
box's fresh store refuses the interception certificate, correctly, and
tools like `cargo` cannot reach their registries.

**The better answer is not to open the connection.** Anything a recipe can
name by URL and digest belongs in
[`[[image.artifact]]`](manifest.md#imageartifact--fetched-on-the-host-proved-by-digest):
wormhole fetches it on the host, where your trust store already works, and
the build box reads bytes it never had to verify. A build that pins all
its fetches has no TLS to intercept. What follows is for what is left —
the toolchain `rustup` fetches for itself, whatever `cargo` reaches at run
time.

`[access] host_ca = true` mounts the host's CA bundle read-only over the
box's store, at `/etc/ssl/certs/ca-certificates.crt`. Wormhole finds the
bundle wherever the distro keeps it, resolves symlinks to the real file,
and prints what it trusted. The trade is the same one the host already
made: the middlebox can read and, in principle, alter the box's TLS
traffic to intercepted hosts.

It **adds** the host's roots rather than replacing the image's. OpenSSL
reads `SSL_CERT_DIR` — which defaults to the image's own
`/etc/ssl/certs`, whose hash links verify on their own — as well as the
bundle, and trusts the union. Wormhole deliberately does not set
`SSL_CERT_DIR`: naming it would drop the image's roots instead of adding
the host's to them.

### It reaches the build box too

The box that builds an image is a box like any other, and it terminates
its own TLS for whatever the recipe did not pin. Before, `host_ca` was a
`[access]` key that applied to the *running* box only — so on an
intercepting network you could run a box and never build one, and the
failure was a certificate error naming no cause.

It now applies to both. In the build box the bundle is bound at
`/run/wormhole-ca.crt`, and neither of the two paths it avoids is an
accident:

- **Not `/etc/ssl/certs/ca-certificates.crt`.** `apk add ca-certificates`
  *writes* that path while the build is running — the file belongs to
  `ca-certificates-bundle` — so a read-only bind over it would break the
  install that needs it.
- **Not `/tmp`.** That is where artifacts land, and a recipe naming
  `into = "/tmp/wormhole-ca.crt"` would be handed your certificates where
  its own digest promised its own bytes.

`/run` is a tmpfs of wormhole's own, so the finished image does not carry
your bundle either.

### Mounting a bundle is not the same as reading one

Most TLS clients do not consult that path unless they are told to, and
several ignore the variable everyone reaches for first. Measured on the
image wormhole ships:

| Client | Reads the mounted bundle on its own? | What it does read |
|---|---|---|
| Node — **and so the agent** | no, ever | `NODE_EXTRA_CA_CERTS` |
| git | no | `GIT_SSL_CAINFO`, `http.sslCAInfo` |
| cargo | no | `CARGO_HTTP_CAINFO` |
| Python `requests` | no (it uses certifi) | `REQUESTS_CA_BUNDLE` |
| OpenSSL, Go, `rustls-native-certs` | yes | `SSL_CERT_FILE` also works |

Node ships a snapshot of the Mozilla store fixed at its release, and never
looks at the filesystem — so installing a CA on your host is invisible to
it. `SSL_CERT_FILE` does not reach it, and does not reach git either: git
sets libcurl's CA path only from its own variables, and libcurl reads no
environment at all.

So `host_ca = true` sets all of those for you, pointed at the bundle it
mounted. One declaration, one path, every client. A manifest that declares
one of those variables itself means it and wins:

```toml
[access]
host_ca = true

[env.NODE_EXTRA_CA_CERTS]
fixed = "/some/other/bundle.pem"    # this wins over the mounted one
```

### If the host bundle does not have the CA either

Nothing in the box can trust a certificate the host's *merged* bundle does
not carry. A client that writes its root somewhere else — Cloudflare's
writes `/usr/local/share/ca-certificates/managed-warp.crt`, and only when
"Install CA to system certificate store" is enabled — needs the merge to
have been run:

```sh
sudo update-ca-certificates          # Debian, Ubuntu, Alpine
sudo update-ca-trust extract         # Fedora, RHEL
```

Check what actually made it in:

```sh
grep -c CERTIFICATE /etc/ssl/certs/ca-certificates.crt
```

## What a credential grant really carries

A grant is a file, but what the file *unlocks* travels with it. The
canonical example: granting `~/.claude/.credentials.json` gives the boxed
Claude Code your claude.ai login — and everything attached to that
account.

That includes hosted MCP connectors (Atlassian JIRA and Confluence,
Slack, Gmail, Google Drive, ...). Ask the boxed agent whether it can reach
JIRA and it truthfully answers yes. No JIRA traffic crosses the box
boundary: the agent asks Anthropic's servers, and the connector runs
server-side against the account's authorization. Neither the mount plan
nor [the broker](broker.md) can see or stop it, because from the box it is
just the same API endpoint the agent already needs for inference. The
broker moving the credential to the host does not narrow this: it is the
account's reach, not the box's.

So read a credentials grant as: **the box may act as this account** —
with every capability the account has, wherever that capability actually
executes. The preview shows the file; the account's connector list lives
outside wormhole's sight. If a box must not reach JIRA, the account you
hand it must not have JIRA attached: use a separate agent account for
boxed work, or detach connectors from the one you grant.

The same reading applies to any credential: `~/.ssh` is not a directory,
it is every host those keys open.
