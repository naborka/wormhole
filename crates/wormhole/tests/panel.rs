//! The panel on a real pty. The panel holds the terminal in raw mode,
//! where a newline moves down without returning to the left margin: every
//! line it draws ends `\r\n`. Anything else that writes to that terminal —
//! a scan reporting a home it could not read, once a second, forever —
//! lands as a staircase of text across the box list. So the panel's output
//! must contain no newline that has no carriage return in front of it.

use std::path::Path;
use std::process::Command;

mod common;
use common::on_a_terminal;

/// Everything the panel wrote to a pty before quitting, keystrokes and
/// all. `q` is sent only once the hints are on screen, which is proof the
/// panel is already in raw mode.
fn panel_output(data_home: &Path) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data_home);
    on_a_terminal(command, "q quit", b"q")
}

/// A home with no box record in it, which is what a box start that died
/// before writing one leaves behind.
fn unreadable_home(data_home: &Path, name: &str) {
    std::fs::create_dir_all(data_home.join("wormhole/homes").join(name)).expect("home");
}

/// The bug this pins: the scan behind the box list used to print what it
/// could not read straight to stderr, from inside the panel's own redraw
/// loop, once a second, in raw mode. The panel is the only thing allowed
/// to write to the terminal it holds.
#[test]
fn a_home_the_scan_cannot_read_is_drawn_on_the_panel_not_printed_over_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    unreadable_home(temp.path(), "proj-0123456789ab");
    let seen = panel_output(temp.path());

    assert!(seen.contains("could not read:"), "{seen:?}");
    assert!(
        seen.contains("proj-0123456789ab holds no box record"),
        "{seen:?}"
    );
    let bytes = seen.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        assert!(
            *byte != b'\n' || (at > 0 && bytes[at - 1] == b'\r'),
            "a newline with no carriage return at byte {at}: {seen:?}"
        );
    }
}
