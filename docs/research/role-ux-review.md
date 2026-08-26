# Role and box UX: what is hard, and what to do about it

A review of the mental model a wormhole user has to hold, the defects that
make it harder than it needs to be, and the changes that would fix them.

Evidence is `file:line` in this repository, or a transcript of the built
binary run against a throwaway `HOME`.

Prior art from other tools is in [role-ux-prior-art.md](role-ux-prior-art.md).

## 0. The finding in one paragraph

The defaults are simple and they are right: a user answers two questions —
which recipe, and which box — and both have a default that means `wormhole
box` alone keeps working forever. The weight is not in the model; it is in
roles, and most of it is accidental rather than earned. `--role` takes
three forms that look like three spellings of one thing and are not: which
one you type decides which box you resume (D1), whether the panel can see
your role at all (D6), and whether `role add` will even talk to you (D2).
Fix that and the three forms become what a user already assumes they are —
equivalent ways to name one role. What is left over after that is the
pin, the checkout and the approval, and that part is earned: it is the
price of running a stranger's shell on your machine, and it cannot be made
smaller without making it dishonest.

**So: not "is the model too big" — it is not. It is "the parts that look
interchangeable are not interchangeable."**

## 1. Measuring the load

### Six keying schemes

Everything wormhole keeps on disk is filed under a key. There are six, and
five of them are derived from content or from a path:

| Thing | Keyed by | Where |
|---|---|---|
| base | the tarball's `sha256` | `data/wormhole/bases/<sha256>` |
| image | digest of the `[image]` table | `data/wormhole/images/<digest>` |
| checkout, approval | the commit sha | `data/wormhole/checkouts/<sha>`, `<sha>.approved` |
| box — home, lock, snapshot | `sha256(workspace path [+ ordinal])[..12]` | `data/wormhole/homes/<basename>-<id>` |
| installed role | a name the user chose | `config/wormhole/roles/<name>` |
| **which box a role resumes** | **the string the user typed after `--role`** | `box.toml` → `role = "…"` |

The last row is the only key that is neither content nor path. It is raw
user input compared with `==` (`crates/wormhole-core/src/home.rs:95`), and
it is the one that misbehaves. See defect D1.

### Three forms for one flag

`--role` accepts a pinned git ref, a directory path, or an installed name,
told apart by punctuation (`crates/wormhole/src/main.rs:1434`):

1. starts with a transport (`https://`, `ssh://`, `file://`, `git@`, `github:`) → remote ref
2. contains a `/` → a directory, right there
3. otherwise → a name under `~/.config/wormhole/roles/`

Rule 3 is the trap: a bare word is **never** a directory in the current
folder, however plainly one is sitting there. See defect D4.

### Ten public commands, and one that is not documented

`doctor`, `build`, `box`, `role`, `gc`, `ps`, `usage`, `attach`, the bare
panel, and `run`. The usage string (`crates/wormhole/src/main.rs:1537`)
advertises all of them.

`wormhole run` appears in no page of the handbook — only in `STATUS.md`,
which also records that it "inherits the host's environment apart from
`HOME`, `HOSTNAME` and `PATH`" (`STATUS.md:282`) where `box` passes only
what the manifest declares. It is the longest entry in the help text and
the one a user is least equipped to use safely.

`wormhole tui` is an undocumented alias for the bare panel
(`crates/wormhole/src/main.rs:56`); it appears in no doc and no help text.

### The first wall is before roles ever come up

There is no `wormhole init`. The quickstart's step 1 is "write a manifest",
and the minimal one it prints carries a rootfs URL and its 64-character
`base_sha256` (`docs/src/guide/quickstart.md:12`). A new user must source
both by hand before anything runs.

## 2. Defects

Five confirmed by running `target/debug/wormhole` with `HOME`,
`XDG_CONFIG_HOME` and `XDG_DATA_HOME` pointed at a throwaway directory.

### D1 — two spellings of one role are two boxes, silently

A box's record stores `--role` exactly as typed
(`crates/wormhole/src/main.rs:729`), and resume matches it with `==`
(`crates/wormhole-core/src/home.rs:95`).

So this workspace ends up with two boxes and two homes for one role:

```
ID            NAME     STATE  ROLE       AGENT   LAST USED  WORKSPACE
465c1c07d375  Alphaca  idle   ./alphaca  claude  …          …/ws
b7c08b2a8c33  Alphaca  idle   alphaca    claude  …          …/ws
```

Same role directory. Different spelling. The history, logins and installed
toolchain of the first are unreachable from the second, and nothing says so.

The same happens across a re-pin typed straight at the flag —
`--role github:you/r@<sha1>` then `@<sha2>` are two strings, so two boxes —
while the installed-name form keeps one home across the same re-pin,
because the string `r` never changed.

The doc comment says the intent is role *identity*: "a box's home carries
that role's toolchain and persona" (`crates/wormhole-core/src/home.rs:86`).
Sensitivity to spelling is not that intent; it is an accident of storing
the reference where the identity belongs.

**Root cause.** `role_source_dir` already resolves all three forms
(`crates/wormhole/src/main.rs:1434`) and then throws the result away. The
raw reference travels on to `write_record`. There is no resolved-role type,
so nothing forces the two to agree.

### D2 — `role add` on a local directory gives nonsense advice

```
$ wormhole role add ./alphaca
wormhole: ./alphaca names no commit; a remote role is pinned, never followed: add @<40 hex characters>
```

`role_cmd` parses its argument as a remote ref before anything else
(`crates/wormhole/src/main.rs:122`). Telling a user to append a commit sha
to a directory path is advice that cannot be followed.

Installing a local role is possible today, but only by hand — the handbook
instructs `mkdir -p` and `ln -s` into wormhole's own config directory
(`docs/src/guide/roles.md:147`).

### D3 — hand-installed roles bypass the name guard

`role add` refuses a name that is not letters, digits, `-` or `_`
(`crates/wormhole-core/src/source.rs:143`), because the name becomes a
directory. `installed_roles()` applies no such filter — it accepts any
entry whose directory holds a manifest or a pointer
(`crates/wormhole/src/main.rs:1165`). A hand-made `ln -s` under a name
`role add` would have refused is listed and startable:

```
$ ln -s ~/work/alphaca ~/.config/wormhole/roles/bad.name

$ wormhole role add github:a/b@000…0 --as bad.name
wormhole: "bad.name" cannot name a role; names are letters, digits, - and _, so pass --as <name>

$ wormhole build --role bad.name
manifest: role bad.name (…/.config/wormhole/roles/bad.name)
no image for this manifest yet; building it
```

One rule about what may name a role, two answers.

### D4 — a bare name never looks at the directory of the same name

```
$ ls
alphaca/          # holds wormhole.toml

$ wormhole build --role alphaca
wormhole: no role alphaca in …/.config/wormhole/roles; `wormhole role add` installs one from a repository

$ wormhole build --role ./alphaca
manifest: role at ./alphaca      # works
```

The refusal names the config directory and a command that only installs
from repositories (`crates/wormhole/src/main.rs:1470`). It never mentions
the role one character away.

### D5 — `role` has one verb

`add`, and nothing else (`crates/wormhole/src/main.rs:114`). No `list`, no
`show`, no `remove`. The handbook's removal instruction is `rm -r`
(`docs/src/guide/roles.md:63`), and the only lister is the panel.

### D6 — the panel cannot start a role that was not installed

`n` offers the workspace manifest and installed roles only
(`crates/wormhole/src/main.rs:1112`), which the handbook states plainly:
"A `--role ./path` does not — it was never installed, and nothing knows to
look there" (`docs/src/guide/roles.md:161`).

This is the real cost of D2. The discoverable entry point — running
`wormhole` with no arguments — is blind to the form the handbook recommends
for trying a role out and for working on one you are writing
(`docs/src/guide/roles.md:129`).

### D7 — the base tarball and its digest are written out four times

`wormhole.toml:11`, `roles/alphaca/wormhole.toml:17`,
`docs/src/guide/manifest.md:40` and `docs/src/guide/quickstart.md:12` each
carry the same URL and the same 64 hex characters. Nothing checks that the
four agree. When Alpine 3.22.2 is superseded, three of them go stale
quietly and the two in the handbook are what a new user copies.

## 3. What to change

Ordered by whether the current behaviour is *wrong* or merely *heavy*.

### Wrong today

#### R1 — resolve `--role` to an identity, and match on that (fixes D1)

Give the resolved role a type, so the reference the user typed and the
thing it names can never drift apart:

```rust
/// What `--role` named, once it has been resolved to something on disk.
enum Role {
    /// A directory on this machine, canonicalised.
    Local(PathBuf),
    /// A repository. The commit is which version; the URL is which role.
    Remote { url: String, sha: String },
}
```

An installed name resolves to one of these — `Local` through the symlink,
`Remote` through the pointer — so the name itself never reaches the record.

What goes in `box.toml` for matching:

| Form | Identity |
|---|---|
| no `--role` | none — the workspace's own manifest |
| `Local(p)` | the canonical path of `p` |
| `Remote { url, .. }` | `url`, **not** the sha |

The URL and not the sha, because that is what the installed-name form
already does: re-pinning keeps the home, which is the behaviour worth
having and the one the handbook describes. It also matches a rule the code
already holds elsewhere — "two names for one commit share its checkout and
its approval" (`crates/wormhole/tests/role_source.rs:310`). Identity is
where a role comes from, not what you called it.

Canonicalisation is for the identity only. The directory handed downstream
stays exactly as it is today, because `instructions` and `hooks` resolve
against it (`crates/wormhole/src/main.rs:1256`) and rewriting it would
change which files a launch reads.

**Migration.** Existing homes hold `role = "alphaca"`. Writing an identity
where a reference used to be would orphan every one of them — the exact
harm this fixes, caused by the fix. So keep two fields with two jobs:

- `role` — the reference as typed, for the `ROLE` column and for nothing else
- `source` — the identity, for matching; `#[serde(default)]`, so an old
  record parses

`resumable` matches on `source` when the record has one and falls back to
comparing `role` when it does not. An old home is picked up once by the
old rule and rewritten with a `source` on that start.

`Record` already carries `#[serde(default)]` on every optional field and
refuses no unknown key (`crates/wormhole-core/src/home.rs:39`), so the new
field parses old files, and the existing `resumable` tests
(`crates/wormhole-core/src/home.rs:248`) pass unchanged under the fallback.

There is precedent for exactly this. When groups were removed, the
changelog records that "a box registry entry written by the older wormhole
still carries a `group` line. It keeps parsing, so a box running across the
upgrade stays listable" (`CHANGELOG.md:83`). Adding a field is the same
move in the other direction.

**Pre-attack.** Two names deliberately installed from one repository would
now share a home. That is the intended reading of identity-by-source, and
their manifests are the same bytes, so the toolchain is the same — but it
is a behaviour change and belongs in the changelog. `canonicalize` also
fails on a path that does not exist; the resolver must already have
established the directory holds a manifest before it is used as identity.

#### R2 — `wormhole role add <dir>` (fixes D2, and D6 with it)

Take a local directory, not only a ref. Symlink it into
`config/wormhole/roles/<name>`: it is the smallest thing that works,
resolution needs no new branch, and edits to the original stay live —
which is the whole reason people use local roles.

- name defaults to the directory's basename, `--as` overrides, both pass
  `is_usable_name`
- the same slot rules as a remote add: refuse to write over a hand-written
  role, show old → new when replacing
- **print** the recipe preview; do not gate on it. Nothing was fetched, and
  a directory can change one second after any approval, so an approval
  here would be a lie. Print what was installed and say nothing about trust.

This closes D6 without touching the launch path: once the role can be
installed with one command, the panel lists it like any other.

**Pre-attack.** `role remove` must `remove_file` a symlink and never
recurse — a recursive delete through the link would eat the user's role
source. That hazard arrives with this change and must be handled in R5.

#### R3 — a bare name that misses should look next door (fixes D4)

On `RoleKind::Absent`, stat `./<name>/wormhole.toml` before giving up:

```
wormhole: no role alphaca in ~/.config/wormhole/roles
  there is a role at ./alphaca — start it with `--role ./alphaca`,
  or name it with `wormhole role add ./alphaca`
```

One extra stat, on the error path only.

#### R4 — apply the name guard where roles are listed (fixes D3)

`installed_roles()` filters by `role_kind` and not by `is_usable_name`
(`crates/wormhole/src/main.rs:1165`). One rule about what may name a role,
enforced in one place, checked by both the writer and the reader.

#### R5 — finish the `role` verbs (fixes D5)

```
wormhole role add <dir|ref> [--as <name>]
wormhole role list
wormhole role show <name|dir|ref>
wormhole role remove <name>
```

- `list` — name, kind, where it points, and whether it is ready to start.
  Today the panel is the only lister and it cannot be piped.
- `show` — the same preview screen `add` puts up, without installing.
  Gives `--role` a dry run, and makes the approval screen reachable again
  after the fact, which today it is not.
- `remove` — replaces `rm -r` in the handbook (`docs/src/guide/roles.md:63`).
  `remove_file` on a symlink; recursive delete only on a real directory.

### Heavy today

#### R6 — `wormhole init`

The first wall is before roles are reached at all: a new user must produce
a rootfs URL and its digest by hand (`docs/src/guide/quickstart.md:12`).

Ship one minimal starter manifest in the repository and have `init` copy
it. Offline, deterministic, and the digest is then maintained in one place
under CI like anything else pinned — which also fixes D7, if the handbook
includes that file instead of retyping it.

The handbook can already do that. `docs/src/concept.md` is a two-line stub
around `{{#include ../../CONCEPT.md}}`, so mdBook is reaching outside `src`
in this repository today; the manifest reference and the quickstart could
include the starter manifest the same way.

Not the repository's own `roles/alphaca`: it grants `~/.ssh`
(`roles/alphaca/wormhole.toml`), which is not a default to hand anyone.

**Pre-attack.** A baked digest ages. Fetching at init to discover the
digest instead would always be current, but it needs the network at init
and it is new machinery — wormhole verifies a declared digest today, it
never discovers one. The staleness is visible and fixable in one file; the
new code path is neither.

#### R7 — one page for the six keys

Section 1's table. The handbook explains each key where its own feature is
explained and never puts them side by side, so the shape of the store has
to be assembled from five pages.

Nothing answers "what have I got", either. `doctor` probes the kernel —
user namespaces, landlock, overlayfs, cgroups, reflink
(`crates/wormhole/src/probes.rs:21`) — so it answers "can this machine run
wormhole" and nothing else. `ps --all` covers boxes, R5's `role list` would
cover roles, and `gc` is the only view of the store as a whole, which it
frames entirely as things that could be deleted.

#### R8 — decide what `wormhole run` is

It is in the help text, in no page of the handbook, and `STATUS.md:282`
records that it inherits the host environment where `box` does not. Either
document it with that caveat stated, or take it out of the usage string
and leave it as the development tool it is used as. Advertising a command
in help that the handbook will not explain is the worst of both.

Same question, smaller, for the undocumented `wormhole tui` alias
(`crates/wormhole/src/main.rs:56`).

## 4. Why it feels heavy: the interface is bigger than the flag

A useful way to name the problem precisely.

A module's **interface** is not its signature. It is everything a caller
must know to use it correctly — the signature, plus the invariants, the
ordering constraints and the error modes. **Depth** is how much behaviour
a caller gets per unit of interface they have to learn.

`--role` has a one-string signature and a large implementation, which
*looks* deep. Count what a user actually has to know to use it correctly:

1. which of three forms their string will be read as, and the punctuation
   that decides it
2. that a bare word is never a directory in the current folder
3. that a remote ref needs `role add` first, unless a terminal is attached
4. that **which spelling they pick decides which box they resume** (D1)
5. that the path form is invisible to the panel (D6)
6. that approval is per commit and host-wide, and that a path gets none

Six invariants behind one flag. The complexity was not hidden; it was moved
out of the signature and into the user's head. That is a shallow module
wearing a deep signature, and it is exactly the feeling being reported.

The fixes in section 3 do not shrink the signature — it is already one
flag. They **delete invariants**:

| Invariant | After |
|---|---|
| 1 — punctuation decides the form | cosmetic, once the forms behave alike |
| 2 — a bare word is never a directory | R3: the tool says so, at the moment it matters |
| 3 — a remote ref needs `role add` first | stays. Earned. |
| 4 — spelling decides your box | R1: gone |
| 5 — a path role is invisible to the panel | R2: gone |
| 6 — approval is per commit; paths get none | stays. Earned. |

Six down to two, and the two that remain are the security model — the part
that must stay in the user's head, because it is the price of running a
stranger's shell on their machine.

### The structural cause

`role_source_dir` returns a `PathBuf` (`crates/wormhole/src/main.rs:1434`).
It resolves all three forms and then throws away *which form it was*. The
seam carries less than its callers need, so `write_record` reaches around
it for the raw string instead (`crates/wormhole/src/main.rs:363`).

That is the whole of D1: not a wrong line, but a return type that cannot
express the answer. R1's `Role` enum is the missing seam, and all three
callers cross it — `build` (`crates/wormhole/src/main.rs:101`), `box`
(`crates/wormhole/src/main.rs:332`) and the panel
(`crates/wormhole/src/main.rs:1139`). Under R2, `role add` becomes a fourth,
which is another reason to give the seam a type before widening it.

### One more, while the seam is open

`role_source_dir` also fetches from the network and prompts a human, in the
middle of resolving a name (`crates/wormhole/src/main.rs:1442`):

```rust
if someone_is_present() {
    fetch_and_approve(&data_home(), &pinned);
}
```

Three jobs in one body: decide what a string names, move bytes over the
network, and ask a person a question. The comment above it argues the case
well, and the behaviour is wanted — but it belongs to the caller that has a
terminal, not to the resolver. A resolver that can block on a human is one
no caller can reason about, and it is why the panel needs a second,
non-failing entry point into the same logic (`try_resolve_manifest`,
`crates/wormhole/src/main.rs:1406`).

Splitting it — resolve returns `Role` and what it still needs; the caller
decides whether to fetch and ask — removes that second entry point and
makes the panel's "listed but not startable" case ordinary rather than
special.

## 5. The one addition worth making

Everything above removes accidents. This adds a capability, and it is the
largest single "easier to use" change available.

### R9 — let a workspace name its role

A workspace manifest must today carry a whole recipe: `image` is the one
table with no `#[serde(default)]`
(`crates/wormhole-core/src/manifest.rs:31`). So a project either writes out
a full `[image]` — base URL, digest, package list, build lines — or it has
no manifest at all and everyone working on it types `--role X` by hand,
every time, correctly, forever.

There is no third option. There should be:

```toml
# wormhole.toml
version = 1
role = "github:you/alphaca-java@<40 hex characters>"
```

Then `wormhole box` alone is the whole command, in every workspace, for
every person on the project. The pin lives in version control, is reviewed
in a pull request like anything else, and moves when somebody decides it
moves rather than when somebody's shell history differs.

**What it is not.** Not inheritance. `role` names the recipe *instead of*
carrying one; a manifest with `role` beside `image` is refused rather than
merged. Merging is where this kind of key turns into an override system
nobody can predict, and nothing here needs it.

**Why it is safe as it stands.** A launch already never fetches and never
asks. A fresh clone whose manifest names a pin this host has not approved
refuses and prints the `role add` that fixes it — exactly what typing the
same ref at `--role` does today. Cloning a repository still causes nothing
to be fetched and nothing to run.

**Why it composes with R1.** `role = "…"` resolves through the same
resolver and yields the same identity, so a box started from the file and a
box started from the flag naming the same role are one box. Without R1 they
would be two, which is D1 again in a new place — so R1 comes first.

**Pre-attack.** It is a second way to say something `--role` already says,
and two ways to say one thing is a cost. The flag has to win, which the
handbook already asserts as the rule ("the flag is you speaking, the file
is a default"). `Manifest` is `deny_unknown_fields`, so an older wormhole
reading a newer manifest fails with an unknown-key error rather than
silently ignoring the role and building something else — which is the
correct failure, but it is a failure, and it belongs in the changelog.

## 6. What the prior art says

Full citations in [role-ux-prior-art.md](role-ux-prior-art.md).

### What it validates

**The `(workspace, role)` tuple is right.** The devcontainer CLI keys
container reuse on exactly `devcontainer.local_folder` +
`devcontainer.config_file` — and the config-file half was **added later**,
with a migration that rebuilds containers carrying only the old label.
Folder alone stopped working the moment one folder had several recipes.
Wormhole reached the same tuple independently. R1 is the same lesson
applied one level down: the second half of the tuple has to be an identity,
not a spelling.

**Commit-keyed approval is the strongest family surveyed, and it is small.**
Only direnv is comparable, and it is the same idea: the approval token *is*
`sha256(absolute path + contents)`, so a changed file is structurally
un-approved. Everything else is weaker — mise keys trust to the path by
default and content only in `paranoid` mode, Homebrew keys to the tap name
and says so plainly ("Trust a whole tap only when you accept all current
and future formulae"), VS Code keys to the folder path, Cargo has no gate
at all by explicit Rust policy, and Terraform does not pin remote modules
in the lock file at all. Nothing here suggests softening the pin.

**"A launch never fetches and never asks" has an explicit endorsement.**
clig.dev: "Actions crossing the boundary of the program's internal world
should usually be explicit … Talking to a remote server, e.g. to download
a file."

**Printing what was resolved is ahead of the field.** Wormhole prints
`manifest: role …` on the *success* path. No tool surveyed does that; Nix
is the only one that reliably says which form it took, and only in its
error messages.

### What it contradicts

**The three-form rule is the industry rule, backwards.** Both tools that
document this exact trap guard it the other way — they make the *path*
mark itself and leave the *name* unmarked:

> Nix: "relative paths must start with `.` to avoid ambiguity with registry
> lookups (e.g. `nixpkgs` is a registry lookup; `./nixpkgs` is a relative
> path)."

> Terraform: "A local path must begin with either `./` or `../` to indicate
> that a local path is intended, to distinguish from a module registry
> address."

Wormhole reads anything containing a `/` as a path, so `roles/alphaca` is a
directory and `alphaca` is a name. Same trap as theirs; no guard on it.

Two designs avoid the ambiguity entirely, both for the cost of one word:
cargo's mutually exclusive `--git` / `--path` / `--registry`, and
asdf/mise's arity rule (one argument is a name, two are a name and a URL).

**There is no way to force a form.** Every tool that sniffs ships an escape
hatch — Nix's `path:`, go-getter's `git::` — and go-getter states the
reason: "the protocol to use is ambiguous depending on the source URL …
Forced protocol syntax is used to disambiguate this URL." Wormhole has a
forcing prefix for remote (`github:`, `https://`) and none for "definitely
a name" or "definitely a path".

**The no-TTY rule is a fork, not a default.** npx gates on a human for a
stated reason — "To prevent security and user-experience problems from
mistyping package names" — and then documents how the gate disappears:
"When standard input is not a TTY or a CI environment is detected, `--yes`
is assumed." Wormhole resolves the identical situation the opposite way and
refuses. clig.dev is on wormhole's side ("skip prompting and just require
those flags/args") and licenses the departure — "Do so with intention and
clarity of purpose" — which the code comment supplies. Worth saying out
loud in the handbook that this is a choice, because the most-used tool in
the space made the other one.

**Install-before-use is nearly extinct**, which settles the earlier
question about mandating it. uv states the principle: "In most cases,
executing a tool with `uvx` is more appropriate than installing the tool.
Installing the tool is useful if you need the tool to be available to other
programs on your system." Install exists to serve third parties, not the
person typing. `gh extension` is the only mandatory-install tool surveyed,
and its justification is trust — "Extensions are not verified, signed, or
endorsed by GitHub" — which is a case wormhole already answers with the pin.

### R3, revisited

Two ways to fix D4, and the prior art supports both.

**R3a, as written.** Diagnose it: when a bare name misses and `./<name>`
holds a manifest, say so. Non-breaking, and it is what cargo actually does
— it errors with "Use `cargo install --git <url>` if you meant to install
from a git repository."

**R3b, the root fix.** Adopt the Nix and Terraform rule: a path must start
with `./` or `../`, and a bare `roles/alphaca` becomes an error rather than
a silently different thing from `alphaca`. It removes the ambiguity instead
of reporting it, and it is what the two tools that thought hardest about
this chose.

R3b is breaking — `--role roles/alphaca` works today. And once R1 lands,
most of the *harm* of the ambiguity is already gone: the spelling stops
deciding which box you resume, so what is left is a confusing error
message, which is exactly what R3a fixes. So R3a is sufficient, and R3b is
optional strictness rather than a correction. Taking R3b anyway would buy
one thing R3a cannot: a bare `roles/alphaca` could never again mean
something different from `alphaca` without saying so.

## 7. Two more, from the prior art

### R10 — let a box have a name

Every tool surveyed pairs a machine identity with a human selector. Vagrant
documents the split wormhole is missing: a name works "from within a
Vagrant project", an id "allows you to call `vagrant up id` from any
directory". tmux, docker, toolbx and distrobox all take a name.

Wormhole has the id and no user-settable name. `attach` and `--id` take
twelve hex characters and nothing else. The `Record` already has a `name`,
but it comes from the manifest and "nothing functional hangs off it"
(`crates/wormhole-core/src/manifest.rs:27`).

`wormhole box --new --as api`, then `wormhole attach api`. The id stays
what it is — assigned, stable, unforgeable — and the name is the thing a
person types. Two boxes in one workspace is the ordinary case here
(`docs/src/guide/boxes.md:8`), which is exactly when remembering which
twelve hex characters is which stops being possible.

### R11 — one word is doing three jobs

Homebrew names every layer of this separately: `formula` is the recipe,
`tap` is where it came from, `keg` is one installed version, `rack` is
every version of one formula, `opt prefix` is the alias pointing at the
active one.

Wormhole's "role" is currently the recipe, the installed alias *and* the
fetched commit. `CONTEXT.md`'s own entry contains the conflation in one
line: "Named by path, by installed name, or by pinned commit." The
vocabulary is where the three-forms confusion starts — the document already
has separate words for the pieces (`pin`, `checkout`), and no word at all
for the third thing, the alias under `config/wormhole/roles/<name>`.

Naming that one thing would make `role list` obviously a list of aliases
rather than an ambiguous list of roles.

**Second collision, closer to home.** "Workspace" is the term this project
shares with the most neighbours — Cargo, Terraform CLI, HCP Terraform, and
VS Code all use it for something else. This repository is itself a Cargo
workspace (`Cargo.toml:1`), so inside wormhole's own tree the word already
means two things.

**Pre-attack, with a cautionary tale.** Renaming out of a collision only
works if the new name is not already claimed. Terraform renamed
`terraform env` to `terraform workspace` "in response to feedback that the
previous naming was confusing due to collisions with other concepts of the
same name" — and landed in a worse collision it has been explaining ever
since: "Both HCP Terraform and Terraform CLI have features called
workspaces, but they function differently." `CONTEXT.md` is a deliberate
document and its author has already ruled on these words; this is a finding
to weigh, not a change to make on a reviewer's say-so.
