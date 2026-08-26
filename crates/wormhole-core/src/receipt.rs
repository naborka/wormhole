//! What the agent changed in your workspace, as a fact rather than a
//! promise.
//!
//! CONCEPT.md §10 calls the live workspace mount the whole remaining local
//! blast radius and accepts the loss. Accepting it is one thing; not being
//! able to *see* it is another. A box takes a reflink snapshot of the
//! workspace before it starts and compares against it on the way out, so
//! what happened is proved rather than trusted.
//!
//! The comparison is decided here, from two listings. Walking the trees is
//! the binary's.

use std::fmt;
use std::path::PathBuf;

/// One file in a workspace listing, as cheaply as a walk can know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Relative to the workspace root, so the snapshot and the workspace
    /// are comparable at all.
    pub path: PathBuf,
    pub bytes: u64,
    /// Modification time in unix seconds.
    pub modified_unix: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    Added,
    Changed,
    Removed,
}

impl Change {
    fn word(self) -> &'static str {
        match self {
            Change::Added => "added",
            Change::Changed => "changed",
            Change::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// Sorted: kind first, then path, so two runs over the same changes
    /// read the same way.
    pub changes: Vec<(Change, PathBuf)>,
}

impl Receipt {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// What changed between the snapshot taken at start and the workspace at
/// exit.
///
/// Size and modification time, not content. Hashing a gigabyte-scale tree
/// on the way out of every box would cost more than the box, and the
/// receipt would arrive too late to be read. The limit is worth naming
/// rather than hiding: an edit that restores a file's exact length and
/// exact timestamp does not appear here. Nothing an agent does by accident
/// looks like that; something that set out to hide would have to try.
pub fn compare(before: &[Entry], after: &[Entry]) -> Receipt {
    let mut changes = Vec::new();
    for old in before {
        match after.iter().find(|new| new.path == old.path) {
            None => changes.push((Change::Removed, old.path.clone())),
            Some(new) if new.bytes != old.bytes || new.modified_unix != old.modified_unix => {
                changes.push((Change::Changed, old.path.clone()));
            }
            Some(_) => {}
        }
    }
    for new in after {
        if !before.iter().any(|old| old.path == new.path) {
            changes.push((Change::Added, new.path.clone()));
        }
    }
    changes.sort();
    Receipt { changes }
}

/// The receipt as the last thing a box prints. `limit` caps the list — a
/// build that touched ten thousand files must not bury the terminal — and
/// what is left over is counted rather than dropped silently.
pub fn render(receipt: &Receipt, limit: usize) -> String {
    if receipt.is_empty() {
        return "workspace: unchanged\n".to_owned();
    }
    let mut text = format!(
        "workspace: {} file{} changed\n",
        receipt.changes.len(),
        if receipt.changes.len() == 1 { "" } else { "s" }
    );
    for (change, path) in receipt.changes.iter().take(limit) {
        text.push_str(&format!("  {:<7} {}\n", change.word(), path.display()));
    }
    // Never a silent truncation: a list that stops without saying so reads
    // as "that was all of it".
    if let Some(rest) = receipt.changes.len().checked_sub(limit).filter(|n| *n > 0) {
        text.push_str(&format!("  … and {rest} more\n"));
    }
    text
}

impl fmt::Display for Receipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", render(self, 20))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, bytes: u64, modified_unix: u64) -> Entry {
        Entry {
            path: PathBuf::from(path),
            bytes,
            modified_unix,
        }
    }

    #[test]
    fn an_untouched_workspace_has_an_empty_receipt() {
        let tree = [entry("src/main.rs", 10, 100), entry("README.md", 5, 100)];
        assert!(compare(&tree, &tree).is_empty());
        assert_eq!(render(&compare(&tree, &tree), 20), "workspace: unchanged\n");
    }

    #[test]
    fn every_kind_of_change_is_named_with_its_path() {
        let before = [
            entry("kept", 1, 1),
            entry("edited", 1, 1),
            entry("gone", 1, 1),
        ];
        let after = [
            entry("kept", 1, 1),
            entry("edited", 2, 1),
            entry("new", 1, 1),
        ];
        assert_eq!(
            compare(&before, &after).changes,
            vec![
                (Change::Added, PathBuf::from("new")),
                (Change::Changed, PathBuf::from("edited")),
                (Change::Removed, PathBuf::from("gone")),
            ]
        );
    }

    /// A file rewritten to the same length is still a change; the
    /// timestamp is what catches it.
    #[test]
    fn a_rewrite_that_keeps_the_length_is_still_a_change() {
        let before = [entry("f", 10, 100)];
        let after = [entry("f", 10, 200)];
        assert_eq!(
            compare(&before, &after).changes,
            vec![(Change::Changed, PathBuf::from("f"))]
        );
    }

    /// A build that touched ten thousand files must not bury the terminal
    /// — and must not pretend the list it printed was all of it.
    #[test]
    fn a_long_receipt_is_capped_and_says_how_much_it_left_out() {
        let after: Vec<Entry> = (0..25).map(|i| entry(&format!("f{i:02}"), 1, 1)).collect();
        let text = render(&compare(&[], &after), 20);
        assert!(text.contains("25 files changed"), "{text}");
        assert!(text.contains("… and 5 more"), "{text}");
        assert_eq!(text.lines().count(), 22);
    }

    #[test]
    fn one_change_is_not_pluralised() {
        let text = render(&compare(&[], &[entry("f", 1, 1)]), 20);
        assert!(text.contains("1 file changed"), "{text}");
    }
}
