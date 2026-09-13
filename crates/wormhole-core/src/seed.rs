//! What wormhole writes into a box home before the agent starts, beyond
//! the instructions file: the answers to questions the box's existence
//! already settles, merged into the agent's own config files.

use serde_json::{Map, Value};

use crate::manifest::{ConfigWrite, Entry, Format, Rule};

/// One of the agent's config files as a start leaves it: what the box home
/// held, `existing`, with every entry of `write` applied. For a start that
/// hands over a login, the login fields of the host's own copy, `host`,
/// are carried in where the box has none of its own, so a `/login` the box
/// did is never undone. Everything else in the file is the agent's own and
/// survives.
///
/// # Errors
///
/// A file that does not parse, or a key an entry passes through that holds
/// something other than a table: the agent's data is refused, never
/// overwritten.
pub fn config_file(
    write: &ConfigWrite,
    existing: Option<&str>,
    host: Option<&str>,
) -> Result<String, String> {
    match write.format {
        Format::Json => merge::<Map<String, Value>>(write, existing, host),
        Format::Toml => merge::<toml::Table>(write, existing, host),
    }
}

/// One body for both formats, so a JSON file and a TOML file can never be
/// merged by different rules.
fn merge<T: Document>(
    write: &ConfigWrite,
    existing: Option<&str>,
    host: Option<&str>,
) -> Result<String, String> {
    let file = write.file;
    let mut root = match existing {
        None => T::default(),
        Some(text) => T::parse(text)
            .map_err(|e| format!("the box home's {file} is not valid {}: {e}", T::FORMAT))?,
    };
    let login = match host {
        Some(text) if !write.login_fields.is_empty() => {
            let host = T::parse(text)
                .map_err(|e| format!("the host's {file} is not valid {}: {e}", T::FORMAT))?;
            write
                .login_fields
                .iter()
                .filter_map(|field| {
                    host.field(field).map(|value| Entry {
                        table: Vec::new(),
                        key: (*field).to_owned(),
                        value,
                        rule: Rule::Initial,
                    })
                })
                .collect()
        }
        _ => Vec::new(),
    };
    for entry in write.entries.iter().chain(&login) {
        apply(&mut root, entry, file)?;
    }
    root.render()
        .map_err(|e| format!("cannot serialize {file}: {e}"))
}

fn apply<T: Document>(root: &mut T, entry: &Entry, file: &str) -> Result<(), String> {
    let mut table = root;
    for (depth, key) in entry.table.iter().enumerate() {
        table = table.child(key).ok_or_else(|| {
            format!(
                "{} in {file} is not a table",
                entry.table[..=depth].join(".")
            )
        })?;
    }
    if entry.rule == Rule::Settled || !table.has(&entry.key) {
        table
            .set(&entry.key, &entry.value)
            .map_err(|e| format!("cannot write {} into {file}: {e}", entry.key))?;
    }
    Ok(())
}

/// A parsed config file: keys, each holding a value or a nested table.
trait Document: Default {
    const FORMAT: &'static str;
    fn parse(text: &str) -> Result<Self, String>;
    /// The table at `key`, made where there is none; `None` when `key`
    /// holds something else.
    fn child(&mut self, key: &str) -> Option<&mut Self>;
    fn has(&self, key: &str) -> bool;
    fn field(&self, key: &str) -> Option<Value>;
    fn set(&mut self, key: &str, value: &Value) -> Result<(), String>;
    fn render(&self) -> Result<String, String>;
}

impl Document for Map<String, Value> {
    const FORMAT: &'static str = "JSON";

    fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    fn child(&mut self, key: &str) -> Option<&mut Self> {
        self.entry(key)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
    }

    fn has(&self, key: &str) -> bool {
        self.contains_key(key)
    }

    fn field(&self, key: &str) -> Option<Value> {
        self.get(key).cloned()
    }

    fn set(&mut self, key: &str, value: &Value) -> Result<(), String> {
        self.insert(key.to_owned(), value.clone());
        Ok(())
    }

    fn render(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }
}

impl Document for toml::Table {
    const FORMAT: &'static str = "TOML";

    fn parse(text: &str) -> Result<Self, String> {
        text.parse().map_err(|e: toml::de::Error| e.to_string())
    }

    fn child(&mut self, key: &str) -> Option<&mut Self> {
        self.entry(key)
            .or_insert_with(|| toml::Value::Table(Self::new()))
            .as_table_mut()
    }

    fn has(&self, key: &str) -> bool {
        self.contains_key(key)
    }

    fn field(&self, key: &str) -> Option<Value> {
        self.get(key)
            .and_then(|value| serde_json::to_value(value).ok())
    }

    fn set(&mut self, key: &str, value: &Value) -> Result<(), String> {
        let value = toml::Value::try_from(value).map_err(|e| e.to_string())?;
        self.insert(key.to_owned(), value);
        Ok(())
    }

    fn render(&self) -> Result<String, String> {
        toml::to_string(self).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::manifest::{self, Manifest};

    const WORKSPACE: &str = "/home/me/proj";
    const CLAUDE: &str = "run = \"claude\"\n";
    const CODEX: &str = "run = \"codex\"\n";
    const GROK: &str = "run = \"grok\"\n";

    fn manifest(agent: &str) -> Manifest {
        manifest::parse(&format!(
            "version = 1\n[image]\nbase = \"https://example.test/rootfs.tar.gz\"\nbase_sha256 = \"{}\"\n[agent]\n{agent}",
            "0".repeat(64)
        ))
        .expect("valid manifest")
    }

    /// Every file a start writes for `agent`, merged into what `home`
    /// already held there, by file.
    fn start(
        agent: &str,
        home: &[(&str, &str)],
        host: Option<&str>,
    ) -> Result<BTreeMap<&'static str, String>, String> {
        manifest::config_writes(&manifest(agent), WORKSPACE)
            .iter()
            .map(|write| {
                let existing = home
                    .iter()
                    .find(|(file, _)| *file == write.file)
                    .map(|(_, text)| *text);
                config_file(write, existing, host).map(|text| (write.file, text))
            })
            .collect()
    }

    fn json_in(agent: &str, home: &[(&str, &str)], file: &str) -> Value {
        let files = start(agent, home, None).expect("seeded");
        serde_json::from_str(&files[file]).expect("valid JSON out")
    }

    fn toml_in(agent: &str, home: &[(&str, &str)], file: &str) -> toml::Table {
        let files = start(agent, home, None).expect("seeded");
        files[file].parse().expect("valid TOML out")
    }

    #[test]
    fn a_fresh_claude_home_has_every_first_run_question_answered() {
        let state = json_in(CLAUDE, &[], ".claude.json");
        assert_eq!(state["hasCompletedOnboarding"], json!(true));
        assert_eq!(state["bypassPermissionsModeAccepted"], json!(true));
        assert_eq!(state["hasAcknowledgedCostThreshold"], json!(true));
        assert_eq!(state["theme"], json!("dark"));
        let project = &state["projects"][WORKSPACE];
        for answered in [
            "hasTrustDialogAccepted",
            "hasCompletedProjectOnboarding",
            "hasClaudeMdExternalIncludesApproved",
            "hasClaudeMdExternalIncludesWarningShown",
        ] {
            assert_eq!(project[answered], json!(true), "{answered}");
        }
    }

    /// Two offers wait under the prompt for one keypress: auto mode as the
    /// default, which a set `defaultMode` is what brings on, and medium
    /// effort. Either answered by a stray key leaves the box gated or
    /// thinking less.
    #[test]
    fn claude_is_never_offered_auto_mode_or_less_effort() {
        let state = json_in(CLAUDE, &[], ".claude.json");
        assert_eq!(state["hasSeenAutoDefaultNudge"], json!(true));
        assert_eq!(state["hasSeenEffortMediumNudge"], json!(true));
        for model in ["claude-opus-5", "claude-fable-5-1"] {
            assert_eq!(
                state["hasSeenEffortMediumNudgeByModel"][model],
                json!(true),
                "{model}"
            );
        }
    }

    /// The flag reaches only the process wormhole starts. A resumed
    /// session, a background worker and a `claude` typed in an attached
    /// shell read the user settings instead.
    #[test]
    fn claude_asks_no_permission_on_any_path() {
        let settings = json_in(CLAUDE, &[], ".claude/settings.json");
        assert_eq!(settings["skipDangerousModePermissionPrompt"], json!(true));
        assert_eq!(
            settings["permissions"]["defaultMode"],
            json!("bypassPermissions")
        );
        assert_eq!(settings["enableAllProjectMcpServers"], json!(true));
    }

    #[test]
    fn claude_does_not_switch_models_when_a_message_is_flagged() {
        let settings = json_in(CLAUDE, &[], ".claude/settings.json");
        assert_eq!(settings["switchModelsOnFlag"], json!(false));
    }

    /// What the box settles it settles on every start: a past session
    /// cannot talk it out of it. Everything else in the file is the
    /// agent's own and survives.
    #[test]
    fn a_settled_answer_replaces_what_the_agent_wrote_and_nothing_else() {
        let own = r#"{"permissions":{"defaultMode":"default","allow":["Bash"]},"hooks":{"PreToolUse":[]}}"#;
        let settings = json_in(
            CLAUDE,
            &[(".claude/settings.json", own)],
            ".claude/settings.json",
        );
        assert_eq!(
            settings["permissions"]["defaultMode"],
            json!("bypassPermissions")
        );
        assert_eq!(settings["permissions"]["allow"], json!(["Bash"]));
        assert_eq!(settings["hooks"], json!({"PreToolUse": []}));
    }

    /// A starting point is only that: once a choice is made, the next
    /// start leaves it alone.
    #[test]
    fn a_choice_already_made_wins_over_a_starting_point() {
        let state = json_in(
            CLAUDE,
            &[(".claude.json", r#"{"theme":"light"}"#)],
            ".claude.json",
        );
        assert_eq!(state["theme"], json!("light"));
        let settings = json_in(
            CLAUDE,
            &[(".claude/settings.json", r#"{"switchModelsOnFlag":true}"#)],
            ".claude/settings.json",
        );
        assert_eq!(settings["switchModelsOnFlag"], json!(true));
    }

    /// The kept home accumulates the agent's own state — logins,
    /// counters. Seeding must never destroy it.
    #[test]
    fn everything_claude_wrote_survives_the_merge() {
        let own = r#"{"oauthAccount":{"id":"me"},"numStartups":7,"projects":{"/home/me/proj":{"allowedTools":["Bash"]}}}"#;
        let state = json_in(CLAUDE, &[(".claude.json", own)], ".claude.json");
        assert_eq!(state["oauthAccount"]["id"], json!("me"));
        assert_eq!(state["numStartups"], json!(7));
        let project = &state["projects"][WORKSPACE];
        assert_eq!(project["allowedTools"], json!(["Bash"]));
        assert_eq!(project["hasTrustDialogAccepted"], json!(true));
    }

    /// The token file is half a login; the account fields beside it are
    /// the other half. A host login is carried over only where the box
    /// has none of its own, so a `/login` the box did is never undone.
    #[test]
    fn a_host_login_is_carried_over_only_where_the_box_has_none() {
        let host = r#"{"oauthAccount":{"emailAddress":"me@x.test"},"theme":"light"}"#;
        let seeded = start(CLAUDE, &[], Some(host)).expect("seeded");
        let state: Value = serde_json::from_str(&seeded[".claude.json"]).expect("JSON");
        assert_eq!(state["oauthAccount"]["emailAddress"], "me@x.test");
        assert_eq!(state["theme"], "dark", "only the login travels");
        let own = r#"{"oauthAccount":{"emailAddress":"box@x.test"}}"#;
        let kept = start(CLAUDE, &[(".claude.json", own)], Some(host)).expect("merged");
        let state: Value = serde_json::from_str(&kept[".claude.json"]).expect("JSON");
        assert_eq!(state["oauthAccount"]["emailAddress"], "box@x.test");
        assert!(start(CLAUDE, &[], Some("{ not json")).is_err());
    }

    /// Codex reads its config on every path the flag does not reach — a
    /// `codex resume` typed in an attached shell, an app-server thread.
    #[test]
    fn codex_asks_no_approval_and_runs_no_sandbox_of_its_own_on_any_path() {
        let config = toml_in(CODEX, &[], ".codex/config.toml");
        assert_eq!(config["approval_policy"].as_str(), Some("never"));
        assert_eq!(config["sandbox_mode"].as_str(), Some("danger-full-access"));
        assert_eq!(config["web_search"].as_str(), Some("live"));
        assert_eq!(
            config["notice"]["hide_rate_limit_model_nudge"].as_bool(),
            Some(true)
        );
        assert_eq!(
            config["projects"][WORKSPACE]["trust_level"].as_str(),
            Some("trusted")
        );
    }

    /// The manifest's model wins each start, the rule a fixed env
    /// variable already follows for claude.
    #[test]
    fn the_manifests_model_wins_over_codexs_own() {
        let home = [(".codex/config.toml", "model = \"old\"\n")];
        let config = toml_in(
            "run = \"codex\"\nmodel = \"new\"\n",
            &home,
            ".codex/config.toml",
        );
        assert_eq!(config["model"].as_str(), Some("new"));
        let config = toml_in(CODEX, &home, ".codex/config.toml");
        assert_eq!(config["model"].as_str(), Some("old"));
    }

    #[test]
    fn everything_codex_wrote_survives_the_merge() {
        let own = "[mcp_servers.context7]\ncommand = \"npx\"\n\n[projects.\"/other\"]\ntrust_level = \"trusted\"\n";
        let config = toml_in(CODEX, &[(".codex/config.toml", own)], ".codex/config.toml");
        assert_eq!(
            config["mcp_servers"]["context7"]["command"].as_str(),
            Some("npx")
        );
        assert_eq!(
            config["projects"]["/other"]["trust_level"].as_str(),
            Some("trusted")
        );
    }

    #[test]
    fn grok_always_approves_and_may_fetch_the_web() {
        let config = toml_in(GROK, &[], ".grok/config.toml");
        assert_eq!(
            config["ui"]["permission_mode"].as_str(),
            Some("always-approve")
        );
        assert_eq!(config["features"]["web_fetch"].as_bool(), Some(true));
    }

    /// Grok saves a mode toggled in its TUI to this file, where every
    /// later start without the flag would read it.
    #[test]
    fn a_grok_mode_toggled_in_a_past_session_is_set_back() {
        let own = "[ui]\npermission_mode = \"ask\"\nscreen_mode = \"minimal\"\n";
        let config = toml_in(GROK, &[(".grok/config.toml", own)], ".grok/config.toml");
        assert_eq!(
            config["ui"]["permission_mode"].as_str(),
            Some("always-approve")
        );
        assert_eq!(config["ui"]["screen_mode"].as_str(), Some("minimal"));
    }

    #[test]
    fn a_corrupt_config_is_refused_not_replaced() {
        let err = start(CLAUDE, &[(".claude.json", "{ not json")], None).unwrap_err();
        assert!(err.contains(".claude.json is not valid JSON"), "{err}");
        let err = start(CODEX, &[(".codex/config.toml", "not = = toml")], None).unwrap_err();
        assert!(err.contains("config.toml is not valid TOML"), "{err}");
    }

    /// A key an answer has to pass through holds the agent's own data,
    /// so the file is refused rather than that data overwritten.
    #[test]
    fn a_value_where_a_table_belongs_is_refused_not_replaced() {
        let err = start(CLAUDE, &[(".claude.json", r#"{"projects":3}"#)], None).unwrap_err();
        assert!(err.contains("projects in .claude.json"), "{err}");
        let err = start(GROK, &[(".grok/config.toml", "ui = 3\n")], None).unwrap_err();
        assert!(err.contains("ui in .grok/config.toml"), "{err}");
    }
}
