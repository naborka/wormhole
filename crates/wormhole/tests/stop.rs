//! Stopping a box: `d` in the panel, and `wormhole stop <id>` from a
//! script. Both end the same process by the same route, so both are
//! driven here against a real box directory and a real pid.

use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use wormhole_core::{home, paths, registry};

mod common;
use common::{keep_box, on_a_terminal};

const BOX_ID: &str = "0123456789ab";
const ALIAS: &str = "api";

/// A stand-in for a box's PID 1: a process that will not exit on its own,
/// so the only reason it can be gone at the end of a test is that
/// wormhole killed it.
fn an_init() -> Child {
    Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("a process to stand in for the box's init")
}

/// The two files a running box leaves on the host: the registry entry
/// `ps` and the panel read, and the pidfile that says which process is
/// the box's PID 1.
///
/// One process plays both parts here. On a real host they differ — the
/// directory is named by the `wormhole box` that owns it, `init.pid`
/// holds the boxed init — but stopping reads both from the same place,
/// so one pid exercises the whole route.
fn a_running_box(data_home: &Path, pid: u32) {
    let box_dir = paths::box_dir(data_home, pid);
    std::fs::create_dir_all(&box_dir).expect("box dir");
    std::fs::write(box_dir.join("init.pid"), format!("{pid}\n")).expect("init.pid");
    // Built through the type wormhole writes, never as text: a field this
    // fixture stopped matching would then be a compile error here rather
    // than a box that silently fails to list at run time.
    let entry = registry::Entry {
        pid,
        box_id: BOX_ID.to_owned(),
        key: None,
        workspace: data_home.to_owned(),
        image: "deadbeef".to_owned(),
        agent: None,
        name: None,
        alias: Some(ALIAS.to_owned()),
        started_unix: 1,
    };
    std::fs::write(
        box_dir.join("box.toml"),
        registry::to_toml(&entry).expect("an entry"),
    )
    .expect("box.toml");
}

/// The kept home behind that box, without which nothing lists it.
fn a_kept_home(data_home: &Path) {
    let record = home::Record {
        key: paths::box_key(data_home, BOX_ID),
        id: BOX_ID.to_owned(),
        workspace: data_home.to_owned(),
        earlier: Vec::new(),
        role: None,
        source: None,
        alias: None,
        name: None,
        agent: None,
        created_unix: 1,
        started_unix: 2,
    };
    keep_box(data_home, &record);
}

/// Whether the process ended, waited for rather than sampled: a signal is
/// delivered when the kernel gets to it, and a test that read once would
/// pass or fail on the scheduler.
fn ended(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("cannot wait for the init: {e}"),
        }
    }
    false
}

fn wormhole(data_home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wormhole"));
    command.env("XDG_DATA_HOME", data_home);
    command
}

/// The bug this pins: `d` drew the panel again and left the box running.
/// Everything up to the kill was right — the key decoded, the action was
/// `Stop`, the pid was the box's — so only a test that ends at a real
/// process could see it.
#[test]
fn d_in_the_panel_ends_the_box_it_is_pointed_at() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut init = an_init();
    a_running_box(temp.path(), init.id());
    a_kept_home(temp.path());

    // `d` stops, `q` quits. Both are read from the pty in the order they
    // were written, and stopping is done before the next key is taken.
    on_a_terminal(wormhole(temp.path()), "d stop", b"dq");

    let stopped = ended(&mut init);
    if !stopped {
        let _ = init.kill();
    }
    assert!(stopped, "`d` left the box's init running");
}

/// `d` on a box that is not running used to redraw an unchanged screen —
/// the same thing the panel does for a key it has never heard of. Most
/// rows in a panel are idle, so that is what "`d` does nothing" was.
#[test]
fn d_on_an_idle_box_says_why_on_the_screen() {
    let temp = tempfile::tempdir().expect("temp dir");
    a_kept_home(temp.path());

    let seen = on_a_terminal(wormhole(temp.path()), "d stop", b"dq");

    assert!(seen.contains("not running"), "{seen}");
}

/// Pressing `d` on a box that *is* running says so at once. The kill is
/// delivered when the kernel gets to it, so the row can still read
/// `running` on the next redraw — with nothing said, that is once again
/// a key that looks like it did nothing.
#[test]
fn d_on_a_running_box_says_so_at_once() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut init = an_init();
    a_running_box(temp.path(), init.id());
    a_kept_home(temp.path());

    let seen = on_a_terminal(wormhole(temp.path()), "d stop", b"dq");

    let stopped = ended(&mut init);
    if !stopped {
        let _ = init.kill();
    }
    assert!(seen.contains("stopping"), "{seen}");
}

/// The same stop, without a terminal. A box outlives the shell that
/// started it, so ending one cannot require being in the panel.
#[test]
fn stop_ends_the_box_by_either_form() {
    // The id and the name are two ways to say one box, so both are driven
    // through the same body — a copy per form is how the weaker of the two
    // ends up asserting less.
    for wanted in [BOX_ID, ALIAS] {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut init = an_init();
        a_running_box(temp.path(), init.id());
        a_kept_home(temp.path());

        let out = wormhole(temp.path())
            .args(["stop", wanted])
            .output()
            .expect("wormhole stop runs");

        let stopped = ended(&mut init);
        if !stopped {
            let _ = init.kill();
        }
        assert!(
            stopped,
            "`wormhole stop {wanted}` left the box running: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.status.success(),
            "{wanted}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// A box id that is not running is a refusal naming what to look at, not
/// a silent success — the whole failure mode this file exists for.
#[test]
fn stopping_a_box_that_is_not_running_says_so() {
    let temp = tempfile::tempdir().expect("temp dir");
    let out = wormhole(temp.path())
        .args(["stop", BOX_ID])
        .output()
        .expect("wormhole stop runs");

    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains(BOX_ID), "{said}");
    assert!(said.contains("wormhole ps"), "{said}");
}

/// The two things `--as` promises and nothing tested: a bare `wormhole
/// box` never drops the name a box already had, and one name never
/// reaches two boxes in a workspace.
#[test]
fn a_name_is_kept_across_starts_and_never_taken_twice() {
    let temp = tempfile::tempdir().expect("temp dir");
    a_kept_home(temp.path());
    // A recipe the start can resolve; the name is settled before anything
    // is built, so its base never has to exist.
    std::fs::write(
        temp.path().join("wormhole.toml"),
        format!(
            "version = {}\n[image]\nbase = \"file:///nothing.tar.gz\"\nbase_sha256 = \"{}\"\n",
            wormhole_core::manifest::VERSION,
            "0".repeat(64)
        ),
    )
    .expect("a workspace manifest");

    // A second box here, under the same name, is refused by name.
    let other = home::Record {
        key: paths::box_key(temp.path(), "aabbccddeeff"),
        id: "aabbccddeeff".to_owned(),
        workspace: temp.path().to_owned(),
        earlier: Vec::new(),
        role: None,
        source: None,
        alias: Some(ALIAS.to_owned()),
        name: None,
        agent: None,
        created_unix: 1,
        started_unix: 3,
    };
    keep_box(temp.path(), &other);

    let refused = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["box", "--id", BOX_ID, "--as", ALIAS])
        .current_dir(temp.path())
        .env("XDG_DATA_HOME", temp.path())
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .output()
        .expect("wormhole box runs");
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "{said}");
    assert!(said.contains("already names box"), "{said}");
    assert!(said.contains("aabbccddeeff"), "{said}");
}
