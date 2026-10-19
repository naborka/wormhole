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

/// Every Alpine recipe this repository ships, the starter included,
/// installs each named package.
fn every_alpine_recipe_installs(packages: &[&str]) {
    let mut recipes = shipped_manifests();
    recipes.push(repo_root().join("templates/wormhole.toml"));
    let mut checked = 0;
    for path in &recipes {
        let text = std::fs::read_to_string(path).expect("manifest");
        let parsed = manifest::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if !parsed.image.base.contains("alpine") {
            continue;
        }
        checked += 1;
        for package in packages {
            assert!(
                parsed.image.packages.iter().any(|p| p == package),
                "{} is an Alpine recipe without the `{package}` package",
                path.display()
            );
        }
    }
    assert!(
        checked > 1,
        "only {checked} Alpine recipes were found to check"
    );
}

/// Alpine's `grep`, `find`, `sed`, `diff`, `tar`, `tree` and the rest are
/// BusyBox applets, and BusyBox refuses the GNU flags every agent writes
/// and every wrapper emits: rtk passes `--null` to `grep`, so in a BusyBox
/// image each plain `grep` from Claude fails, and `rtk tree` prints its
/// own options back as errors. Measured in `docs/research/box-tools.md`.
#[test]
fn every_alpine_recipe_this_repo_ships_installs_the_gnu_tools() {
    every_alpine_recipe_installs(&[
        "grep",
        "findutils",
        "sed",
        "coreutils",
        "diffutils",
        "gawk",
        "tree",
        "tar",
    ]);
}

/// A minirootfs has no zoneinfo, so `TZ=Europe/Belgrade date` is UTC and
/// PostgreSQL answers "time zone not recognized".
#[test]
fn every_alpine_recipe_this_repo_ships_carries_zoneinfo() {
    every_alpine_recipe_installs(&["tzdata"]);
}

/// The build box maps one uid, so root there cannot `chown` to any other.
/// GNU tar running as root restores the owner a tarball records, and a
/// GitHub release records the runner's 1001: the nextest step failed with
/// "Cannot change ownership to uid 1001" once GNU tar replaced BusyBox's.
#[test]
fn every_build_step_that_extracts_keeps_no_owner() {
    for path in shipped_manifests() {
        let text = std::fs::read_to_string(&path).expect("manifest");
        let parsed = manifest::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for step in parsed
            .image
            .build
            .iter()
            .filter(|step| step.contains("tar -x"))
        {
            assert!(
                step.contains("--no-same-owner"),
                "{} extracts without --no-same-owner: {step}",
                path.display()
            );
        }
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

/// Codex's background app-server records the process it spawned by
/// running `ps -p <pid> -o stat= -o lstart=`, and the whole start fails
/// when that exits non-zero. Alpine's `ps` is a BusyBox applet with no
/// `-p` and no `lstart`, so a codex box died on its first start with
/// "failed to read start time for pid-managed app server". Every Alpine
/// recipe that can run codex installs `procps-ng`, whose `ps` answers.
#[test]
fn every_alpine_recipe_that_runs_codex_installs_procps() {
    let mut checked = 0;
    for path in shipped_manifests() {
        let text = std::fs::read_to_string(&path).expect("manifest");
        let parsed = manifest::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let runs_codex = parsed.agent.run.iter().any(|product| product == "codex");
        if !parsed.image.base.contains("alpine") || !runs_codex {
            continue;
        }
        checked += 1;
        assert!(
            parsed.image.packages.iter().any(|p| p == "procps-ng"),
            "{} runs codex on Alpine without the `procps-ng` package",
            path.display()
        );
    }
    assert!(
        checked > 0,
        "no Alpine recipe that runs codex was found to check"
    );
}
