# The panel: what it cannot reach, and what to build instead

A review of bare `wormhole` — the control surface — against the tool it is
supposed to control: every defect found, every feature and action the tool
has, which of them the panel can perform, and a rebuild that closes the
gap without giving up the purity rule the design is built on.

Evidence is `file:line` in this repository, or a transcript of the built
binary (`target/debug/wormhole`) run against a fabricated `XDG_DATA_HOME`.

Companion reviews: [role-ux-review.md](role-ux-review.md) (the mental
model), [shared-box.md](shared-box.md) (where `+N` came from).

## 0. The finding in one paragraph

The panel is not a control surface; it is a launcher with four extra keys.
Of the thirty-six things wormhole does, it performs ten, four more only
partly, and twenty-two not at all. The two defects a user feels first are the
same defect: **a box knows every workspace it has served, and no surface
shows them or lets you pick one.** The record carries the list
(`home.rs:45`), the table collapses it to `+N` (`home.rs:586`), and
`resume_in` chooses for you (`home.rs:433`). Under that sits a structural
cause: the panel's view is a `String` built without knowing the terminal's
width or height (`tui.rs:259`), so there is no viewport, no scrolling, no
truncation, no second pane to put the workspace list in — and no place to
put the other twenty-two either. Fix the view type and the rest
becomes ordinary work; leave it and every feature added from here is
another line appended to a string that already overflows an 80-column
terminal by 57 characters.

## 1. What the panel is today

```
crates/wormhole-core/src/tui.rs      1030 lines   the decisions, pure
crates/wormhole/src/panel.rs          185 lines   raw mode, keys, redraw
```

The split is right and worth keeping: `Tui::update(Key) -> Action` is a
pure state machine, `Tui::view(now) -> String` is a pure render, and the
binary only enters raw mode, decodes keys and writes bytes
(`panel.rs:1-3`). That is the reason the panel is testable at all, and
nothing below asks for it to change. What changes is the *type* of the
view.

Three screens exist: the box list (`Tui`), a single-column chooser
(`Picker`), and a text-plus-confirm screen (`panel::confirm`). The new-box
flow chains them: role → product → permission preview → start
(`main.rs:1077-1150`).

### The screen, as drawn

`Tui::view` is the `ps --all` table with a two-character cursor column
prepended to each line, then the problems, then one hint line
(`tui.rs:259-292`). Against a store holding four boxes and one unreadable
home, `ps --all` prints:

```
ID            NAME     ALIAS  STATE  ROLE          AGENT   LAST USED  WORKSPACE
a8fce893c0ae  Alphaca  -      idle   alphaca       codex   2h30m      /home/nabor/_projects/rtk
a4ae69443c54  Alphaca  -      idle   alphaca-java  grok    3d0h       /home/nabor/work/very/deeply/nested/monorepo/services/checkout +1
42858e6453f7  Alphaca  -      idle   alphaca       claude  never      /home/nabor/_projects/scentbird-backend
d18d506b4e34  Java     -      idle   -             claude  never      /home/nabor/_projects/wormhole
```

and the panel adds `> ` to one of those lines and

```

could not read:
  .../homes/proj-0123456789ab holds no box record

enter attach or resume   n new   d stop   x remove   r reset   q quit
```

Measured line widths for those rows: 79, 95, **135**, 109, 100 columns,
plus two for the cursor.

## 2. The inventory

Everything wormhole can do, from the dispatch (`main.rs:28-78`), the help
table (`help.rs:22-155`) and the manifest (`manifest.rs`). "Panel" is what
bare `wormhole` can perform today.

### Boxes

| Action | CLI | Panel |
|---|---|---|
| start a box here, resuming the last free one | `box` | `n` starts a **new** one only |
| start another box here | `box --new` | `n` |
| start this exact box | `box --id ID\|NAME` | Enter, in a workspace the panel chooses |
| **start it in a named workspace** | `cd DIR && box --id ID` | **no** |
| start from a role | `box --role NAME\|DIR\|REF` | `n`, installed roles only |
| choose the product | `box --run claude\|codex\|grok` | `n`, when the role lists several |
| name the box at start | `box --as NAME` | **no** |
| login mode for this start | `box --credentials none\|copy\|share` | **no** |
| declare a variable | `box --env NAME[=VALUE]` | **no** |
| run a command instead of the agent | `box -- CMD...` | **no** |
| list running boxes | `ps` | the list, mixed with idle |
| list every kept box | `ps --all` | the list |
| one box with its baked env | `ps ID\|NAME` | **no** |
| second terminal into a running box | `attach ID\|NAME` | Enter |
| attach running a command | `attach ID -- CMD...` | **no** |
| attach refreshing a variable | `attach ID --env NAME` | **no** |
| end a running box | `stop ID\|NAME` | `d` |
| name a box without starting it | `rename ID\|NAME NEW` | **no** |
| empty a box's home, keep the box | `reset ID\|NAME` | `r`, confirmed by `y` |
| take boxes away | `remove ID\|NAME...` | `x`, confirmed by `y`, one at a time |

### Roles

| Action | CLI | Panel |
|---|---|---|
| install a local role | `role add DIR [--as NAME]` | **no** |
| install a pinned remote role, with its recipe shown and approved | `role add URL@SHA [--as NAME]` | **no** |
| list installed roles and whether each can start | `role list` | the `n` picker lists them; startability is found on Enter |
| show a role's whole recipe | `role show NAME\|DIR` | the preview inside `n`, only on the way to starting |
| uninstall a role name | `role remove NAME` | **no** |

### Host, store, secrets, workspace

| Action | CLI | Panel |
|---|---|---|
| can this host run boxes | `doctor` | **no** |
| write a starter manifest here | `init` | **no** |
| build the image now | `build [--role ...]` | **no** (a start builds) |
| what the store holds and what can go | `gc` | **no** |
| delete what is proven dead | `gc --delete` | **no** |
| also delete what nothing references | `gc --delete --unreferenced` | **no** |
| list kept secret names | `secret list` | **no** |
| keep a secret, masked or piped | `secret set NAME` | asked inline at a start that needs one |
| forget a secret | `secret remove NAME` | **no** |
| the whole tool on one page | `help` | **no** |
| which wormhole this is | `version` | **no** |

**Count: 36 rows. Ten the panel performs, four it performs partly** (the
role list only as a side effect of starting, `role show` only on the way
to a start, `secret set` only when a start blocks on one, a bare resume
only if you find the row yourself), **and twenty-two it cannot reach at
all.** Every flag that changes what a start *is* — `--as`, the login
mode, `--env`, a command instead of the agent — is in the last group.

## 3. Defects

### T1 — a box's workspaces are recorded, collapsed to `+N`, and reachable from nowhere

`Record::earlier` holds every other workspace the box ran in, most recent
first (`home.rs:45`), and `Record::workspaces()` yields the lot
(`home.rs:108`). One surface consumes it:

```rust
match record.earlier.len() {
    0 => record.workspace.display().to_string(),
    more => format!("{} +{more}", record.workspace.display()),
}
```
`home.rs:584-587`

`+2` is the whole of what a user is told. There is no command that prints
the list — `ps ID` prints the same row (`main.rs:878-881`) — and no key in the
panel that shows it.

Choosing one is worse than invisible, it is decided elsewhere:

```rust
pub fn resume_in(record, here, exists) -> Result<PathBuf, String> {
    if record.serves(here) && exists(here) { return Ok(here.to_owned()); }
    record.workspaces().find(|w| exists(w)) ...
}
```
`home.rs:433-455`

Enter on an idle box takes the most recently served workspace that still
exists. The escape hatch is in the refusal text that only fires when
*none* exists: `wormhole box --id X` in the one it should run in. So the
answer to "how do I go back to that other tree?" is: quit the panel, `cd`
there, retype the id. The panel is the surface whose stated job is that a
kept box "can be reached from it" (`tui.rs:7-9`), and this is the one way
of reaching one it cannot express.

This was a deliberate first cut, recorded in
[shared-box.md:356-366](shared-box.md) — "None left is a refusal naming
`wormhole box --id X` in the workspace you want." It is the right default
and the wrong *only* option. A shared box (`resume = "anywhere"`) exists
precisely to serve several trees; the surface that lists it must be able
to say which.

### T2 — the cursor is an index into a list that re-sorts under it

`Tui::selected` is a `usize` (`tui.rs:16`), moved by `step`
(`tui.rs:39-46`) and clamped on refresh (`tui.rs:162-166`). The list it
indexes is sorted by `started_unix` descending (`home.rs:368`, applied at
`home.rs:500`), and the panel re-scans once a second (`panel.rs:43-46`).

So: another terminal starts a box, the sort changes, and the row under the
cursor becomes a different box. `x` and `r` ask a question that names the
id, so they fail visibly. `d` does not ask — it stops what is under the
cursor at the moment of the press. A user watching row 3 and pressing `d`
can stop row 3's *new* occupant.

The fix is not a confirm on `d`: stop is reversible and a confirm there is
friction over nothing (`tui.rs:116-121`, and it is right). The fix is that
a cursor should name a box, not a row.

### T3 — the view is a `String` and does not know the terminal

```rust
pub fn view(&self, now_unix: u64) -> String
```
`tui.rs:259`

No width, no height. Consequences, all present today:

- **No truncation.** The row measured in §1 is 135 columns wide. On an
  80-column terminal it wraps, and the `> ` cursor then marks the first
  half of a logical row while the second half occupies the next screen
  line with no cursor. The alignment `table::render` computes
  (`table.rs:7-30`) is destroyed by the wrap it cannot see.
- **No viewport.** Every box is rendered. Twenty boxes on a 24-line
  terminal push the hint line, the problems and the note off the screen,
  and moving the cursor below the fold moves it out of sight. There is no
  scroll offset anywhere in the file.
- **No layout.** There is one column of text, so there is nowhere to put a
  detail pane — which is where T1's workspace list, the role source, the
  pin, the image digest and the baked env belong.

### T4 — a resize is not redrawn

`next_key` returns `None` for every non-key event, resize included
(`panel.rs:118-130`), and `draw_text` returns early when the rendered
string is unchanged (`panel.rs:150-153`). A resize changes the terminal,
not the string. The panel therefore keeps whatever the old geometry left
on screen until something else changes the text — up to one second for a
scan tick, or forever on a store that is not changing.

### T5 — the panel is a launcher that exits

`Pick::Attach` and `Pick::Resume` both leave for good: `attach_box` and
`resume_from_panel` are `-> !` (`main.rs:1452`, `main.rs:1059`). Detach
from the agent and you are at your shell, not back at the panel.

`CONCEPT.md:356` states the intended shape: "The TUI is not a launcher
that exits. It is the live control plane: **`Ctrl-\` toggles** between the
agent and a panel that shows the active grants *while the agent runs*."
`PLAN.md` Step 12 names the mechanism — PTY master plus a `vt100` screen
model, restored with `contents_formatted()`. None of it is built: the
boundary `fork`s and `execvp`s with the terminal inherited
(`boundary.rs:519`, waited at `:531`); there is no `openpty` in the binary
outside the test harness.

### T6 — eight columns is everything a box ever says

`ID NAME ALIAS STATE ROLE AGENT LAST USED WORKSPACE`. Not shown anywhere,
for an idle box: the role's source and pin (`Record::source`), the created
time, the served workspaces (T1), the recipe digest, whether its image is
built, its grants, its credentials mode, its limits. `ps ID` adds the
baked env, and only while the box runs (`main.rs:882-886`).

Two of the eight columns are `-` in the transcript above. The two most
expensive columns by width carry the least.

### T7 — the new-box flow cannot say most of what a start can say

`new_box_from_panel` builds exactly `--new [--role R] [--run P]`
(`main.rs:1135-1143`). No `--as`, no `--credentials`, no `--env`, no
`-- CMD`. The preview screen shows the credentials mode and the declared
variables as facts about the manifest (`tui.rs:392-423`) and offers no way
to override either for this start, though `box` accepts both.

### T8 — roles, the store, secrets and doctor have no surface at all

A role that is listed but not installed-and-approved is shown, then
refused on Enter with the reason (`main.rs:1106-1113`) — correct, and a
dead end: `role add` is not reachable from here. The same for every other
noun. `gc` is the one command that reports disk in gigabytes and the one
most in need of a list you can select rows in; it has no screen.

And the panel dies rather than degrades: with no `wormhole.toml` here and
no installed roles, `n` calls `fail()` and takes the process down
(`main.rs:1080-1085`) — the box list goes with it.

### T9 — one hint line is the whole of discoverability

`enter attach or resume   n new   d stop   x remove   r reset   q quit`.
No `?`, no per-row context menu, no way to see what a key will do before
pressing it. The binding table and the hints are pinned against each other
by a test (`tui.rs:906-928`), which is good and is not the same as being
discoverable.

`j/k` and the arrows work; `g`/`G`, `ctrl-d`/`ctrl-u`, Home/End, PageUp/
PageDown do not (`panel.rs:134-143`, `tui.rs:54-86`).

### T10 — problems are capped at five and the rest are unreachable

`PROBLEMS_SHOWN = 5` and "... and N more" (`tui.rs:35`, `tui.rs:270-283`).
The cap is right — an unreadable store must not push the box list off the
screen — and there is no key that shows the rest.

### T11 — no filter, no search, no sort

One order (recency), no way to narrow. A host with thirty boxes across ten
trees is the case this tool is for, and the only way to find one is to
read.

### T12 — a cancelled question says nothing

Any key but `y` clears the question (`tui.rs:178-183`), and the clearing
happens by setting `note = None` at the top of `update`. Pressing `j` to
move away from a remove prompt looks exactly like pressing `j` with no
prompt up. The panel already knows that two different outcomes must not
look the same (`tui.rs:17-23`) and does not apply it here.

### T13 — the whole store is re-read once a second, and not at all while keys arrive

`box_listings` reads every record file and walks the boxes directory
(`main.rs:1020-1030`). That is one full scan per second while the panel is
up. And the scan happens only on the *timeout* branch (`panel.rs:43-46`):
hold `j` down and the list never refreshes.

### T14 — every screen is a bespoke type

`Tui` and `Picker` each carry `selected`, each implement `update` and
`view`, each have their own `Action` enum. They share `step` and `cursor`
and nothing else. A tabbed surface with a detail pane, a text input, a
palette and a scrollable table is six more of those, each with its own
cursor-clamping bug to write and test.

## 4. Prior art

| Tool | What it does that is relevant |
|---|---|
| **k9s** | `:` command mode reaches any resource view by name, `/` filters (regex or fuzzy), `?` lists active mnemonics, `<esc>` leaves any mode. One list, many views, reached by typing rather than by a key per noun. ([k9scli.io/topics/commands](https://k9scli.io/topics/commands/)) |
| **lazygit** | Context panels side by side; `?` opens keybindings for the current panel; `<esc>` cancels; `<`/`>`/`J`/`K`/`ctrl-d`/`ctrl-u` for movement. Keys are scoped to the focused panel, so the same letter means the right thing in each. ([keybindings docs](https://github.com/jesseduffield/lazygit/blob/master/docs/keybindings/Keybindings_en.md)) |
| **ratatui** | The Rust standard for this shape: immediate-mode drawing into a double-buffered cell grid with diffed output, `Table`/`List` with their own scroll state, constraint layout, unicode-aware truncation. 0.30.2, MSRV 1.88. ([crates.io](https://crates.io/crates/ratatui)) |
| **Elm architecture / Bubble Tea** | Model–Update–View with the view a pure function of the model. Already what `tui.rs` is; worth naming because the rebuild keeps it. |

The pattern common to k9s and lazygit is the one this panel needs: **a
small number of keys for movement and the frequent verbs, plus one key
that opens everything else**, so the surface grows without the key table
growing.

## 5. The rebuild

Five changes. The first is structural and the other four depend on it.

### 5.1 The view becomes data, not text

```rust
pub struct Viewport { pub width: u16, pub height: u16 }

pub enum Screen {
    Boxes(BoxesView),
    Roles(RolesView),
    Store(StoreView),
    Secrets(SecretsView),
    Doctor(DoctorView),
}

pub struct BoxesView {
    pub rows: Vec<Row>,          // cells, plus an emphasis per row
    pub cursor: Option<usize>,   // the row the identity cursor resolved to
    pub window: Range<usize>,    // the rows this viewport shows
    pub detail: Vec<Field>,      // name/value for the selected box
    pub hints: Vec<Hint>,        // (keycap, what it does) — generated, not typed
    pub note: Option<Note>,      // Info | Refusal | Question
    pub overlay: Option<Overlay>,// Confirm | Pick | Input | Palette | Help
}

pub fn view(&self, viewport: Viewport, now_unix: u64) -> Screen;
```

Still pure: `Viewport` is an input, not a syscall. What it buys:

- Scrolling, truncation and elision become decisions in `wormhole-core`,
  which is where the purity rule already says decisions live
  (`CONCEPT.md:368-372`, "The TUI holds no logic").
- Tests assert on structure — `view.window`, `view.rows[2].cells[7]`,
  `view.detail` — instead of `screen.contains("not running")`. Strictly
  stronger than what `tui.rs`'s 30 tests do today, and the existing tests
  port over as assertions on cells.
- The binary is free to render with ratatui without holding a decision.

### 5.2 The cursor names a box (fixes T2)

`selected: Selection` where `Selection` is a box id, resolved to a row
index at view time, falling back to the nearest surviving row when the box
is gone. A re-sort moves the cursor with the box. The failing test that
drives it: refresh with a list whose order changed, assert the cursor is
still on the same id, assert `Key::Stop` produces that box's pid.

### 5.3 The workspace, in full (fixes T1)

Three parts, smallest first.

**a. The detail pane lists them.** `Record::workspaces()` already yields
them in order; each gets a mark for whether it still exists (the
`exists: impl Fn(&Path) -> bool` shape `resume_in` already uses keeps the
I/O out of core). The `+N` cell in the table stays as the hint that there
are more — it is honest, it is compact, and with a detail pane beside it,
it is no longer a dead end.

**b. `w` opens a workspace picker for the selected box.** Items: every
served workspace that still exists, most recent first, marked with the one
`resume_in` would have chosen; plus `here`, when `here` is a workspace this
box may legally serve. Enter resumes there.

**c. The binary `cd`s and does what the refusal already tells you to do.**
`resume_from_panel` grows a workspace argument; it already calls
`set_current_dir` before `run_box` (`main.rs:1067-1071`). No new CLI flag:
`CONCEPT.md:352` says "No subcommand takes a workspace argument. The
workspace is where you are", and that rule stays true — the panel is
standing you in the directory, not passing it to `box`.

Which workspaces may be offered is a rule, so it is pure and tested:

- a box made from a workspace's own manifest: only its one workspace —
  `refused_elsewhere` already says why (`home.rs:377-389`);
- a role box: any workspace it has served that still exists;
- plus `here`, for a role box, because `wormhole box --id X` in a new tree
  is the sanctioned way to widen one ([shared-box.md:345](shared-box.md));
- none of them exist: the refusal that exists today, unchanged.

Offering `here` needs the same weight as a grant: the preview screen, or at
minimum a line naming the tree the agent is about to be pointed at.

**d. `ps ID` prints the list too.** A fact reachable only from a TUI is a
fact no script can have.

### 5.4 Every action reachable (fixes T7, T8, T9, T11)

**Tabs for the nouns.** `1`–`5` or the palette: Boxes, Roles, Store,
Secrets, Doctor. They are the CLI's own noun groups (`help.rs`), so the
surface and the help page stay the same shape.

- **Boxes** — the list and detail above; Enter attach/resume, `w`
  workspace, `s`/`d` stop, `x` remove, `r` reset, `N` rename, `e` env
  (the baked table `boxenv::table` already renders), `n` new.
- **Roles** — `role list` as a table (name, source, pin, startable);
  Enter shows the whole recipe (`tui::preview`, already written); `a` adds
  one, which for a pinned ref is the existing fetch-and-approve flow, on a
  screen instead of on stdout; `x` removes the name.
- **Store** — `gc`'s items, verdicts and sizes, biggest first, exactly as
  `gc::plan`/`gc::report` compute them today (`gc.rs`); select rows,
  `x` deletes with the same confirm, `u` toggles `--unreferenced`. This is
  the screen `gc` has always deserved.
- **Secrets** — names only, never values (`secrets.rs`); `s` sets one
  through a masked input; `x` forgets one.
- **Doctor** — `doctor::evaluate(probes::run_all())` rendered per probe
  with its own verdict, `r` re-runs.

**A command palette for the rest.** `:` opens a fuzzy list of every verb,
bound to the current selection, showing the exact command line it will
run. This is what makes "all functionality through the TUI" true without a
key per action, and showing the command line is what keeps the TUI from
becoming a second, divergent way to describe what the tool does.

**A form for the parameterized starts.** The `n` chain gains an editable
summary before the preview: role, product, alias, credentials mode, env
overrides, command. Defaults are the manifest's; what you change shows as
the flags it becomes.

**`?` opens help, generated from the binding table.** `Key::from_char` is
already the single source (`tui.rs:75-86`) and a test already pins the
hints against it; generating both from one table replaces the test with an
impossibility.

**`/` filters** the focused list by substring across id, alias, name, role
and workspace; `<esc>` clears.

### 5.5 The renderer: ratatui, measured

Hand-rolling is in this project's character — it owns its terminfo
(`terminfo.rs`), its table (`table.rs`), its seccomp filter
(`seccomp.rs`). The question is whether what a renderer does is worth
owning, and the measurement is what decides it.

**Cost, measured.** `cargo add ratatui --no-default-features --features
crossterm,underline-color` against a probe crate, then the delta against
`cargo tree -e normal -p wormhole`:

```
wormhole today                     57 crates
ratatui adds                       33 crates
                                   ---
                                   90 crates
```

The 33: `ratatui`, `ratatui-core`, `ratatui-widgets`, `ratatui-crossterm`,
`kasuari`, `compact_str`, `castaway`, `hashbrown`, `foldhash`,
`allocator-api2`, `equivalent`, `lru`, `itertools`, `either`,
`unicode-segmentation`, `unicode-truncate`, `line-clipping`, `strum`,
`strum_macros`, `derive_more`, `derive_more-impl`, `darling`,
`darling_core`, `darling_macro`, `ident_case`, `strsim`, `convert_case`,
`heck`, `instability`, `indoc`, `rustversion`, `ryu`, `static_assertions`.

**One thing that does not cost anything:** ratatui-crossterm 0.1.2
resolves to **crossterm 0.29**, the version wormhole already depends on.
No duplicate crossterm, no upgrade forced.

**What owning it would mean writing:** a cell grid, a diff against the
previous frame, cursor-relative output, unicode width and grapheme-aware
truncation, scroll-state arithmetic per list, resize handling, and a
constraint or ratio layout. That is between five hundred and a thousand
lines whose failure modes are precisely T3 and T4 — the two defects this
review found by reading, in the hundred lines of rendering that exist.

**Recommendation: take ratatui.** The purity rule is satisfied by §5.1,
not by owning the renderer: with the view as data, every decision is still
in `wormhole-core` and still tested without a terminal, and ratatui never
sees a decision — it sees a `Screen`. The dependency argument deserves a
straight answer rather than a dismissal: these 33 crates are compiled into
the binary that builds the boundary, so they are in the trust base. They
are also the most-exercised TUI stack in Rust, and the alternative is not
*no* code — it is the same code, written here, exercised by one project.

**The tradeoff, stated so it can be rejected:** if the trust base is the
overriding constraint, the honest alternative is not a hand-rolled ratatui.
It is a smaller design — one list, one detail pane, no tabs, no palette —
rendered by the ~200 lines that shape genuinely needs. That design cannot
hold §5.4. Pick the design first; the renderer follows from it.

### 5.6 What this does not build

**The `Ctrl-\` supervisor (T5) is a separate step and should stay one.**
Making the panel live while the agent runs means wormhole becomes the PTY
master for the box's whole life: `openpty`, a `vt100` screen model,
`contents_formatted()` restoration, `SIGWINCH` propagation, and a new
failure mode where wormhole dying takes the agent's terminal with it. The
design is already written (`CONCEPT.md:356-366`, `PLAN.md` Step 12) and it
is a larger change than everything above put together. Everything above is
worth having without it, and none of it is wasted by it: a `Screen` is a
`Screen` whether it is drawn on its own terminal or over a suspended
agent.

**What is worth doing now, cheaply:** make Enter *return*. `attach_box`
and `resume_from_panel` are `-> !` because they exec. Forking, waiting and
redrawing the panel afterwards turns a launcher into something you come
back to, without a PTY anywhere. It costs the exec's cleanliness, which is
a real loss and a small one.

## 6. Order, each step with the test that drives it

**Steps 1–12 are built.** What steps 1–6 cost and turned up is in §8;
what 7–12 cost and turned up is in §9.

| # | Step | The failing test first |
|---|---|---|
| 1 | ~~`Selection` by box id~~ **done** | a refresh that re-sorts leaves the cursor on the same id |
| 2 | ~~`Viewport` in, `Screen` out; port the existing view~~ **done** | a list longer than the viewport yields a `window` that contains the cursor; a row wider than the viewport is truncated with a mark |
| 3 | ~~ratatui renderer in `panel.rs`, `Event::Resize` redraws~~ **done** | on a pty, a resize redraws (the panel suite already drives a real pty) |
| 4 | ~~detail pane, with the workspace list~~ **done** | a box with two earlier workspaces shows three, marked for existence |
| 5 | ~~`w` workspace picker + `resume_from_panel(id, workspace)`~~ **done** | picking the second entry starts the box there; a workspace-manifest box offers only its own; `here` is offered only when the box may legally serve it |
| 6 | ~~`ps ID` prints the workspace list~~ **done** | a record with `earlier` prints every one |
| 7 | ~~`?` help and hints generated from one binding table~~ **done** | every binding has a hint and every hint a binding, by construction |
| 8 | ~~`/` filter, `g`/`G`/PageUp/PageDown~~ **done** | a filter that matches nothing keeps the cursor valid |
| 9 | ~~tabs: Roles, Store, Secrets, Doctor~~ **done** | each renders from the same pure function the command prints from |
| 10 | ~~the `n` form: alias, credentials, env, command~~ **done** | the form's state renders to the exact argv `box` would take |
| 11 | ~~`:` palette~~ **done** | every dispatched command is in the palette (read off the dispatch, like the help test at `main.rs:1699`) |
| 12 | ~~cancelling a question says so~~ **done** (in step 1) | `Key::Down` on an open question leaves a note that names the cancel |

Steps 1–6 close what this review was opened for. 7–12 close the rest.

## 7. Where this proposal is weakest

- **33 crates.** §5.5 argues it and does not make it free. If the answer
  is no, §5.4 has to shrink, and the shrinking should be explicit rather
  than attempted and abandoned halfway.
- **`Screen` is a second interface to maintain.** Today the tests assert
  on the string a user sees, which is the truest thing to assert on. After
  this they assert on a structure and a renderer stands between it and the
  user, untested by them. The mitigation is the pty suite that already
  exists (`tests/panel.rs`) — it must grow, not stay at two tests, or this
  trade is a downgrade.
- **Offering `here` in the workspace picker widens a box's reach with a
  keystroke.** `wormhole box --id X` in a new tree is deliberate and typed.
  A picker entry is not. It needs the preview, and if that feels heavy, the
  right answer may be that the picker offers only workspaces already
  served and widening stays a typed command.
- **A palette that lists everything can run `gc --delete` from a fuzzy
  match.** Every destructive verb needs its confirm inside the palette
  path, not only on its key.
- **Tabs may be one abstraction too many.** Five nouns, and four of them
  are visited rarely. A palette alone might reach all of them with no tab
  bar at all. The cheap experiment is to build the palette first (step 11
  early) and see whether the tabs are still wanted.
- **Step 3 changes what "the panel" is before steps 4–12 make it worth
  it.** If work stops after step 3, the tool has 33 more dependencies and
  the same six actions.

## 8. What steps 1–6 turned up

Built as planned, with four things the plan did not predict. Each was a
defect the old renderer had and the tests could not see.

**The pty in the test harness had no window size.** `openpty(None, None)`
makes a terminal that reports zero by zero. The old panel wrote lines
blindly and worked; a panel that measures its surface drew nothing at all,
for as long as it was up. That is not only a test problem — a `wormhole`
started under automation gets exactly that terminal. Two fixes: the
harness now gives its pty a size (80×24), and a terminal that cannot be
measured is drawn at a fixed fallback rather than at nothing.

**The pty tests were asserting on bytes, not on a screen.** A drawn screen
writes only the cells that changed and moves the cursor over the rest, so
`x` and `remove` arrive with an escape sequence between them and the space
between them is never written. `common::shown` turns the stream back into
the words on the screen, and the assertions moved onto that.

**One test had been passing on the defect it was meant to prevent.**
`approving_on_a_terminal_installs_the_role` asserted that the recipe's
`[image] build` lines reached the screen — and they did reach the *byte
stream*, drawn past the bottom of a 24-line terminal where no reader could
see them. Paging the preview made the test fail, which is the first time
it told the truth. It now scrolls to the end before asserting, so what it
proves is that those lines can be *reached*.

**The gap between two hints was 2 in the budget and 3 in the renderer.**
Three characters of overflow, and `q quit` came out as `q` on an
eighty-column terminal. `tui::GAP` is now one number both sides use. The
general lesson is the one §5.1 was built on: every measurement belongs on
one side of the line, and the renderer is not that side.

Two decisions were taken against the plan as written:

- **`Field` carries a finished line**, not a name and a value. Which end
  of a value is dropped when it does not fit is a decision — a path keeps
  its tail, everything else keeps its head — and §5.1's whole point is
  that decisions do not live in the renderer.
- **Hints degrade to keycaps** rather than being cut. The hints are the
  list of what can be done on the screen; half a word loses a key without
  saying that it has.

### What is measurably different

| | Before | After |
|---|---|---|
| Widest drawn row on an 80-column terminal | 137 columns, wrapped | ≤ 80, by test, at every width from 1 |
| A box's other workspaces | `+2` | named, marked `(last used)`/`(gone)`, in the panel and in `ps <id>` |
| Choosing one | quit, `cd`, retype the id | `w` |
| List longer than the screen | ran off the bottom, cursor with it | scrolls, cursor always visible, `3-9 of 22` |
| Resize | ignored until something else changed | redraws, proven on a real pty |
| Cursor after a re-sort | a different box | the same box |
| Permission preview past the fold | written off-screen | scrolls |
| Cancelling a question | looked like an unknown key | says `cancelled; nothing was touched` |
| Dependencies | 57 crates | 90 crates, one `crossterm` (0.29), no duplication |
| Tests | 545 | 571 |

### Still open

Steps 7–12. The panel reaches boxes and nothing else: roles, the store,
secrets and `doctor` have no screen, `n` still cannot say `--as`,
`--credentials`, `--env` or a command, there is no `?` and no filter, and
Enter still execs away instead of returning. Nothing built here forecloses
any of it — a `Screen` is a `Screen` whichever tab produces it.

## 9. What steps 7–12 turned up

Steps 7–12 were the rest of the tool: the keys on a screen, the list
narrowed, the other four nouns, the whole of a start, and every command on
one list. Three things the plan did not predict.

**The binary was still deciding what a letter meant.** Step 8 wanted a
filter, and a filter is typed with the same letters the keys are: on a
screen where `q` leaves and `w` picks a folder, searching for `qw` is two
commands. The old split had the binary decoding characters into commands
(`Key::from_char`) before the core ever saw them, so the core could not
have decided otherwise. The binary now reports what the terminal said
(`Press::Typed('w')`) and the core says what it means, given the screen.
That is the same division every other decision here already followed; it
was simply not finished.

**One table, or three screens that drift.** The hints, the decoder and
`?` were three lists of the same facts. They are now one `BINDINGS` table
with a screen-set per row, which is what makes `r` reset a box on one tab
and re-run the probes on another without either screen guessing. The old
drift test — "every keycap the hints show is a key the panel decodes" —
became true by construction, so what it now pins is that both still read
the one table.

**The hint line could not hold the panel any more.** Twelve keys will not
fit eighty columns with their words. The first answer, dropping the words,
lost too much; the second, dropping keys off the end, lost a key with
nothing saying so. They now wrap to a second line, and only when that
cannot hold them either do the words go — every keycap survives at every
width that can draw them.

Two boundaries were drawn on purpose rather than built:

- **`a` installs a local role only.** Fetching a stranger's repository and
  approving the shell it would run on this machine stays `wormhole role
  add <url>@<sha>`. That approval is the whole defence for a role somebody
  else wrote, and it should cost a typed command rather than a keystroke.
  The panel refuses a remote ref with the command that does it.
- **The palette runs the four printing commands as themselves.** `init`,
  `build`, `version` and `help` are run in a child and read on a screen,
  rather than being reimplemented in the panel. The palette shows the
  command line; running anything but that line would make the line a lie.

### What is measurably different

| | After steps 1–6 | After 7–12 |
|---|---|---|
| Actions reachable from the panel | 10 of 36 | every one the dispatch answers to, by test |
| Nouns with a screen | boxes | boxes, roles, store, secrets, host |
| Finding a key | one hint line | `?` on every screen, generated |
| Finding a box | read the list | `/` |
| A start from `n` | role and product | every flag `box` takes, as the line it runs |
| Tests | 571 | 594 |

### Still open

The `Ctrl-\` control plane of `CONCEPT.md` §9: Enter on a box still execs
away, so the panel is left rather than returned to. That needs wormhole to
become the PTY master for the box's whole life, which §5.6 scoped out
deliberately and nothing here forecloses.

## 10. Enter became the wizard

After steps 1–12, choosing a workspace was `w`. That closed the hole only
for somebody who had found `w`; Enter — the reflex — still picked one
silently, which is where T1 started. So Enter now asks, and `w` is gone.

**The rule.** Enter on a running box joins it: there is nothing to choose,
the box is already somewhere. Enter on an idle box opens the list of
places it can start, every time, including when there is only one. That
was a deliberate choice over asking only when the list has two or more —
a screen that sometimes appears is a screen you cannot build a habit on,
and the cost of always asking is bounded by the next paragraph.

**What keeps it from being friction.** The cursor starts on the entry a
start that asked nobody would have taken, which is `home::resume_in`'s
answer — the folder you are standing in when the box has served it,
otherwise the one it used last. So the old one-keystroke path is now two
keystrokes, both Enter, and everything else that was unreachable is one
cursor move away. A test pins that: for every directory the cursor's entry
equals `resume_in`'s.

**What moved into core.** The binary used to build the item strings, glue
on `"(here — this box has never run here)"`, and recover meaning from the
picked index with `choices.served.get(at)`. `home::Choices` now returns
`Vec<Choice>` with `latest`/`here`/`fresh` flags, a `line()` that writes
the marks, and the index the cursor starts on. The binary picks an index
and reads the flags back — no wording, no arithmetic. That was §9's
altitude finding 5, closed here rather than separately.

**What went.** `w`, `Key::Workspace`, `Action::Resume`, `Pick::Resume` and
`resume_from_panel`. Enter and `w` did the same thing, and a second key
for one thing is a key that starts meaning something else.

| | Before | After |
|---|---|---|
| Keystrokes to resume where it last ran | 1 (Enter) | 2 (Enter, Enter) |
| Keystrokes to resume somewhere else | quit, `cd`, retype the id | Enter, move, Enter |
| Discoverable without reading anything | no | yes |
| Keys on the box tab | 8 | 7 |

## 11. Enter returns

The cheap step §5.6 named is built. `tui()` no longer calls anything that
is `-> !`: Enter, the workspace picker and the `n` form each hand an
argument list to `session`, which runs this same binary as a child on the
terminal the panel just gave back, waits, and lets the loop draw the panel
again. One body, the same shape `Host::run` already had for the palette.

What it costs is what §5.6 said: one more `wormhole` process alive for the
session's life, and `Ctrl-C` now reaching two processes. The second is
handled the way `boundary` already handled it for `__boxed` and `attach`:
`SIGINT` and `SIGQUIT` are ignored while the child runs — and, new here,
restored after, because this waiter goes on living.

A session that exits non-zero prints to the terminal and the panel would
cover it, so the panel waits for a key under the message before it comes
back. On a real pty: Enter, Enter into a box whose folder has no manifest
fails at once, the pause is shown, a key brings the panel, `q` leaves it.

Still open, and still a separate step: `Ctrl-\` while the agent runs (T5).
