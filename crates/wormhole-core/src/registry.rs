//! The record a running box leaves behind so it can be found: what is in
//! `boxes/<pid>/box.toml`. The binary does the reading, writing and
//! pid-liveness checks.

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
    toml::from_str(text)
        .map(|mut entry: Entry| {
            // A box started by a wormhole that misfiled its role's identity
            // here; that is no name anybody can type.
            entry.alias = entry
                .alias
                .filter(|alias| crate::home::is_usable_alias(alias));
            entry
        })
        .map_err(|e| format!("box.toml is not valid: {}", e.message()))
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
}
