//! `wormhole ps` against a synthetic registry: listing, reaping, and the
//! empty case. No namespaces involved — `ps` only reads the host.

use std::path::Path;
use std::process::Output;

use wormhole_core::{home, registry};

fn ps(data_home: &Path) -> Output {
    run_ps(data_home, &["ps"])
}

fn ps_all(data_home: &Path) -> Output {
    run_ps(data_home, &["ps", "--all"])
}

fn run_ps(data_home: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(args)
        .env("XDG_DATA_HOME", data_home)
        .output()
        .expect("wormhole binary should spawn")
}

fn write_home(data_home: &Path, key: &str, id: &str) {
    let record = home::Record {
        id: id.to_owned(),
        workspace: std::path::PathBuf::from("/home/me/proj"),
        role: Some("alphaca".to_owned()),
        source: Some("/home/me/roles/alphaca".to_owned()),
        alias: Some("api".to_owned()),
        name: Some("architect".to_owned()),
        agent: Some("claude".to_owned()),
        created_unix: 1,
        started_unix: 2,
    };
    let file = data_home
        .join("wormhole/homes")
        .join(key)
        .join(home::RECORD);
    std::fs::create_dir_all(file.parent().expect("record dir")).expect("home");
    std::fs::write(&file, home::to_toml(&record).expect("toml")).expect("record written");
}

fn write_entry(data_home: &Path, pid: u32) {
    let dir = data_home.join("wormhole/boxes").join(pid.to_string());
    std::fs::create_dir_all(&dir).expect("box dir");
    let entry = registry::Entry {
        box_id: "0123456789ab".to_owned(),
        pid,
        workspace: std::path::PathBuf::from("/home/me/proj"),
        image: "/data/img".to_owned(),
        agent: Some("claude".to_owned()),
        name: Some("architect".to_owned()),
        alias: Some("api".to_owned()),
        started_unix: 1,
    };
    std::fs::write(
        dir.join("box.toml"),
        registry::to_toml(&entry).expect("toml"),
    )
    .expect("entry written");
}

#[test]
fn no_boxes_says_so() {
    let temp = tempfile::tempdir().expect("temp dir");
    let output = ps(temp.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "no boxes running\n"
    );
}

#[test]
fn a_running_box_is_listed_with_its_name_and_agent() {
    let temp = tempfile::tempdir().expect("temp dir");
    write_entry(temp.path(), std::process::id());
    let output = ps(temp.path());
    assert!(output.status.success());
    let listing = String::from_utf8_lossy(&output.stdout);
    for cell in ["architect", "claude", "/home/me/proj"] {
        assert!(listing.contains(cell), "{listing}");
    }
}

/// Every wormhole on the host scans this directory while another writes
/// it. One entry it cannot read is that box's problem, not the listing's:
/// `ps` must still show the others and still succeed.
#[test]
fn an_unreadable_entry_does_not_take_the_listing_down_with_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    write_entry(temp.path(), std::process::id());
    let live_but_corrupt = temp
        .path()
        .join("wormhole/boxes")
        .join(std::os::unix::process::parent_id().to_string());
    std::fs::create_dir_all(&live_but_corrupt).expect("box dir");
    std::fs::write(live_but_corrupt.join("box.toml"), "pid = \"half a line").expect("corrupt");

    let output = ps(temp.path());
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("architect"),
        "{output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unreadable entry"),
        "{output:?}"
    );
}

/// A home with no record in it — what a box start that died before
/// writing one leaves behind. `ps --all` names it once and still lists
/// every box it could read.
#[test]
fn an_unreadable_home_is_named_once_and_hides_none_of_the_others() {
    let temp = tempfile::tempdir().expect("temp dir");
    write_home(temp.path(), "proj-0123456789ab", "0123456789ab");
    std::fs::create_dir_all(temp.path().join("wormhole/homes/proj-ffffffffffff")).expect("home");

    let output = ps_all(temp.path());
    assert!(output.status.success(), "{output:?}");
    let listing = String::from_utf8_lossy(&output.stdout);
    assert!(listing.contains("0123456789ab"), "{listing}");
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        said.matches("holds no box record").count(),
        1,
        "said once, not per scan: {said}"
    );
    assert!(said.contains("proj-ffffffffffff"), "{said}");
}

/// One unreadable box is that box's problem; a store that cannot be read
/// at all is everybody's. "No boxes" would be a lie, so `ps` refuses
/// instead of printing one. A file where the directory belongs is the
/// portable way to make the read fail — a mode nobody can bypass.
#[test]
fn a_registry_that_cannot_be_read_is_refused_not_reported_as_empty() {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join("wormhole")).expect("data home");
    std::fs::write(temp.path().join("wormhole/boxes"), "not a directory").expect("blocker");

    let output = ps(temp.path());
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot read"),
        "{output:?}"
    );
}

#[test]
fn homes_that_cannot_be_read_are_refused_not_reported_as_empty() {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join("wormhole")).expect("data home");
    std::fs::write(temp.path().join("wormhole/homes"), "not a directory").expect("blocker");

    let output = ps_all(temp.path());
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot read"),
        "{output:?}"
    );
}

/// A box killed between creating its directory and writing its entry
/// leaves a directory with no entry at all. Its pid is in the name, so it
/// is still reapable — nothing about a dead box should need its file.
#[test]
fn a_dead_box_that_never_wrote_an_entry_is_still_reaped() {
    let temp = tempfile::tempdir().expect("temp dir");
    let dead = 4_000_000; // beyond any default pid_max
    let dir = temp.path().join("wormhole/boxes").join(dead.to_string());
    std::fs::create_dir_all(dir.join("root")).expect("box dir");

    assert!(ps(temp.path()).status.success());
    assert!(!dir.exists(), "an entryless dead box survived");
}

/// A box that died without cleanup must not haunt the listing: its entry
/// is removed, not just skipped.
#[test]
fn a_dead_boxs_entry_is_reaped() {
    let temp = tempfile::tempdir().expect("temp dir");
    let dead = 4_000_000; // beyond any default pid_max
    write_entry(temp.path(), dead);
    let output = ps(temp.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "no boxes running\n"
    );
    assert!(
        !temp
            .path()
            .join("wormhole/boxes")
            .join(dead.to_string())
            .exists(),
        "stale entry survived"
    );
}
