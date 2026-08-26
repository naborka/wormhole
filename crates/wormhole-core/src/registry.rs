//! The record a running box leaves behind so it can be found: what is in
//! `boxes/<pid>/box.toml` and how `wormhole ps` shows it. The binary does
//! the reading, writing and pid-liveness checks.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One running box. The pid is `wormhole box` itself — the process that
/// owns the entry and removes it on exit.
///
/// A key this version does not know is ignored, not refused. Nobody writes
/// this file by hand, so there is no typo for strictness to catch — and
/// refusing one would mean every field wormhole stops writing orphans the
/// boxes already running, which is how removing groups made a live box
/// unlistable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub pid: u32,
    /// Which kept box this process is running. Several boxes may share a
    /// workspace, so the workspace no longer names one.
    #[serde(default)]
    pub box_id: String,
    pub workspace: PathBuf,
    pub image: String,
    #[serde(default)]
    pub agent: Option<String>,
    /// The manifest's `name`, when it has one.
    #[serde(default)]
    pub name: Option<String>,
    /// What the person who started it called it, if anything. Carried
    /// here as well as in the home so `attach` and `stop` can take it
    /// without reading every kept home.
    #[serde(default)]
    pub alias: Option<String>,
    /// Unix seconds when the box started.
    pub started_unix: u64,
}

pub fn to_toml(entry: &Entry) -> Result<String, String> {
    toml::to_string(entry).map_err(|e| format!("cannot serialize box entry: {e}"))
}

pub fn parse(text: &str) -> Result<Entry, String> {
    toml::from_str(text).map_err(|e| format!("box.toml is not valid: {}", e.message()))
}

/// What `wormhole ps` prints. Entries whose process is gone must be
/// reaped by the caller before this; here every entry is a live box.
pub fn ps_table(entries: &[Entry], now_unix: u64) -> String {
    if entries.is_empty() {
        return "no boxes running\n".to_owned();
    }
    let mut rows = vec![
        ["ID", "NAME", "ALIAS", "PID", "AGENT", "UPTIME", "WORKSPACE"]
            .map(str::to_owned)
            .to_vec(),
    ];
    for entry in entries {
        rows.push(vec![
            entry.box_id.clone(),
            crate::table::or_dash(entry.name.as_deref()),
            crate::table::or_dash(entry.alias.as_deref()),
            entry.pid.to_string(),
            crate::table::or_dash(entry.agent.as_deref()),
            crate::table::age(now_unix.saturating_sub(entry.started_unix)),
            entry.workspace.display().to_string(),
        ]);
    }
    crate::table::render(&rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Entry {
        Entry {
            pid: 4242,
            box_id: "0123456789ab".to_owned(),
            workspace: PathBuf::from("/home/me/proj"),
            image: "/data/wormhole/images/abc".to_owned(),
            agent: Some("claude".to_owned()),
            name: Some("architect".to_owned()),
            alias: None,
            started_unix: 1000,
        }
    }

    #[test]
    fn an_entry_survives_the_round_trip() {
        let text = to_toml(&entry()).expect("serializes");
        assert_eq!(parse(&text), Ok(entry()));
    }

    #[test]
    fn a_corrupt_entry_is_an_error_not_a_panic() {
        assert!(parse("pid = \"not a number\"").is_err());
        assert!(parse("{").is_err());
    }

    #[test]
    fn the_table_shows_name_pid_agent_uptime_and_workspace() {
        let table = ps_table(&[entry()], 1000 + 3 * 3600 + 12 * 60);
        let mut lines = table.lines();
        let header = lines.next().expect("header");
        let row = lines.next().expect("one row");
        for column in ["NAME", "PID", "AGENT", "UPTIME", "WORKSPACE"] {
            assert!(header.contains(column), "{header}");
        }
        for cell in ["architect", "4242", "claude", "3h12m", "/home/me/proj"] {
            assert!(row.contains(cell), "{row}");
        }
    }

    /// Entries written while groups existed carry a `group` line. A box
    /// running under the older wormhole must stay listable across the
    /// upgrade that removed groups, so the gone key is ignored rather than
    /// refused.
    #[test]
    fn an_entry_from_before_groups_were_removed_still_parses() {
        let mut text = to_toml(&entry()).expect("serializes");
        text.push_str("group = \"trio\"\n");
        assert_eq!(parse(&text), Ok(entry()));
    }

    /// The id is first, because it is what every other command takes:
    /// `attach <id>`, `box --id <id>`.
    #[test]
    fn the_id_leads_the_row_because_it_is_what_you_type_next() {
        let table = ps_table(&[entry()], 1000);
        assert!(table.starts_with("ID  "), "{table}");
        assert!(
            table
                .lines()
                .nth(1)
                .expect("row")
                .starts_with("0123456789ab"),
            "{table}"
        );
    }

    #[test]
    fn a_box_without_a_name_or_agent_shows_dashes() {
        let anonymous = Entry {
            agent: None,
            name: None,
            ..entry()
        };
        let table = ps_table(&[anonymous], 1000);
        let row = table.lines().nth(1).expect("row");
        assert!(row.contains(" -  "), "{row}");
    }

    /// The name you can type instead of the id gets its own column. Not
    /// folded into NAME: the manifest's name is a label several boxes
    /// share, and only the alias names exactly one.
    #[test]
    fn the_name_you_gave_a_box_is_shown_beside_the_one_its_manifest_gave_it() {
        let called = Entry {
            alias: Some("api".to_owned()),
            ..entry()
        };
        let table = ps_table(&[called], 1000);
        let header = table.lines().next().expect("header");
        assert!(header.contains("ALIAS"), "{header}");
        let row = table.lines().nth(1).expect("row");
        assert!(row.contains("api"), "{row}");
        assert!(row.contains("architect"), "{row}");
    }

    #[test]
    fn no_boxes_says_so_instead_of_an_empty_table() {
        assert_eq!(ps_table(&[], 0), "no boxes running\n");
    }

    #[test]
    fn uptime_reads_like_an_age() {
        for (seconds, shown) in [
            (42, "42s"),
            (7 * 60 + 5, "7m"),
            (3 * 3600 + 12 * 60, "3h12m"),
            (2 * 86400 + 3 * 3600, "2d3h"),
        ] {
            assert_eq!(crate::table::age(seconds), shown);
        }
    }

    /// A clock that went backwards must not underflow into a huge age.
    #[test]
    fn a_start_time_in_the_future_shows_zero_not_garbage() {
        let table = ps_table(&[entry()], 0);
        assert!(table.lines().nth(1).expect("row").contains("0s"));
    }
}
