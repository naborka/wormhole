//! One claim per box, held by the kernel. No namespaces involved — the
//! claim is an `flock` on a host file, so this runs anywhere.
//!
//! A workspace is not what is claimed. It holds as many boxes as you make;
//! only starting the *same* box twice is refused.

use std::path::Path;

use nix::fcntl::{Flock, FlockArg};
use wormhole_core::paths;

/// A workspace with a manifest whose image nothing has built and nothing
/// can build: `wormhole box` gets as far as the claim, starts the build a
/// missing image now means, and stops there. The `file://` URL names
/// nothing, so the fetch fails at once and reaches no network.
fn workspace(dir: &Path) {
    std::fs::write(
        dir.join("wormhole.toml"),
        concat!(
            "version = 1\n",
            "[agent]\nrun = \"claude\"\n",
            "[image]\n",
            "base = \"file:///nothing/here.tar.gz\"\n",
            "base_sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n",
        ),
    )
    .expect("manifest written");
}

/// What a box says when it has got past the claim and reached the image.
const REACHED_THE_IMAGE: &str = "no image for this manifest yet";

mod common;

/// The whole want: one home, its login and its setup, for every project a
/// role is started in.
#[test]
fn a_role_that_resumes_anywhere_resumes_its_box_in_a_new_workspace() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path().join("data");
    let role = common::a_role(&temp.path().join("role"), "resume = \"anywhere\"\n");
    let first = common::a_dir(temp.path(), "a");
    let (id, _) = common::a_role_box(&data, &first, &role);
    let second = common::a_dir(temp.path(), "b");

    let output = run_box(&second, &data, &["--role", role.to_str().expect("utf-8")]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "box: {id} (resumed; last used in {})",
            first.display()
        )),
        "{output:?}"
    );
}

/// Not saying keeps every existing role's answer: a box per workspace.
#[test]
fn a_role_that_resumes_here_makes_a_new_box_in_a_new_workspace() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path().join("data");
    let role = common::a_role(&temp.path().join("role"), "");
    let first = common::a_dir(temp.path(), "a");
    let (id, _) = common::a_role_box(&data, &first, &role);
    let second = common::a_dir(temp.path(), "b");

    let output = run_box(&second, &data, &["--role", role.to_str().expect("utf-8")]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("(new)"), "{output:?}");
    assert!(!stdout.contains(&id), "{output:?}");
}

/// The bug this pins: `--id` read the recipe of the directory it was typed
/// in and wrote that over the record, so a role box forgot its role.
#[test]
fn naming_a_role_box_from_another_workspace_starts_it_with_its_own_role() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path().join("data");
    let role = common::a_role(&temp.path().join("role"), "");
    let first = common::a_dir(temp.path(), "a");
    let (id, _) = common::a_role_box(&data, &first, &role);
    let elsewhere = common::a_dir(temp.path(), "b");
    workspace(&elsewhere);

    let output = run_box(&elsewhere, &data, &["--id", &id]);
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("role-recipe"), "{said}");
    assert!(!said.contains("nothing/here"), "{said}");
    assert!(!said.contains("belongs to"), "{said}");
}

/// Its recipe is that workspace's own file, which no other start reads.
#[test]
fn a_box_made_from_a_workspace_manifest_is_refused_by_name_elsewhere() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path().join("data");
    let first = common::a_dir(temp.path(), "a");
    workspace(&first);
    let id = paths::box_id(&first, 0);
    common::keep_box(&data, &common::a_record(&first, &id, None));
    let elsewhere = common::a_dir(temp.path(), "b");
    workspace(&elsewhere);

    let output = run_box(&elsewhere, &data, &["--id", &id]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = String::from_utf8_lossy(&output.stderr);
    assert!(refusal.contains("runs only there"), "{refusal}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(REACHED_THE_IMAGE),
        "{output:?}"
    );
}

/// A box is made from one role for its whole life; naming it with another
/// is a refusal, never a quiet change of what its home holds.
#[test]
fn naming_a_box_with_another_role_is_refused() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path().join("data");
    let alphaca = common::a_role(&temp.path().join("alphaca"), "");
    let java = common::a_role(&temp.path().join("java"), "");
    let first = common::a_dir(temp.path(), "a");
    let (id, _) = common::a_role_box(&data, &first, &alphaca);

    let output = run_box(
        &first,
        &data,
        &["--id", &id, "--role", java.to_str().expect("utf-8")],
    );
    assert!(!output.status.success(), "{output:?}");
    let refusal = String::from_utf8_lossy(&output.stderr);
    assert!(
        refusal.contains(&wormhole_core::source::dir_source(&alphaca)),
        "{refusal}"
    );
}

/// A workspace's own manifest has no other workspace for its box to
/// follow you to, and a cloned repository must not widen its own box.
#[test]
fn a_workspace_manifest_cannot_resume_anywhere() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = temp.path().join("data");
    let here = common::a_dir(temp.path(), "a");
    std::fs::write(
        here.join("wormhole.toml"),
        format!(
            "version = 1\n[agent]\nrun = \"claude\"\nresume = \"anywhere\"\n\
             [image]\nbase = \"file:///nothing/here.tar.gz\"\nbase_sha256 = \"{}\"\n",
            "0".repeat(64)
        ),
    )
    .expect("manifest");

    let output = run_box(&here, &data, &[]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = String::from_utf8_lossy(&output.stderr);
    assert!(refusal.contains("resume"), "{refusal}");
}

fn run_box(workspace: &Path, data_home: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .arg("box")
        .args(args)
        .current_dir(workspace)
        .env("XDG_DATA_HOME", data_home)
        // Roles live in the config home. Pointed at the temp one so no test
        // here can be decided by what the developer has installed.
        .env("XDG_CONFIG_HOME", data_home)
        .output()
        .expect("wormhole binary should spawn")
}

/// The lock file of the nth box in a workspace.
fn lock_of(data_home: &Path, workspace: &Path, ordinal: u32) -> std::path::PathBuf {
    let id = paths::box_id(workspace, ordinal);
    paths::lock_file(data_home, &paths::box_key(workspace, &id))
}

/// Takes a box's claim the way a running box holds it, and stamps the file
/// with a pid so a refusal has someone to name.
fn hold(lock: &Path) -> Flock<std::fs::File> {
    std::fs::create_dir_all(lock.parent().expect("parent")).expect("lock dir");
    std::fs::write(lock, "4242").expect("holder pid");
    Flock::lock(
        std::fs::File::options()
            .read(true)
            .write(true)
            .open(lock)
            .expect("open"),
        FlockArg::LockExclusiveNonblock,
    )
    .expect("this test holds the box")
}

/// The claim is what `box` does first: nothing about the workspace or the
/// kept home is touched before it, so a refusal costs nothing.
#[test]
fn a_box_claims_itself_before_it_looks_for_an_image() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = tempfile::tempdir().expect("temp dir");
    workspace(temp.path());
    let real = temp.path().canonicalize().expect("real path");

    let output = run_box(temp.path(), data.path(), &[]);
    assert!(!output.status.success(), "{output:?}");
    // It got past the claim and reached the image, which is the ordering:
    // the lock is taken, then the work begins.
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(REACHED_THE_IMAGE),
        "{output:?}"
    );
    let lock = lock_of(data.path(), &real, 0);
    assert!(lock.is_file(), "no lock file at {}", lock.display());
}

/// The limit that is gone: a busy box no longer holds the folder. A second
/// `wormhole box` there is a second box, with its own claim and its own
/// kept home.
#[test]
fn a_busy_box_does_not_hold_the_workspace_against_another_one() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = tempfile::tempdir().expect("temp dir");
    workspace(temp.path());
    let real = temp.path().canonicalize().expect("real path");
    let held = hold(&lock_of(data.path(), &real, 0));

    let output = run_box(temp.path(), data.path(), &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stderr.contains("already running"), "{stderr}");
    assert!(stdout.contains(REACHED_THE_IMAGE), "{stdout}");
    assert!(
        stdout.contains(&format!("box: {} (new)", paths::box_id(&real, 1))),
        "{stdout}"
    );
    let second = lock_of(data.path(), &real, 1);
    assert!(second.is_file(), "no second claim at {}", second.display());

    drop(held);
}

/// What is still refused, and all that is: starting the *same* box twice.
/// Two processes in one home would write one history, one config and one
/// instructions file at the same time.
#[test]
fn starting_the_same_box_twice_is_refused_by_the_kernel() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = tempfile::tempdir().expect("temp dir");
    workspace(temp.path());
    let real = temp.path().canonicalize().expect("real path");
    let id = paths::box_id(&real, 0);
    // The box exists: `--id` names one that is there, never one it makes.
    std::fs::create_dir_all(paths::home_dir(data.path(), &paths::box_key(&real, &id)))
        .expect("box home");
    let held = hold(&lock_of(data.path(), &real, 0));

    let output = run_box(temp.path(), data.path(), &["--id", &id]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = String::from_utf8_lossy(&output.stderr);
    assert!(refusal.contains("already running"), "{refusal}");
    // The refusal names who holds it, out of the locked file's content.
    assert!(refusal.contains("4242"), "{refusal}");

    drop(held);
}

/// A box that does not exist is a typo, not an invitation to make one:
/// a mistyped id must never quietly become a stray box with an empty home.
#[test]
fn naming_a_box_that_does_not_exist_is_refused() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = tempfile::tempdir().expect("temp dir");
    workspace(temp.path());

    let output = run_box(temp.path(), data.path(), &["--id", "0123456789ab"]);
    assert!(!output.status.success(), "{output:?}");
    let refusal = String::from_utf8_lossy(&output.stderr);
    assert!(refusal.contains("no box 0123456789ab"), "{refusal}");
}

/// What a registry scan could never give: the claim ends with the process,
/// however it ends. A lock file left behind by a box that was killed is
/// not a claim, and the next box takes it without anyone reaping anything.
#[test]
fn a_lock_left_by_a_dead_box_does_not_hold_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    let data = tempfile::tempdir().expect("temp dir");
    workspace(temp.path());
    let real = temp.path().canonicalize().expect("real path");

    let lock = lock_of(data.path(), &real, 0);
    std::fs::create_dir_all(lock.parent().expect("parent")).expect("lock dir");
    std::fs::write(&lock, "4242").expect("a dead box's pid");

    let output = run_box(temp.path(), data.path(), &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("already running"), "{stderr}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(REACHED_THE_IMAGE),
        "{output:?}"
    );
    // It took the claim, so the file now names this run, not the dead one.
    assert_ne!(
        std::fs::read_to_string(&lock).expect("lock file").trim(),
        "4242"
    );
}
