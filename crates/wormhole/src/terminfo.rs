//! Carrying the host's description of its own terminal into the box.
//!
//! `TERM` is only a name; every curses program turns it into a description
//! by looking it up, and a box carries only what its image ships. So a box
//! handed the name and not the description behind it does not know what it
//! is drawing on — and a box handed a name the image happens to carry has
//! been told something untrue instead.
//!
//! Both are the same hole: the box's terminal is decided without the box
//! being able to resolve it. Carrying the description closes it, and the
//! name can then be the true one. Where to read and where to write is
//! decided in `wormhole_core::terminfo`; this only moves the bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wormhole_core::terminfo;

/// The host environment as a box may be given it: the terminal's
/// description carried into the home, and `TERM` dropped when it could not
/// be — a name with no description behind it is worse than the merely
/// approximate one the manifest declares, and dropping it is what lets
/// that declaration stand.
pub fn carried(mut host: BTreeMap<String, String>, home: &Path) -> BTreeMap<String, String> {
    if carry(&host, home).is_none() {
        host.remove("TERM");
    }
    host
}

/// Carries the host's description of `TERM` into the box home. Returns the
/// name the box can now look up, or `None` when nothing could be carried.
pub fn carry(host: &BTreeMap<String, String>, home: &Path) -> Option<String> {
    let term = host.get("TERM")?;
    let target = home.join(terminfo::seeded_at(term)?);
    let description = std::fs::read(found(host, term)?).ok()?;
    crate::replace_file(&target, &description).ok()?;
    Some(term.clone())
}

/// The first description of `term` in the places the host reads them.
fn found(host: &BTreeMap<String, String>, term: &str) -> Option<PathBuf> {
    let named = terminfo::candidates(term)?;
    for root in terminfo::search_roots(host) {
        for under in &named {
            let path = root.join(under);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    fn scratch() -> TempDir {
        TempDir::new().expect("temporary directory")
    }

    /// One description, in a terminfo database rooted at `root`.
    fn describe(root: &Path, term: &str, body: &[u8]) {
        let entry = root.join(&term[..1]).join(term);
        std::fs::create_dir_all(entry.parent().expect("parent")).expect("database directory");
        std::fs::write(&entry, body).expect("description");
    }

    /// A host terminfo database holding one description, and the root to
    /// point `TERMINFO` at.
    fn host_database(term: &str, body: &[u8]) -> (TempDir, String) {
        let dir = scratch();
        describe(dir.path(), term, body);
        let root = dir.path().display().to_string();
        (dir, root)
    }

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn the_hosts_description_of_its_terminal_travels_into_the_box() {
        let body = b"\x1a\x01a terminal the image has never met";
        let (_host, root) = host_database("xterm-ghostty", body);
        let home = scratch();

        let carried = carry(
            &env(&[("TERM", "xterm-ghostty"), ("TERMINFO", &root)]),
            home.path(),
        );

        assert_eq!(carried.as_deref(), Some("xterm-ghostty"));
        assert_eq!(
            std::fs::read(home.path().join(".terminfo/x/xterm-ghostty")).expect("carried"),
            body,
            "a description that changed on the way is one the box cannot use"
        );
    }

    /// The name means nothing without the description, so a terminal the
    /// host cannot describe must not be named to the box.
    #[test]
    fn a_terminal_the_host_cannot_describe_is_not_named_to_the_box() {
        let (_host, root) = host_database("xterm-ghostty", b"x");
        let home = scratch();

        let carried = carry(
            &env(&[("TERM", "xterm-kitty"), ("TERMINFO", &root)]),
            home.path(),
        );

        assert_eq!(carried, None);
        assert!(
            !home.path().join(".terminfo").exists(),
            "nothing carried must leave nothing behind"
        );
    }

    #[test]
    fn a_host_that_names_no_terminal_carries_nothing() {
        let home = scratch();
        assert_eq!(carry(&BTreeMap::new(), home.path()), None);
        assert!(!home.path().join(".terminfo").exists());
    }

    /// The name is used as a path. One that is really a path must find
    /// nothing, rather than reach out of the database or into the home.
    #[test]
    fn a_terminal_name_that_is_really_a_path_carries_nothing() {
        let (_host, root) = host_database("xterm-ghostty", b"x");
        let home = scratch();

        assert_eq!(
            carry(
                &env(&[("TERM", "x/../../etc/passwd"), ("TERMINFO", &root)]),
                home.path()
            ),
            None
        );
        assert!(!home.path().join(".terminfo").exists());
    }

    /// A box home is kept and started again. A second start in another
    /// terminal must describe that one too.
    #[test]
    fn a_second_start_in_another_terminal_carries_that_one_too() {
        let (_first, first) = host_database("xterm-ghostty", b"ghostty");
        let (_second, second) = host_database("xterm-kitty", b"kitty");
        let home = scratch();

        carry(
            &env(&[("TERM", "xterm-ghostty"), ("TERMINFO", &first)]),
            home.path(),
        );
        let carried = carry(
            &env(&[("TERM", "xterm-kitty"), ("TERMINFO", &second)]),
            home.path(),
        );

        assert_eq!(carried.as_deref(), Some("xterm-kitty"));
        assert_eq!(
            std::fs::read(home.path().join(".terminfo/x/xterm-kitty")).expect("carried"),
            b"kitty"
        );
    }

    /// A description the user compiled for themselves lives in their own
    /// home, and that user is exactly the one whose terminal the image
    /// does not ship.
    #[test]
    fn a_description_the_user_compiled_for_themselves_travels_too() {
        let host_home = scratch();
        describe(
            &host_home.path().join(".terminfo"),
            "xterm-ghostty",
            b"mine",
        );
        let home = scratch();

        let carried = carry(
            &env(&[
                ("TERM", "xterm-ghostty"),
                ("HOME", &host_home.path().display().to_string()),
            ]),
            home.path(),
        );

        assert_eq!(carried.as_deref(), Some("xterm-ghostty"));
        assert_eq!(
            std::fs::read(home.path().join(".terminfo/x/xterm-ghostty")).expect("carried"),
            b"mine"
        );
    }

    /// The environment handed on keeps `TERM` only when the description
    /// went with it. What the manifest then makes of an absent `TERM` is
    /// `box_env`'s own rule, proved against the shipped manifests in
    /// `tests/manifests.rs`.
    #[test]
    fn the_environment_handed_on_names_the_terminal_only_once_it_resolves() {
        let (_host, root) = host_database("xterm-ghostty", b"ghostty");
        let home = scratch();

        let told = carried(
            env(&[("TERM", "xterm-ghostty"), ("TERMINFO", &root)]),
            home.path(),
        );
        assert_eq!(told.get("TERM").map(String::as_str), Some("xterm-ghostty"));

        let told = carried(
            env(&[("TERM", "xterm-kitty"), ("TERMINFO", &root)]),
            home.path(),
        );
        assert_eq!(
            told.get("TERM"),
            None,
            "a terminal with no description must not be named to the box"
        );
    }
}
