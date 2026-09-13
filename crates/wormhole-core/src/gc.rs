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
use std::path::{Path, PathBuf};

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
    /// Proven that no box on this host would start from it — which is not
    /// the same as proven unwanted, because a recipe with no box yet
    /// references nothing that can be counted. Removed only when asked
    /// for by name, never by a bare `--delete`.
    Unreferenced(String),
    /// Nothing records what references this, so nothing can prove it is
    /// unreferenced. Reported with its size; never removed.
    Unproven(String),
}

/// What a `gc` run was asked to take.
///
/// One value rather than two loose booleans threaded through a report and
/// a delete loop: the two must agree, and the way to make them agree is
/// for there to be one of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sweep {
    /// Remove what the scan proved dead. Off by default: looking is the
    /// safe thing, so looking is what a bare `gc` does.
    pub delete: bool,
    /// Also remove what no box on this host starts from. A wider and less
    /// certain claim than dead, so it is asked for separately.
    pub unreferenced: bool,
}

impl Verdict {
    /// Whether this sweep takes it.
    fn taken_by(&self, sweep: Sweep) -> bool {
        match self {
            Verdict::Dead(_) => true,
            Verdict::Unreferenced(_) => sweep.unreferenced,
            Verdict::Live(_) | Verdict::Unproven(_) => false,
        }
    }

    fn reason(&self) -> &str {
        match self {
            Verdict::Dead(r)
            | Verdict::Live(r)
            | Verdict::Unreferenced(r)
            | Verdict::Unproven(r) => r,
        }
    }

    fn word(&self) -> &'static str {
        match self {
            Verdict::Dead(_) => "dead",
            Verdict::Live(_) => "live",
            Verdict::Unreferenced(_) => "unreferenced",
            Verdict::Unproven(_) => "unproven",
        }
    }
}

/// Whether a built thing — an image, a base rootfs, a fetched artifact —
/// is still referenced by a recipe some box on this host would start from.
///
/// `referenced` is every digest collected from those recipes; `complete`
/// says whether all of them could be read. One unreadable recipe may be
/// the very one that references this, so an incomplete answer proves
/// nothing and says which it is — the alternative is deleting a gigabyte
/// on a gap in the evidence.
pub fn built_verdict(
    digest: &str,
    referenced: &std::collections::BTreeSet<String>,
    complete: bool,
    what: &str,
) -> Verdict {
    if referenced.contains(digest) {
        return Verdict::Live(format!("a box on this host starts from this {what}"));
    }
    if complete {
        Verdict::Unreferenced(format!("no box on this host starts from this {what}"))
    } else {
        Verdict::Unproven(format!(
            "a box's recipe could not be read, so nothing can prove this {what} unreferenced"
        ))
    }
}

/// Whether a lock file is still a claim on anything, from its file stem
/// and the key of every kept home. A box's lock whose home is gone belongs
/// to a box that no longer exists.
///
/// `rm` leaves one behind on purpose: unlinking the lock it is holding
/// would let another start take a second, different one for the same box.
/// So reclaiming them is this command's job, and the rule lives here with
/// every other verdict.
///
/// A build claim lives in the same directory and never has a home. It is
/// never taken: unlinked while held, the next builder locks a new file.
pub fn lock_verdict(stem: &str, kept: &std::collections::BTreeSet<String>) -> Verdict {
    if crate::paths::is_build_lock(stem) {
        return Verdict::Live("a build's claim, never a box's".to_owned());
    }
    kept_verdict(stem, kept)
}

/// Whether a lock or a record still belongs to a box, from its file stem
/// and the key of every kept home. The home is the box; without one there
/// is nothing left to claim or describe.
pub fn kept_verdict(stem: &str, kept: &std::collections::BTreeSet<String>) -> Verdict {
    if kept.contains(stem) {
        Verdict::Live("the box it belongs to is still kept".to_owned())
    } else {
        Verdict::Dead("the box it belonged to is gone".to_owned())
    }
}

/// What a kept box's recipe says about who can still start it, as far as
/// it could be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipe {
    /// Resumed only where it has run: a role that says so, or a box made
    /// from its workspace's own manifest.
    Here,
    /// A role that lets any workspace resume its box.
    Anywhere,
    /// A role whose directory, named here, no longer exists.
    Gone(String),
    /// A role that could not be read.
    Unread,
}

/// What a kept box's recipe says about who can still start it. `read` is
/// its manifest when that could be read; `exists` says whether a path is
/// still there.
pub fn recipe(
    record: &crate::home::Record,
    read: Option<&crate::manifest::Manifest>,
    exists: impl Fn(&Path) -> bool,
) -> Recipe {
    if record.role.is_none() && record.source.is_none() {
        return Recipe::Here;
    }
    if let Some(dir) = record.source.as_deref().and_then(crate::source::dir_of)
        && !exists(Path::new(dir))
    {
        return Recipe::Gone(dir.to_owned());
    }
    match read.map(|manifest| manifest.agent.resume) {
        Some(crate::manifest::Resume::Anywhere) => Recipe::Anywhere,
        Some(crate::manifest::Resume::Here) => Recipe::Here,
        // Older than role identities, so judged the way such boxes always
        // were: by their workspace.
        None if record.source.is_none() => Recipe::Here,
        None => Recipe::Unread,
    }
}

/// What `gc` knows about one kept box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence<'a> {
    /// Where it last ran.
    pub workspace: &'a str,
    /// Whether any workspace it ran in is still there.
    pub any_workspace_exists: bool,
    pub recipe: Recipe,
}

/// Whether a kept home is still wanted, from its record.
///
/// A box with no record is left alone: a digest cannot be inverted to find
/// out which workspace it served, and removing it on a guess would take an
/// agent's whole history with it. A box nothing can start is dead: its
/// workspaces are gone and its role resumes only where it ran, or its role
/// is gone.
pub fn home_verdict(evidence: Option<Evidence<'_>>) -> Verdict {
    let Some(evidence) = evidence else {
        return Verdict::Unproven(
            "this home records no workspace, so nothing can tell whether it is still wanted"
                .to_owned(),
        );
    };
    let workspace = evidence.workspace;
    match evidence.recipe {
        Recipe::Gone(role) => Verdict::Dead(format!(
            "the role at {role} no longer exists, so nothing can start this box"
        )),
        Recipe::Anywhere => Verdict::Live("its role lets any workspace resume it".to_owned()),
        _ if evidence.any_workspace_exists => Verdict::Live(format!(
            "a workspace it ran in is still there; last {workspace}"
        )),
        Recipe::Unread => Verdict::Unproven(format!(
            "{workspace} and every other workspace it ran in are gone, and its role \
             could not be read to say whether another may resume it"
        )),
        Recipe::Here => Verdict::Dead(format!(
            "{workspace} no longer exists, nor any other workspace it ran in"
        )),
    }
}

/// What a sweep takes and what it leaves, decided once.
///
/// The report prints this and the caller deletes from it, so the set that
/// was described and the set that was removed are the same set by
/// construction — the alternative is a report that can say one thing while
/// the loop does another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan<'a> {
    pub take: Vec<&'a Item>,
    pub keep: Vec<&'a Item>,
    /// What `--unreferenced` would add, when this sweep is not taking it.
    /// Zero when it is, so the offer is made exactly when it is an offer.
    pub offered: u64,
}

pub fn plan(items: &[Item], sweep: Sweep) -> Plan<'_> {
    let (take, keep) = items.iter().partition(|item| item.verdict.taken_by(sweep));
    let offered = if sweep.unreferenced {
        0
    } else {
        items
            .iter()
            .filter(|item| matches!(item.verdict, Verdict::Unreferenced(_)))
            .map(|item| item.bytes)
            .sum()
    };
    Plan {
        take,
        keep,
        offered,
    }
}

/// What `wormhole gc` prints: every item, with the total that would be
/// given back. Sizes are shown for everything, including what is kept — a
/// pile you cannot see is a pile you cannot decide about.
pub fn report(items: &[Item], sweep: Sweep, plan: &Plan<'_>) -> String {
    if items.is_empty() {
        return "nothing in the data home yet\n".to_owned();
    }
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(items.len() * 2);
    for item in items {
        rows.push(vec![
            item.verdict.word().to_owned(),
            Bytes(item.bytes).to_string(),
            item.path.display().to_string(),
        ]);
        // The reason on its own row, so the columns stay a table and a
        // long sentence never decides how wide the size column is.
        rows.push(vec![
            String::new(),
            String::new(),
            item.verdict.reason().to_owned(),
        ]);
    }
    let mut text = crate::table::render(&rows);
    let weight = |of: &[&Item]| -> u64 { of.iter().map(|item| item.bytes).sum() };
    text.push_str(&format!(
        "\n{} reclaimable, {} kept\n",
        Bytes(weight(&plan.take)),
        Bytes(weight(&plan.keep))
    ));
    if !sweep.delete && !plan.take.is_empty() {
        text.push_str("nothing was removed; run `wormhole gc --delete` to give it back\n");
    }
    // Said whenever there is unreferenced weight this run is not taking:
    // the space is the whole reason to look, and a total that silently
    // leaves out a gigabyte of images is the report failing at its job.
    if plan.offered > 0 {
        text.push_str(&format!(
            "{} more is referenced by no box here; \
             `wormhole gc --delete --unreferenced` gives that back too\n",
            Bytes(plan.offered)
        ));
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

    /// The two sweeps every verdict is judged against: a bare `--delete`,
    /// and one widened by `--unreferenced`.
    const LOOK: Sweep = Sweep {
        delete: true,
        unreferenced: false,
    };
    const WIDE: Sweep = Sweep {
        delete: true,
        unreferenced: true,
    };

    /// Whether a sweep takes a verdict, asked through the same partition
    /// the report and the delete loop both read.
    fn taken(verdict: &Verdict, sweep: Sweep) -> bool {
        let items = [item("/d/x", 1, verdict.clone())];
        !plan(&items, sweep).take.is_empty()
    }

    fn said(items: &[Item], sweep: Sweep) -> String {
        report(items, sweep, &plan(items, sweep))
    }

    fn kept(any_workspace_exists: bool, recipe: Recipe) -> Option<Evidence<'static>> {
        Some(Evidence {
            workspace: "/home/me/proj",
            any_workspace_exists,
            recipe,
        })
    }

    /// The gap this closes: a kept home outlives the workspace it was kept
    /// for, and nothing ever reclaims it.
    #[test]
    fn a_home_whose_workspace_is_gone_is_dead() {
        let verdict = home_verdict(kept(false, Recipe::Here));
        assert!(taken(&verdict, LOOK), "{verdict:?}");
        assert!(verdict.reason().contains("/home/me/proj"));
    }

    #[test]
    fn a_home_whose_workspace_is_still_there_is_live() {
        for recipe in [Recipe::Here, Recipe::Unread] {
            assert!(!taken(&home_verdict(kept(true, recipe)), WIDE));
        }
    }

    /// Every project it ran in may be gone; the next one started with its
    /// role still resumes it.
    #[test]
    fn a_box_any_workspace_may_resume_is_live_without_its_workspaces() {
        let verdict = home_verdict(kept(false, Recipe::Anywhere));
        assert!(!taken(&verdict, WIDE), "{verdict:?}");
    }

    /// Nothing can start a box whose role is gone: every start resolves
    /// the role it records, and a start from another role is refused.
    #[test]
    fn a_box_whose_role_is_gone_is_dead_wherever_it_ran() {
        let verdict = home_verdict(kept(true, Recipe::Gone("/roles/alphaca".to_owned())));
        assert!(taken(&verdict, LOOK), "{verdict:?}");
        assert!(verdict.reason().contains("/roles/alphaca"), "{verdict:?}");
    }

    #[test]
    fn a_box_recipe_is_read_for_who_may_still_start_it() {
        let made_here = crate::home::Record {
            key: "w-0123456789ab".to_owned(),
            id: "0123456789ab".to_owned(),
            workspace: PathBuf::from("/w"),
            earlier: Vec::new(),
            role: None,
            source: None,
            alias: None,
            name: None,
            agent: None,
            created_unix: 1,
            started_unix: 1,
        };
        let anywhere = crate::manifest::parse(&format!(
            "version = 1\n[agent]\nresume = \"anywhere\"\n\
             [image]\nbase = \"x\"\nbase_sha256 = \"{}\"\n",
            "a".repeat(64)
        ))
        .expect("valid");
        let all = |_: &Path| true;
        assert_eq!(recipe(&made_here, None, all), Recipe::Here);
        let role = crate::home::Record {
            role: Some("alphaca".to_owned()),
            source: Some("dir:/roles/alphaca".to_owned()),
            ..made_here
        };
        assert_eq!(recipe(&role, Some(&anywhere), all), Recipe::Anywhere);
        assert_eq!(recipe(&role, None, all), Recipe::Unread);
        assert_eq!(
            recipe(&role, Some(&anywhere), |_| false),
            Recipe::Gone("/roles/alphaca".to_owned())
        );
        let older = crate::home::Record {
            source: None,
            ..role
        };
        assert_eq!(recipe(&older, None, all), Recipe::Here);
    }

    /// Its role might let any workspace resume it; unread, nothing says.
    #[test]
    fn a_box_whose_role_cannot_be_read_and_whose_workspaces_are_gone_is_unproven() {
        let verdict = home_verdict(kept(false, Recipe::Unread));
        assert!(matches!(verdict, Verdict::Unproven(_)), "{verdict:?}");
    }

    /// `rm` leaves the lock it was holding behind on purpose, so this is
    /// what eventually takes it — and only once its box is provably gone.
    #[test]
    fn a_lock_outlives_its_box_and_then_becomes_reclaimable() {
        let kept = keys(&["proj-0123456789ab"]);
        assert!(taken(&lock_verdict("gone-0123456789ab", &kept), LOOK));
        assert!(!taken(&lock_verdict("proj-0123456789ab", &kept), WIDE));
    }

    #[test]
    fn a_record_whose_home_is_gone_is_dead() {
        let kept = keys(&["proj-0123456789ab"]);
        assert!(taken(&kept_verdict("gone-0123456789ab", &kept), LOOK));
        assert!(!taken(&kept_verdict("proj-0123456789ab", &kept), WIDE));
    }

    fn keys(names: &[&str]) -> std::collections::BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// A build claim shares the directory and never has a home. Unlinked
    /// while held, a second builder locks a fresh file and both write one
    /// `.partial`.
    #[test]
    fn a_build_claim_is_never_judged_as_a_box_that_is_gone() {
        let build = format!("build-{}", "ab".repeat(32));
        assert!(!taken(&lock_verdict(&build, &keys(&[])), WIDE));
        // A workspace called `build` is still a box, judged by its home.
        assert!(taken(&lock_verdict("build-0123456789ab", &keys(&[])), LOOK));
    }

    /// The report prints the plan and the caller deletes from it, so the
    /// set described and the set removed are the same set.
    #[test]
    fn the_plan_is_what_the_report_describes_and_what_the_caller_removes() {
        let items = [
            item("/d/dead", 1, Verdict::Dead("gone".to_owned())),
            item("/d/unref", 2, Verdict::Unreferenced("nothing".to_owned())),
            item("/d/live", 4, Verdict::Live("in use".to_owned())),
        ];
        let narrow = plan(&items, LOOK);
        assert_eq!(narrow.take.len(), 1);
        assert_eq!(narrow.take[0].path, PathBuf::from("/d/dead"));
        assert_eq!(narrow.offered, 2);

        let wide = plan(&items, WIDE);
        assert_eq!(wide.take.len(), 2);
        assert_eq!(wide.keep.len(), 1);
        // Already taking it, so there is nothing left to offer.
        assert_eq!(wide.offered, 0);
    }

    fn referenced(digests: &[&str]) -> std::collections::BTreeSet<String> {
        digests.iter().map(|d| (*d).to_owned()).collect()
    }

    /// The gap this closes: every pin bump leaves a gigabyte behind and
    /// nothing could ever prove it was safe to take.
    #[test]
    fn a_built_thing_no_box_starts_from_is_unreferenced() {
        let verdict = built_verdict("def", &referenced(&["abc"]), true, "image");
        assert!(matches!(verdict, Verdict::Unreferenced(_)), "{verdict:?}");
        assert!(verdict.reason().contains("image"), "{verdict:?}");
    }

    /// Unreferenced is not unwanted: a recipe nobody has started a box
    /// from yet references nothing that can be counted. So a bare
    /// `--delete` leaves it and `--unreferenced` is what asks for it.
    #[test]
    fn unreferenced_is_given_back_only_when_it_is_asked_for_by_name() {
        let verdict = built_verdict("def", &referenced(&["abc"]), true, "image");
        assert!(!taken(&verdict, LOOK), "{verdict:?}");
        assert!(taken(&verdict, WIDE), "{verdict:?}");
    }

    #[test]
    fn a_built_thing_a_box_starts_from_is_live_however_it_is_asked() {
        let verdict = built_verdict("abc", &referenced(&["abc"]), true, "image");
        assert!(matches!(verdict, Verdict::Live(_)), "{verdict:?}");
        assert!(!taken(&verdict, WIDE), "{verdict:?}");
    }

    /// One recipe that could not be read may be the very one that
    /// references this. An incomplete answer proves nothing, and deleting
    /// on a gap in the evidence is what this module exists to refuse.
    #[test]
    fn nothing_is_unreferenced_while_a_recipe_could_not_be_read() {
        let verdict = built_verdict("abc", &referenced(&[]), false, "base");
        assert!(matches!(verdict, Verdict::Unproven(_)), "{verdict:?}");
        assert!(!taken(&verdict, WIDE), "{verdict:?}");
    }

    /// A home is where an agent's history, settings and logins live. One
    /// kept by an older wormhole records no workspace, and a digest cannot
    /// be inverted — so it is reported, never guessed at.
    #[test]
    fn a_home_that_records_no_workspace_is_never_removed_on_a_guess() {
        let verdict = home_verdict(None);
        assert!(!taken(&verdict, WIDE), "{verdict:?}");
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
        let text = said(&items, Sweep::default());
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
        assert!(!said(&items, LOOK).contains("gc --delete"));
    }

    /// A total that leaves a gigabyte of images out of both columns
    /// without saying so is the report failing at the one job it has.
    #[test]
    fn the_report_says_how_much_more_the_unreferenced_flag_would_give_back() {
        let items = [item(
            "/d/images/abc",
            1024 * 1024 * 1024,
            Verdict::Unreferenced("no box on this host starts from this image".to_owned()),
        )];
        let offered = said(&items, LOOK);
        assert!(offered.contains("1.0 GB more"), "{offered}");
        assert!(offered.contains("--unreferenced"), "{offered}");
        // Already taking it: there is nothing left to offer.
        let widened = said(&items, WIDE);
        assert!(!widened.contains("--unreferenced"), "{widened}");
        assert!(widened.contains("1.0 GB reclaimable"), "{widened}");
    }

    #[test]
    fn an_empty_data_home_says_so() {
        assert!(said(&[], Sweep::default()).contains("nothing in the data home"));
    }

    #[test]
    fn sizes_read_as_a_person_reads_them() {
        assert_eq!(Bytes(512).to_string(), "512 B");
        assert_eq!(Bytes(2048).to_string(), "2.0 kB");
        assert_eq!(Bytes(1024 * 1024 * 3 / 2).to_string(), "1.5 MB");
    }
}
