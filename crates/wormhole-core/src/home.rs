//! What wormhole records about a kept box, and which box a `wormhole box`
//! should be.
//!
//! A box outlives every process that ever ran it: its home holds the
//! agent's history, its logins and whatever the preflight hook installed.
//! Its record does not live there. The home is the agent's to write, and a
//! record the agent could rewrite would let it pick which workspace the
//! next start mounts and which role resumes it. So the record sits beside
//! the lock, host-side.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::manifest::Resume;
use crate::table;

/// Where an older wormhole kept a box's record, relative to its home.
/// Read until that box's next start moves it host-side.
pub const LEGACY_RECORD: &str = ".wormhole/box.toml";

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
    /// What the box's store entries are named by: its home, its lock and
    /// this record's own file. Taken from where the record was found and
    /// never written into it, so the two cannot disagree.
    #[serde(skip)]
    pub key: String,
    pub id: String,
    /// Where this box last ran. Nothing else says which trees a home has
    /// seen, or whether any of them still exists.
    pub workspace: PathBuf,
    /// Every other workspace it has run in, most recent first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub earlier: Vec<PathBuf>,
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

/// A record's text, read as the box `key` names.
pub fn parse(text: &str, key: &str) -> Result<Record, String> {
    toml::from_str(text)
        .map(|record: Record| Record {
            key: key.to_owned(),
            ..record.healed()
        })
        .map_err(|e| format!("box record is not valid: {}", e.message()))
}

impl Record {
    /// This record once the box has run in `here`: there first, and
    /// every other workspace kept once, most recent first.
    #[must_use]
    pub fn ran_in(mut self, here: &Path) -> Record {
        if self.workspace != here {
            let previous = std::mem::replace(&mut self.workspace, here.to_owned());
            self.earlier.retain(|workspace| workspace != here);
            self.earlier.insert(0, previous);
        }
        self
    }

    /// Whether this box has ever run in `here`.
    #[must_use]
    pub fn serves(&self, here: &Path) -> bool {
        self.workspace == here || self.earlier.iter().any(|workspace| workspace == here)
    }

    /// A wormhole once wrote a start's name into `source` and its role's
    /// identity into `alias`. An identity is tagged (`dir:`/`repo:`) and a
    /// usable alias holds no `:`, so the two are mutually exclusive: route
    /// each written value to the one field it can belong to.
    fn healed(mut self) -> Self {
        for value in [self.alias.take(), self.source.take()]
            .into_iter()
            .flatten()
        {
            if crate::source::is_source(&value) {
                self.source = Some(value);
            } else if is_usable_alias(&value) {
                self.alias = Some(value);
            }
        }
        self
    }
}

/// The record a pre-id home implies: the workspace it stamped, and the id
/// its directory name already ends in. Both zero times, so a listing sorts
/// it last and the first start writes the real thing.
pub fn from_legacy(home: &Path, stamped: &str) -> Option<Record> {
    let key = home.file_name()?.to_str()?;
    let id = crate::paths::key_id(key)?.to_owned();
    let workspace = stamped.trim();
    if workspace.is_empty() {
        return None;
    }
    Some(Record {
        key: key.to_owned(),
        id,
        workspace: PathBuf::from(workspace),
        earlier: Vec::new(),
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
/// typed, whichever form it was.
pub fn no_box_found(wanted: &str) -> String {
    format!("no box {wanted} on this host; `wormhole ps --all` lists the kept ones")
}

/// A box a lifecycle command is about to act on.
///
/// The key is what every store path is built from — home and lock
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

/// The box `wanted` names, from any workspace: by its record where there
/// is one, and by its home directory where there is not.
///
/// An id names exactly one box everywhere, and names a box by its
/// directory even when nothing in it can be read. A name lives *in* the
/// record, so only boxes with one answer to it. It is looked for first
/// among the boxes that ran `here`, so two boxes an older wormhole let
/// share a name in two workspaces each keep answering to it in their
/// own; two elsewhere is a refusal that names both.
///
/// `keys` is every home directory the store holds. Pure, so the rule that
/// decides which box gets deleted is testable without a store.
pub fn target(
    records: &[Record],
    keys: &[String],
    here: &Path,
    wanted: &str,
) -> Result<Target, String> {
    let of = |record: &Record| Target {
        id: record.id.clone(),
        key: record.key.clone(),
        record: Some(record.clone()),
    };
    if crate::paths::is_box_id(wanted) {
        if let Some(record) = records.iter().find(|record| record.id == wanted) {
            return Ok(of(record));
        }
        return keys
            .iter()
            .find(|key| crate::paths::key_id(key) == Some(wanted))
            .map(|key| Target {
                id: wanted.to_owned(),
                key: key.clone(),
                record: None,
            })
            .ok_or_else(|| no_box_found(wanted));
    }
    let answering: Vec<&Record> = records
        .iter()
        .filter(|record| answers_to(&record.id, record.alias.as_deref(), wanted))
        .collect();
    let ran_here: Vec<&Record> = answering
        .iter()
        .copied()
        .filter(|record| record.serves(here))
        .collect();
    match (ran_here.as_slice(), answering.as_slice()) {
        ([one], _) | ([], [one]) => Ok(of(one)),
        ([], []) => Err(no_box_found(wanted)),
        ([], many) | (many, _) => Err(format!(
            "{wanted} names boxes {}; use the id",
            many.iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    }
}

/// The box already answering to `alias`, when it is not `self_id`.
///
/// One name names one box on this host, because a box may run in any
/// workspace and its name has to reach it from each. One rule and one
/// sentence, because a start, a rename and the panel all have to give the
/// same answer, and the answer names the box holding it.
pub fn alias_conflict<'a>(records: &'a [Record], alias: &str, self_id: &str) -> Option<&'a Record> {
    records
        .iter()
        .find(|record| record.alias.as_deref() == Some(alias) && record.id != self_id)
}

/// What to say when it is. Beside the rule, so the two cannot drift.
pub fn alias_taken(alias: &str, by: &str) -> String {
    format!("{alias} already names box {by}; `wormhole rename {by} <other>` frees the name")
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
    /// The product this start will run. Two products of one role are two
    /// boxes: each home holds that product's login and history.
    pub run: Option<&'a str>,
}

impl Record {
    /// Whether this box was started from the role now being asked for.
    ///
    /// A record that carries an identity is judged on it alone: the
    /// reference is one of several spellings, and comparing spellings is
    /// what made two names for one role into two boxes. A record from
    /// before identities existed has nothing else to be judged on.
    ///
    /// The product is a third axis: one role, two products, two homes.
    /// A home written before products were identity matches any product
    /// of that role, and the first start fills `agent` in.
    fn is_role(&self, wanted: Wanted<'_>) -> bool {
        let same_role = match self.source {
            Some(_) => self.source.as_deref() == wanted.source,
            None => self.role.as_deref() == wanted.typed,
        };
        same_role && self.is_product(wanted.run)
    }

    fn is_product(&self, wanted: Option<&str>) -> bool {
        match (self.agent.as_deref(), wanted) {
            (_, None) | (None, Some(_)) => true,
            (Some(have), Some(want)) => have == want,
        }
    }
}

/// Which box a bare `wormhole box` should be, in order of preference:
/// this role's boxes that ran in `here`, most recently used first; then,
/// when the role resumes `anywhere`, its boxes that ran elsewhere.
///
/// The caller starts the first one it can claim, so this never has to ask
/// whether a box is running — taking its lock is that question, asked
/// without a race. Nothing free gets a new box, which is what "no limit
/// of one per folder" means.
///
/// The role is part of the match because a box's home carries that role's
/// toolchain and persona. Resuming an `alphaca` box for an `alphaca-java`
/// run would hand the agent the wrong home and call it continuity. The
/// product is part of the match for the same reason: a grok login does
/// not belong in a claude home. Only a box whose role identity is known
/// is taken from elsewhere: a spelling is not proof of the same role.
pub fn resumable<'a>(
    records: &'a [Record],
    here: &Path,
    wanted: Wanted<'_>,
    resume: Resume,
) -> Vec<&'a Record> {
    let recent = |a: &&Record, b: &&Record| {
        b.started_unix
            .cmp(&a.started_unix)
            .then_with(|| a.id.cmp(&b.id))
    };
    let (mut found, mut elsewhere): (Vec<&Record>, Vec<&Record>) = records
        .iter()
        .filter(|record| record.is_role(wanted))
        .partition(|record| record.serves(here));
    found.sort_by(recent);
    if resume == Resume::Anywhere && wanted.source.is_some() {
        elsewhere.retain(|record| record.source.is_some());
        elsewhere.sort_by(recent);
        found.extend(elsewhere);
    }
    found
}

/// Why a start that named this box cannot run it in `here` from the
/// recipe it resolved, or `None` when it can.
///
/// A box made from a workspace's own manifest runs only there: its recipe
/// is that file. A role box runs wherever it is named, and only as the
/// role it was made from. A record from before roles had an identity is
/// the one exception: the start fills it in.
pub fn refused_by_name(record: &Record, here: &Path, wanted: Wanted<'_>) -> Option<String> {
    let id = &record.id;
    match (record.source.as_deref(), record.role.is_some()) {
        (None, false) if record.workspace != here => Some(format!(
            "box {id} is made from {}'s own wormhole.toml, so it runs only there",
            record.workspace.display()
        )),
        (None, false) => wanted.source.map(|asked| {
            format!("box {id} is made from this workspace's own wormhole.toml, not {asked}")
        }),
        (Some(have), _) => match wanted.source {
            Some(asked) if asked == have => None,
            Some(asked) => Some(format!("box {id} runs {have}, not {asked}")),
            None => Some(format!(
                "box {id} runs {have}, not this workspace's own wormhole.toml"
            )),
        },
        (None, true) => None,
    }
}

impl Record {
    /// The `--role` that starts this box again, wherever it starts: the
    /// directory of a local role, the ref typed for one from a repository.
    /// `None` for a box made from its workspace's own manifest.
    ///
    /// A relative path typed before roles had an identity meant the
    /// workspace it was typed in, not wherever the next start stands.
    #[must_use]
    pub fn role_ref(&self) -> Option<String> {
        if let Some(dir) = self.source.as_deref().and_then(crate::source::dir_of) {
            return Some(dir.to_owned());
        }
        let typed = self.role.as_deref()?;
        match crate::source::names(typed) {
            crate::source::Names::Dir(path) if Path::new(path).is_relative() => {
                Some(self.workspace.join(path).display().to_string())
            }
            _ => Some(typed.to_owned()),
        }
    }
}

/// Where the panel starts an idle box: `here` when the box has run here,
/// otherwise the latest workspace it ran in that still exists.
///
/// Never a directory it has not run in. Enter names a box, not a place to
/// mount, and the panel may be open anywhere, `~` included.
pub fn resume_in(
    record: &Record,
    here: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    if record.serves(here) && exists(here) {
        return Ok(here.to_owned());
    }
    std::iter::once(&record.workspace)
        .chain(&record.earlier)
        .find(|workspace| exists(workspace))
        .cloned()
        .ok_or_else(|| {
            format!(
                "no workspace box {id} ran in exists any more; \
                 `wormhole box --id {id}` in the one it should run in",
                id = record.id
            )
        })
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
/// is. `wormhole ps` and the panel both list these, so both are looking
/// at the same thing.
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

/// Every box this host holds, paired with the pid running it: the kept
/// records, most recently used first, plus any running box whose record
/// could not be read. The registry is what says a box is running, so a
/// box it names is listed whatever its home says — from the entry, which
/// carries everything a row needs.
pub fn scan(mut kept: Vec<Record>, live: &[crate::registry::Entry]) -> Vec<Listing> {
    let running: std::collections::BTreeMap<&str, u32> = live
        .iter()
        .map(|entry| (entry.box_id.as_str(), entry.pid))
        .collect();
    let recorded: std::collections::BTreeSet<&str> =
        kept.iter().map(|record| record.id.as_str()).collect();
    let unrecorded: Vec<Record> = live
        .iter()
        .filter(|entry| !recorded.contains(entry.box_id.as_str()))
        .map(Record::from_entry)
        .collect();
    kept.extend(unrecorded);
    kept.sort_by(|a, b| {
        b.started_unix
            .cmp(&a.started_unix)
            .then_with(|| a.id.cmp(&b.id))
    });
    kept.into_iter()
        .map(|record| Listing {
            running: running.get(record.id.as_str()).copied(),
            record,
        })
        .collect()
}

impl Record {
    /// What a running box's registry entry says about it, for a box whose
    /// home record cannot be read. It names no role: the entry does not
    /// carry one.
    fn from_entry(entry: &crate::registry::Entry) -> Record {
        Record {
            key: entry.key(),
            id: entry.box_id.clone(),
            workspace: entry.workspace.clone(),
            earlier: Vec::new(),
            role: None,
            source: None,
            alias: entry.alias.clone(),
            name: entry.name.clone(),
            agent: entry.agent.clone(),
            created_unix: entry.started_unix,
            started_unix: entry.started_unix,
        }
    }
}

/// The box among these that `wanted` names: its id from anywhere, or the
/// alias somebody gave it.
pub fn listed<'a>(boxes: &'a [Listing], wanted: &str) -> Option<&'a Listing> {
    boxes
        .iter()
        .find(|listing| answers_to(&listing.record.id, listing.record.alias.as_deref(), wanted))
}

/// What a listing with no rows says, so `ps --all` and the panel cannot
/// drift by a word.
pub const NO_BOXES_YET: &str = "no boxes yet";
pub const NO_BOXES_RUNNING: &str = "no boxes running";

/// The one table every listing draws: `ps`, `ps --all`, `ps <id>` and
/// the panel. An idle box is listed with what it would resume as, because
/// the whole point of keeping it is that it can be resumed. Nothing
/// renders as nothing; the caller says so in its own words.
pub fn list(boxes: &[Listing], now_unix: u64) -> String {
    if boxes.is_empty() {
        return String::new();
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
            match record.earlier.len() {
                0 => record.workspace.display().to_string(),
                more => format!("{} +{more}", record.workspace.display()),
            },
        ]);
    }
    table::render(&rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, workspace: &str, role: Option<&str>, started: u64) -> Record {
        Record {
            key: crate::paths::box_key(Path::new(workspace), id),
            id: id.to_owned(),
            workspace: PathBuf::from(workspace),
            earlier: Vec::new(),
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

    fn named(records: &[Record], here: &str, wanted: &str) -> Result<String, String> {
        target(records, &[], Path::new(here), wanted).map(|found| found.id)
    }

    /// The id is derived and unmemorable; the alias is what a person
    /// types. Both name one box, and every command takes either.
    #[test]
    fn a_box_answers_to_its_id_and_to_the_name_you_gave_it() {
        let records = [called(record("0123456789ab", "/w", None, 1), "api")];
        for wanted in ["0123456789ab", "api"] {
            assert_eq!(
                named(&records, "/w", wanted),
                Ok("0123456789ab".to_owned()),
                "{wanted}"
            );
        }
        assert!(named(&records, "/w", "web").is_err());
    }

    /// A box may run in several workspaces, so its name has to reach it
    /// from any of them, or from a new one.
    #[test]
    fn an_alias_names_its_box_from_any_workspace() {
        let records = [called(record("0123456789ab", "/other", None, 1), "api")];
        assert_eq!(named(&records, "/w", "api"), Ok("0123456789ab".to_owned()));
    }

    /// Two boxes an older wormhole let share a name in two workspaces keep
    /// answering to it in their own.
    #[test]
    fn an_alias_on_a_box_that_ran_here_wins_over_one_elsewhere() {
        let records = [
            called(record("aaaaaaaaaaaa", "/other", None, 1), "api"),
            called(record("bbbbbbbbbbbb", "/w", None, 1), "api"),
        ];
        assert_eq!(named(&records, "/w", "api"), Ok("bbbbbbbbbbbb".to_owned()));
    }

    #[test]
    fn an_alias_two_boxes_elsewhere_answer_to_is_refused_naming_both() {
        let records = [
            called(record("aaaaaaaaaaaa", "/a", None, 1), "api"),
            called(record("bbbbbbbbbbbb", "/b", None, 1), "api"),
        ];
        let refused = named(&records, "/w", "api").unwrap_err();
        assert!(refused.contains("aaaaaaaaaaaa"), "{refused}");
        assert!(refused.contains("bbbbbbbbbbbb"), "{refused}");
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
        let refused = target(&[], &keys, Path::new("/w"), "api").unwrap_err();
        assert!(refused.contains("no box api"), "{refused}");
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
    /// panel cannot answer it three ways. A box may run anywhere, so its
    /// name is taken everywhere.
    #[test]
    fn a_name_another_box_holds_anywhere_is_a_conflict_and_the_box_itself_is_not() {
        let records = [
            called(record("0123456789ab", "/other", None, 1), "api"),
            record("0123456789ac", "/w", None, 1),
        ];
        assert_eq!(
            alias_conflict(&records, "api", "0123456789ac").map(|r| r.id.as_str()),
            Some("0123456789ab")
        );
        // Its own name is not a conflict with itself.
        assert_eq!(alias_conflict(&records, "api", "0123456789ab"), None);
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
    fn an_id_names_its_box_from_any_workspace() {
        let records = [called(record("0123456789ab", "/other", None, 1), "api")];
        assert_eq!(
            named(&records, "/w", "0123456789ab"),
            Ok("0123456789ab".to_owned())
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
            run: None,
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
                    run: None,
                },
                Resume::Here,
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
                run: None,
            },
            Resume::Here,
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
                run: None,
            },
            Resume::Here,
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
                run: None,
            },
            Resume::Here,
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    /// One role, two products, two homes: a grok login is not continuity
    /// with a claude history.
    #[test]
    fn two_products_of_one_role_are_two_boxes() {
        let claude = from(record("a", "/w", Some("alphaca"), 100), "/w/roles/alphaca");
        let mut grok = claude.clone();
        grok.id = "b".to_owned();
        grok.agent = Some("grok".to_owned());
        let records = [claude, grok];
        let found = resumable(
            &records,
            Path::new("/w"),
            Wanted {
                source: Some("/w/roles/alphaca"),
                typed: Some("alphaca"),
                run: Some("grok"),
            },
            Resume::Here,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].id, "b");
        assert_eq!(found[0].agent.as_deref(), Some("grok"));
    }

    /// A bare start that has not named a product yet still has a box to
    /// go back to: the most recently used one of this role, whichever
    /// product it runs. `--run` is what narrows that to one CLI.
    #[test]
    fn an_unspecified_product_resumes_either_and_the_newer_one_first() {
        let claude = from(record("a", "/w", Some("alphaca"), 100), "/w/roles/alphaca");
        let mut grok = claude.clone();
        grok.id = "b".to_owned();
        grok.agent = Some("grok".to_owned());
        grok.started_unix = 200;
        let records = [claude, grok];
        let found = resumable(
            &records,
            Path::new("/w"),
            Wanted {
                source: Some("/w/roles/alphaca"),
                typed: Some("alphaca"),
                run: None,
            },
            Resume::Here,
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].agent.as_deref(), Some("grok"));
        assert_eq!(found[1].agent.as_deref(), Some("claude"));
    }

    /// A home written before the product was identity still resumes for
    /// any product of that role; the first start fills `agent` in.
    #[test]
    fn a_home_from_before_products_resumes_for_the_product_now_asked() {
        let mut old = from(record("a", "/w", Some("alphaca"), 100), "/w/roles/alphaca");
        old.agent = None;
        let records = [old];
        let found = resumable(
            &records,
            Path::new("/w"),
            Wanted {
                source: Some("/w/roles/alphaca"),
                typed: Some("alphaca"),
                run: Some("grok"),
            },
            Resume::Here,
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_record_survives_the_round_trip() {
        let mut written = record("0123456789ab", "/w/proj", Some("alphaca"), 100);
        written.earlier = vec![PathBuf::from("/w/other")];
        assert_eq!(
            parse(&to_toml(&written).expect("toml"), &written.key),
            Ok(written)
        );
    }

    /// The key names the file, so it is never written into it: a record
    /// that said one key inside a file named by another would be two
    /// answers to which box this is.
    #[test]
    fn the_key_comes_from_where_the_record_was_found_not_from_its_text() {
        let written = record("0123456789ab", "/w/proj", None, 100);
        let text = to_toml(&written).expect("toml");
        assert!(!text.contains("key"), "{text}");
        assert_eq!(
            parse(&text, "elsewhere-0123456789ab").expect("parses").key,
            "elsewhere-0123456789ab"
        );
    }

    /// A box that runs somewhere new keeps every place it ran before, and
    /// the new one is where it last ran.
    #[test]
    fn running_in_a_workspace_puts_it_first_and_keeps_the_rest_once() {
        let mut kept = record("0123456789ab", "/a", Some("alphaca"), 100);
        kept.earlier = vec![PathBuf::from("/b")];
        let moved = kept.ran_in(Path::new("/b"));
        assert_eq!(moved.workspace, PathBuf::from("/b"));
        assert_eq!(moved.earlier, [PathBuf::from("/a")]);
        let again = moved.clone().ran_in(Path::new("/b"));
        assert_eq!(again, moved);
        assert!(again.serves(Path::new("/a")));
        assert!(!again.serves(Path::new("/c")));
    }

    /// A wormhole once filed a start's name under `source` and the role's
    /// identity under `alias`. The two cannot be mistaken for each other —
    /// an identity is tagged and a name may hold no `:` — so such a record
    /// reads back the right way round instead of orphaning its box.
    #[test]
    fn a_record_written_with_name_and_identity_swapped_reads_back_the_right_way_round() {
        let mut swapped = record("0123456789ab", "/w", Some("./roles/a"), 100);
        swapped.alias = Some("dir:/w/roles/a".to_owned());
        swapped.source = Some("api".to_owned());
        let healed = parse(&to_toml(&swapped).expect("toml"), "w-0123456789ab").expect("parses");
        assert_eq!(healed.alias.as_deref(), Some("api"));
        assert_eq!(healed.source.as_deref(), Some("dir:/w/roles/a"));

        let mut unnamed = record("0123456789ab", "/w", Some("r"), 100);
        unnamed.alias = Some("repo:https://example.test/r".to_owned());
        let healed = parse(&to_toml(&unnamed).expect("toml"), "w-0123456789ab").expect("parses");
        assert_eq!(healed.alias, None);
        assert_eq!(
            healed.source.as_deref(),
            Some("repo:https://example.test/r")
        );

        let mut named_only = record("0123456789ab", "/w", None, 100);
        named_only.source = Some("api".to_owned());
        let healed = parse(&to_toml(&named_only).expect("toml"), "w-0123456789ab").expect("parses");
        assert_eq!(healed.alias.as_deref(), Some("api"));
        assert_eq!(healed.source, None);

        let right = from(
            called(record("0123456789ab", "/w", None, 1), "api"),
            "dir:/r",
        );
        assert_eq!(
            parse(&to_toml(&right).expect("toml"), "w-0123456789ab"),
            Ok(right)
        );
    }

    #[test]
    fn an_unreadable_record_is_refused_rather_than_guessed_at() {
        assert!(parse("id = 4\n", "w-0123456789ab").is_err());
        assert!(parse("", "w-0123456789ab").is_err());
    }

    /// A key wormhole has stopped writing must not orphan the box that
    /// still carries it. Refusing one would make every removed field an
    /// unlistable, unresumable home.
    #[test]
    fn a_record_carrying_a_key_this_version_dropped_still_parses() {
        let written = record("0123456789ab", "/w/proj", Some("alphaca"), 100);
        let mut text = to_toml(&written).expect("toml");
        text.push_str("member = \"ship\"\n");
        assert_eq!(parse(&text, &written.key), Ok(written));
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
        let order: Vec<&str> = resumable(&records, Path::new("/w"), typed("alphaca"), Resume::Here)
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
        let order: Vec<&str> =
            resumable(&records, Path::new("/w"), Wanted::default(), Resume::Here)
                .iter()
                .map(|r| r.id.as_str())
                .collect();
        assert_eq!(order, ["a", "b"]);
    }

    #[test]
    fn a_box_from_another_workspace_is_never_resumed() {
        let records = [record("a", "/other", Some("alphaca"), 100)];
        assert!(resumable(&records, Path::new("/w"), typed("alphaca"), Resume::Here).is_empty());
    }

    const ALPHACA: Wanted<'static> = Wanted {
        source: Some("dir:/roles/alphaca"),
        typed: Some("alphaca"),
        run: None,
    };

    fn role_box(id: &str, workspace: &str, started: u64) -> Record {
        from(
            record(id, workspace, Some("alphaca"), started),
            "dir:/roles/alphaca",
        )
    }

    fn ids<'a>(records: &[&'a Record]) -> Vec<&'a str> {
        records.iter().map(|record| record.id.as_str()).collect()
    }

    /// The point of a role that says `anywhere`: one home, its login and
    /// its setup, for every project it is started in.
    #[test]
    fn a_role_that_resumes_anywhere_takes_its_box_from_another_workspace() {
        let records = [role_box("a", "/other", 100)];
        let here = Path::new("/w");
        assert_eq!(
            ids(&resumable(&records, here, ALPHACA, Resume::Anywhere)),
            ["a"]
        );
        assert!(resumable(&records, here, ALPHACA, Resume::Here).is_empty());
    }

    /// Its transcripts for this tree are in the box that ran here, so that
    /// one comes first even when another is newer.
    #[test]
    fn a_box_that_ran_here_is_resumed_before_a_newer_one_from_elsewhere() {
        let mut older = role_box("a", "/elsewhere", 100);
        older.earlier = vec![PathBuf::from("/w")];
        let records = [role_box("b", "/other", 300), older];
        assert_eq!(
            ids(&resumable(
                &records,
                Path::new("/w"),
                ALPHACA,
                Resume::Anywhere
            )),
            ["a", "b"]
        );
    }

    /// Its recipe is that workspace's file, which no other start reads.
    #[test]
    fn a_box_made_from_a_workspace_manifest_is_never_resumed_elsewhere() {
        let records = [record("a", "/other", None, 100)];
        let found = resumable(
            &records,
            Path::new("/w"),
            Wanted::default(),
            Resume::Anywhere,
        );
        assert!(found.is_empty(), "{found:?}");
    }

    /// Naming a box is the consent: a role box runs wherever it is named,
    /// as the role it is.
    #[test]
    fn a_role_box_named_from_another_workspace_runs_there_as_its_own_role_only() {
        let kept = role_box("0123456789ab", "/a", 1);
        assert_eq!(refused_by_name(&kept, Path::new("/w"), ALPHACA), None);
        let java = Wanted {
            source: Some("dir:/roles/java"),
            typed: Some("java"),
            run: None,
        };
        let refused = refused_by_name(&kept, Path::new("/w"), java).expect("refused");
        assert!(refused.contains("dir:/roles/alphaca"), "{refused}");
        assert!(refused.contains("dir:/roles/java"), "{refused}");
    }

    #[test]
    fn a_box_made_from_a_workspace_manifest_runs_only_there() {
        let kept = record("0123456789ab", "/a", None, 1);
        let refused = refused_by_name(&kept, Path::new("/w"), Wanted::default()).expect("refused");
        assert!(refused.contains("/a"), "{refused}");
        assert_eq!(
            refused_by_name(&kept, Path::new("/a"), Wanted::default()),
            None
        );
        assert!(refused_by_name(&kept, Path::new("/a"), ALPHACA).is_some());
    }

    /// What `--id` and the panel start from when no `--role` is typed: the
    /// role the box records, never whatever the current directory holds.
    #[test]
    fn a_box_names_its_own_role_whatever_was_typed_and_where() {
        let local = from(
            record("a", "/w", Some("./roles/alphaca"), 1),
            "dir:/w/roles/alphaca",
        );
        assert_eq!(local.role_ref().as_deref(), Some("/w/roles/alphaca"));
        let fetched = from(
            record("a", "/w", Some("github:you/r@abc"), 1),
            "repo:https://github.com/you/r",
        );
        assert_eq!(fetched.role_ref().as_deref(), Some("github:you/r@abc"));
        let older = record("a", "/w", Some("./roles/alphaca"), 1);
        assert_eq!(older.role_ref().as_deref(), Some("/w/./roles/alphaca"));
        assert_eq!(
            record("a", "/w", Some("alphaca"), 1).role_ref().as_deref(),
            Some("alphaca")
        );
        assert_eq!(record("a", "/w", None, 1).role_ref(), None);
    }

    /// Enter on an idle row never mounts wherever the panel happened to be
    /// opened: here only if the box already ran here.
    #[test]
    fn the_panel_resumes_a_box_here_only_where_it_has_run() {
        let mut kept = role_box("0123456789ab", "/b", 1);
        kept.earlier = vec![PathBuf::from("/a")];
        let all = |_: &Path| true;
        assert_eq!(
            resume_in(&kept, Path::new("/a"), all),
            Ok(PathBuf::from("/a"))
        );
        assert_eq!(
            resume_in(&kept, Path::new("/home/me"), all),
            Ok(PathBuf::from("/b"))
        );
        let only_a = |path: &Path| path == Path::new("/a");
        assert_eq!(
            resume_in(&kept, Path::new("/home/me"), only_a),
            Ok(PathBuf::from("/a"))
        );
        let refused = resume_in(&kept, Path::new("/home/me"), |_| false).unwrap_err();
        assert!(
            refused.contains("wormhole box --id 0123456789ab"),
            "{refused}"
        );
    }

    #[test]
    fn the_listing_counts_the_other_workspaces_a_box_has_run_in() {
        let mut shared = record("0123456789ab", "/w/proj", Some("alphaca"), 100);
        shared.earlier = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        let text = list(
            &[Listing {
                record: shared,
                running: None,
            }],
            160,
        );
        assert!(text.contains("/w/proj +2"), "{text}");
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
        let found = resumable(
            &records,
            Path::new("/w"),
            typed("alphaca-java"),
            Resume::Here,
        );
        assert!(found.is_empty(), "{found:?}");
        let workspace_manifest =
            resumable(&records, Path::new("/w"), Wanted::default(), Resume::Here);
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

    /// Nothing renders as nothing; the surface with nothing to show says so
    /// in its own words, and `ps` and `ps --all` have different words.
    #[test]
    fn nothing_kept_renders_nothing() {
        assert_eq!(list(&[], 0), "");
    }

    fn entry(id: &str, pid: u32, started: u64) -> crate::registry::Entry {
        crate::registry::Entry {
            pid,
            box_id: id.to_owned(),
            key: None,
            workspace: PathBuf::from("/w/proj"),
            image: "/data/img".to_owned(),
            agent: Some("claude".to_owned()),
            name: Some("architect".to_owned()),
            alias: Some("api".to_owned()),
            started_unix: started,
        }
    }

    /// One scan behind `ps`, `ps --all` and the panel: every kept box with
    /// the pid running it, most recently used first.
    #[test]
    fn a_scan_pairs_every_kept_box_with_the_pid_running_it() {
        let kept = vec![
            record("aaaaaaaaaaaa", "/w/proj", None, 100),
            record("bbbbbbbbbbbb", "/w/proj", None, 300),
        ];
        let boxes = scan(kept, &[entry("aaaaaaaaaaaa", 4242, 100)]);
        let ids: Vec<(&str, Option<u32>)> = boxes
            .iter()
            .map(|b| (b.record.id.as_str(), b.running))
            .collect();
        assert_eq!(ids, [("bbbbbbbbbbbb", None), ("aaaaaaaaaaaa", Some(4242))]);
    }

    /// The registry is what says a box is running. A running box whose
    /// home record cannot be read is still running, so it is still listed —
    /// from the entry, which carries everything the row needs.
    #[test]
    fn a_running_box_with_no_readable_record_is_still_listed() {
        let boxes = scan(Vec::new(), &[entry("aaaaaaaaaaaa", 4242, 100)]);
        assert_eq!(boxes.len(), 1);
        let shown = &boxes[0];
        assert_eq!(shown.running, Some(4242));
        assert_eq!(shown.record.id, "aaaaaaaaaaaa");
        assert_eq!(shown.record.alias.as_deref(), Some("api"));
        assert_eq!(shown.record.name.as_deref(), Some("architect"));
        assert_eq!(shown.record.workspace, PathBuf::from("/w/proj"));
        assert_eq!(shown.record.started_unix, 100);
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
