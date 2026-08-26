//! `wormhole init`. The first wall used to be before roles were reached
//! at all: the quickstart's step one was "write a manifest", and the
//! smallest one it printed carried a rootfs URL and its sixty-four hex
//! characters for a person to source by hand.

use std::path::Path;
use std::process::{Command, Output};

fn wormhole(workspace: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(args)
        .current_dir(workspace)
        .env("XDG_DATA_HOME", workspace.join("data"))
        .env("XDG_CONFIG_HOME", workspace.join("config"))
        .output()
        .expect("wormhole should spawn")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// What `init` writes has to be a manifest wormhole itself accepts —
/// otherwise the first command after it is the one that fails.
#[test]
fn init_writes_a_manifest_this_wormhole_can_read() {
    let temp = tempfile::tempdir().expect("temp dir");
    let written = wormhole(temp.path(), &["init"]);
    assert!(written.status.success(), "{}", stderr(&written));

    let path = temp.path().join("wormhole.toml");
    assert!(path.is_file(), "init wrote no manifest");
    let text = std::fs::read_to_string(&path).expect("read it back");
    wormhole_core::manifest::parse(&text).expect("init writes a manifest wormhole can parse");
}

/// A manifest is the one file in a workspace wormhole must never
/// overwrite: it is the recipe somebody wrote, and there is no undo.
#[test]
fn init_never_writes_over_a_manifest_already_here() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("wormhole.toml");
    std::fs::write(&path, "version = 1\n").expect("an existing manifest");

    let refused = wormhole(temp.path(), &["init"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("already"), "{}", stderr(&refused));
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "version = 1\n"
    );
}

/// The starter grants the agent's credential and nothing else. A default
/// that handed over `~/.ssh` would be a default nobody read.
#[test]
fn the_starter_grants_nothing_but_the_agents_credential() {
    let temp = tempfile::tempdir().expect("temp dir");
    wormhole(temp.path(), &["init"]);
    let text = std::fs::read_to_string(temp.path().join("wormhole.toml")).expect("manifest");
    let manifest = wormhole_core::manifest::parse(&text).expect("parses");
    assert_eq!(manifest.access.grants, ["~/.claude/.credentials.json"]);
}
