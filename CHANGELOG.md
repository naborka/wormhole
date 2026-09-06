# What changed

Written for a person, not a machine. Newest first. `STATUS.md` is the
precise tracker; this is the plain-words version of the same story.

Wormhole runs a coding agent inside a box. The box only sees what you
explicitly hand it. Everything below is about making that sentence more
true.

## Unreleased

### Fixed: the build box lost its network

Making "no route" the default also took the route away from the build
box, so every `apk add` and `rustup` fetch failed with `temporary error`
and the build stopped at exit 14. The build has no broker to ride — it
fetches for itself, on the host's network, resolving through `[access]
dns` — and now says so explicitly instead of inheriting a default that
was written for the running box.

### The safe way is now the default way

A manifest that says nothing about access gets the design's whole
promise: no route off the machine, and the model API reached through
the host-side broker, so no credential of yours is ever inside the box.
Wormhole starts that broker for you — one per box, with that box's own
allowlist — and stops it with the box. Nothing to run by hand any more.
`network = "host"` remains as the explicit opt-out.

### The box can reach the hosts you name, and only those

New manifest key: `[access] egress = ["crates.io", "*.crates.io"]`.
Every tool that honors `HTTPS_PROXY` — cargo, git, curl, the agent's
own web fetches — reaches those hosts through the broker's CONNECT
tunnel, resolved and dialed on the host, with nothing intercepted. A
host you did not name gets a clear 403 that says exactly how to allow
it. The baseline is empty on purpose.

### `wormhole allow` — unblock a host without restarting anything

When the agent hits a blocked host, the refusal names the fix:
`wormhole allow <box> <host>`. Typed on the host, it takes effect on
the agent's very next attempt — the box, the agent and the broker all
keep running. `wormhole deny` takes a host back the same way. The box
remembers your additions across restarts; your manifest is never
edited behind your back, and nothing inside a box can widen its own
list.

### Secrets are asked for once, ever

New env kind: `[env.CONTEXT7_API_KEY] ask = true`. The first start that
finds it empty asks on the terminal (typed masked, stored host-side in
`~/.config/wormhole/secrets.toml`, mode 0600) and every later box of
every role that declares the same name just has it. One key, typed one
time, serves all your roles. `wormhole secret list | set | remove`
manages the store; values never appear in shell history or on any
screen. Credentials never belong in preflight scripts — those are for
plugins and tools, and the docs now say so.

### Installing from git works on a plain toolchain again

`cargo install` no longer demands the musl target. Where it is missing,
the in-box forwarder is built against the host's own libc, statically,
and the build refuses to embed anything that would need a loader from
somebody's image — so the fix cannot silently undo the guarantee it
bends.

### A brokered reply no longer claims to be HTTP/2

The broker's upstream leg speaks whatever the API negotiates — HTTP/2 —
and the reply's status line said so, verbatim: `HTTP/2 400`. The box's
client speaks HTTP/1.1 on its side and refuses a version its connection
never negotiated. The broker now rewrites just the version token of the
status line to `HTTP/1.1` before anything streams; status, reason and
every following byte move untouched.

### The broker's in-box forwarder now runs in any image

A brokered box used to run wormhole's own binary inside the image, as the
forwarder. Built against the host's glibc, that binary could not start in
a musl image (Alpine), so wormhole refused up front and told you to
rebuild it static. That whole trade is gone: the forwarder is now its own
tiny helper, compiled static against musl and carried inside wormhole's
binary, written into the box at start. It runs under any libc, wormhole
itself can be built however your host likes, and there is still nothing
to install. Building wormhole now needs the musl target
(`rustup target add x86_64-unknown-linux-musl`); the build says exactly
that if it is missing.

### A box's environment is now visible, updatable, and honest on attach

Starting a box with variables in front of the command looked like it
worked, then a later `attach` quietly ran without them: attach used to
carry the attacher's whole shell environment instead of the box's own.
Now the environment a start computes is written down as the box's baked
env, and every attach session gets exactly that — refreshed from your
shell, so exporting a rotated token before attaching is enough. Your
exported value wins, the box's own survives, `fixed` never moves.

New on the command line: `--env NAME[=VALUE]` on `box` and `attach`
declares or updates a variable (bare `NAME` carries the host's value, so
a secret stays out of shell history), and `wormhole env <id>` shows every
variable with its value, source and refresh rule. Every start prints one
line counting what is set and unset; an attach prints only what changed.

New in the manifest: `[env.NAME]` takes `required = true` — the box
refuses to start until the variable has a value, naming the fix — and
`secret = true`, which masks the value on every screen wormhole prints.

An attach no longer leaks the attacher's whole environment into the box:
a session's environment is the baked env and nothing else, the same
whitelist discipline a start already had.

### `wormhole help` is the whole tool on one page

There was no help. A wrong command printed one unbroken line naming every
command and flag wormhole has — the worst of both, too long to scan and too
short to explain anything. Nothing said what a box was, what went in
`wormhole.toml`, how to make a role, or how to move the agent's version.

`wormhole help` (or `--help`, or `-h`) now answers all of that in under 140
lines and 80 columns: the commands grouped by what you are trying to do, a
commented manifest, the four files a role is made of, and the update flow
end to end. Short on purpose — it is written to be read in full, by a person
who has never seen the tool and by an agent with one screenful to spare.

A refusal no longer prints the wall. It prints the usage line for the exact
command you got wrong, then one line saying where the rest is:

```
$ wormhole remove
usage: wormhole remove <id|name> [<id|name>...]
run `wormhole help` for the whole tool on one page
```

The command list has one home, and a test reads the binary's own dispatch
and fails if a command is missing from the page — or on the page and
answered by nothing. It caught one the moment it was written: `wormhole tui`
had been dispatched and undocumented since the panel shipped. A help page
that has drifted is worse than none, because it is believed.

### A box can be renamed, emptied, or taken away

Wormhole could make a box and stop one. It could not get rid of one. A
folder that had collected six boxes over a week stayed at six forever, and
the only way out was `rm -rf` on a directory under
`~/.local/share/wormhole/` that you had to work out the name of yourself.

Three commands close that:

```sh
wormhole rename api web    # call it something else, without starting it
wormhole reset api         # keep the box, throw away what is in it
wormhole remove api web    # take them away for good
```

`remove`, not `rm`: `wormhole role remove` already spells it that way, and
a CLI whose verb changes with its noun is the thing consistency is for.

`rename` sets the name you type instead of twelve hex characters. `--as`
already did that at start; this does it without starting anything, so a
box you named badly in a hurry is not a box you have to boot to fix.

`reset` is the one that is easy to miss. It empties the home and keeps the
*box* — same id, same name, same workspace, same role. That is the
difference between starting over and starting somewhere else: `--new`
gives you a second box beside the first, `reset` gives you the first one
back, empty.

`remove` takes the home and the snapshot beside it. Several at once, because
clearing up after a day's work is what it is for, and every name is
resolved before any box goes — a typo at the end of the line refuses the
whole line rather than leaving half of it done.

All three need the box idle, and none of them asks a list. They take the
box's own claim first, the same lock a start takes, so the kernel is what
answers "is this running" and no removal can ever delete a home an agent
is writing to. A running box is refused, with the `wormhole stop` line
that comes first.

A home whose record cannot be read is still a box here. Its directory name
carries the id — which is why `--id` could always start one — so `remove` now
takes one too. A box you can see and cannot get rid of was the one state
this store should never have had.

### The panel can do all of it too

`x` removes the box the cursor is on, `r` resets it. Both ask first, and
`y` is the only key that answers; every other key cancels, `q` included,
because `q` is the reflex for "no" and taking the whole panel down on it
would be the one answer nobody meant.

They call the same bodies the commands do, so what `x` does in the panel
and what `wormhole remove` does outside it cannot drift apart.

### `wormhole gc` can finally prove an image is unreferenced

Every time you bumped a pinned version, the old image stayed on disk — a
gigabyte at a time — and nothing could ever say it was safe to take.
Wormhole deletes only what it can prove, and nothing recorded what
referenced an image.

Something does now, and it was already there. An image, a base rootfs and
a fetched artifact are each named by a digest, and a recipe names every
digest the store keeps for it. So reading the recipe of every box on this
host is a proof, not a guess:

```sh
wormhole gc                            # report only
wormhole gc --delete                   # remove what is proven dead
wormhole gc --delete --unreferenced    # and what no box here starts from
```

The two flags are two different claims, and keeping them apart is the
point. *Dead* means finished with. *Unreferenced* means no box on this
host starts from it — weaker, because a recipe you have built but never
run a box from references nothing that can be counted. So a bare
`--delete` leaves those, and the report says how much more the second flag
would give back, rather than letting a gigabyte go missing from both
columns.

One recipe that cannot be read may be the very one that references an
image, so a single unreadable manifest makes the whole answer *unproven*
and nothing is taken. The image a running box is using is always kept —
under `rootfs = "readonly"` that image is the box's live root.

Lock files are reclaimed too. `wormhole remove` leaves one behind on purpose:
unlinking a lock while holding it would let another start take a second,
different lock on the same box.

### `gc` reads what is here, and nothing else

Reading every box's recipe meant reaching for the resolver a *launch*
uses — and that one may fetch a pinned commit and stop the whole terminal
for an approval. A bare `wormhole gc` could therefore hit the network, take
the screen, and exit before printing a line. Worse, its answer depended on
whether anyone was watching: at a terminal it fetched and could prove an
image referenced, from cron it could prove nothing.

Whether a resolve may fetch is now something a command says rather than
something inferred further down from whether stdin is a terminal. A launch
may; a listing may not. Same store, same answer, whoever is looking.

### A recipe can name a file by its digest, and wormhole fetches it for you

A build used to fetch its own tools. Every one of those fetches was a TLS
connection opened *inside* the build box, verified against the trust store
that box happened to have — and on a network that intercepts TLS,
Cloudflare WARP or a corporate proxy, it failed with a certificate error
that named no cause. You could run a box on such a network and never build
one.

Now a recipe says what it needs:

```toml
[[image.artifact]]
url = "https://static.rust-lang.org/rustup/archive/1.29.0/x86_64-unknown-linux-musl/rustup-init"
sha256 = "9cd3fda5fd293890e36ab271af6a786ee22084b5f6c2b83fd8323cec6f0992c1"
into = "/tmp/rustup-init"
```

Wormhole fetches it on your machine — where your resolver, your proxy and
your trust store already work — checks the digest, and binds the file into
the build box read-only. **The box opens no connection for it.** A network
that intercepts TLS has nothing to intercept, and cannot substitute the
bytes either: the digest is fixed.

This is not a new idea here, it is the old one applied evenly. The base
rootfs has always been a URL and a `sha256`, and a role has always been a
git commit whose hash is the proof. The roles in this repository were
already writing it by hand — every `wget` was followed by
`sha256sum -c -`. That pair is now one declaration, on the side of the
boundary that can be configured.

It also fixed something the digest was quietly lying about. A recipe that
said `npm install -g @anthropic-ai/claude-code` installed whatever was
newest that day, under a digest that claimed two images were the same. The
shipped manifests now pin the version, and install it with `--offline` —
which is the proof rather than the optimisation: if anything still wanted
the registry, the line would fail instead of quietly fetching.

Three rules, each refused rather than worked around: `into` must be under
`/tmp`, which is a tmpfs, so the finished image carries what the artifact
*did* and not the artifact; the file is read-only with no execute bit,
because bytes off the network are not something to run by accident; and an
artifact no `build` line could use is refused, because it would put two
byte-identical images under two digests.

### `host_ca` reaches the box that builds the image

`[access] host_ca` applied to the *running* box only. The box that builds
an image is a box too, and terminates its own TLS for whatever a recipe
cannot pin — the toolchain `rustup` fetches for itself, whatever `apk`
resolves. So the key that exists for intercepted networks was absent from
the one place those networks broke first.

It now applies to both. In the build box the bundle is bound at
`/run/wormhole-ca.crt`, and both paths it avoids are deliberate.
Not `/etc/ssl/certs/ca-certificates.crt`: `apk add ca-certificates` **writes**
that path while the build is running, so a read-only bind over it would
break the install that needs it. And not `/tmp`, which is where artifacts
land — a recipe naming that path as an `into` would be handed your
certificates where its own digest promised its own bytes.

Two corrections came with it. `host_ca` **adds** the host's roots to the
image's rather than replacing them — OpenSSL reads `SSL_CERT_DIR` as well
as the bundle and trusts the union — and the manifests that claimed the
box then trusts "what the host trusts and no more" were saying something
the code does not do. Wormhole deliberately leaves `SSL_CERT_DIR` alone:
naming it would drop the image's own roots instead.

### A build says what it is doing, and checks what `apk` left behind

A build that died printed `exit 1` and named no command. Every step now
announces itself first, so the last line printed is the one that failed.

And after `apk add`, the build refuses an image whose CA store the
packages left empty. Alpine's `ca-certificates` trigger runs
`update-ca-certificates` with its output discarded and `exit 0` after it,
so a bundle it failed to write is reported as a successful install — and
every later `https` fetch fails with a verify error naming no cause.

`wormhole gc` learned to report cached artifacts beside bases and images:
one of this repository's own is 100 MB, and a cache nothing can see is a
cache nobody reclaims.

### `host_ca = true` now means what its name says

Mounting the host's CA bundle over the box's was only half the job, and it
was the half that did not matter for the agent. Node ships a snapshot of
the Mozilla root store fixed at its release and never reads a bundle off
the filesystem at all — so on a network that intercepts TLS with its own
CA (a corporate proxy, Cloudflare One), Claude Code went on refusing to
connect while `host_ca = true` sat in the manifest looking like it had
done something.

It now also points every TLS client in the box at the bundle it mounted:
`NODE_EXTRA_CA_CERTS`, `GIT_SSL_CAINFO`, `CARGO_HTTP_CAINFO`,
`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`. There is no single
variable that covers them — git ignores `SSL_CERT_FILE` *and*
`CURL_CA_BUNDLE`, because libcurl reads no environment at all — so all of
them are set, at one path, from one declaration. A manifest that declares
one of those itself means it and wins.

This cannot help with a CA your host's *merged* bundle does not carry.
`sudo update-ca-certificates` is what puts one there;
[what the box can reach](guide/access.md) says how to check.

### A workspace can name its role

A project either wrote out a whole recipe or everyone working on it typed
`--role X` by hand, every time, correctly, forever. There was no third
option. Now there is, and it is one line:

```toml
version = 1
role = "github:you/alphaca-java@<40 hex characters>"
```

`wormhole box` — nothing else — then starts that role, for you and for
everyone who clones the repository. The pin lives in version control and
moves in a pull request rather than in somebody's shell history.

It is not inheritance: `role` names the recipe *instead of* carrying one,
and a manifest holding both is refused rather than merged. A launch still
fetches nothing and asks nobody, so a fresh clone naming a pin you have not
approved refuses and names the `role add` that fixes it.

### Boxes can have names

An id is twelve hex characters, derived and unmemorable. Now a box can be
called something:

```sh
wormhole box --new --as api
wormhole attach api
wormhole stop api
```

Everything that takes a box takes both forms. Which one you typed comes
from the string itself, which is why a name may never be twelve hex
characters — that is what an id is, and one string cannot mean two boxes.
`wormhole ps` shows the name in a new `ALIAS` column, beside the `NAME` the
manifest gives, because several boxes share the latter and only the former
names exactly one.

### A `git worktree` does work, if it is inside the workspace

The handbook said a worktree "will not do", because its `.git` is a line of
text pointing back into the main checkout and the box sees only the
workspace it was started in.

That is right about a worktree kept *outside* the workspace and wrong about
one kept inside it. Inside, the path that line names is inside the mount,
and every git command works. It is also the better answer to the problem
that section raises — two boxes sharing one `.git/index` — because each
worktree has its own index, its own `HEAD` and its own branch, while one
object store is still shared where a clone would copy it.

```sh
git worktree add wt/feature-a
git worktree add wt/feature-b
```

### `d` in the panel says what it did, and what it could not do

Pressing `d` on a box that was not running drew the panel again and said
nothing — exactly what the panel does for a key it has never heard of.
Most rows in a panel are idle, so `d` read as broken.

It now says so, under the hints, until the next key. Pressing `d` on a box
that *is* running says that too: the kill lands when the kernel gets to
it, so without a word the row could still read `running` on the next
redraw and look like nothing had happened. A stop that fails now says why
on the panel instead of being swallowed.

### `wormhole stop <id>`

Stopping a box used to need the panel — one key press, in a screen you had
to be sitting at. A box outlives the terminal that started it, so ending
one now takes the same id everything else does:

```sh
wormhole stop a3f9c1e40b2d
```

### One role is one box, however you spell it

`--role alphaca` and `--role ./roles/alphaca` name one directory. They used
to give you two boxes, with two homes, and the second could not see the
first's history, logins or installed toolchain. Nothing said so.

A role is now identified by **where it comes from** — the canonical
directory of a local one, the repository URL of a fetched one — and that
is what decides which box a start resumes. Every spelling of one role is
one box. Re-pinning a fetched role to a newer commit keeps the box you
were working in, which is what installing it under a name already did.

Homes kept before this are matched on the reference they recorded, exactly
as they used to be, so nothing is orphaned; the first start under this
version writes the identity in.

### Roles you wrote get a name from one command

Naming a local role meant `mkdir -p` and `ln -s` into wormhole's own
config directory, because `role add` read its argument as a repository
before anything else and asked a directory for a commit.

```sh
wormhole role add ./roles/alphaca
wormhole role list
wormhole role show alphaca
wormhole role remove alphaca
```

`add` links the directory rather than copying it, so the folder you edit
stays the role. Nothing is fetched and nothing is approved: there is no
commit here to approve, and a directory can change a second after any
answer, so a gate would be a promise wormhole cannot keep. It prints the
recipe instead — and `role show` prints it again whenever you want it,
which is also the only way to read an approval screen after answering it.

`role remove` replaces the `rm -r` the handbook used to give. It unlinks
the name and never follows it, so taking a name back leaves the directory
you work in where it was.

### A bare name that misses looks next door

A word with no `/` is never read as a directory. If a role of that name is
sitting in the folder you are in, the refusal now says so:

```
$ wormhole box --role alphaca
wormhole: no role alphaca in ~/.config/wormhole/roles
  there is a role at ./alphaca — start it with `--role ./alphaca`,
  or name it with `wormhole role add ./alphaca`
```

One rule about what may name a role is also now checked where roles are
listed, not only where they are written: a directory placed under the
roles folder by hand, under a name `role add` would have refused, is no
longer offered.

### `wormhole init`

The first wall was before anything interesting: step one of the quickstart
was "write a manifest", and the smallest one it printed carried a rootfs
URL and its sixty-four hex characters to source by hand.

```sh
wormhole init
```

writes a working one — Alpine, Node, Claude Code, your credential and
nothing else of your host. It never writes over a manifest already there.
There is one starter recipe, in `templates/wormhole.toml`, and the
handbook prints that file rather than a copy of it.

### Roles can live in a repository of their own

`wormhole role add github:you/some-role@<commit sha>` installs a role that
lives somewhere else. wormhole fetches that exact commit, shows you
everything it would do, asks, and then it is a role like any other:
`wormhole box --role some-role`.

**A commit, never a branch.** A ref with no `@<sha>` is refused where you
type it. `git` checks every object it downloads against its own hash, so
the commit is the proof — the same forty characters mean the same bytes
forever, and nothing about the connection has to be trusted. A branch
would mean somebody else's push silently changes what runs on your
machine.

**You see what runs, before it runs.** A role's recipe carries shell
commands that build its image on your machine. The screen you approve now
shows all of them, along with the base it starts from, its packages, the
host paths it wants, and its environment. That screen was always missing
the build lines — the part that actually executes — so it showed the
grants and hid the point. It does not any more, for roles you wrote as
well as roles you fetched.

Approval is per commit and covers the whole machine, because a commit
names one set of bytes wherever it is used. Re-pinning a name you already
have shows the old commit, the new one and the fresh recipe, and asks
again.

**Nothing fetches or asks at launch.** By the time you start a box,
everything human is already behind you, so a role from a repository works
from a script or a timer exactly like a local one. If the commit is not on
this machine, or nobody has approved it, the launch refuses and prints the
`wormhole role add` line that fixes it. Typing a ref straight at `--role`
is the exception — that is you, at a terminal, so it fetches and asks.

Worth being plain about what this does **not** do: nothing limits what a
role may ask for. A role that asks for your whole home gets it if you
approve it. Read the screen.

### The handbook is the only documentation

Every document this repository carries is now a page in the handbook. The
working documents at the root — the concept, the plan, the status, this
file — were already rendered into it; the instructions every box hands its
agent and the project's own vocabulary are now pages too, so there is one
place to read and no second copy to drift.

The research notes on autonomous groups are gone with the feature they
described.

### Only one role ships in this repository

`alphaca-java` and `geohod-engineer` have moved to repositories of their
own — `naborka/wormhole-alphaca-java` and
`naborka/wormhole-alphaca-geohod-engineer` — and `alphaca` stays because
it is the role wormhole itself is built with. Roles were never meant to
live in the tool's own repository; now they do not have to.

### Groups are gone

`wormhole group`, `task`, `hall` and `answer` have been removed, together
with the hall protocol, the member wrapper, the roster, and the crew that
was built on them. `--hall`, `--group` and `--member` are no longer flags
on `wormhole box` or `wormhole run`, and `wormhole ps` no longer has a
GROUP column.

They did not work. Rather than leave a feature in the tool that reads as
finished and is not, the whole of it comes out — nothing about groups is
half-present. Everything that made a single box what it is stays exactly
as it was; the earlier entries below describing group work are kept
because they are what happened, not what is here now.

A box registry entry written by the older wormhole still carries a `group`
line. It keeps parsing, so a box running across the upgrade stays
listable.

### The box is told the truth about your terminal

`TERM` is only a name. Every full-screen program looks it up to find out
what the terminal can do, and the box carries only the descriptions its
image ships — which is why the box used to be told it was in an
`xterm-256color` whatever you were really running. Close, but not true, and
programs that ask about the difference got the wrong answer.

Wormhole now copies your terminal's own description out of your machine's
terminfo database into the box home on every start, so your real `TERM`
reaches the box and resolves inside it. Declare `TERM` with `default` now,
not `fixed`; the `default` is what the box is told when there was no
description to copy.

`wormhole attach` does the same. It had the opposite half of the bug:
attaching handed the box the attaching terminal's name with no description
behind it at all, so a second terminal into a box was the worst case of
both.

### The Rust boxes get a Rust that can build current code

`roles/alphaca` and `roles/crew` took their compiler from Alpine's package
list, and Alpine pins one compiler per release: 3.22 is 1.87 and always
will be. A box built from those roles could not compile a project asking
for anything newer — including wormhole itself.

Both now install rustup instead, pinned by digest like every other download
in those recipes, with `rustfmt`, `clippy` and `rust-analyzer` from the same
toolchain. Bumping the compiler is one line in the recipe.

Your crate cache still lives in the kept home, so it survives between boxes.
Nothing changes in how you use the box: `cargo`, `rustc` and the rest are on
`PATH` exactly as before.

Rebuild to get it: `wormhole build --role alphaca`, or just start a box —
the recipe changed, so the next start builds the new image.

### You no longer have to build before you start

`wormhole box` used to stop on a fresh project and tell you to run
`wormhole build` first. It now builds the image itself and says so:

```
no image for this manifest yet; building it
fetching https://…/rootfs.tar.gz
```

`wormhole group` does the same for every role its members need, after its
refusals and before the first box — so a group that cannot run does not
make you wait for a fetch to find that out.

`wormhole build` has not gone anywhere. It is now the way to pay that cost
when it suits you rather than at your next start.

Start two boxes at once on an image nothing has built and the second waits
for the first, then uses what it built. Before, both would have assembled
the same half-finished directory.

### A group can keep working when nobody is talking to it

A group used to move only when you sent it something. Now a roster can say
`tick_secs = 1800`, and when nothing is in any inbox and nothing on the
board is assigned, the supervisor wakes the lead and asks what is next.

The lead has three answers and all three are allowed: assign the next
thing, ask you about something, or say there is nothing left to do. The
last one writes a file, and while that file is there the group stays
asleep — so a group with nothing to do stops instead of inventing work.
Sending it something (`wormhole task`, or answering a question) is what
wakes it again.

Leave `tick_secs` out and nothing changes: the group answers you and
nobody else.

### A group can ask you a question, and wait as long as it takes

Any member that hits a decision a person should make writes it down and
stops. `wormhole hall` shows it above everything else, with the command
that answers it:

```
waiting on you:
  q3  from arch
    Add billing to the concept, or keep this release read-only?
    answer: wormhole answer crew q3 "…"
```

```sh
wormhole answer crew q3 "Keep it read-only. Billing is next quarter."
```

Waiting costs nothing — no process is parked and no tokens are spent,
because a member does not exist between turns. Your answer is what brings
it back, with its own question repeated inside it, since nothing on the
other side remembers asking.

While a question is open the group holds: it will not wake itself past a
decision it just said was yours, and it stops chasing the member that
asked.

### One member can be trusted with something the others are not

A roster entry can now name grants of its own:

```toml
[[members]]
name = "ship"
grants = ["~/.config/gh"]
```

That member sees that path and no other member does. It is what lets four
agents share one role and one built image while only one of them can merge
a pull request or run a release — so the members that read a repository
full of somebody else's text are not the members that can ship it.

### A group can be one box, and one is where to start

The measured evidence says extra agents make a coding task *worse*, so a
group of one is now a first-class shape rather than a degenerate case. It
gets everything: it wakes itself when idle, it can ask you and wait, and
it is told it is alone — before, it was told to split the work between
colleagues it did not have, and to read a roster listing only itself.

Add a second box for what it *is* — a credential the first one does not
hold, a context that has not read the code — not for what you hope it
will do better. The crew page opened with the numbers.

### The crew: a worked example that takes a concept to merged code

`roles/crew/` and `groups/crew/` are four boxes — plan, build, review,
ship — with a setup wizard and a mission file you can edit. It was meant
to be copied and changed.

### Work is no longer lost when an agent cannot start

A member's turn that failed used to look exactly like one that worked. The
message was filed as done, the work vanished with it, and the group went
quiet with nothing saying why. A closed usage window or an expired login
was enough.

Now a failed turn keeps its message and tries again, waiting longer each
time. After three failures the member leaves a note where you will find
it. Nothing is thrown away, and a group that is stuck says so.

A member of a group that reaches the API through the broker also works
now; before, it was pointed at a door nothing had opened.

### Give each box a clone, not a worktree

The guides used to suggest `git worktree` for a second box on the same
project. That does not work, and now they say so: a worktree keeps its
real git directory inside the main checkout, which the box cannot see, so
git inside answers `fatal: not a git repository`. Use `git clone`.

### A folder can hold as many boxes as you want

Until now one folder meant one box. A second `wormhole box` in a tree that
already had one was refused, so two agents on the same code meant two
checkouts whether you wanted them or not.

That limit is gone. A box is now a kept thing with a name — its own home,
its own history, its own logins, its own installed tools — and you can
have as many as you like, in the same folder or anywhere else.

```sh
wormhole box                 # first time here: makes a box
wormhole box                 # later: back to that same box
wormhole box --new           # another box, beside the first
wormhole ps --all            # every box you have, running or not
wormhole box --id a3f9c1e40b2d   # that exact box, whenever
```

A plain `wormhole box` goes back to the box you used last here, so typing
it twice does not quietly leave you with two. Making another is something
you ask for.

The panel (`wormhole` on its own) now lists every box, not only the
running ones. Enter does the one thing that row allows: join a box that is
running, start one that is not — in its own folder, wherever that is.

What is still refused is starting the *same* box twice, because two of
them would be writing one history and one set of settings at once.

Boxes in one folder do edit the same files, with nobody coordinating
them. That is the point, and it is worth knowing where it bites: whoever
saves last wins, and one agent running `git commit -a` will sweep up the
other's half-finished work. If you want the parallelism without that, give
each box its own `git worktree`.

Nothing to do on upgrade. Homes kept by an older wormhole are picked up as
their folder's first box, with everything in them.

### The recipe file is shorter and says what it means

`wormhole.toml` used to be a flat list of twenty-odd keys with no order to
them, so nothing on the page told you which ones cost you a rebuild. Now
each key sits in a table named after what it decides:

```toml
version = 1
name = "Alphaca"

[image]     # what gets built — change a line here and it rebuilds
[agent]     # who runs, and what it reads before starting
[access]    # what the box can reach
[runtime]   # how the box is made, and what it leaves behind
[limits]    # what it may use of the machine
[env.NAME]  # one table per environment variable
```

That grouping is the answer to the question people kept asking: editing
`[image]` rebuilds, editing anything else does not.

Several keys were renamed to say what they are. `rootfs` and `sha256`
became `[image] base` and `base_sha256`, so it is clear which file the
digest belongs to. `repositories` became `package_sources` — they were
never git repositories. `setup` became `build`, because that is when it
runs. `trust_host_ca` became `[access] host_ca`. In an environment
variable, `value` became `fixed`, which is what it actually does: ignore
the host.

`version` is now the number `1` instead of `"v1alpha1"`. It earns its
place on exactly one case — a file written for an older wormhole now says
so plainly:

```
wormhole: this wormhole reads manifest version 1, not "v1alpha1"
```

Existing manifests need rewriting; there is no automatic migration. Both
shipped roles and the example in the [manifest
reference](guide/manifest.md) are already in the new shape.

### The credential can now stay on your machine

Until now, giving a boxed agent access to Claude meant handing it your
login file. The agent held your credential, and it opened its own
connection to the internet.

Both of those are gone if you want them gone. A new command,
`wormhole broker`, runs on your machine and keeps the credential. The box
gets a private pipe to it instead — no login file, no address to dial. Ask
for it with two lines in the recipe:

```toml
[access]
broker = true
network = "none"
```

`network = "none"` means the box has no way off the machine at all. Not
"blocked" — there is simply no road. `broker = true` is what still lets the
agent talk to Claude anyway. Together they are the point the whole project
was built for.

### Several agents can now work as a team

A **group** is several boxes started together, each with its own role, its
own copy of the code, and its own memory. They share exactly one folder —
the hall — and they talk by dropping files into each other's inboxes.

The nice part: an idle member costs nothing. Nobody sits in a loop asking
"anything for me?". A member sleeps until a file lands in its inbox, does
that one job, and goes back to sleep.

```sh
wormhole group trio                  # start the team
wormhole task trio "plan the migration"   # give it work
wormhole hall trio                   # see who is doing what
```

`wormhole task` hands work to the team lead by default, or to a named
member with `--to`. `wormhole hall` is how you find out whether a team is
busy or quietly stuck.

Every member is now *told* what the shared folder means — its own name, who
the others are and what each is for, how to send a message, where to leave
a finished result. Before this, the plumbing worked but nobody had been
handed the instructions.

One supervisor watches the whole thing. If a job passes its deadline with
no result, the owner gets a reminder; if it passes twice the deadline, the
lead is told it is stuck. Each of those is said once, not every thirty
seconds.

Members can differ by model without differing by setup: a top model for the
lead, a cheap one for small mechanical jobs. That used to mean building a
separate multi-gigabyte image per member. Now it is one line.

### You can cap what a box uses

```toml
[limits]
cpu = "1.5"
memory = "512M"
pids = 256
```

If a cap cannot be applied, the box does not start. A recipe that asked for
a ceiling and quietly got none is exactly the failure a ceiling exists to
prevent.

### You can get a receipt for what the agent touched

`snapshot = true` takes a copy of your project before the box starts and
prints what actually changed when it exits:

```
workspace: 3 files changed
  added   src/broker.rs
  changed Cargo.toml
  removed notes.txt
```

Off by default, on purpose: on some filesystems an honest snapshot means
physically copying your whole project every single time, and that is not a
cost to charge you without asking.

### Boxes can start much faster

`root = "readonly"` skips the per-box copy of the image entirely. Start-up
stops getting slower as the image gets bigger. The trade is real and worth
knowing: a box in this mode cannot install new software while it runs.

### Disk space you can get back

```sh
wormhole gc            # show me
wormhole gc --delete   # take it back
```

It only removes what it can **prove** is dead — a box that has exited, a
saved home whose project folder is gone. Images are listed with their size
and never deleted, because nothing yet records which recipe built which
image, and guessing wrong costs an hour-long rebuild.

### Java and Kotlin work

A second shipped role, `alphaca-java`, with JDK 21 and 17, Gradle, Maven
and a language server. Same persona as the Rust one, different toolbox.

### Safety fixes worth naming

- **The start-up banner told the truth again.** Every box prints one line
  saying what it can reach. A box using the broker was printing "reaches
  nothing" while it was reaching Claude through the broker. The line is now
  built from the whole launch instead of three hand-picked pieces of it, so
  a future way out cannot be added without appearing there.
- **Two boxes can no longer share one project folder.** The claim is now
  held by the operating system for the life of the process, so a box killed
  at any moment leaves nothing stale and nothing another box can misread.
- **The box checks its own work.** After starting, it reads back its own
  mounts and refuses to run if anything writable was not in the plan; it
  reads back its own privileges and refuses if any were left behind. The
  plan being correct was never proof that the box matched it.
- **One question to the usage server, not six.** A team of six boxes used
  to each ask separately how much of your quota was left. Now one asks and
  the rest read the answer.
- **A single bad line no longer stops the watchdog.** A malformed line on
  the team's task board used to disable the supervisor entirely. It now
  names the bad line and reads the rest.
- **The box now knows what terminal it is in.** `TERM` was being taken from
  your machine, so a host running Ghostty or kitty named a terminal nothing
  inside the box had ever heard of, and every full-screen program in there
  — `less`, `vim`, `htop` — was left guessing. The box now names its own,
  and `COLORTERM` and `TERM_PROGRAM`, which need no lookup table, carry the
  real terminal's identity and its colours in instead. A test keeps every
  shipped manifest honest about this.

### Handbook

The manifest reference was missing five settings that the code has been
reading for a while — `network`, `broker`, `root`, `snapshot` and
`[limits]`. All five are documented now, with what each one trades away.

New page for the broker. The security page now covers what the box can
*dial*, not only what it can *see*. The groups page no longer opens with a
warning that contradicts its own ending.

## Earlier

**A box at all.** One recipe file, `wormhole.toml`, builds an image and
runs a coding agent inside it, with permission prompts off — because the
box, not the prompt, is what holds the line. The agent sees your project
folder, the paths you named, and nothing else.

**`wormhole doctor`** checks your machine and says plainly what will and
will not work.

**Roles** let one recipe be used from any project folder. **The panel**
(just type `wormhole`) lists running boxes, previews exactly what a new one
would be able to see, and starts it. **`wormhole ps`** and
**`wormhole attach`** show what is running and open a second terminal into
it. **`wormhole usage`** shows what is left of your Claude quota, and the
same reading appears in the agent's status bar while you talk to it —
fetched on your machine, so the box needs no credential to display it.
