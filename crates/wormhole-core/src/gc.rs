//! What in the data home can be given back, and what only looks like it
//! can. The walking and the deleting are the binary's; the verdict and the
//! report are decided here.
//!
//! The rule the whole module turns on: **wormhole deletes only what it can
//! prove is dead.** A kept home holds an agent's history, settings and
//! logins, and an image costs over a gigabyte to rebuild. Guessing wrong in
//! either direction is worse than leaving the pile alone, so anything whose
//! references are not recorded is reported and never removed.

use std::fmt;
use std::path::PathBuf;

/// One thing in the data home, and what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub bytes: u64,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Proven dead: the box's process is gone, or the workspace this home
    /// was kept for no longer exists. Safe to remove.
    Dead(String),
    /// In use, or claimed by something still running.
    Live(String),
    /// Nothing records what references this, so nothing can prove it is
    /// unreferenced. Reported with its size; never removed.
    Unproven(String),
}

impl Verdict {
    pub fn reclaimable(&self) -> bool {
        matches!(self, Verdict::Dead(_))
    }

    fn reason(&self) -> &str {
        match self {
            Verdict::Dead(r) | Verdict::Live(r) | Verdict::Unproven(r) => r,
        }
    }

    fn word(&self) -> &'static str {
        match self {
            Verdict::Dead(_) => "dead",
            Verdict::Live(_) => "live",
            Verdict::Unproven(_) => "unproven",
        }
    }
}

/// Whether a kept home is still wanted. `workspace` is what the home's own
/// stamp says it belongs to, and `exists` is whether that path is still
/// there.
///
/// A home with no stamp is left alone: it was kept by a build that did not
/// record its workspace, and a digest cannot be inverted to find out which
/// one. Removing it on a guess would take an agent's whole history with it.
pub fn home_verdict(workspace: Option<&str>, exists: bool) -> Verdict {
    match workspace {
        None => Verdict::Unproven(
            "this home records no workspace, so nothing can tell whether it is still wanted"
                .to_owned(),
        ),
        Some(path) if exists => Verdict::Live(format!("{path} is still there")),
        Some(path) => Verdict::Dead(format!("{path} no longer exists")),
    }
}

/// What `wormhole gc` prints: every item, grouped, with the total that
/// would be given back. Sizes are shown for everything, including what is
/// kept — a pile you cannot see is a pile you cannot decide about.
pub fn report(items: &[Item], deleting: bool) -> String {
    let mut text = String::new();
    for item in items {
        text.push_str(&format!(
            "{:<9} {:>9}  {}\n          {}\n",
            item.verdict.word(),
            Bytes(item.bytes).to_string(),
            item.path.display(),
            item.verdict.reason(),
        ));
    }
    if items.is_empty() {
        text.push_str("nothing in the data home yet\n");
        return text;
    }

    let reclaimable: u64 = items
        .iter()
        .filter(|item| item.verdict.reclaimable())
        .map(|item| item.bytes)
        .sum();
    let held: u64 = items
        .iter()
        .filter(|item| !item.verdict.reclaimable())
        .map(|item| item.bytes)
        .sum();
    text.push_str(&format!(
        "\n{} reclaimable, {} kept\n",
        Bytes(reclaimable),
        Bytes(held)
    ));
    if !deleting && reclaimable > 0 {
        text.push_str("nothing was removed; run `wormhole gc --delete` to give it back\n");
    }
    text
}

/// A size a person reads, not a number of bytes.
pub struct Bytes(pub u64);

impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0;
        for (limit, suffix, scale) in [
            (1024u64.pow(3), "GB", 1024u64.pow(3)),
            (1024u64.pow(2), "MB", 1024u64.pow(2)),
            (1024, "kB", 1024),
        ] {
            if bytes >= limit {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "one decimal place of a size a person reads"
                )]
                return write!(f, "{:.1} {suffix}", bytes as f64 / scale as f64);
            }
        }
        write!(f, "{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gap this closes: a kept home outlives the workspace it was kept
    /// for, and nothing ever reclaims it.
    #[test]
    fn a_home_whose_workspace_is_gone_is_dead() {
        let verdict = home_verdict(Some("/home/me/proj"), false);
        assert!(verdict.reclaimable(), "{verdict:?}");
        assert!(verdict.reason().contains("/home/me/proj"));
    }

    #[test]
    fn a_home_whose_workspace_is_still_there_is_live() {
        assert!(!home_verdict(Some("/home/me/proj"), true).reclaimable());
    }

    /// A home is where an agent's history, settings and logins live. One
    /// kept by an older wormhole records no workspace, and a digest cannot
    /// be inverted — so it is reported, never guessed at.
    #[test]
    fn a_home_that_records_no_workspace_is_never_removed_on_a_guess() {
        let verdict = home_verdict(None, false);
        assert!(!verdict.reclaimable(), "{verdict:?}");
        assert!(matches!(verdict, Verdict::Unproven(_)));
    }

    fn item(path: &str, bytes: u64, verdict: Verdict) -> Item {
        Item {
            path: PathBuf::from(path),
            bytes,
            verdict,
        }
    }

    #[test]
    fn the_report_totals_what_would_come_back_and_what_stays() {
        let items = [
            item(
                "/d/homes/gone-abc",
                2 * 1024 * 1024 * 1024,
                Verdict::Dead("/w/gone no longer exists".to_owned()),
            ),
            item(
                "/d/images/abc",
                1024 * 1024 * 1024,
                Verdict::Unproven("nothing records what uses this".to_owned()),
            ),
        ];
        let text = report(&items, false);
        assert!(text.contains("2.0 GB reclaimable"), "{text}");
        assert!(text.contains("1.0 GB kept"), "{text}");
        assert!(text.contains("/d/homes/gone-abc"), "{text}");
        assert!(text.contains("no longer exists"), "{text}");
        assert!(text.contains("gc --delete"), "{text}");
    }

    /// Listing is the default, so nothing is given back by a command that
    /// was only meant to look.
    #[test]
    fn the_report_does_not_offer_the_delete_hint_when_it_is_deleting() {
        let items = [item("/d/x", 1, Verdict::Dead("gone".to_owned()))];
        assert!(!report(&items, true).contains("gc --delete"));
    }

    #[test]
    fn an_empty_data_home_says_so() {
        assert!(report(&[], false).contains("nothing in the data home"));
    }

    #[test]
    fn sizes_read_as_a_person_reads_them() {
        assert_eq!(Bytes(512).to_string(), "512 B");
        assert_eq!(Bytes(2048).to_string(), "2.0 kB");
        assert_eq!(Bytes(1024 * 1024 * 3 / 2).to_string(), "1.5 MB");
    }
}
