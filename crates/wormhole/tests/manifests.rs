//! The manifests this repository ships: the workspace's own and every
//! role's. Nothing else reads them until a box starts, so a mistake in one
//! only ever shows up as a box that will not run.

use std::path::PathBuf;

use wormhole_core::manifest;

mod common;
use common::repo_root;

fn shipped_manifests() -> Vec<PathBuf> {
    let root = repo_root();
    let mut found = vec![root.join("wormhole.toml")];
    for role in std::fs::read_dir(root.join("roles")).expect("roles directory") {
        let manifest = role.expect("role entry").path().join("wormhole.toml");
        if manifest.is_file() {
            found.push(manifest);
        }
    }
    found.sort();
    found
}

#[test]
fn every_manifest_this_repo_ships_parses() {
    let manifests = shipped_manifests();
    assert!(manifests.len() > 1, "no role manifests were found to check");
    for path in &manifests {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        manifest::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

/// A digest of zeros is a note to fill in later, and a build that reaches
/// it fails on the download it names. It has no place in a shipped recipe.
#[test]
fn no_shipped_manifest_carries_a_placeholder_digest() {
    for path in shipped_manifests() {
        let text = std::fs::read_to_string(&path).expect("manifest");
        assert!(
            !text.contains(&"0".repeat(64)),
            "{} has a placeholder digest",
            path.display()
        );
    }
}
