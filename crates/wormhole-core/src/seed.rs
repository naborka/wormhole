//! What wormhole writes into a box home before the agent starts, beyond
//! the instructions file: the answers to questions the box's existence
//! already settles.

use serde_json::{Map, Value, json};

/// Claude Code asks four first-run questions — theme, security notes,
/// workspace trust, the Bypass Permissions warning — and every answer is
/// already given by the box existing. This merges those answers into the
/// home's `.claude.json`, keeping everything the agent wrote there itself
/// (logins, counters, its own theme choice).
///
/// Key names read off Claude Code 2.1.228, not guessed:
/// `hasCompletedOnboarding` and `bypassPermissionsModeAccepted` are
/// top-level (the latter verified against the binary's own sandbox
/// seeder); trust lives per project under the workspace path; `theme`
/// defaults to `dark` and an existing choice wins.
pub fn claude_config(existing: Option<&str>, workspace: &str) -> Result<String, String> {
    let mut root: Map<String, Value> = match existing {
        None => Map::new(),
        Some(text) => serde_json::from_str(text)
            .map_err(|e| format!(".claude.json in the box home is not valid JSON: {e}"))?,
    };
    root.insert("hasCompletedOnboarding".to_owned(), json!(true));
    root.insert("bypassPermissionsModeAccepted".to_owned(), json!(true));
    root.entry("theme").or_insert(json!("dark"));

    let project = root
        .entry("projects")
        .or_insert(json!({}))
        .as_object_mut()
        .ok_or("projects in .claude.json is not an object")?
        .entry(workspace)
        .or_insert(json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("projects[{workspace}] in .claude.json is not an object"))?;
    project.insert("hasTrustDialogAccepted".to_owned(), json!(true));
    project.insert("hasCompletedProjectOnboarding".to_owned(), json!(true));

    serde_json::to_string_pretty(&root).map_err(|e| format!("cannot serialize .claude.json: {e}"))
}

/// Points Claude Code's status line at the limits file wormhole keeps
/// fresh in the box home, by merging into `.claude/settings.json`. A
/// status line the agent's user configured themselves wins — wormhole
/// adds a default, it does not take the surface over.
pub fn claude_settings(existing: Option<&str>, command: &str) -> Result<String, String> {
    let mut root: Map<String, Value> = match existing {
        None => Map::new(),
        Some(text) => serde_json::from_str(text)
            .map_err(|e| format!("settings.json in the box home is not valid JSON: {e}"))?,
    };
    root.entry("statusLine")
        .or_insert(json!({"type": "command", "command": command}));
    serde_json::to_string_pretty(&root).map_err(|e| format!("cannot serialize settings.json: {e}"))
}

/// Answers codex's first-run questions in the home's `.codex/config.toml`
/// — merged, never overwritten, so logins and the agent's own choices in
/// the kept home survive.
///
/// Two answers the box's existence already settles: the workspace is
/// trusted (codex asks per directory, even under its bypass flag on some
/// releases), and the model is the manifest's — codex reads no env var
/// for one, so the config key is where the manifest's `model` lands. The
/// manifest wins the model key each start, the same rule a fixed env
/// variable follows; everything else in the file is the agent's own.
pub fn codex_config(
    existing: Option<&str>,
    workspace: &str,
    model: Option<&str>,
) -> Result<String, String> {
    let mut root: toml::Table = match existing {
        None => toml::Table::new(),
        Some(text) => text
            .parse()
            .map_err(|e| format!("config.toml in the box home is not valid TOML: {e}"))?,
    };
    if let Some(model) = model {
        root.insert("model".to_owned(), toml::Value::String(model.to_owned()));
    }
    let projects = root
        .entry("projects")
        .or_insert(toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or("projects in config.toml is not a table")?;
    let project = projects
        .entry(workspace.to_owned())
        .or_insert(toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("projects.\"{workspace}\" in config.toml is not a table"))?;
    project.insert(
        "trust_level".to_owned(),
        toml::Value::String("trusted".to_owned()),
    );
    toml::to_string(&root).map_err(|e| format!("cannot serialize config.toml: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).expect("valid JSON out")
    }

    fn toml_parsed(text: &str) -> toml::Table {
        text.parse().expect("valid TOML out")
    }

    #[test]
    fn a_fresh_codex_home_gets_trust_and_the_manifests_model() {
        let config =
            toml_parsed(&codex_config(None, "/home/me/proj", Some("gpt-5-codex")).expect("seeded"));
        assert_eq!(config["model"].as_str(), Some("gpt-5-codex"));
        assert_eq!(
            config["projects"]["/home/me/proj"]["trust_level"].as_str(),
            Some("trusted")
        );
    }

    #[test]
    fn a_manifest_without_a_model_leaves_the_agents_own_choice_alone() {
        let existing = "model = \"mine\"\n";
        let config = toml_parsed(&codex_config(Some(existing), "/w", None).expect("merged"));
        assert_eq!(config["model"].as_str(), Some("mine"));
    }

    /// The manifest's model wins each start, the rule a fixed env
    /// variable already follows for claude.
    #[test]
    fn the_manifests_model_wins_over_the_agents_own() {
        let config = toml_parsed(
            &codex_config(Some("model = \"old\"\n"), "/w", Some("new")).expect("merged"),
        );
        assert_eq!(config["model"].as_str(), Some("new"));
    }

    /// The kept home accumulates the agent's own state. Seeding must
    /// never destroy it.
    #[test]
    fn everything_codex_wrote_survives_the_merge() {
        let existing =
            "approval_policy = \"never\"\n\n[projects.\"/other\"]\ntrust_level = \"trusted\"\n";
        let config = toml_parsed(&codex_config(Some(existing), "/w", None).expect("merged"));
        assert_eq!(config["approval_policy"].as_str(), Some("never"));
        assert_eq!(
            config["projects"]["/other"]["trust_level"].as_str(),
            Some("trusted")
        );
        assert_eq!(
            config["projects"]["/w"]["trust_level"].as_str(),
            Some("trusted")
        );
    }

    #[test]
    fn a_corrupt_codex_config_is_refused_not_replaced() {
        let err = codex_config(Some("not = = toml"), "/w", None).unwrap_err();
        assert!(err.contains("not valid TOML"), "{err}");
    }

    #[test]
    fn a_fresh_home_gets_all_four_questions_answered() {
        let config = parsed(&claude_config(None, "/home/me/proj").expect("seeded"));
        assert_eq!(config["hasCompletedOnboarding"], json!(true));
        assert_eq!(config["bypassPermissionsModeAccepted"], json!(true));
        assert_eq!(config["theme"], json!("dark"));
        let project = &config["projects"]["/home/me/proj"];
        assert_eq!(project["hasTrustDialogAccepted"], json!(true));
        assert_eq!(project["hasCompletedProjectOnboarding"], json!(true));
    }

    /// The kept home accumulates the agent's own state — logins,
    /// counters. Seeding must never destroy it.
    #[test]
    fn everything_the_agent_wrote_survives_the_merge() {
        let existing = r#"{"oauthAccount":{"id":"me"},"numStartups":7}"#;
        let config = parsed(&claude_config(Some(existing), "/w").expect("merged"));
        assert_eq!(config["oauthAccount"]["id"], json!("me"));
        assert_eq!(config["numStartups"], json!(7));
        assert_eq!(config["hasCompletedOnboarding"], json!(true));
    }

    #[test]
    fn a_theme_the_agent_chose_wins_over_the_seeded_one() {
        let config = parsed(&claude_config(Some(r#"{"theme":"light"}"#), "/w").expect("merged"));
        assert_eq!(config["theme"], json!("light"));
    }

    #[test]
    fn an_existing_project_entry_keeps_its_other_fields() {
        let existing = r#"{"projects":{"/w":{"allowedTools":["Bash"]}}}"#;
        let config = parsed(&claude_config(Some(existing), "/w").expect("merged"));
        let project = &config["projects"]["/w"];
        assert_eq!(project["allowedTools"], json!(["Bash"]));
        assert_eq!(project["hasTrustDialogAccepted"], json!(true));
    }

    #[test]
    fn a_corrupt_config_is_refused_not_replaced() {
        let err = claude_config(Some("{ not json"), "/w").unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");
    }

    #[test]
    fn fresh_settings_get_the_wormhole_status_line() {
        let settings = parsed(&claude_settings(None, "sh x.sh").expect("seeded"));
        assert_eq!(settings["statusLine"]["type"], json!("command"));
        assert_eq!(settings["statusLine"]["command"], json!("sh x.sh"));
    }

    /// The status line is the user's surface first: one they set up
    /// themselves must survive every box start.
    #[test]
    fn a_status_line_the_user_configured_wins() {
        let existing = r#"{"statusLine":{"type":"command","command":"my-own.sh"},"model":"opus"}"#;
        let settings = parsed(&claude_settings(Some(existing), "sh x.sh").expect("merged"));
        assert_eq!(settings["statusLine"]["command"], json!("my-own.sh"));
        assert_eq!(settings["model"], json!("opus"));
    }

    #[test]
    fn corrupt_settings_are_refused_not_replaced() {
        let err = claude_settings(Some("{ not json"), "sh x.sh").unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");
    }
}
