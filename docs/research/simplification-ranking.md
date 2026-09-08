# Simplification candidates: what is left, and in what order

A check of nine simplification candidates against the working tree on
2026-09-08, with what each would remove, what it would break, and which
help text and handbook pages it touches.

Evidence is `file:line` in this repository. Every line number is from the
working tree, not from `HEAD`: the tree carries one uncommitted change set
of 38 files (+1661, -1960 against `d52b196`), and that change set is where
almost every item on the list was done.

## 0. The finding in one paragraph

The list is stale. Eight of the nine items are done, all of them in the
uncommitted working tree, and `CHANGELOG.md:12-59` already tells the
story under "Unreleased". `HEAD` (`d52b196`) still has `wormhole run`,
`wormhole env`, `wormhole tui`, `Probe::MicroVmDevices`, `receipt.rs`,
`[env] required` and `secret`, and a 2696-line `main.rs`; the tree has
none of them, and 455 tests pass on it. What is left is two things: the
handbook still renders `CONCEPT.md` and `PLAN.md` unchanged, and both
still promise a `MicroVM` boundary the code no longer has a port for; and
`main.rs` is 1603 lines that fall into four more modules nobody has cut
yet. The first is wrong and small. The second is not wrong, only large.

**So: not "which of these to do" but "commit what is done, fix the two
documents that now lie, and decide whether a 1600-line `main.rs` is a
defect."**

## 1. Status

| Item | Status | Evidence |
|---|---|---|
| a1. Codex role bakes rtk and codex with a placeholder digest | done, uncommitted | The only `[[image.artifact]]` left is rustup (`roles/alphaca-codex/wormhole.toml:51-54`); the preflight fetches both into `~/.local/bin` (`roles/alphaca-codex/hooks/preflight.sh:17-52`). `HEAD` had the `0000…` digest (`git show HEAD:roles/alphaca-codex/wormhole.toml`, one match). `crates/wormhole/tests/manifests.rs:39` now refuses any shipped placeholder digest |
| a. One box table for `ps`, `ps --all`, panel | done, uncommitted | `home::list` is the one table, eight columns (`crates/wormhole-core/src/home.rs:424-466`); `ps` prints it for all three modes (`crates/wormhole/src/main.rs:745-765`); the panel prints its lines with a cursor (`crates/wormhole-core/src/tui.rs:265-269`). `table.rs` is the renderer only (`crates/wormhole-core/src/table.rs:7`). `HEAD` had `ps` columns of its own in `main.rs` |
| b. Drop "tui", fold env ID into `ps` ID | done, uncommitted | Dispatch has no `"tui"` and no `"env"` (`crates/wormhole/src/main.rs:30-66`; `HEAD` had both at its lines 48 and 50). `ps <id>` prints the row and the env table (`main.rs:756-764`, `crates/wormhole-core/src/boxenv.rs:296-298`). Help says so (`crates/wormhole-core/src/help.rs:68-72`). Only internal names keep the word: `fn tui()` at `main.rs:871`, the `wormhole_core::tui` module, `STATUS.md:115` |
| c. Remove `Boundary::MicroVm` and `Probe::MicroVmDevices` | done in code, docs stale | `rg MicroVm crates/` finds nothing; `rg -i kvm crates/` finds nothing. `HEAD` had `Probe::MicroVmDevices` and `/dev/kvm` (`git show HEAD:crates/wormhole/src/probes.rs`, its lines 32 and 107-108). `CHANGELOG.md:52-53` records it. There was never a `Boundary` enum in the binary: `MicroVm` lived in `probes.rs` and in the concept documents, and those are the part still open (see 2.1) |
| d. `run` internal, or `--grant` only | done, uncommitted | Public `run` is gone; `__run` takes its place and is marked as the kernel tests' entry point (`main.rs:52-62`). The eight flags stay because `to_argv` rebuilds them for the `__boxed` re-exec (`crates/wormhole-core/src/run.rs:254-290`). The help-page test skips `__` names (`main.rs:1571`). `tests/kernel.rs` drives `__run` throughout, behind `#![cfg(feature = "kernel-tests")]` (`kernel.rs:3`), so that file ran 0 tests here |
| e. `[env] required` and `secret` | done, uncommitted | Refused by name (`crates/wormhole-core/src/manifest.rs:1139-1142`); `ask` alone marks a secret and is refused beside a value (`manifest.rs:213-217`, `manifest.rs:502`). `HEAD` `manifest.rs` had 18 matches for the two words |
| f. `[runtime] snapshot` drags `receipt.rs` | done, uncommitted | `receipt.rs` is staged for deletion (`git status`: `D crates/wormhole-core/src/receipt.rs`); `rg snapshot crates/` finds nothing; `CHANGELOG.md:43-46` says the `snapshots/` directory can be deleted by hand |
| g. Split `main.rs` into lock, seed, roles | partly done | `lock.rs`, `seed.rs`, `roles.rs` exist and are staged as added (`git status`: three `A` lines; `main.rs:1-8` declares them). `main.rs` went from 2696 lines at `HEAD` to 1603. What is left is in 2.2 |
| h. Banner prints only non-default parts | done, uncommitted | `launch::banner` returns `None` for a baseline start and names only `dns`, `root: readonly` and a grant count (`crates/wormhole-core/src/launch.rs:230-244`); `run_box` prints it only when `Some` (`main.rs:364-366`). The env line likewise (`main.rs:283-285`). `HEAD` printed `boundary: namespaces (host kernel SHARED) · …` on every start (its `launch.rs:264,454`). Pinned by `launch.rs:415-430` and `450-459` |

The test run on the tree: 455 passed, 0 failed (`cargo test --workspace`;
355 in `wormhole-core`, 100 across the binary's unit and integration
tests). `kernel.rs` is feature-gated and did not run.

## 2. Open items, ranked

Ranked by whether the current state is wrong, then by how much a fix
removes, then by risk. Not by effort.

### 2.1 `CONCEPT.md` and `PLAN.md` still promise the MicroVM stage

**Wrong; fixed in the same change set.** `docs/src/concept.md:9` is `{{#include ../../CONCEPT.md}}` and
`docs/src/plan.md` includes `PLAN.md` the same way, so both are handbook
pages. `CONCEPT.md:37` says `MicroVM` "Ships second"; `CONCEPT.md:430`
says "`Namespaces` and `MicroVM` implement" a `Boundary` port;
`CONCEPT.md:436` calls it "a stated deliverable"; `CONCEPT.md:460` lists
it as stage 2; `CONCEPT.md:47`, `:395` and `:410` say the banner names
the stage that runs.
`PLAN.md:296` keeps the `MicroVM` boundary row, and `PLAN.md:78` says the
banner "renders the live boundary". None of that is true of the tree: no
`Boundary` port exists (`rg -w Boundary crates/` finds no type), the
banner names no stage (`launch.rs:230-244`), and `CHANGELOG.md:52-53`
says the stage was removed. A reader of the rendered book is told two
opposite things by two pages of the same book.

Proposal: edit the eight `CONCEPT.md` passages and the two `PLAN.md` lines
to say there is one boundary, namespaces, and that a second one is not
planned; or, if a guest kernel is still wanted some day, say it is an
idea and not a port, and drop the banner claim. Removes about 10 lines
of prose. Risk: none in code. `tests/wiki.rs:213` renders the book and
fails on a broken link, so a rewrite that removes a heading another page
links to would fail there; nothing links to these passages today.

### 2.2 `main.rs` is 1603 lines in ten groups

**Not wrong, but item g is only a third done.** The three named modules
exist. The rest of `main.rs` splits cleanly by the function list
(`main.rs:76-1603`):

| Group | Lines | Functions |
|---|---|---|
| build and image | 76-213 | `build`, `init_cmd`, `ensure_image`, `fetch_artifacts`, `claim_build` |
| start | 214-384 | `run_box` |
| records and registry | 384-528 | `kept_boxes`, `read_record`, `write_record`, `register_box`, `replace_file*` |
| gc | 529-738 | `gc_cmd`, `running_image`, `extend_referenced`, `read_dir`, `tree_bytes` |
| listing | 739-870 | `ps`, `print_boxes`, `live_boxes`, `box_dir_pid`, `alive`, `reap` |
| panel | 871-1017 | `tui`, `box_listings`, `panel_act`, `resume_from_panel`, `new_box_from_panel` |
| lifecycle | 1018-1310 | `stop_box`, `stop_cmd`, targets, `remove_cmd`, `reset_cmd`, `rename_cmd`, `box_alias`, `running_box` |
| attach | 1309-1380 | `attach`, `attach_box`, `init_pid`, baked env read/write |
| secrets | 1380-1515 | `secret_cmd`, store read/write, `ask_secrets`, `prompt_secret`, `refuse_unfilled` |
| plumbing | 1515-1603 | xdg dirs, `fail`, `usage`, the help-page tests (`mod tests` at 1559) |

Proposal: cut four more modules, `gc.rs`, `lifecycle.rs`, `listing.rs`
(ps, panel glue, registry scan) and `secrets.rs`, on the same pattern
as `roles.rs`. Moves about 900 lines; removes nothing. Risk: low. Every
integration test drives the binary, so no test changes; the two help-page
tests at `main.rs:1559-1603` read `main.rs` by `include_str!` for
`Some("…")` arms, so the dispatch must stay in `main.rs`, which it would.
Visibility churn: `pub(crate)` on `now_unix`, `data_home`, `fail`,
`usage`, `replace_file` and the record helpers.

Pre-attack: a module per command group is the same shape `roles.rs`
already has, so this is not new structure. The one real question is
whether `run_box` (170 lines, one function) is a module or a function
that wants splitting; that is a separate review.

### 2.3 Not open

The word `tui` survives only as internal names: `fn tui()` at
`main.rs:871`, the `wormhole_core::tui` module, and `STATUS.md:115`,
which names the module on purpose. No help text, usage line or handbook
page says it (`rg -w tui docs/src crates/wormhole-core/src/help.rs`
finds nothing). Renaming the module to `panel` would collide with the
binary's `panel.rs`, so the split is the right one: `tui` is the pure
state machine, `panel` the terminal shell (`crates/wormhole/src/panel.rs:1-3`).

## 3. What each open item would touch

### 2.1 stale concept and plan

- Help text: none.
- Handbook: `CONCEPT.md:37`, `:47`, `:56`, `:395`, `:410`, `:430`, `:436`, `:460`
  and `PLAN.md:78`, `:296`, rendered through `docs/src/concept.md` and
  `docs/src/plan.md`.
- Tests: `tests/wiki.rs:213` re-renders the book; passes unless a linked
  heading goes.

### 2.2 the rest of the split

- Help text: none. `help.rs` is the one command list and the dispatch stays
  in `main.rs`.
- Handbook: none. `STATUS.md:116` names `run::parse_*`, `tui::Act` and
  other core paths, not binary functions, so it stays true.
- Tests: none change. `main.rs:1559-1603` keep reading `main.rs`.

### The done items, for the record

The change set already updated what they touch: `help.rs` (`ps` blurb at
`:69-72`, no `run`, `env` or `tui` line), `docs/src/index.md:44`,
`docs/src/guide/quickstart.md:63-65`, `docs/src/guide/boxes.md:34-38`
(the sample output matches the live columns), `docs/src/guide/manifest.md`,
`docs/src/guide/access.md`, `docs/src/guide/install.md`, and the tests
`tests/ps.rs`, `tests/lifecycle.rs`, `tests/manifests.rs`,
`tests/kernel.rs` (all listed as `M` in `git status`). What it did not do
is commit: everything in section 1 is in the working tree only.
