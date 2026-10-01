//! The panel on a real pty — the only place a screen can be seen.
//!
//! What these read is [`common::shown`], not the raw stream: a drawn
//! screen writes only the cells that changed and moves the cursor over
//! the rest, so the bytes are not the words. What is asserted on the raw
//! bytes is the one property that is about the bytes: the panel holds the
//! terminal in raw mode, where a newline moves down without returning to
//! the left margin, so anything else writing there — a scan reporting a
//! home it could not read, once a second, forever — lands as a staircase
//! across the box list.

use std::path::Path;
use std::process::Command;

mod common;
use common::{on_a_terminal, shown};

/// Everything the panel wrote to a pty before quitting, keystrokes and
/// all. `q` is sent only once the hints are on screen, which is proof the
/// panel is already in raw mode.
fn panel_output(data_home: &Path) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data_home);
    on_a_terminal(command, "quit", b"q")
}

/// A home with no box record in it, which is what a box start that died
/// before writing one leaves behind.
fn unreadable_home(data_home: &Path, name: &str) {
    std::fs::create_dir_all(data_home.join("wormhole/homes").join(name)).expect("home");
}

/// A store holding one kept box, and the workspace it ran in.
fn one_box(data: &Path) -> (String, std::path::PathBuf) {
    let workspace = data.join("proj");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let workspace = workspace.canonicalize().expect("canonical");
    let id = wormhole_core::paths::box_id(&workspace, 0);
    (id, workspace)
}

/// The gap this closes: the panel listed every box on this host and was
/// the one surface from which none of them could be taken away.
///
/// Driven end to end on a real terminal rather than through the state
/// machine alone, because the keys, the question and the removal have to
/// line up — `x` asks, `y` answers, and only then is the home gone.
#[test]
fn x_then_y_removes_the_selected_box_from_the_panel() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let (id, workspace) = one_box(data);
    let home = common::keep_box(data, &common::a_record(&workspace, &id, None));

    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data);
    // Three keys in one answer: ask, confirm, leave. The panel reads them
    // one at a time, which is what the sequence is here to prove.
    let seen = shown(&on_a_terminal(command, "remove", b"xyq"));

    assert!(seen.contains(&format!("remove box {id}")), "{seen:?}");
    assert!(seen.contains("removed"), "{seen:?}");
    assert!(!home.exists(), "the home survived the panel");
}

/// The gap this closes: a box's record has always carried every workspace
/// it served, and the only surface that listed boxes showed `+N` and no
/// way to see or reach one.
#[test]
fn the_panel_shows_the_workspaces_the_selected_box_has_served() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let (id, workspace) = one_box(data);
    let mut record = common::a_record(&workspace, &id, None);
    // One it can still go back to, one that has been deleted since.
    let other = common::a_dir(data, "other");
    record.earlier = vec![other.clone(), data.join("deleted")];
    common::keep_box(data, &record);

    let seen = shown(&panel_output(data));

    assert!(seen.contains("workspaces"), "{seen:?}");
    assert!(
        seen.contains(&format!("{} (last used)", workspace.display())),
        "{seen:?}"
    );
    assert!(seen.contains(&other.display().to_string()), "{seen:?}");
    assert!(
        seen.contains(&format!("{} (gone)", data.join("deleted").display())),
        "{seen:?}"
    );
}

/// Enter on an idle box asks where to start it. The list it opens is the
/// places that box can actually be started, and the cursor is already on
/// the one a start with nobody choosing would have taken.
#[test]
fn enter_asks_where_to_start_an_idle_box() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let (id, workspace) = one_box(data);
    let mut record = common::a_record(&workspace, &id, None);
    let other = common::a_dir(data, "other");
    record.earlier = vec![other.clone(), data.join("deleted")];
    common::keep_box(data, &record);

    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data);
    // Enter opens the list, `q` backs out of it, `q` leaves the panel.
    let seen = shown(&on_a_terminal(command, "join, or start", b"\rqq"));
    // The picker's own screen, from its heading to its hints: the detail
    // pane names the workspace that is gone, and that is right — what
    // must not name it is the list of places this box can be started.
    let from = seen
        .find("start this box in:")
        .unwrap_or_else(|| panic!("{seen:?}"));
    let picker = &seen[from..];
    let picker = &picker[..picker
        .find("enter choose")
        .unwrap_or_else(|| panic!("{picker:?}"))];

    assert!(
        picker.contains(&workspace.display().to_string()),
        "{picker:?}"
    );
    assert!(picker.contains(&other.display().to_string()), "{picker:?}");
    // A workspace that is gone cannot be started in, so it is not offered.
    assert!(!picker.contains("deleted"), "{picker:?}");
    // And the cursor starts on the workspace a silent start would have
    // taken, so agreeing costs one more key than not being asked.
    assert!(
        picker.contains(&format!("> {}", workspace.display())),
        "{picker:?}"
    );
}

/// The bug this pins: a resize was dropped on the floor. The panel read
/// events and returned nothing for anything that was not a key, and the
/// redraw only happened when the *text* changed — which a resize does
/// not do. So the panel kept whatever the old geometry had left on the
/// screen, for as long as nothing else changed.
#[test]
fn a_resize_redraws_the_panel_at_the_new_width() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let (id, workspace) = one_box(data);
    common::keep_box(data, &common::a_record(&workspace, &id, None));

    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data);
    let (before, after) = common::resized(command, "quit", (40, 24), b"q");
    let (before, after) = (shown(&before), shown(&after));

    // Eighty columns has room for the manifest's name; forty does not,
    // and the column is dropped rather than squeezed.
    assert!(before.contains("architect"), "{before:?}");
    assert!(
        !after.is_empty(),
        "nothing was drawn after the resize: {before:?}"
    );
    assert!(after.contains(&id), "{after:?}");
    assert!(!after.contains("architect"), "{after:?}");
}

/// The bug this pins: the scan behind the box list used to print what it
/// could not read straight to stderr, from inside the panel's own redraw
/// loop, once a second, in raw mode. The panel is the only thing allowed
/// to write to the terminal it holds.
#[test]
fn a_home_the_scan_cannot_read_is_drawn_on_the_panel_not_printed_over_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    unreadable_home(temp.path(), "proj-0123456789ab");
    let raw = panel_output(temp.path());
    let seen = shown(&raw);

    assert!(seen.contains("could not read:"), "{seen:?}");
    assert!(
        seen.contains("proj-0123456789ab holds no box record"),
        "{seen:?}"
    );
    let bytes = raw.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        assert!(
            *byte != b'\n' || (at > 0 && bytes[at - 1] == b'\r'),
            "a newline with no carriage return at byte {at}: {seen:?}"
        );
    }
}

/// The gap this closes: one hint line was the whole of what the panel
/// told anybody about itself, and the keys it did not fit were keys
/// nobody could find.
#[test]
fn question_mark_puts_every_key_of_the_screen_on_a_screen() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let (id, workspace) = one_box(data);
    common::keep_box(data, &common::a_record(&workspace, &id, None));

    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data);
    // `?` opens the keys, `q` closes them, `q` leaves.
    let seen = shown(&on_a_terminal(command, "quit", b"?qq"));

    assert!(seen.contains("keys on this screen"), "{seen:?}");
    // Including the ones the hint line has no room to name.
    for expected in ["a screen down", "first", "rename"] {
        assert!(seen.contains(expected), "{seen:?} misses {expected}");
    }
}

/// The gap this closes: the panel reached boxes and nothing else. Roles,
/// the store, secrets and the host had no screen at all.
#[test]
fn tab_reaches_the_rest_of_the_tool() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let (id, workspace) = one_box(data);
    common::keep_box(data, &common::a_record(&workspace, &id, None));

    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data);
    command.env("XDG_CONFIG_HOME", data.join("config"));
    // Round every tab and back to the boxes, then leave.
    let seen = shown(&on_a_terminal(command, "quit", b"\t\t\t\t\tq"));

    // Each tab by what only it says. Not by the tab bar: only the cells
    // that changed are redrawn, so the bar arrives in pieces.
    assert!(seen.contains("no roles installed"), "{seen:?}");
    assert!(seen.contains("WHAT SIZE PATH"), "{seen:?}");
    assert!(seen.contains("no secrets kept"), "{seen:?}");
    assert!(seen.contains("PROBE RESULT"), "{seen:?}");
}

/// Every command this tool has, on a list, showing the line each one is.
#[test]
fn the_palette_lists_the_commands_with_the_lines_they_run() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path();
    let (id, workspace) = one_box(data);
    common::keep_box(data, &common::a_record(&workspace, &id, None));

    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data);
    // `:` opens it, `q` backs out, `q` leaves.
    let seen = shown(&on_a_terminal(command, "quit", b":qq"));

    assert!(seen.contains("every command"), "{seen:?}");
    assert!(seen.contains("wormhole gc --delete"), "{seen:?}");
    assert!(seen.contains(&format!("wormhole remove {id}")), "{seen:?}");
}
