//! What a kept home records about the box it belongs to, and which box a
//! `wormhole box` should be.
//!
//! A box outlives every process that ever ran it: its home holds the
//! agent's history, its logins and whatever the preflight hook installed.
//! So the home is where the box's own record lives — one file, written by
//! the box itself on every start. Nothing else has to be kept in step,
//! and a home carried to another machine carries its box with it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::table;

/// Where a box's record sits, relative to its home.
pub const RECORD: &str = ".wormhole/box.toml";

/// The box's kept egress additions, relative to its home: what
/// `wormhole allow` appended, surviving the box's restarts the way its
/// history does. The manifest stays the role's word; this is the
/// person's, for this one box.
pub const KEPT_EGRESS: &str = ".wormhole/egress-extra";

/// What wormhole wrote there before boxes had ids: the workspace path,
/// alone. Still read, so a home from before this change is picked up as
/// its workspace's first box rather than orphaned.
pub const LEGACY_STAMP: &str = ".wormhole/workspace";

/// One box, as its home records it.
///
/// Like the registry entry, a key this version does not know is ignored
/// rather than refused: wormhole is this file's only writer, and a home
/// that stops parsing is a box that can no longer be listed or resumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    /// The workspace this box works in. A home is named by a digest, and
    /// a digest cannot be inverted: without this nothing could ever tell
    /// which workspace a home belongs to, or whether it still exists.
    pub workspace: PathBuf,
    /// The role this box was started from, as the user wrote it. `None`
    /// means the workspace's own manifest.
    ///
    /// For the listing and nothing else. What the user typed is one of
    /// several ways to name one role, so it can never be what decides
    /// whether two boxes are the same box.
    #[serde(default)]
    pub role: Option<String>,
    /// Where that role comes from: the canonical directory of a local
    /// one, the repository URL of a fetched one. This is what decides
    /// which box a start resumes.
    ///
    /// `None` on a home written before roles had an identity, and on any
    /// box started from the workspace's own manifest. The first start
    /// under a wormhole that knows identities fills it in.
    #[serde(default)]
    pub source: Option<String>,
    /// What you called this box, if you called it anything.
    ///
    /// A box already has an id, which is derived and stable and cannot be
    /// chosen. An alias is the name a person types instead. Twelve hex
    /// characters is always an id, so an alias may never look like one —
    /// see [`is_usable_alias`].
    #[serde(default)]
    pub alias: Option<String>,
    /// The manifest's `name`, for listings.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    pub created_unix: u64,
    pub started_unix: u64,
}

pub fn to_toml(record: &Record) -> Result<String, String> {
    toml::to_string(record).map_err(|e| format!("cannot serialize the box record: {e}"))
}

pub fn parse(text: &str) -> Result<Record, String> {
    toml::from_str(text).map_err(|e| format!("box.toml is not valid: {}", e.message()))
}

/// The record a pre-id home implies: the workspace it stamped, and the id
/// its directory name already ends in. Both zero times, so a listing sorts
/// it last and the first start writes the real thing.
pub fn from_legacy(home: &Path, stamped: &str) -> Option<Record> {
    let id = crate::paths::key_id(home.file_name()?.to_str()?)?.to_owned();
    let workspace = stamped.trim();
    if workspace.is_empty() {
        return None;
    }
    Some(Record {
        id,
        workspace: PathBuf::from(workspace),
        role: None,
        source: None,
        alias: None,
        name: None,
        agent: None,
        created_unix: 0,
        started_unix: 0,
    })
}

/// Whether a string may be an alias for a box.
///
/// The same characters a role's name allows, and one rule of its own: an
/// alias must not be spellable as a box id, or `wormhole attach a3f9c1e40b`
/// would name two things and the tool would have to guess which.
pub fn is_usable_alias(alias: &str) -> bool {
    crate::source::is_usable_name(alias) && !crate::paths::is_box_id(alias)
}

/// Whether this box answers to what was typed: its id, or the alias
/// somebody gave it.
///
/// One rule, so every command that takes a box takes the same two forms.
/// Which form it is comes from the string itself — twelve hex characters
/// is an id and can be nothing else, because an alias that looked like one
/// is refused where it is set.
///
/// Takes the two fields rather than a record, because a kept home and a
/// running box carry them on different types and neither may answer
/// differently from the other.
pub fn answers_to(id: &str, alias: Option<&str>, wanted: &str) -> bool {
    if crate::paths::is_box_id(wanted) {
        id == wanted
    } else {
        alias == Some(wanted)
    }
}

/// The one sentence every command says when nothing answers to what was
/// typed, whichever form it was and whichever store was searched.
pub fn no_box(wanted: &str) -> String {
    no_box_in(wanted, "in this workspace")
}

/// The one body behind every "nothing answers to that" sentence, so the
/// scope is the only thing that ever differs and the rest cannot drift.
fn no_box_in(wanted: &str, scope: &str) -> String {
    format!("no box {wanted} {scope}; `wormhole ps --all` lists the kept ones")
}

/// The box in this workspace that `wanted` names.
///
/// Scoped to the workspace, because an alias belongs to the one it was set
/// in; the id is what reaches across.
pub fn by_name<'a>(records: &'a [Record], workspace: &Path, wanted: &str) -> Option<&'a Record> {
    records.iter().find(|record| {
        record.workspace == workspace && answers_to(&record.id, record.alias.as_deref(), wanted)
    })
}

/// The box `wanted` names for a command that may reach any box on this
/// host.
///
/// An id is derived from a workspace and an ordinal, so it names exactly
/// one box everywhere and needs no workspace to be found by. A name
/// belongs to the workspace it was set in, exactly as it does elsewhere.
///
/// [`by_name`] is the same question asked strictly inside one workspace,
/// which is what *starting* a box needs: `--id` may only name a box of
/// the tree you are standing in. `rm`, `reset` and `rename` are reached
/// from the panel too, and the panel lists every box on the host — one
/// it can show has to be one they can name.
pub fn find<'a>(records: &'a [Record], workspace: &Path, wanted: &str) -> Option<&'a Record> {
    match by_id(records, wanted) {
        Some(found) => Some(found),
        // Not an id at all, or an id nothing kept. Either way the name
        // rule is what is left to try.
        None if crate::paths::is_box_id(wanted) => None,
        None => by_name(records, workspace, wanted),
    }
}

/// The box with this id, from anywhere. `None` when the string is not an
/// id at all, so a caller that has only an id — the panel, whose rows all
/// carry one — needs no workspace to pass in.
pub fn by_id<'a>(records: &'a [Record], wanted: &str) -> Option<&'a Record> {
    if !crate::paths::is_box_id(wanted) {
        return None;
    }
    records
        .iter()
        .find(|record| answers_to(&record.id, record.alias.as_deref(), wanted))
}

/// A box a lifecycle command is about to act on.
///
/// The key is what every store path is built from — home, lock, snapshot
/// — and it is the home directory's own name, so it is known even when
/// nothing inside that directory can be read. The record is what the box
/// says about itself, and a box that can say nothing is still a box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub key: String,
    /// `None` for a home whose record is missing or corrupt. `--id`
    /// already starts one of those; refusing to *remove* one would leave
    /// a box you can see, cannot read, and cannot get rid of.
    pub record: Option<Record>,
}

/// The box `wanted` names: by its record where there is one, and by its
/// home directory where there is not.
///
/// The second half is the rule `--id` already lives by — an id names a box
/// by its directory, and nothing in that directory has to be readable for
/// it to. Only an id can do it: a name lives *in* the record, so a box
/// with no record has none.
///
/// `keys` is every home directory the store holds. Pure, so the rule that
/// decides which box gets deleted is testable without a store.
pub fn target(
    records: &[Record],
    keys: &[String],
    workspace: &Path,
    wanted: &str,
) -> Option<Target> {
    if let Some(record) = find(records, workspace, wanted) {
        return Some(Target {
            id: record.id.clone(),
            key: crate::paths::box_key(&record.workspace, &record.id),
            record: Some(record.clone()),
        });
    }
    if !crate::paths::is_box_id(wanted) {
        return None;
    }
    keys.iter()
        .find(|key| crate::paths::key_id(key) == Some(wanted))
        .map(|key| Target {
            id: wanted.to_owned(),
            key: key.clone(),
            record: None,
        })
}

/// The box in `workspace` already answering to `alias`, when it is not
/// `self_id`.
///
/// One rule and one sentence for "that name is taken", because a start,
/// a rename and the panel all have to give the same answer — and the
/// answer names the box holding it, so the way out is on the screen.
pub fn alias_conflict<'a>(
    records: &'a [Record],
    workspace: &Path,
    alias: &str,
    self_id: &str,
) -> Option<&'a Record> {
    by_name(records, workspace, alias).filter(|taken| taken.id != self_id)
}

/// What to say when it is. Beside the rule, so the two cannot drift.
pub fn alias_taken(alias: &str, by: &str) -> String {
    format!(
        "{alias} already names box {by} in that workspace; \
         `wormhole rename {by} <other>` frees the name"
    )
}

/// Why [`find`] came back empty, said in the scope it actually searched.
///
/// [`no_box`] is the same sentence for the workspace-scoped lookup a
/// start does. Both exist because they answer different questions, and
/// one sentence claiming a scope it did not search is how a person ends
/// up looking for a box in the wrong place.
pub fn no_box_found(wanted: &str) -> String {
    if crate::paths::is_box_id(wanted) {
        no_box_in(wanted, "on this host")
    } else {
        no_box(wanted)
    }
}

/// The role a start is asking for.
///
/// Two fields because two kinds of home have to be matched at once: one
/// written by a wormhole that knows a role's identity, and one written
/// before that, which has only the reference somebody typed at the time.
/// Both mean the same box; only their records differ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wanted<'a> {
    /// Where the role comes from — see [`Record::source`].
    pub source: Option<&'a str>,
    /// The reference as the user wrote it this time.
    pub typed: Option<&'a str>,
}

impl Record {
    /// Whether this box was started from the role now being asked for.
    ///
    /// A record that carries an identity is judged on it alone: the
    /// reference is one of several spellings, and comparing spellings is
    /// what made two names for one role into two boxes. A record from
    /// before identities existed has nothing else to be judged on.
    fn is_role(&self, wanted: Wanted<'_>) -> bool {
        match self.source {
            Some(_) => self.source.as_deref() == wanted.source,
            None => self.role.as_deref() == wanted.typed,
        }
    }
}

/// Which box a bare `wormhole box` should be, in order of preference:
/// this workspace's boxes for this role, most recently used first.
///
/// The caller starts the first one it can claim, so this never has to ask
/// whether a box is running — taking its lock is that question, asked
/// without a race. A workspace with nothing free gets a new box, which is
/// what "no limit of one per folder" means.
///
/// The role is part of the match because a box's home carries that role's
/// toolchain and persona. Resuming an `alphaca` box for an `alphaca-java`
/// run would hand the agent the wrong home and call it continuity.
pub fn resumable<'a>(
    records: &'a [Record],
    workspace: &Path,
    wanted: Wanted<'_>,
) -> Vec<&'a Record> {
    let mut found: Vec<&Record> = records
        .iter()
        .filter(|record| record.workspace == workspace && record.is_role(wanted))
        .collect();
    found.sort_by(|a, b| {
        b.started_unix
            .cmp(&a.started_unix)
            .then_with(|| a.id.cmp(&b.id))
    });
    found
}

/// The smallest ordinal this workspace has no box for. Ordinals are dense
/// and reused once a box's home is gone, so a workspace cycled through
/// many boxes does not climb forever.
pub fn free_ordinal(taken: &[String], workspace: &Path) -> u32 {
    (0..)
        .find(|n| !taken.contains(&crate::paths::box_id(workspace, *n)))
        .unwrap_or(u32::MAX)
}

/// One box as a surface shows it: the box, and the pid running it when one
/// is. `wormhole ps --all` and the panel both list these, so both are
/// looking at the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub record: Record,
    pub running: Option<u32>,
}

/// What one scan of the store found: the boxes, and the things it could
/// not read. A scan returns its problems rather than printing them — the
/// panel owns the terminal it draws on, and a reader that writes there
/// draws over the screen, once a second, for as long as the panel is up.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scan {
    pub boxes: Vec<Listing>,
    pub problems: Vec<String>,
}

/// Every box this host holds, running or not. An idle box is listed with
/// what it would resume as, because the whole point of keeping it is that
/// it can be resumed.
pub fn list(boxes: &[Listing], now_unix: u64) -> String {
    if boxes.is_empty() {
        return "no boxes yet\n".to_owned();
    }
    let mut rows = vec![
        [
            "ID",
            "NAME",
            // What you can type instead of the id. Its own column, not
            // folded into NAME: the manifest's name is a label several
            // boxes share, and only this one names exactly one box.
            "ALIAS",
            "STATE",
            "ROLE",
            "AGENT",
            "LAST USED",
            "WORKSPACE",
        ]
        .map(str::to_owned)
        .to_vec(),
    ];
    for entry in boxes {
        let record = &entry.record;
        rows.push(vec![
            record.id.clone(),
            table::or_dash(record.name.as_deref()),
            table::or_dash(record.alias.as_deref()),
            match entry.running {
                Some(pid) => format!("running {pid}"),
                None => "idle".to_owned(),
            },
            table::or_dash(record.role.as_deref()),
            table::or_dash(record.agent.as_deref()),
            match record.started_unix {
                0 => "never".to_owned(),
                started => table::age(now_unix.saturating_sub(started)),
            },
            record.workspace.display().to_string(),
        ]);
    }
    table::render(&rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, workspace: &str, role: Option<&str>, started: u64) -> Record {
        Record {
            id: id.to_owned(),
            workspace: PathBuf::from(workspace),
            role: role.map(str::to_owned),
            source: None,
            alias: None,
            name: None,
            agent: Some("claude".to_owned()),
            created_unix: 1,
            started_unix: started,
        }
    }

    fn called(mut record: Record, alias: &str) -> Record {
        record.alias = Some(alias.to_owned());
        record
    }

    /// The id is derived and unmemorable; the alias is what a person
    /// types. Both name one box, and every command takes either.
    #[test]
    fn a_box_answers_to_its_id_and_to_the_name_you_gave_it() {
        let records = [called(record("0123456789ab", "/w", None, 1), "api")];
        for wanted in ["0123456789ab", "api"] {
            assert_eq!(
                by_name(&records, Path::new("/w"), wanted).map(|r| r.id.as_str()),
                Some("0123456789ab"),
                "{wanted}"
            );
        }
        assert_eq!(by_name(&records, Path::new("/w"), "web"), None);
    }

    /// An alias belongs to the workspace it was set in — the id is what
    /// reaches across, exactly as it does everywhere else.
    #[test]
    fn an_alias_from_another_workspace_is_not_found() {
        let records = [called(record("0123456789ab", "/other", None, 1), "api")];
        assert_eq!(by_name(&records, Path::new("/w"), "api"), None);
    }

    /// The gap this closes: a home whose record cannot be read is listed
    /// as a problem and never as a box, so `--id` could start one and
    /// nothing could ever remove it. Its directory name carries the id.
    #[test]
    fn a_box_whose_record_cannot_be_read_is_still_named_by_its_directory() {
        let keys = ["proj-0123456789ab".to_owned()];
        let found = target(&[], &keys, Path::new("/w"), "0123456789ab").expect("a target");
        assert_eq!(found.id, "0123456789ab");
        assert_eq!(found.key, "proj-0123456789ab");
        assert_eq!(found.record, None);
    }

    /// A name lives in the record, so a box with no record has none —
    /// and a directory scan must never answer a name with a guess.
    #[test]
    fn only_an_id_reaches_a_box_that_has_no_record() {
        let keys = ["proj-0123456789ab".to_owned()];
        assert_eq!(target(&[], &keys, Path::new("/w"), "api"), None);
    }

    #[test]
    fn a_box_with_a_record_is_targeted_by_the_record_it_has() {
        let records = [called(record("0123456789ab", "/w", None, 1), "api")];
        let found = target(&records, &[], Path::new("/w"), "api").expect("a target");
        assert_eq!(found.key, "w-0123456789ab");
        assert_eq!(
            found.record.expect("the record").alias.as_deref(),
            Some("api")
        );
    }

    /// One rule for "that name is taken", so a start, a rename and the
    /// panel cannot answer it three ways.
    #[test]
    fn a_name_another_box_here_holds_is_a_conflict_and_the_box_itself_is_not() {
        let records = [
            called(record("0123456789ab", "/w", None, 1), "api"),
            record("0123456789ac", "/w", None, 1),
        ];
        assert_eq!(
            alias_conflict(&records, Path::new("/w"), "api", "0123456789ac").map(|r| r.id.as_str()),
            Some("0123456789ab")
        );
        // Its own name is not a conflict with itself.
        assert_eq!(
            alias_conflict(&records, Path::new("/w"), "api", "0123456789ab"),
            None
        );
        // And a name is workspace-scoped, here as everywhere.
        assert_eq!(
            alias_conflict(&records, Path::new("/other"), "api", "0123456789ac"),
            None
        );
    }

    /// The refusal names the box holding the name, so the way out is on
    /// the screen rather than in the reader's head.
    #[test]
    fn a_taken_name_says_which_box_holds_it_and_how_to_free_it() {
        let said = alias_taken("api", "0123456789ab");
        assert!(said.contains("api"), "{said}");
        assert!(said.contains("wormhole rename 0123456789ab"), "{said}");
    }

    /// The panel lists every box on the host, and `x` on one of them has
    /// to reach it. An id is derived from a workspace and an ordinal, so
    /// it already names exactly one box everywhere.
    #[test]
    fn an_id_names_its_box_from_any_workspace_but_a_name_does_not() {
        let records = [called(record("0123456789ab", "/other", None, 1), "api")];
        assert_eq!(
            find(&records, Path::new("/w"), "0123456789ab").map(|r| r.id.as_str()),
            Some("0123456789ab")
        );
        assert_eq!(find(&records, Path::new("/w"), "api"), None);
        assert_eq!(
            find(&records, Path::new("/other"), "api").map(|r| r.id.as_str()),
            Some("0123456789ab")
        );
    }

    /// One rule, whichever store is searched: a kept home and a running
    /// box carry the two fields on different types, and `stop api` must
    /// not mean something other than `box --id api`.
    #[test]
    fn the_two_forms_are_told_apart_by_the_string_alone() {
        assert!(answers_to("0123456789ab", None, "0123456789ab"));
        assert!(answers_to("0123456789ab", Some("api"), "api"));
        // A word is never read as an id, so a box with no alias answers
        // to nothing but its id.
        assert!(!answers_to("0123456789ab", None, "api"));
        // And twelve hex characters is never read as an alias.
        assert!(!answers_to(
            "aabbccddeeff",
            Some("0123456789ab"),
            "0123456789ab"
        ));
    }

    /// An alias that could be read as an id would make `attach <that>`
    /// name two things, so it is refused where it is set.
    #[test]
    fn an_alias_may_not_be_spellable_as_an_id() {
        assert!(is_usable_alias("api"));
        assert!(is_usable_alias("feature-a"));
        assert!(!is_usable_alias("0123456789ab"));
        assert!(!is_usable_alias("a/b"));
        assert!(!is_usable_alias(""));
    }

    /// A start that names a role only by what was typed — which is all a
    /// home from before identities can be matched on.
    fn typed(reference: &str) -> Wanted<'_> {
        Wanted {
            source: None,
            typed: Some(reference),
        }
    }

    /// The same record, written by a wormhole that knows where its role
    /// came from.
    fn from(mut record: Record, source: &str) -> Record {
        record.source = Some(source.to_owned());
        record
    }

    /// The bug this pins: `--role alphaca` and `--role ./roles/alphaca`
    /// name one directory, and used to give two boxes with two homes —
    /// the second unable to see the first's history, logins or toolchain.
    #[test]
    fn two_spellings_of_one_role_resume_one_box() {
        let records = [from(
            record("a", "/w", Some("./roles/alphaca"), 100),
            "/w/roles/alphaca",
        )];
        for spelling in ["alphaca", "./roles/alphaca", "/w/roles/alphaca"] {
            let found = resumable(
                &records,
                Path::new("/w"),
                Wanted {
                    source: Some("/w/roles/alphaca"),
                    typed: Some(spelling),
                },
            );
            assert_eq!(found.len(), 1, "{spelling}");
        }
    }

    /// A role is still what a box is, so two different ones stay apart —
    /// judged on where they come from rather than on what they are called.
    #[test]
    fn two_roles_from_different_places_are_still_two_boxes() {
        let records = [from(record("a", "/w", Some("alphaca"), 100), "/w/roles/a")];
        let found = resumable(
            &records,
            Path::new("/w"),
            Wanted {
                source: Some("/w/roles/b"),
                // The same name it was installed under, pointing elsewhere.
                typed: Some("alphaca"),
            },
        );
        assert!(found.is_empty(), "{found:?}");
    }

    /// A home written before roles had an identity keeps resuming on the
    /// reference it recorded — the whole point of keeping both fields.
    /// The first start under this wormhole rewrites it with a source.
    #[test]
    fn a_home_from_before_identities_resumes_on_what_was_typed() {
        let records = [record("a", "/w", Some("alphaca"), 100)];
        let found = resumable(
            &records,
            Path::new("/w"),
            Wanted {
                source: Some("/w/roles/alphaca"),
                typed: Some("alphaca"),
            },
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    /// A re-pinned remote role keeps its box: the identity is the
    /// repository, and the commit is which version of it.
    #[test]
    fn a_re_pinned_role_resumes_the_box_it_was_working_in() {
        let sha = "1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e";
        let records = [from(
            record("a", "/w", Some(&format!("github:you/r@{sha}")), 100),
            "https://github.com/you/r",
        )];
        let found = resumable(
            &records,
            Path::new("/w"),
            Wanted {
                source: Some("https://github.com/you/r"),
                typed: Some("github:you/r@0000000000000000000000000000000000000000"),
            },
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_record_survives_the_round_trip() {
        let written = record("0123456789ab", "/w/proj", Some("alphaca"), 100);
        assert_eq!(parse(&to_toml(&written).expect("toml")), Ok(written));
    }

    #[test]
    fn an_unreadable_record_is_refused_rather_than_guessed_at() {
        assert!(parse("id = 4\n").is_err());
        assert!(parse("").is_err());
    }

    /// A key wormhole has stopped writing must not orphan the box that
    /// still carries it. Refusing one would make every removed field an
    /// unlistable, unresumable home.
    #[test]
    fn a_record_carrying_a_key_this_version_dropped_still_parses() {
        let written = record("0123456789ab", "/w/proj", Some("alphaca"), 100);
        let mut text = to_toml(&written).expect("toml");
        text.push_str("member = \"ship\"\n");
        assert_eq!(parse(&text), Ok(written));
    }

    /// A home written before boxes had ids is that workspace's first box.
    /// Losing it would throw away the agent's whole history and whatever
    /// the preflight hook installed, for a change that renames nothing on
    /// disk.
    #[test]
    fn a_home_from_before_box_ids_is_adopted_as_its_first_box() {
        let workspace = Path::new("/home/me/proj");
        let id = crate::paths::box_id(workspace, 0);
        let home = PathBuf::from(format!("/data/wormhole/homes/proj-{id}"));
        let found = from_legacy(&home, "/home/me/proj\n").expect("adopted");
        assert_eq!(found.id, id);
        assert_eq!(found.workspace, workspace);
        assert_eq!(found.started_unix, 0);
    }

    #[test]
    fn a_directory_that_is_not_a_home_is_not_adopted() {
        assert_eq!(from_legacy(Path::new("/data/homes/junk"), "/w"), None);
        assert_eq!(
            from_legacy(Path::new("/data/homes/proj-0123456789ab"), "  \n"),
            None
        );
    }

    #[test]
    fn the_most_recently_used_box_is_resumed_first() {
        let records = [
            record("a", "/w", Some("alphaca"), 100),
            record("b", "/w", Some("alphaca"), 300),
            record("c", "/w", Some("alphaca"), 200),
        ];
        let order: Vec<&str> = resumable(&records, Path::new("/w"), typed("alphaca"))
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(order, ["b", "c", "a"]);
    }

    /// Two boxes that have never run sort by id rather than by whichever
    /// order the filesystem handed them back.
    #[test]
    fn boxes_that_never_ran_still_have_one_order() {
        let records = [record("b", "/w", None, 0), record("a", "/w", None, 0)];
        let order: Vec<&str> = resumable(&records, Path::new("/w"), Wanted::default())
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(order, ["a", "b"]);
    }

    #[test]
    fn a_box_from_another_workspace_is_never_resumed() {
        let records = [record("a", "/other", Some("alphaca"), 100)];
        assert!(resumable(&records, Path::new("/w"), typed("alphaca")).is_empty());
    }

    /// A box's home carries its role's toolchain and persona. Handing an
    /// `alphaca-java` run the `alphaca` home would be the wrong home
    /// called continuity.
    #[test]
    fn a_box_from_another_role_is_never_resumed() {
        let records = [
            record("a", "/w", Some("alphaca"), 100),
            record("b", "/w", None, 100),
        ];
        let found = resumable(&records, Path::new("/w"), typed("alphaca-java"));
        assert!(found.is_empty(), "{found:?}");
        let workspace_manifest = resumable(&records, Path::new("/w"), Wanted::default());
        assert_eq!(workspace_manifest.len(), 1);
        assert_eq!(workspace_manifest[0].id, "b");
    }

    #[test]
    fn the_first_free_ordinal_is_the_smallest_one() {
        let workspace = Path::new("/w/proj");
        assert_eq!(free_ordinal(&[], workspace), 0);
        let taken: Vec<String> = (0..3).map(|n| crate::paths::box_id(workspace, n)).collect();
        assert_eq!(free_ordinal(&taken, workspace), 3);
    }

    /// A workspace cycled through many boxes must not climb forever: a
    /// gap left by a removed home is the next box's.
    #[test]
    fn an_ordinal_freed_by_a_removed_home_is_used_again() {
        let workspace = Path::new("/w/proj");
        let taken = vec![
            crate::paths::box_id(workspace, 0),
            crate::paths::box_id(workspace, 2),
        ];
        assert_eq!(free_ordinal(&taken, workspace), 1);
    }

    #[test]
    fn nothing_kept_says_so_plainly() {
        assert_eq!(list(&[], 0), "no boxes yet\n");
    }

    #[test]
    fn the_listing_separates_what_is_running_from_what_can_be_resumed() {
        let running = record("0123456789ab", "/w/proj", Some("alphaca"), 100);
        let idle = record("ba9876543210", "/w/proj", Some("alphaca"), 40);
        let text = list(
            &[
                Listing {
                    record: running,
                    running: Some(4242),
                },
                Listing {
                    record: idle,
                    running: None,
                },
            ],
            160,
        );
        assert!(text.contains("running 4242"), "{text}");
        assert!(text.contains("idle"), "{text}");
        assert!(text.contains("0123456789ab"), "{text}");
        assert!(text.contains("2m"), "{text}");
        assert!(text.contains("/w/proj"), "{text}");
    }

    /// A box that exists but has never been started reads as never used,
    /// not as one that started at the epoch.
    #[test]
    fn a_box_that_never_ran_says_never() {
        let never = record("0123456789ab", "/w", None, 0);
        let text = list(
            &[Listing {
                record: never,
                running: None,
            }],
            9_000_000,
        );
        assert!(text.contains("never"), "{text}");
    }
}
