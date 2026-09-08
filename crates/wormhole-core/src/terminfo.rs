//! Where a terminal's compiled description lives — on the host, and in the
//! box. `TERM` is only a name; every curses program turns it into a
//! description by looking it up, and a box carries only what its image
//! ships. So a host running Ghostty or kitty names a terminal nothing in
//! the box can find, and the box is left guessing what it is drawing on.
//!
//! Naming a terminal the image happens to carry answers that by telling
//! the box something untrue. Carrying the host's own description across
//! answers it by making the true name resolvable, which is what this
//! module is for: it says where to read the description from and where the
//! box reads it back.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// What every box is told about its terminal unless the manifest says
/// otherwise. `TERM` falls back to a name every image's ncurses carries,
/// for when there was no host description to carry across; the other
/// three pass the host's value through and describe what `TERM` does not.
pub const ENV_DEFAULTS: [(&str, &str); 4] = [
    ("TERM", "xterm-256color"),
    ("COLORTERM", ""),
    ("TERM_PROGRAM", ""),
    ("TERM_PROGRAM_VERSION", ""),
];

/// Where the box reads descriptions it was given. ncurses searches this
/// before any system directory and without being told to, so nothing in
/// the box needs configuring.
const IN_BOX: &str = ".terminfo";

/// The compiled-in list ncurses falls back on, in its order.
const SYSTEM: [&str; 5] = [
    "/usr/share/terminfo",
    "/etc/terminfo",
    "/lib/terminfo",
    "/usr/lib/terminfo",
    "/usr/share/lib/terminfo",
];

/// The directories ncurses reads, in the order it reads them: `TERMINFO`,
/// then the user's own, then `TERMINFO_DIRS`, then the compiled-in list.
/// An empty `TERMINFO_DIRS` element means the compiled-in list, which is
/// why it can appear in the middle.
pub fn search_roots(env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut add = |path: PathBuf| {
        if !roots.contains(&path) {
            roots.push(path);
        }
    };
    if let Some(one) = env.get("TERMINFO").filter(|value| !value.is_empty()) {
        add(PathBuf::from(one));
    }
    if let Some(home) = env.get("HOME").filter(|value| !value.is_empty()) {
        add(PathBuf::from(home).join(IN_BOX));
    }
    for entry in env
        .get("TERMINFO_DIRS")
        .map_or("", String::as_str)
        .split(':')
    {
        if entry.is_empty() {
            SYSTEM.iter().for_each(|path| add(PathBuf::from(path)));
        } else {
            add(PathBuf::from(entry));
        }
    }
    SYSTEM.iter().for_each(|path| add(PathBuf::from(path)));
    roots
}

/// Both paths a compiled description for `term` can have under a root:
/// ncurses names the directory after the first character, or after that
/// character's hex code where a case-insensitive filesystem forced it to.
/// `None` when the name is not one a terminal may have — a name is used as
/// a path here, so anything that could leave the directory it belongs in
/// is refused rather than escaped.
pub fn candidates(term: &str) -> Option<[PathBuf; 2]> {
    usable(term).then(|| {
        [
            PathBuf::from(&term[..1]).join(term),
            PathBuf::from(format!("{:x}", term.as_bytes()[0])).join(term),
        ]
    })
}

/// Where the box reads the description back, relative to its home.
pub fn seeded_at(term: &str) -> Option<PathBuf> {
    let [by_letter, _] = candidates(term)?;
    Some(PathBuf::from(IN_BOX).join(by_letter))
}

/// A terminal name, and nothing that is really a path. ncurses itself
/// allows more, but every name it puts in a database is of this shape, and
/// what is refused here is refused into a fallback rather than an error.
fn usable(term: &str) -> bool {
    term.len() <= 64
        && term
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && term
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'+' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// The name is used as a path, so a name that is a path must find
    /// nothing at all rather than a file outside the database.
    #[test]
    fn a_name_that_is_really_a_path_names_no_description() {
        for hostile in [
            "",
            ".",
            "..",
            "../../etc/passwd",
            "x/../../etc/passwd",
            "xterm/ghostty",
            "xterm ghostty",
            "xterm\nghostty",
            "xterm\0",
        ] {
            assert_eq!(candidates(hostile), None, "{hostile:?}");
            assert_eq!(seeded_at(hostile), None, "{hostile:?}");
        }
    }

    #[test]
    fn a_description_is_looked_for_by_letter_and_by_hex() {
        assert_eq!(
            candidates("xterm-ghostty"),
            Some([
                PathBuf::from("x/xterm-ghostty"),
                PathBuf::from("78/xterm-ghostty"),
            ])
        );
    }

    #[test]
    fn the_box_reads_it_back_from_its_own_home() {
        assert_eq!(
            seeded_at("xterm-ghostty"),
            Some(PathBuf::from(".terminfo/x/xterm-ghostty"))
        );
    }

    /// The order is ncurses's own: what the user set beats what the system
    /// ships, or a box could never be handed a description at all.
    #[test]
    fn the_search_order_is_the_one_ncurses_uses() {
        let roots = search_roots(&env(&[
            ("TERMINFO", "/one"),
            ("HOME", "/home/me"),
            ("TERMINFO_DIRS", "/two"),
        ]));
        assert_eq!(
            &roots[..3],
            [
                PathBuf::from("/one"),
                PathBuf::from("/home/me/.terminfo"),
                PathBuf::from("/two"),
            ]
        );
        assert_eq!(
            roots.last(),
            Some(&PathBuf::from("/usr/share/lib/terminfo"))
        );
    }

    /// ncurses reads an empty `TERMINFO_DIRS` element as "the compiled-in
    /// list", and it can sit in the middle of one.
    #[test]
    fn an_empty_element_stands_for_the_system_directories() {
        let roots = search_roots(&env(&[("TERMINFO_DIRS", "/first::/last")]));
        assert_eq!(roots.first(), Some(&PathBuf::from("/first")));
        assert!(roots.contains(&PathBuf::from("/usr/share/terminfo")));
        assert!(
            roots.iter().position(|r| r == &PathBuf::from("/last"))
                > roots
                    .iter()
                    .position(|r| r == &PathBuf::from("/usr/share/terminfo")),
            "{roots:?}"
        );
    }

    /// A host that names the same directory twice must not make the box
    /// read it twice.
    #[test]
    fn a_directory_named_twice_is_searched_once() {
        let roots = search_roots(&env(&[("TERMINFO", "/etc/terminfo")]));
        assert_eq!(
            roots
                .iter()
                .filter(|r| *r == &PathBuf::from("/etc/terminfo"))
                .count(),
            1,
            "{roots:?}"
        );
    }

    #[test]
    fn a_host_with_no_terminfo_variables_still_reads_the_system_list() {
        assert_eq!(
            search_roots(&BTreeMap::new()),
            SYSTEM.map(PathBuf::from).to_vec()
        );
    }
}
