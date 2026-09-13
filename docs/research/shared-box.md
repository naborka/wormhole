# One box, several workspaces

What it takes for one role box, one kept home with one login and one
toolchain, to serve several workspaces: one `alphaca` box for every Rust
project on the machine instead of one per project.

Second pass, 2026-09-13. The first pass read the code; this one ran it,
had the change designed three ways, and changed the verdict where the
evidence said so. What the first pass got right is kept. Prior art for
the same question in other tools is in
[shared-box-prior-art.md](shared-box-prior-art.md). What the vendor CLIs
document about several sessions on one home is in
[shared-box-concurrency.md](shared-box-concurrency.md). Read this beside
[CONTEXT.md](../../CONTEXT.md) and
[ADR 0004](../adr/0004-one-role-three-homes.md).

**Built, 2026-09-13**, recorded in
[ADR 0005](../adr/0005-a-box-owns-its-identity.md). D1, D2, D4 and D7 as
recommended below. Three differences, each for a reason found while
building:

- **D6: the key's label stays the workspace basename.** A new box's id
  comes from the workspace and a free ordinal. Two roles starting in one
  workspace at once can pick the same ordinal; with the role's name as the
  label they would take two different lock files and both claim one id.
  With the basename they meet on one lock and the loser takes the next.
- **D5, widened to `gc` as a whole.** A box of a role that resumes only
  `here` is dead when every workspace it ran in is gone, as before; the
  opposite rule would keep every such box forever. Only a box whose role
  resumes `anywhere` outlives its workspaces. A box whose role directory
  is gone is dead; one whose role cannot be read is unproven once its
  workspaces are gone.
- **D3: a busy shared box.** One process per box stays, so a bare start
  that finds the shared box running elsewhere takes another free box of
  the role, or makes one. The concurrency research names two blockers for
  two runs of one home: wormhole's unlocked seed of `.claude.json` and
  `config.toml` racing a live writer, and Codex's unlocked in-place
  `auth.json` write.

Not built: the hint line naming the sharing command when a `here` role
makes a new box, and the refusal naming the workspace a running box is
in. The "What has to be true" list at the end is otherwise done.

---

## Verdict

1. **The want is one home per role box, not one per (role, workspace).**
   Everything on the list (login, product binary, cargo registry, skills,
   plugins, MCP, transcripts) lives in the home. Unchanged from the first
   pass.

2. **Today a box's identity is the directory you are standing in.** Not
   "seven places", as the first pass counted: one rule, `key = basename of
   the caller's cwd + id`, applied in seven places. Two consequences were
   proven at the command line (below): a `--id` start resolves its recipe
   from the cwd and then writes the record over, so a role box loses its
   role; and a `--id` from another directory finds the box only when the
   basenames match, and then starts it with the wrong recipe if its record
   happens to be unreadable.

3. **The fix is that a box owns its identity.** A box is made from one
   *recipe* (this workspace's own manifest, or a role by its source) with
   one product, fixed for its life; its store entries are found by its id,
   never rebuilt from where you stand; and the workspaces it has served
   are a list it keeps. Three independent designs converged on this shape.

4. **The record that says so must not live where the agent writes.**
   Today the record is `~/.wormhole/box.toml` inside the home. An agent
   that rewrites its own `source` gets its home resumed by another role's
   next bare start in that workspace. Host-wide sharing turns that into
   every workspace on the host. The lock already lives host-side for
   exactly this reason (`paths.rs:126-128`: "a claim that a `rm -rf` could
   drop is not a claim"); the same sentence holds for the record. This is
   the one thing no design brief asked for and all three needed.

5. **Sharing is the role's choice, not the default.** `[agent] resume =
   "here" | "anywhere"`, default `"here"`, which is today's behaviour for
   every existing role. `alphaca` sets `"anywhere"`. Naming a box with
   `--id` from a new workspace always adds that workspace, whatever the
   role says: naming is consent. The first pass recommended the same split
   under a different key name.

6. **One process per box stays, for now.** Two runs of one home in two
   workspaces is what a person does on a laptop with one `~/.claude`, and
   it is probably fine, but "probably" is not a proof: the vendor evidence
   is still being gathered, and wormhole's own start-time merge into
   `.claude.json` races a live writer. The claim's shape keeps the seam so
   the second cut changes one function.

7. **What proves a shared box dead is that its recipe is gone**, not that
   its workspaces are: a role box whose projects were all deleted is still
   one `--id` away from working somewhere else. A `dir:` role box is dead
   when the role's directory is gone; a `repo:` role box is never provably
   dead and stays unproven until `wormhole remove`. Whether a role is
   *installed* is never evidence, because a role started by path never
   was.

8. **Older binaries must read the new state as unproven, never as dead.**
   With the record host-side and the in-home copy removed on adoption, an
   older `gc` says "records no workspace", an older `ps` prints a problem
   line, and an older `--id` or `remove` still works by directory name. No
   stand-in directory is needed to keep an old `gc` honest.

9. **Two bugs were found on the way**, both older than this question: a
   held build lock is deleted by `gc --delete` (proven live), and a role
   given as a relative path is resolved against `gc`'s cwd instead of the
   box's workspace. The second disappears under the new design; the first
   does not and is listed at the end.

---

## What the code does now

| What | Where | Carries the workspace how |
|---|---|---|
| box id | `paths.rs:53-60` `box_id(workspace, ordinal)` | `sha256(path[\0ordinal])[..12]` |
| key (home dir, lock file name) | `paths.rs:72-78` `box_key(workspace, id)` | `<basename>-<id>`, computed from the **caller's** cwd at `main.rs:258,315,323`, `lock.rs:37,62,109`; from the record at `home.rs:241`; from the registry entry at `main.rs:1440` |
| record in the home | `home.rs:30-68` `Record.workspace` | absolute path, written from this start's arguments on every start (`main.rs:396-409`) |
| running-box registry entry | `registry.rs:24` | absolute path |
| resume rule | `home.rs:351-366` `resumable` | `record.workspace == workspace && is_role(wanted)` |
| `--id` | `lock.rs:36-49` | home looked up under the **cwd's** key; refused when a readable record names another workspace |
| alias scope | `home.rs:166-170` `by_name` | per workspace |
| panel resume | `main.rs:1012-1027` | `set_current_dir(record.workspace)`, then `--id` plus the **typed** `--role` |
| `attach` | `main.rs:1440` | key from the registry entry's workspace |
| `gc` home verdict | `gc.rs:132-141` | dead when the one recorded path no longer exists |
| `gc` recipe scan | `main.rs:759-783` | re-resolves the typed role relative to `record.workspace` |
| mount plan | `mount_plan.rs:457-460` | one rw bind, host-identical path |
| agent config seed | `manifest.rs:426,451-457,494`; `seed.rs:99-118` | per-directory keys under `projects.<workspace>`, merged into the file |

ADR 0004 fixes the resume tuple as *workspace + role source + product*.
CONTEXT.md defines a box as "the running sandbox for **one** workspace"
and a key as "its workspace's basename and its id".

### What is already workspace-neutral

The image (keyed by the recipe digest), the mount plan (takes the
workspace as a parameter), the agent config seed (per-directory keys,
merged, never overwritten: `seed.rs:95`), and the claim (keyed by the key
string alone). The home's *contents* can serve several workspaces today;
only the naming and the rules around the name say otherwise.

---

## Proven at the command line

Debug build of the tree at `aaff7bd`, 2026-09-13, in a throwaway data home.
Setup: a role directory `R` and each workspace hold a manifest whose base
is `file:///nothing/...`, so every start gets exactly as far as the fetch
and no box ever runs. A record is written by hand into
`homes/proj-<id>/.wormhole/box.toml` with `source = "dir:R"`, `role = "R"`,
`alias = "rust"`, `agent = "claude"`.

### 1. `--id` without `--role` starts a role box from the wrong recipe, then forgets its role

```
record before:   role = "<tmp>/roles/alphaca"   source = "dir:<tmp>/roles/alphaca"

$ wormhole box --id 3cc66efc757c                 (in <tmp>/w1/proj, no --role)
  no image for this manifest yet; building it
  fetching file:///nothing/workspace-recipe.tar.gz      ← the workspace's manifest, not the role's

$ wormhole box --id 3cc66efc757c --role <tmp>/roles/alphaca
  manifest: role at <tmp>/roles/alphaca
  fetching file:///nothing/role-recipe.tar.gz           ← only when re-typed

(both images made to exist; the first form again, which now gets past the build)
$ wormhole box --id 3cc66efc757c
  wormhole: unshare(user, uts, pid) failed: ENOSYS      ← this box cannot nest namespaces; the record was already written

record after:    id, workspace, alias, name = "workspace-recipe", agent
                 (no role, no source: the box is now a workspace box)
```

`run_box` resolves the manifest from `--role` or the cwd (`main.rs:252`)
before it knows which box it is, and `write_record` (`main.rs:396-409`)
writes that start's `role_typed` and `role_source` over the record. There
is a mismatch guard for the product (`manifest::product_mismatch`) and none
for the role. The panel avoids this only because `resume_from_panel`
passes the typed `--role` back in (`main.rs:1022-1025`).

### 2. `--id` from another directory is decided by the basename

```
box 7f783865a5af belongs to <tmp>/x/a/proj

$ wormhole box --id 7f783865a5af      (in <tmp>/x/b/proj, same basename)
  wormhole: box 7f783865a5af belongs to <tmp>/x/a/proj, not to this workspace
$ wormhole box --id 7f783865a5af      (in <tmp>/x/c/other)
  wormhole: no box 7f783865a5af in this workspace; `wormhole ps --all` lists the kept ones

(record made unreadable)
$ wormhole box --id 7f783865a5af      (in <tmp>/x/b/proj)
  fetching file:///nothing/b-recipe.tar.gz              ← starts, with b's recipe, in a's home
$ wormhole box --id 7f783865a5af      (in <tmp>/x/c/other)
  wormhole: no box 7f783865a5af in this workspace
```

Same box, three answers, none of them "which box is this". The refusal in
the second line is a record check bolted onto a key computed from the
wrong place; when the record cannot be read the check is gone and the
start goes through.

### 3. What a role box looks like from another workspace today

```
box b33f2ec5bd61 (alias rust) serves <tmp>/y/a/proj

$ wormhole box --id rust                    (in <tmp>/y/b/other)
  wormhole: no box rust in this workspace
$ wormhole box --role <tmp>/roles/alphaca   (in <tmp>/y/b/other)
  box: 4d90af5e5650 (new)
```

Correct by today's rule, and exactly the behaviour this document is about.

---

## What the user actually wants, sharpened

| Want | Held where today | Shared by |
|---|---|---|
| one login | `~/.claude/.credentials.json` in the home | home |
| one installed product binary, kept current by the preflight | `~/.local/bin` in the home | home |
| one cargo registry cache | `~/.cargo` in the home (`roles/alphaca/wormhole.toml:71-72`) | home |
| one set of skills, plugins, MCP servers the preflight installed | home | home |
| the agent's memory and transcripts | `~/.claude/projects/<path>/`, already per directory | home, per project inside it |
| the same persona and toolchain | image + `ROLE.md` | already shared |

Everything in the first column is "the home". The want is precisely **one
home per role box**. Nothing on the list is about the root filesystem,
the image, or the namespaces.

---

## Design: a box owns its identity

### The nouns

**Recipe.** Where a box's manifest comes from: this workspace's own
`wormhole.toml`, or a role, by its source (`dir:<canonical dir>` or
`repo:<url>`). `main.rs:746-747` already says it: "A box's recipe is its
workspace's own manifest, or the role it was started from." Fixed when
the box is made, together with the product. A start from another recipe
is refused, never written over. Filling in a record from before roles had
sources, or before products were identity, is the one exception, as
today.

**Served.** The workspaces a box has run in, most recent first, with the
time. A box made from a workspace's own manifest serves that workspace
only: its recipe lives there. A role box may serve several.

**Key.** Unchanged in shape, `<label>-<id>`, but the label stops meaning
anything to the code. A key is found by its id (the record is named by it,
the home directory ends in it), never rebuilt from the caller's cwd. New
role boxes get the role's name as label (`alphaca-3f9c…`), which is what
you want to see in `eza ~/.local/share/wormhole/homes`; existing keys stay
as they are. `box_key` is called in one place: when a box is made.

### Where the record lives

```
~/.local/share/wormhole/
  homes/<key>/             the kept $HOME: the agent's
  locks/<key>.lock         the claim: the host's
  records/<key>.toml       the box: the host's                 ← new
  boxes/<pid>/box.toml     a running box; now carries `key`
```

```toml
id = "a3f9c1e40b2d"
recipe = { role = "dir:/home/me/roles/alphaca", typed = "alphaca" }
# or:  recipe = { workspace = "/home/me/proj" }
agent = "claude"
alias = "rust"
name = "Alphaca"
created_unix = 1757700000
started_unix = 1757790000

[[served]]
path = "/home/me/rust/b"
last_unix = 1757790000

[[served]]
path = "/home/me/rust/a"
last_unix = 1757700000
```

Why host-side, in the project's own words: the agent is hostile, and the
home is the one place it writes. Today a session can rewrite its own
`source` and `started_unix` and be resumed by another role's next bare
start *in that workspace*: `resumable` trusts the record. Under
`"anywhere"` it would be resumed by that role's next bare start *in any
workspace on the host*, carrying its `.claude/settings.json` hooks and
`CLAUDE.md` into every project. Moving the record beside the lock closes
the whole class, including the version that exists today. The home keeps
nothing of wormhole's but the seeded files (`AGENTS.md`, the preflight).

What it costs: two entries per box to keep in step. `remove` takes both;
`gc` judges a record whose home is gone as dead, like a lock, and a home
with no record as unproven, which is what "holds no box record" already
means. `reset` gets simpler: empty the home, nothing to put back. The
comment at `home.rs:7-8`, "a home carried to another machine carries its
box with it", describes nothing the code does and goes.

**Adoption.** A home with an in-home record (or the older
`.wormhole/workspace` stamp) and no host-side record is read as today and
listed as today. The first start under the new binary writes the host-side
record (`recipe = {workspace}` or `{role: source, typed}`, `served = [the
recorded workspace]`) and removes the in-home files. That is the existing
`from_legacy` path, one step further out. A home never started again is
exactly as it was, so an older binary sees exactly what it saw.

### What a start does

**Bare start** (`wormhole box`, `--role X`, or a manifest that names a
role). Resolve the recipe, as today. Candidates are this recipe's boxes
with a compatible product, in one order: boxes that serve *here*, most
recent first; then, only if the manifest says `resume = "anywhere"`, boxes
that serve other workspaces, most recent first. The first one whose claim
can be taken is resumed; none means a new box. A resumed box moves *here*
to the front of `served`. The line says which and where it last ran:

```
box: a3f9c1e40b2d (resumed; last used in /home/me/rust/a)
```

Under `"here"`, when a new box is made while the role has one elsewhere,
the line names the sharing command:

```
box: 8e1d… (new); alphaca also has a3f9… (rust) in /home/me/rust/a; `wormhole box --id rust` uses it here
```

**`--id <id|alias>`.** Claim first, before anything is resolved: a
refusal must cost nothing, and resolving may fetch. Then read the record
under the claim. A box made from a workspace's own manifest is refused
outside that workspace ("made from /a's own manifest, so it works only
there"). A role box's recipe is resolved from the record: a `dir:` source
is read in place; a `repo:` source is resolved through the typed name or
pin and must land on the same source; a typed `--role` must too, or the
start is refused ("runs dir:/r, not dir:/s"). The product is the recorded
one; `--run` that disagrees is refused as today. *Here* goes to the front
of `served`, this workspace's trust keys are merged into the agent's
config, and the box starts. `--id` from a new workspace is how a
`"here"` role's box is shared on purpose.

**`--new`.** A new box, `served = [here]`, as today.

**Alias.** Set host-wide unique, at `--as` and at `rename`: one name
names one box on this host. Found from the workspace first (boxes that
serve *here*), then anywhere, so the two `api` boxes an existing store may
already hold keep resolving in their own workspaces. Two matches from a
third workspace is a refusal that names both ids.

**Panel Enter on an idle box.** A workspace box resumes in its workspace.
A role box resumes *here* if it serves *here*; otherwise in the most
recently served workspace that still exists (ties by path); the start
says which. None left is a refusal naming `wormhole box --id X` in the
workspace you want. The panel no longer passes a `--role`: the box knows
its recipe.

**`attach`, `stop`, `ps`.** Unchanged, except the home is found by the
key the registry entry now carries (an entry from an older binary falls
back to `box_key(entry.workspace, id)`, which is the key that binary
claimed). `ps --all` shows the most recently served workspace and `+N`.

### The claim

Unchanged for the first cut: one exclusive `flock` on `locks/<key>.lock`,
taken before anything else, held for the process's life. The refusal
gains the workspace: "box a3f9… is already running in /home/me/rust/a
(pid 8213)". Two starts of one shared box in two workspaces at once is
refused, exactly as two starts in one workspace are.

The seam for the second cut is the lock shape and nothing else: a run
takes `<key>.lock` *shared* plus `locks/<key>.d/<digest(workspace)>.lock`
exclusive; the lifecycle verbs keep taking `<key>.lock` exclusive, so they
still exclude every run. The per-workspace locks live in a `<key>.d/`
directory rather than as `<key>@x.lock`, because an older `gc` reads a
lock's file stem as a key: `<key>.d` stems to `<key>` and is kept,
`<key>@x` does not and would be unlinked while held. The registry then
holds several entries per box, `attach <id>` picks the run in the caller's
workspace or names both, and the start-time seed writes only the new
workspace's keys, before the agent starts. Whether any of that is safe
for the vendor CLIs is what `shared-box-concurrency.md` is for.

### What `gc` can prove

| Box | Dead when | Otherwise |
|---|---|---|
| made from a workspace's own manifest | that workspace no longer exists | live |
| role, `dir:` source | the role's directory no longer exists | live |
| role, `repo:` source | never: the URL can always be fetched again | unproven, until `wormhole remove` |
| record unreadable, or none | never | unproven |
| record whose home is gone | always (like a lock) | |

The served list decides nothing here. A role box whose projects are all
gone is still one `--id` away from working in a new one; a role box whose
role directory is gone cannot resolve its recipe and no start can ever
match it, however many of its projects remain. Under `"here"` and
`"anywhere"` alike. This is stricter than today in one way: a `repo:` role
box is never reclaimed by `gc`, which is the honest answer, since nothing
proves it unwanted.

The recipe scan (`extend_referenced`) reads each recipe by its source, a
`dir:` in place and a `repo:` through the pin with `Fetch::Never`, once
per recipe rather than once per (workspace, typed role).

### What older binaries see

They never see a `records/` directory. A box that has been adopted has no
in-home record, so an older `ps` prints "`<home>` holds no box record",
an older bare start never resumes it, an older `gc` reads its home as
"records no workspace, so nothing can tell whether it is still wanted"
and keeps it, and an older `--id <id>` or `remove <id>` still finds the
home by its directory name. The one thing an older binary does that the
new one has to survive: its `--id` writes a fresh in-home record with
`workspace = its cwd`. The new binary ignores an in-home record wherever a
host-side one exists, so nothing is lost; and it removes the in-home file
again on its next start.

Older binaries were the reason all three designs carried a stand-in
directory (an "anchor" whose basename is the key's label, made when a box
joins a second workspace, so an old `gc` sees a path that exists). That
is only needed while the in-home record exists. Moving the record makes
the anchor unnecessary: an old `gc` that cannot read a record already
answers unproven.

---

## Three designs, compared

The change was designed three times in parallel from one brief, each
under a different constraint. All three arrived at: a pure `boxes` module
in `wormhole-core` holding every rule; a binary-side store doing scans,
flocks and writes; the key found from the id; a served list; identity
fixed at creation; the recipe resolved from the record on `--id`; both
policies behind one seam each. Where they differ:

| | A: fewest entry points | B: most flexible | C: default caller first |
|---|---|---|---|
| interface | `claim`, `settle`, `survey` | a `Store` with twelve methods, `Consent` and `Occupancy` traits, a `Consented` token | eight pure functions plus one `Store::start` |
| depth | highest: one call does lookup, policy, race, panel choice | spread thin: `Occupancy::claims` is a pass-through | high for the default caller, plain elsewhere |
| locality | best: no verb can skip identity or alias checks | policy in traits, so a new policy is a new adapter | each defect closes in one named function |
| served list | in the record, in the home | one file per workspace, host-side | in the record, in the home |
| older binaries | anchor symlink under `anchors/<id>/<basename>` | anchor dir under `anchors/<label>` | stamp dir inside the home |
| thin spots | hands the recipe back for the caller to resolve; presentation strings in the interface | typestate for one guarantee; three new store directories | the store is plumbing |

What is taken from each. From **C**, the shape: pure functions with plain
inputs (`plan`, `admit`, `named`, `resume_in`, `evidence`) and one binary
call for the default caller, because the interface is the test surface
and a table of `(kept boxes, start) → decision` tests reads best. From
**B**, the one idea that survived scrutiny of all three: the served list
does not belong in the agent's home. Taken one step further, nothing
about identity does. Its lock-directory trick for older `gc` is kept for
the second cut. Its traits are not: `Consent` has one production adapter
once the manifest decides, and `Occupancy` has one until the second cut,
so both are hypothetical seams and stay values. From **A**, the ordering
(claim before resolve on `--id`) and the discipline that already exists in
`main.rs` as `Claimed`: verbs act only on a box whose claim they hold.

No design questioned the record's home. The first pass did not either.
That it took the hostile-agent sentence from CONCEPT.md §1 to see it is
the argument for reading the threat model before the code.

### The interface, in outline

```rust
// wormhole-core::boxes: pure; every rule, every sentence
pub enum Recipe { Workspace(PathBuf), Role { source: Option<String>, typed: Option<String> } }
pub struct Record { id, recipe: Recipe, agent, alias, name, served: Vec<Served>, created_unix, started_unix }
pub struct Kept   { key: String, record: Option<Record> }          // a home; None: no readable record
pub enum Resume   { Here, Anywhere }                                // [agent] resume

pub fn plan(kept: &[Kept], here: &Path, recipe: &Recipe, product: Option<&str>, resume: Resume) -> Plan;
pub fn named<'a>(kept: &'a [Kept], here: &Path, wanted: &str) -> Result<&'a Kept, String>;
pub fn admit(kept: &Kept, here: &Path, start: &Start<'_>) -> Result<Record, String>;   // identity, served, alias
pub fn new_box(kept: &[Kept], tried: &[String], here: &Path, label: &str) -> Candidate; // the only box_key caller
pub fn resume_in(record: &Record, here: &Path, exists: impl Fn(&Path) -> bool) -> Result<(PathBuf, String), String>;
pub fn evidence(record: Option<&Record>, exists: impl Fn(&Path) -> bool) -> gc::HomeEvidence;
pub fn renamed(record: &Record, kept: &[Kept], alias: &str) -> Result<Record, String>;

// wormhole (binary)::store: scans, flocks, writes; a tempdir in tests
impl Store {
    fn scan(&self) -> (Vec<Kept>, Vec<String>);                // records dir + homes dir + adoption reads
    fn start(&self, here: &Path, args: &BoxArgs) -> Result<Started, String>;  // the default caller's one call
    fn claim(&self, key: &str) -> Result<Claim, String>;       // exclusive; the lifecycle verbs and, today, a run
    fn write(&self, key: &str, record: &Record) -> Result<(), String>;
}
```

`plan` returns candidates in resume order plus whether a new box follows;
the binary walks them taking claims, re-reads the record under the claim,
and calls `admit`, which is where a mismatched recipe or product, a
workspace box outside its workspace, or a taken alias becomes a refusal.
`manifest` gains `resume` under `[agent]`, refused beside `[image]` (a
workspace's own recipe has no other workspace to be anywhere in).

---

## Security, stated plainly

What widens. A shared home means an injection in project A reaches, at
project B's next session: the product binary the preflight keeps in
`~/.local/bin`, `~/.claude/settings.json` and its hooks, `CLAUDE.md`,
skills, plugins, MCP config, the login, `~/.cargo`, and B's transcripts,
readable without B's tree being mounted. None of it is a new kind of
exposure (the home is already the agent's, and a hostile agent already
owns its own next session); what is new is that one injection reaches
every project the box serves. The trust of a shared box is the union of
the trust of its workspaces.

What closes. Moving the record host-side ends the resume hijack: no
session can make another role's start pick up its home, in any
workspace. That hole exists today at workspace scope.

What follows.

- sharing is opt-in per role (`resume = "anywhere"`), and naming a box
  with `--id` is the explicit form for every role;
- the preview and the banner list the workspaces a box has served, so a
  start into a new tree sees whose history it is joining;
- `wormhole reset` is the answer after an incident, and the docs say so:
  a shared box after an incident is reset, not trusted;
- `AGENT.md` stops telling the agent its home is "kept for this
  workspace"; it is kept for this box, which may serve others.

---

## Alternatives considered

**Share pieces of the home, keep boxes per workspace** (`~/.cargo`,
`~/.local/bin`, the credential file, bound from a per-role directory into
each per-workspace home). Rejected, as in the first pass: a second policy
language that grows one line per vendor file, transcripts and memory stay
unshared with no way to say otherwise, and every entry is a
cross-workspace channel anyway, so the security cost is paid in full for
a more complicated model.

**Grants and `credentials = "share"` from the host.** Reachable today
with no code. Rejected: both bind the host's own files into a hostile
box; a poisoned host `~/.cargo` poisons host builds. A shared box home is
strictly safer than this: the host never sees it.

**Overlay: role home as lower, per-workspace upper.** Rejected on a hard
limit: overlayfs requires the lower to be unchanged while mounted, and the
role home is what the agent writes.

**Keep the record in the home and add an anchor for older binaries.** What
all three designs did. Rejected: it keeps agent-writable identity, and
the anchor is weight that exists only to keep an old `gc` from reading a
lie. With the record host-side an old `gc` reads nothing and says so.

**Automatic sharing for every role.** distrobox's and toolbx's model.
Rejected: an untrusted clone started with `--role alphaca` would silently
join the home that holds every other project's transcripts and the login.
One manifest line per role that wants it, in the file where `grants` and
`credentials` already live.

**Two runs of one box now (a claim per (box, workspace)).** The first
pass recommended it first. Deferred, not rejected: the claim's shape keeps
the seam, and the decision waits on what the vendors document and on a
fix for the seed race. Parallel work across projects still works today
with two boxes.

---

## Bugs found on the way

1. **`gc --delete` deletes a held build lock.** `main.rs:671-681` reads
   every file in `locks/`, takes its stem as a box key, and
   `gc::lock_verdict` calls it dead when no home has that key. A
   `build-<digest>.lock` never has a home. Proven: a store holding one
   held `build-…lock` and nothing else, `gc --delete` prints `dead … the
   box that claimed it is gone`, then `removed …`, and the file is gone
   while the lock is held. A second builder then takes a new lock on the
   same digest, and two processes assemble one `.partial`. Root cause: one
   directory holds two kinds of claim and the verdict knows one. Fix at
   the root: a build lock is never a box lock (its own directory, or a
   verdict that knows both kinds). Independent of this document; noted
   because the second cut would add a third kind to the same directory.

2. **A role given as a relative path is resolved against the process
   cwd.** `role_source_dir` (`roles.rs:537-544`) does
   `PathBuf::from(role)` and `try_resolve_manifest_in` (`roles.rs:458-472`)
   never joins the workspace it was given. A box started with `--role
   ./roles/alphaca` in `/w` is resolved by `gc` relative to wherever `gc`
   is run, so from anywhere else its recipe cannot be read and the whole
   answer is unproven (the safe direction, and wrong). The panel escapes
   it by `chdir` first. The design above resolves a recipe by its source,
   a canonical absolute path, so the bug has nowhere to live.

---

## Decisions

Each with the recommended answer first. Nothing below is written into the
code or the glossary yet.

**D1. How a role shares.** Recommended: `[agent] resume = "here" |
"anywhere"`, default `"here"`; `--id` from a new workspace always adds it.
Alternatives: a global default of `"anywhere"` (rejected above); `--id`
only, no manifest key (every new project costs one `--id`, and `alphaca`'s
own recipe cannot say what it is for).

**D2. Where the record lives.** Recommended: host-side, `records/<key>.toml`,
adopted from the in-home record at the first start, in-home copy removed.
Alternative: in the home, with the anchor (keeps the hijack, adds weight).

**D3. Two runs of one box.** Recommended: not in the first cut; refuse
with the workspace named; keep the lock-shape seam. Revisit with
`shared-box-concurrency.md` in hand.

**D4. Alias scope.** Recommended: host-wide unique when set; found from
the workspace first, so existing duplicates keep working. Alternative:
keep per-workspace scope with a union rule for shared boxes (works, harder
to explain).

**D5. `repo:` role boxes and `gc`.** Recommended: never dead; unproven
until `remove`. Alternative: dead when no installed role names the URL and
every served workspace is gone (installation is not evidence: a pinned
ref can be typed straight at `--role`).

**D6. Key label for new role boxes.** Recommended: the role's name.
Cosmetic; two label schemes coexist and nothing parses a label.

**D7. Words.** Recommended: the glossary noun **Recipe** (the code already
uses it), the phrase "the workspaces a box has served", and the manifest
key `resume`. Alternatives considered: **Owner** (the first pass's word:
fits the workspace case badly, a workspace does not own anything else);
`[agent] home = "role" | "workspace"` (the first pass's key: `home` is
already the glossary noun for the directory); `[agent] box = "shared"`
(says less than `resume` does about what changes).

---

## What has to be true before writing code

1. **CONTEXT.md.** Proposed wording, to land once D1, D2, D4 and D7 are
   settled:

   - **Box**: "A kept home with an identity, and the sandbox that runs
     from it. Made from one recipe, fixed for its life; runs in one
     workspace at a time and remembers every workspace it has served. A
     workspace holds as many boxes as you make; two products of one role
     are two boxes."
   - **Recipe** (new, under core nouns): "Where a box's manifest comes
     from: this workspace's own `wormhole.toml`, or a role, by its source.
     Fixed when the box is made, with the product; a start from another
     recipe is refused. A box made from a workspace's own manifest serves
     that workspace only; a role box may serve several. _Avoid_: owner,
     origin, template."
   - **Key**: "What a box's store entries are named by: a label and its
     id, `proj-a3f9c1e40b2d`. The label is the workspace's basename for a
     box made there and the role's name for a role box, and the code
     never reads it: a box is found by its id, never rebuilt from where
     you stand."
   - **Claim**: add "One per box: a box runs in one workspace at a time."
   - **Alias**: "one name names one box on this host" replaces the
     per-workspace sentence.
   - **Dead**: "a home whose recipe is gone: the workspace it was made
     from, or the directory of the role it runs" replaces "a home whose
     workspace no longer exists".
   - **Home**: "The only box state that outlives the box" gains "the
     agent's; nothing wormhole decides by is kept in it."

2. **ADR 0005**, superseding 0004's tuple. Draft: *A box is made from one
   recipe (a workspace's own manifest, or a role by its source) and one
   product, fixed for its life; the workspace is a fact about a run, kept
   as the list of workspaces served. The record lives beside the lock,
   host-side, because the home is the agent's and an identity the agent
   can rewrite is not an identity. A role opts into being resumed from any
   workspace with `[agent] resume = "anywhere"`; naming a box with `--id`
   adds a workspace for any role. Considered and rejected: the workspace
   in the identity (one box per project, the state before this); the
   record in the home with a stand-in directory for older binaries;
   automatic sharing for every role; a claim per (box, workspace) in the
   first cut.* All three tests hold: hard to reverse (ids and store
   layout), surprising later (why does a role box's key not name a
   workspace, why is the record not in the home), a real trade-off.

3. **The failing tests, in this order**, each driving one seam. Pure
   first:

   - `manifest`: `[agent] resume` parses to `Here` by default and
     `Anywhere` when set; refused beside `[image]`.
   - `boxes::plan`: under `Here`, a bare start in B never picks a box
     serving only A; under `Anywhere` it picks the most recent; boxes
     serving *here* come first; a workspace box is never a candidate
     elsewhere; `--new` plans nothing but a new box.
   - `boxes::named`: an id is found from any workspace; an alias serving
     *here* wins over one elsewhere; two elsewhere is a refusal naming both
     ids; a workspace box named from another workspace is refused with
     "works only there".
   - `boxes::admit`: a start from another source, or another product, is
     refused and the record is returned unchanged; a legacy record's
     missing source or product is filled; *here* moves to the front of
     `served`; a workspace box's `served` stays one path; a taken alias is
     refused naming the box that holds it.
   - `boxes::new_box`: a role box's key carries the role's name; a
     workspace box's key is unchanged from today; ordinals stay dense and
     the loser of a race takes the next one.
   - `boxes::resume_in`: *here* when served; else the latest that exists,
     ties by path; the sentence names it; none is a refusal naming `--id`.
   - `gc::home_verdict(evidence)`: the table above, one row per test.
   - `home::parse` of the host-side record: round trip; an in-home record
     and a legacy stamp adopt into one; a record with unknown keys still
     parses.
   - `registry`: an entry carries `key`; one without it falls back to the
     old computation.

   Then through the binary (`tests/lock.rs`, `tests/lifecycle.rs`):

   - `--id <id>` from a workspace with another basename takes the same
     lock file and starts with the box's own recipe, not the cwd's.
   - `--id` on a running box from another workspace is refused naming the
     pid and the workspace.
   - a bare `--role` start in a second workspace makes a new box under
     `"here"` and resumes under `"anywhere"`; the line says where it last
     ran.
   - the first start of a box with an in-home record writes the host-side
     one and removes the in-home files; `ps --all` lists it before and
     after.
   - `remove` takes the record with the home; `gc` calls a record without
     a home dead and a home without a record unproven.
   - `attach` finds the home through the registry entry's key.
   - the launch preview and the banner list the workspaces a shared box
     has served.

4. **Docs that change with the code.** `docs/src/guide/boxes.md` ("What
   each box keeps to itself", "Only starting the same box twice is
   refused", the upgrade note), `docs/src/guide/roles.md` (one box for
   several projects, `resume`), `CONCEPT.md` §3 and §6 ("`$HOME` per
   workspace" becomes per box), `AGENT.md` ("kept between boxes for this
   workspace" becomes "kept for this box"), `STATUS.md`, `CHANGELOG.md`,
   and the handbook's manifest reference for `[agent] resume`.
