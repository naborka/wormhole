//! The manifests this repository ships: the workspace's own and every
//! role's. Nothing else reads them until a box starts, so a mistake in one
//! only ever shows up as a box that will not run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

fn parsed(path: &Path) -> manifest::Manifest {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    manifest::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn every_manifest_this_repo_ships_parses() {
    let manifests = shipped_manifests();
    assert!(manifests.len() > 1, "no role manifests were found to check");
    for path in &manifests {
        parsed(path);
    }
}

/// `TERM` names a description every curses program looks up, and a box
/// carries only what its image ships. Wormhole carries the host's own
/// description in, and drops `TERM` from what it hands the box when it
/// could not — so every manifest must declare a `TERM` the image itself
/// resolves, or that box is left with no name at all and every curses
/// program in it guessing.
#[test]
fn every_manifest_names_a_terminal_the_image_itself_carries() {
    for path in shipped_manifests() {
        let manifest = parsed(&path);
        let told = |host: &BTreeMap<String, String>| {
            manifest::box_env(&manifest, host).get("TERM").cloned()
        };
        assert_eq!(
            told(&BTreeMap::new()).as_deref(),
            Some("xterm-256color"),
            "{} leaves a box whose terminal could not be described with no usable TERM",
            path.display()
        );
    }
}

/// The description travels with the name or the name does not travel:
/// a host `TERM` reaches the box only because `terminfo::carry` put the
/// description where the box reads it, and wormhole removes `TERM` from
/// the host environment when it could not. Both halves of that rule are
/// visible here, on the manifests actually shipped.
#[test]
fn a_host_terminal_reaches_the_box_only_with_its_description() {
    for path in shipped_manifests() {
        let manifest = parsed(&path);
        let carried = BTreeMap::from([("TERM".to_owned(), "xterm-ghostty".to_owned())]);
        assert_eq!(
            manifest::box_env(&manifest, &carried)
                .get("TERM")
                .cloned()
                .as_deref(),
            Some("xterm-ghostty"),
            "{} refuses the terminal the box was given a description of",
            path.display()
        );
    }
}
