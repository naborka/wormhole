//! Where wormhole keeps things on the host. Pure layout decisions; the
//! binary supplies the data home and does the I/O.

use std::path::{Path, PathBuf};

/// The extracted tarball, untouched, under its own digest. Two manifests
/// naming the same rootfs share one extraction, and a changed digest can
/// never reuse the old one.
pub fn base_dir(data_home: &Path, sha256: &str) -> PathBuf {
    data_home.join("wormhole/bases").join(sha256)
}

/// A fetched artifact, under its own digest. The digest is the file's
/// whole identity, so two recipes naming the same bytes at two addresses
/// share one copy and a changed digest can never read the old one.
pub fn artifact_file(data_home: &Path, sha256: &str) -> PathBuf {
    artifacts_dir(data_home).join(sha256)
}

/// Where every fetched artifact lives.
pub fn artifacts_dir(data_home: &Path) -> PathBuf {
    data_home.join("wormhole/artifacts")
}

/// A base with the manifest's packages and setup applied, under the digest
/// of the recipe that produced it. Change the recipe and you get a new
/// image; change nothing and the built one is reused.
pub fn image_dir(data_home: &Path, recipe: &str) -> PathBuf {
    data_home.join("wormhole/images").join(recipe)
}

/// How many characters of a digest name a box. Twelve: short enough to
/// type, far past collision for the number of boxes one host holds.
const ID_LEN: usize = 12;

/// A box's permanent identity. `ordinal` is which box in the workspace
/// this is, so ids need no random source and stay stable across restarts
/// — and ordinal zero lands on exactly the digest a kept home was already
/// named by, so every home that existed before boxes had ids is that
/// workspace's first box, history and toolchain intact.
pub fn box_id(workspace: &Path, ordinal: u32) -> String {
    let path = workspace.display().to_string();
    let seed = match ordinal {
        0 => path,
        n => format!("{path}\0{n}"),
    };
    crate::sha256_hex(seed.as_bytes())[..ID_LEN].to_owned()
}

/// Whether a string could be a box id at all, so a typo is refused as a
/// typo instead of resolving to a directory that will never exist.
pub fn is_box_id(text: &str) -> bool {
    crate::is_lowercase_hex(text, ID_LEN)
}

/// What names a box on disk: its workspace's basename, so a person can
/// find it by eye, and its id, so several boxes in one workspace stay
/// apart. One body, because a box's home, the lock that guards it and the
/// snapshot its receipt is measured against must never disagree about
/// which box they are.
pub fn box_key(workspace: &Path, id: &str) -> String {
    let name = workspace.file_name().map_or_else(
        || "workspace".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    format!("{name}-{id}")
}

/// One kept home per box, so its history, settings, logins and installed
/// toolchain survive every restart of that box. Several boxes may share a
/// workspace; none of them shares a home.
pub fn home_dir(data_home: &Path, key: &str) -> PathBuf {
    data_home.join("wormhole/homes").join(key)
}

/// Where every kept home lives, for the scan that lists boxes.
pub fn homes_dir(data_home: &Path) -> PathBuf {
    data_home.join("wormhole/homes")
}

/// Which directory an XDG variable names: its own value when that is an
/// absolute path, and the default under `$HOME` otherwise. Unset, empty
/// and relative all take the default, which is what the spec says and
/// what safety needs — a relative store follows the working directory,
/// so a box started in one folder would not see the locks, homes and
/// registry of a box started in another.
pub fn xdg_dir(value: Option<&Path>, home: &Path, home_suffix: &str) -> PathBuf {
    match value {
        Some(path) if path.is_absolute() => path.to_owned(),
        _ => home.join(home_suffix),
    }
}

/// The file whose lock means "this box is running". The kernel owns the
/// claim, so it ends when the holding process ends — a box that is
/// killed, or that dies before it can clean up, leaves nothing stale to
/// reap and nothing for a second start to mistake for free.
///
/// It is per box, not per workspace: two boxes in one workspace are two
/// boxes, and only starting the *same* box twice is refused.
///
/// It sits beside the kept home rather than inside it: the home is the
/// agent's, wiped and rebuilt, and a claim that a `rm -rf` could drop is
/// not a claim.
pub fn lock_file(data_home: &Path, key: &str) -> PathBuf {
    data_home.join("wormhole/locks").join(format!("{key}.lock"))
}

/// A role: a manifest plus its instructions, kept in the user's config
/// (not data — roles are written by hand, not by wormhole). A role
/// fetched from a repository keeps only its pointer here; the checkout it
/// names is data, because wormhole wrote it.
pub fn role_dir(config_home: &Path, name: &str) -> PathBuf {
    roles_dir(config_home).join(name)
}

/// Where every installed role lives, for the scan that lists them and for
/// a refusal that has to name the place a role was looked for.
pub fn roles_dir(config_home: &Path) -> PathBuf {
    config_home.join("wormhole/roles")
}

/// One fetched commit, under the commit itself. Named by what was
/// fetched rather than by what wanted it: a pin names a commit, not a
/// role, so two roles pinned to one commit share one checkout and one
/// approval — and a changed pin can never reuse the old bytes.
pub fn checkout_dir(data_home: &Path, sha: &str) -> PathBuf {
    checkouts_dir(data_home).join(sha)
}

/// That a person read what this commit's manifest runs and agreed to it.
///
/// Beside the checkout rather than inside it: the checkout is the fetched
/// repository, byte for byte, and a marker written into it would be
/// wormhole editing what it just verified.
pub fn approval_file(data_home: &Path, sha: &str) -> PathBuf {
    checkouts_dir(data_home).join(format!("{sha}.approved"))
}

/// The one place the checkout layout is spelled, so a checkout and the
/// approval beside it can never drift into different directories.
fn checkouts_dir(data_home: &Path) -> PathBuf {
    data_home.join("wormhole/checkouts")
}

/// Where every running box's record lives; `wormhole ps` scans this.
pub fn boxes_dir(data_home: &Path) -> PathBuf {
    data_home.join("wormhole/boxes")
}

/// Everything one running box owns on the host: `box.toml` (the registry
/// entry) and `root` (the throwaway copy of the image). Named by the pid
/// of the `wormhole box` that runs it, and removed when it exits.
pub fn box_dir(data_home: &Path, pid: u32) -> PathBuf {
    boxes_dir(data_home).join(pid.to_string())
}

/// The last usage reading, shared by every box on this host: the limits
/// are the account's, so one file holds them and one poll refreshes it.
pub fn usage_file(data_home: &Path) -> PathBuf {
    data_home.join("wormhole/usage.json")
}

/// The undo point: a reflink copy of the workspace as it was before this
/// box started. One per box, replaced on that box's every start.
///
/// Per box and not per workspace, because the receipt is a diff against
/// it: sharing one snapshot between two boxes in a workspace would have
/// each box's receipt measured against whenever the *other* one last
/// started, and report the other's edits as its own.
///
/// Outside the box directory on purpose. That directory is thrown away
/// when the box exits, and a snapshot that dies with the box it was
/// protecting you from is not an undo point at all.
pub fn snapshot_dir(data_home: &Path, key: &str) -> PathBuf {
    data_home.join("wormhole/snapshots").join(key)
}

/// The broker's unix socket on the host. Bind-mounted into a box that
/// uses it, so the box speaks to the broker without a route anywhere.
pub fn broker_socket(data_home: &Path) -> PathBuf {
    data_home.join("wormhole/broker.sock")
}

/// The file whose lock means "this process is the one asking the endpoint
/// for the account's usage windows". Age alone cannot hold that line:
/// boxes started together stay in step, so every one of them reads the
/// same stale cache in the same instant and every one of them fetches. The
/// lock is what makes "one poll per host" true rather than likely.
pub fn usage_lock(data_home: &Path) -> PathBuf {
    data_home.join("wormhole/usage.lock")
}

/// The file whose lock means "this process is building the thing that
/// digest names" — a base extraction or an installed image.
///
/// A build assembles one `.partial` directory named by that digest, so two
/// processes building the same thing would write into each other's work.
/// Nothing made that ordinary while building was a command a person typed;
/// a box that builds its own image does, because two boxes started at once
/// want the same image at once.
pub fn build_lock(data_home: &Path, digest: &str) -> PathBuf {
    data_home
        .join("wormhole/locks")
        .join(format!("build-{digest}.lock"))
}

/// Where a directory is assembled before it is renamed into place. A
/// half-written one must never be mistaken for a finished one.
pub fn partial(dir: &Path) -> PathBuf {
    let mut partial = dir.to_owned().into_os_string();
    partial.push(".partial");
    PathBuf::from(partial)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    const WORKSPACE: &str = "/home/me/proj";

    fn key(ordinal: u32) -> String {
        let workspace = Path::new(WORKSPACE);
        box_key(workspace, &box_id(workspace, ordinal))
    }

    /// Boxes had no id before they could share a workspace, and a kept
    /// home was named by a digest of the workspace alone. Ordinal zero is
    /// that digest, so every home that already exists is picked up as its
    /// workspace's first box instead of being orphaned by this change.
    #[test]
    fn the_first_box_lands_on_the_name_a_kept_home_already_had() {
        let workspace = Path::new(WORKSPACE);
        let legacy = &crate::sha256_hex(WORKSPACE.as_bytes())[..12];
        assert_eq!(box_id(workspace, 0), legacy);
        assert_eq!(key(0), format!("proj-{legacy}"));
    }

    #[test]
    fn every_box_in_a_workspace_gets_its_own_id() {
        let workspace = Path::new(WORKSPACE);
        let ids: std::collections::BTreeSet<String> =
            (0..64).map(|n| box_id(workspace, n)).collect();
        assert_eq!(ids.len(), 64);
        assert!(ids.iter().all(|id| is_box_id(id)), "{ids:?}");
    }

    #[test]
    fn a_box_id_is_stable_so_a_box_can_be_started_again() {
        let workspace = Path::new(WORKSPACE);
        assert_eq!(box_id(workspace, 3), box_id(workspace, 3));
    }

    #[test]
    fn two_workspaces_with_the_same_basename_get_different_boxes() {
        assert_ne!(
            box_id(Path::new("/a/proj"), 0),
            box_id(Path::new("/b/proj"), 0)
        );
    }

    /// A typed id that could never name a box is refused as a typo, not
    /// resolved against a directory that will never exist.
    #[test]
    fn only_twelve_lowercase_hex_characters_can_name_a_box() {
        assert!(is_box_id("0123456789ab"));
        for bad in [
            "",
            "0123456789",
            "0123456789abc",
            "0123456789AB",
            "0123456789zz",
        ] {
            assert!(!is_box_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_home_is_named_by_the_workspace_basename_and_the_box_id() {
        let home = home_dir(Path::new("/data"), &key(0));
        assert!(
            home.starts_with("/data/wormhole/homes"),
            "{}",
            home.display()
        );
        let name = home.file_name().expect("name").to_string_lossy();
        let (basename, id) = name.split_once('-').expect("basename-id");
        assert_eq!(basename, "proj");
        assert!(is_box_id(id), "{id}");
    }

    /// Several boxes may share a workspace. None of them shares a home:
    /// one shared `$HOME` is two agents writing one history, one config
    /// and one instructions file at the same time.
    #[test]
    fn two_boxes_in_one_workspace_never_share_a_home() {
        let data = Path::new("/data");
        assert_ne!(home_dir(data, &key(0)), home_dir(data, &key(1)));
    }

    #[test]
    fn a_workspace_without_a_basename_still_gets_a_home() {
        let root = Path::new("/");
        let home = home_dir(Path::new("/data"), &box_key(root, &box_id(root, 0)));
        assert!(
            home.file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("workspace-"),
            "{}",
            home.display()
        );
    }

    #[test]
    fn the_same_box_always_gets_the_same_lock() {
        let data = Path::new("/data");
        assert_eq!(lock_file(data, &key(0)), lock_file(data, &key(0)));
    }

    /// Only starting the *same* box twice is refused. Two boxes in one
    /// workspace are two boxes, and each holds its own claim.
    #[test]
    fn two_boxes_in_one_workspace_hold_separate_claims() {
        let data = Path::new("/data");
        assert_ne!(lock_file(data, &key(0)), lock_file(data, &key(1)));
    }

    /// The home is the agent's to wipe; the claim on it is not. A lock a
    /// `rm -rf ~/.local/share/wormhole/homes/...` could drop would let a
    /// second start in behind the first.
    #[test]
    fn a_lock_is_not_inside_the_home_it_guards() {
        let data = Path::new("/data");
        assert!(!lock_file(data, &key(0)).starts_with(home_dir(data, &key(0))));
    }

    /// The home, the lock and the snapshot answer the same question —
    /// which box is this — so they are named by one key and cannot drift.
    #[test]
    fn the_lock_and_the_snapshot_agree_with_the_home_on_which_box_they_are() {
        let data = Path::new("/data");
        let home = home_dir(data, &key(0));
        let named = home.file_name().expect("name").to_string_lossy();
        assert_eq!(
            lock_file(data, &key(0)).file_name().expect("name"),
            std::ffi::OsStr::new(&format!("{named}.lock"))
        );
        assert_eq!(
            snapshot_dir(data, &key(0)).file_name().expect("name"),
            std::ffi::OsStr::new(&*named)
        );
    }

    /// A receipt is a diff against this box's own snapshot. Sharing one
    /// between boxes would report the other box's edits as this one's.
    #[test]
    fn two_boxes_in_one_workspace_never_share_a_snapshot() {
        let data = Path::new("/data");
        assert_ne!(snapshot_dir(data, &key(0)), snapshot_dir(data, &key(1)));
    }

    #[test]
    fn a_base_lives_under_its_tarball_digest() {
        assert_eq!(
            base_dir(Path::new("/home/me/.local/share"), DIGEST),
            PathBuf::from(format!("/home/me/.local/share/wormhole/bases/{DIGEST}"))
        );
    }

    #[test]
    fn an_artifact_lives_under_its_own_digest() {
        assert_eq!(
            artifact_file(Path::new("/data"), DIGEST),
            PathBuf::from(format!("/data/wormhole/artifacts/{DIGEST}"))
        );
    }

    #[test]
    fn an_image_lives_under_its_recipe_digest_not_the_tarballs() {
        let data_home = Path::new("/data");
        assert_ne!(image_dir(data_home, DIGEST), base_dir(data_home, DIGEST));
    }

    #[test]
    fn a_role_lives_in_config_under_its_name() {
        assert_eq!(
            role_dir(Path::new("/home/me/.config"), "architect"),
            PathBuf::from("/home/me/.config/wormhole/roles/architect")
        );
    }

    /// A fetched role is wormhole's own writing, so it lives in data —
    /// beside the bases and images it is exactly like, keyed by a digest
    /// and never mutated.
    #[test]
    fn a_fetched_commit_lives_in_data_under_the_commit() {
        let data = Path::new("/data");
        let checkout = checkout_dir(data, DIGEST);
        assert_eq!(
            checkout,
            PathBuf::from(format!("/data/wormhole/checkouts/{DIGEST}"))
        );
        assert_ne!(checkout, checkout_dir(data, &DIGEST.replace('0', "1")));
    }

    /// The approval is about a commit, so one commit fetched for two
    /// roles is approved once — and a changed pin has no approval at all.
    #[test]
    fn an_approval_names_the_commit_and_sits_outside_the_checkout() {
        let data = Path::new("/data");
        let approval = approval_file(data, DIGEST);
        assert!(!approval.starts_with(checkout_dir(data, DIGEST)));
        assert_ne!(approval, approval_file(data, &DIGEST.replace('0', "1")));
    }

    #[test]
    fn a_box_lives_under_its_owning_pid() {
        assert_eq!(
            box_dir(Path::new("/data"), 4242),
            PathBuf::from("/data/wormhole/boxes/4242")
        );
    }

    /// A build's claim and a box's claim live in one directory, so a name
    /// they could share would have a build hold a box or the other way
    /// round.
    #[test]
    fn a_build_claim_is_never_a_box_claim() {
        let data = Path::new("/data");
        assert_ne!(build_lock(data, DIGEST), lock_file(data, &key(0)));
        assert_ne!(
            build_lock(data, DIGEST),
            build_lock(data, &DIGEST.replace('0', "1"))
        );
    }

    #[test]
    fn a_partial_directory_cannot_be_mistaken_for_a_finished_one() {
        let dir = image_dir(Path::new("/data"), DIGEST);
        assert_ne!(partial(&dir), dir);
        assert!(partial(&dir).to_string_lossy().ends_with(".partial"));
    }

    /// A relative or empty XDG variable is invalid by the spec, and the
    /// reason is not pedantry: a relative store follows the working
    /// directory, so two boxes started in different folders would keep
    /// their locks, homes and registries in different places and neither
    /// would see the other.
    #[test]
    fn only_an_absolute_xdg_directory_is_used() {
        let home = Path::new("/home/me");
        assert_eq!(
            xdg_dir(Some(Path::new("/data")), home, ".local/share"),
            PathBuf::from("/data")
        );
        for invalid in ["", "share", "./share"] {
            assert_eq!(
                xdg_dir(Some(Path::new(invalid)), home, ".local/share"),
                PathBuf::from("/home/me/.local/share"),
                "{invalid:?} is not an absolute path"
            );
        }
        assert_eq!(
            xdg_dir(None, home, ".config"),
            PathBuf::from("/home/me/.config")
        );
    }
}
