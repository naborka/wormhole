//! One body for what a box's environment is. `resolve` computes it at
//! start from the manifest's declarations, the `--env` flags and the host;
//! the result is the baked env, written beside the box's pid and carried
//! for its whole life. `refresh` recomputes an attach session's view of
//! it: the attacher's host value wins where it is set, the baked value
//! survives where it is not, and `fixed` never moves.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::manifest::EnvVar;
use crate::run::EnvArg;

/// Where a variable's value came from, kept so every later screen can say
/// it without re-deriving anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The manifest owns the value; nothing overrides it, ever.
    Fixed,
    /// A `--env` flag spelled it.
    Cli,
    /// Carried from the host environment; refreshed on attach.
    Host,
    /// The manifest's fallback for a host that had nothing.
    Default,
    /// Declared, but no source had a value.
    Unset,
}

/// One resolved variable of the baked env.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Var {
    pub value: String,
    pub source: Source,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
}

impl Var {
    fn is_set(&self) -> bool {
        !self.value.is_empty()
    }
}

/// The whole environment a box runs under, by name.
pub type BakedEnv = BTreeMap<String, Var>;

/// What `refresh` did to one variable, for the attach diff line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The attacher's host had a newer value for a host-carried variable.
    Refreshed(String),
    /// A `--env` flag set or added it.
    Set(String),
}

/// The one rule for reading the host: an empty value is no value. Both
/// `resolve` and `refresh` decide "does the host have a word" through
/// this body, so start and attach can never disagree on what that means.
fn carried(host: &BTreeMap<String, String>, name: &str) -> Option<String> {
    host.get(name).filter(|v| !v.is_empty()).cloned()
}

/// What one `--env` flag yields: the spelled value, or the host's.
fn cli_value(arg: &EnvArg, host: &BTreeMap<String, String>) -> Option<String> {
    arg.value.clone().or_else(|| carried(host, &arg.name))
}

/// The baked env a start computes. `cli` beats the host, the host beats
/// the default, and `fixed` beats everything — the same order one sentence
/// can say.
#[must_use]
pub fn resolve(
    declared: &BTreeMap<String, EnvVar>,
    cli: &[EnvArg],
    host: &BTreeMap<String, String>,
) -> BakedEnv {
    let unset = || (String::new(), Source::Unset);
    let mut env: BakedEnv = declared
        .iter()
        .map(|(name, var)| {
            let flag = cli.iter().rev().find(|arg| arg.name == *name);
            let (value, source) = if let Some(fixed) = &var.fixed {
                (fixed.clone(), Source::Fixed)
            } else if let Some(arg) = flag {
                cli_value(arg, host).map_or_else(unset, |value| (value, Source::Cli))
            } else if let Some(value) = carried(host, name) {
                (value, Source::Host)
            } else if var.default.is_empty() {
                unset()
            } else {
                (var.default.clone(), Source::Default)
            };
            (
                name.clone(),
                Var {
                    value,
                    source,
                    secret: var.secret,
                    required: var.required,
                },
            )
        })
        .collect();
    for arg in cli {
        if env.contains_key(&arg.name) {
            continue;
        }
        let (value, source) = cli_value(arg, host).map_or_else(unset, |value| (value, Source::Cli));
        env.insert(
            arg.name.clone(),
            Var {
                value,
                source,
                secret: false,
                required: false,
            },
        );
    }
    env
}

/// An attach session's env: the baked one, with `--env` flags applied and
/// every non-fixed variable the attacher's host can improve refreshed.
/// The returned changes are the diff line; an unchanged env reports none.
#[must_use]
pub fn refresh(
    baked: &BakedEnv,
    cli: &[EnvArg],
    host: &BTreeMap<String, String>,
) -> (BakedEnv, Vec<Change>) {
    let mut env = baked.clone();
    let mut changes = Vec::new();
    for arg in cli {
        let Some(value) = cli_value(arg, host) else {
            continue;
        };
        // A bare `--env NAME` carries the host's value, so what it made is
        // a host-carried variable; only a spelled one is the CLI's own.
        let source = if arg.value.is_some() {
            Source::Cli
        } else {
            Source::Host
        };
        match env.entry(arg.name.clone()) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let var = entry.get_mut();
                if var.source == Source::Fixed || var.value == value {
                    continue;
                }
                var.value = value;
                var.source = source;
                changes.push(Change::Set(arg.name.clone()));
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Var {
                    value,
                    source,
                    secret: false,
                    required: false,
                });
                changes.push(Change::Set(arg.name.clone()));
            }
        }
    }
    let touched: std::collections::BTreeSet<&str> =
        cli.iter().map(|arg| arg.name.as_str()).collect();
    for (name, var) in &mut env {
        if var.source == Source::Fixed || touched.contains(name.as_str()) {
            continue;
        }
        if let Some(value) = carried(host, name)
            && value != var.value
        {
            var.value = value;
            var.source = Source::Host;
            changes.push(Change::Refreshed(name.clone()));
        }
    }
    (env, changes)
}

/// The names every bare `--env NAME` left without a value — nothing
/// spelled, nothing on the host, nothing already baked — so the caller
/// can refuse instead of quietly running without something asked for.
#[must_use]
pub fn unfilled_cli<'a>(cli: &'a [EnvArg], env: &BakedEnv) -> Vec<&'a str> {
    cli.iter()
        .filter(|arg| arg.value.is_none())
        .filter(|arg| !env.get(&arg.name).is_some_and(Var::is_set))
        .map(|arg| arg.name.as_str())
        .collect()
}

/// The names of every required variable still without a value — what a
/// start or attach refuses over.
#[must_use]
pub fn missing_required(env: &BakedEnv) -> Vec<&str> {
    env.iter()
        .filter(|(_, var)| var.required && !var.is_set())
        .map(|(name, _)| name.as_str())
        .collect()
}

/// What actually reaches the box's processes.
#[must_use]
pub fn to_env(env: &BakedEnv) -> BTreeMap<String, String> {
    env.iter()
        .map(|(name, var)| (name.clone(), var.value.clone()))
        .collect()
}

pub fn to_toml(env: &BakedEnv) -> Result<String, String> {
    toml::to_string(env).map_err(|e| format!("cannot serialize the box env: {e}"))
}

pub fn parse(text: &str) -> Result<BakedEnv, String> {
    toml::from_str(text).map_err(|e| format!("the box's env.toml is not valid: {}", e.message()))
}

/// A secret's name may be printed; its value may not, anywhere.
fn shown(var: &Var) -> &str {
    if !var.is_set() {
        "-"
    } else if var.secret {
        "••••"
    } else {
        &var.value
    }
}

fn mark(name: &str, var: &Var) -> String {
    if var.secret {
        format!("{name}•")
    } else {
        name.to_owned()
    }
}

/// The one line a start prints: every name, sorted set before unset, so
/// "what does this box run with" never needs a command.
#[must_use]
pub fn start_line(env: &BakedEnv) -> Option<String> {
    if env.is_empty() {
        return None;
    }
    let set: Vec<String> = env
        .iter()
        .filter(|(_, var)| var.is_set())
        .map(|(name, var)| mark(name, var))
        .collect();
    let unset: Vec<String> = env
        .iter()
        .filter(|(_, var)| !var.is_set())
        .map(|(name, _)| name.clone())
        .collect();
    let mut line = String::from("env:");
    if !set.is_empty() {
        let _ = write!(line, " {} set ({})", set.len(), set.join(", "));
    }
    if !unset.is_empty() {
        if !set.is_empty() {
            line.push(',');
        }
        let _ = write!(line, " {} unset ({})", unset.len(), unset.join(", "));
    }
    Some(line)
}

/// The one line an attach prints — only what changed, nothing when
/// nothing did.
#[must_use]
pub fn diff_line(changes: &[Change]) -> Option<String> {
    if changes.is_empty() {
        return None;
    }
    let parts: Vec<String> = changes
        .iter()
        .map(|change| match change {
            Change::Refreshed(name) => format!("{name} refreshed from host"),
            Change::Set(name) => format!("{name} set"),
        })
        .collect();
    Some(format!("env: {}", parts.join(", ")))
}

/// The `wormhole env` table: everything about every variable except a
/// secret's value.
#[must_use]
pub fn table(env: &BakedEnv) -> String {
    if env.is_empty() {
        return "no variables declared\n".to_owned();
    }
    let mut rows = vec![
        ["NAME", "VALUE", "SOURCE", "REFRESH"]
            .map(str::to_owned)
            .to_vec(),
    ];
    for (name, var) in env {
        let source = match var.source {
            Source::Fixed => "fixed",
            Source::Cli => "cli",
            Source::Host => "host",
            Source::Default => "default",
            Source::Unset => "unset",
        };
        let refresh = if var.source == Source::Fixed {
            "never"
        } else {
            "on attach"
        };
        rows.push(vec![
            name.clone(),
            shown(var).to_owned(),
            source.to_owned(),
            refresh.to_owned(),
        ]);
    }
    crate::table::render(&rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(vars: &[(&str, EnvVar)]) -> BTreeMap<String, EnvVar> {
        vars.iter()
            .map(|(name, var)| ((*name).to_owned(), var.clone()))
            .collect()
    }

    fn plain() -> EnvVar {
        EnvVar {
            default: String::new(),
            fixed: None,
            required: false,
            secret: false,
        }
    }

    fn host(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn named(name: &str) -> EnvArg {
        EnvArg {
            name: name.to_owned(),
            value: None,
        }
    }

    fn spelled(name: &str, value: &str) -> EnvArg {
        EnvArg {
            name: name.to_owned(),
            value: Some(value.to_owned()),
        }
    }

    #[test]
    fn fixed_beats_cli_beats_host_beats_default() {
        let vars = declared(&[
            (
                "SHELL",
                EnvVar {
                    fixed: Some("/bin/bash".to_owned()),
                    ..plain()
                },
            ),
            ("A", plain()),
            ("B", plain()),
            (
                "C",
                EnvVar {
                    default: "fallback".to_owned(),
                    ..plain()
                },
            ),
        ]);
        let env = resolve(
            &vars,
            &[spelled("SHELL", "/bin/zsh"), spelled("A", "cli")],
            &host(&[("SHELL", "/bin/fish"), ("A", "host"), ("B", "host")]),
        );
        assert_eq!(env["SHELL"].value, "/bin/bash");
        assert_eq!(env["SHELL"].source, Source::Fixed);
        assert_eq!(env["A"].value, "cli");
        assert_eq!(env["A"].source, Source::Cli);
        assert_eq!(env["B"].value, "host");
        assert_eq!(env["B"].source, Source::Host);
        assert_eq!(env["C"].value, "fallback");
        assert_eq!(env["C"].source, Source::Default);
    }

    /// `--env NAME` declares a variable the manifest never heard of and
    /// carries the host's value for it.
    #[test]
    fn a_cli_flag_declares_an_ad_hoc_variable() {
        let env = resolve(&declared(&[]), &[named("FOO")], &host(&[("FOO", "1")]));
        assert_eq!(env["FOO"].value, "1");
        assert_eq!(env["FOO"].source, Source::Cli);
    }

    #[test]
    fn an_empty_host_value_is_no_value() {
        let env = resolve(&declared(&[("A", plain())]), &[], &host(&[("A", "")]));
        assert_eq!(env["A"].source, Source::Unset);
        assert_eq!(missing_required(&env), Vec::<&str>::new());
    }

    #[test]
    fn a_required_variable_without_a_value_is_reported_missing() {
        let vars = declared(&[(
            "SENTRY_TOKEN",
            EnvVar {
                required: true,
                ..plain()
            },
        )]);
        let env = resolve(&vars, &[], &host(&[]));
        assert_eq!(missing_required(&env), vec!["SENTRY_TOKEN"]);
        let filled = resolve(&vars, &[spelled("SENTRY_TOKEN", "t")], &host(&[]));
        assert_eq!(missing_required(&filled), Vec::<&str>::new());
    }

    #[test]
    fn the_baked_env_survives_the_toml_round_trip() {
        let env = resolve(
            &declared(&[(
                "KEY",
                EnvVar {
                    secret: true,
                    required: true,
                    ..plain()
                },
            )]),
            &[spelled("EXTRA", "x")],
            &host(&[("KEY", "hush")]),
        );
        let text = to_toml(&env).expect("serializes");
        assert_eq!(parse(&text).expect("parses"), env);
    }

    /// The refresh rule in one test: the attacher's host wins where it is
    /// set, the baked value survives where it is not, fixed never moves.
    #[test]
    fn refresh_takes_the_hosts_word_only_where_it_has_one() {
        let baked = resolve(
            &declared(&[
                (
                    "SHELL",
                    EnvVar {
                        fixed: Some("/bin/bash".to_owned()),
                        ..plain()
                    },
                ),
                ("TOKEN", plain()),
                ("KEEP", plain()),
            ]),
            &[],
            &host(&[("TOKEN", "old"), ("KEEP", "kept"), ("SHELL", "/bin/zsh")]),
        );
        let (env, changes) = refresh(&baked, &[], &host(&[("TOKEN", "new"), ("SHELL", "/x")]));
        assert_eq!(env["TOKEN"].value, "new");
        assert_eq!(env["KEEP"].value, "kept");
        assert_eq!(env["SHELL"].value, "/bin/bash");
        assert_eq!(changes, vec![Change::Refreshed("TOKEN".to_owned())]);
    }

    #[test]
    fn refresh_applies_cli_flags_and_reports_them() {
        let baked = resolve(&declared(&[("A", plain())]), &[], &host(&[("A", "1")]));
        let (env, changes) = refresh(
            &baked,
            &[spelled("A", "2"), spelled("NEW", "n")],
            &host(&[]),
        );
        assert_eq!(env["A"].value, "2");
        assert_eq!(env["NEW"].value, "n");
        assert_eq!(
            changes,
            vec![Change::Set("A".to_owned()), Change::Set("NEW".to_owned())]
        );
    }

    #[test]
    fn an_unchanged_attach_says_nothing() {
        let baked = resolve(&declared(&[("A", plain())]), &[], &host(&[("A", "1")]));
        let (env, changes) = refresh(&baked, &[], &host(&[("A", "1")]));
        assert_eq!(env, baked);
        assert_eq!(diff_line(&changes), None);
        // A flag that lands on the value already there changed nothing,
        // and saying otherwise would be a lie on the diff line.
        let (env, changes) = refresh(&baked, &[named("A")], &host(&[("A", "1")]));
        assert_eq!(env, baked);
        assert_eq!(changes, Vec::<Change>::new());
    }

    /// A bare `--env NAME` makes a host-carried variable — later attaches
    /// keep refreshing it — while a spelled one is the CLI's own word.
    #[test]
    fn a_bare_flag_stays_host_carried_a_spelled_one_does_not() {
        let baked = resolve(&declared(&[]), &[], &host(&[]));
        let (env, _) = refresh(
            &baked,
            &[named("A"), spelled("B", "b")],
            &host(&[("A", "a")]),
        );
        assert_eq!(env["A"].source, Source::Host);
        assert_eq!(env["B"].source, Source::Cli);
    }

    /// Refused only when no value exists anywhere: a bare `--env NAME` on
    /// attach is satisfied by the baked value the box already has.
    #[test]
    fn a_bare_flag_is_unfilled_only_with_no_value_anywhere() {
        let baked = resolve(
            &declared(&[("KEPT", plain())]),
            &[],
            &host(&[("KEPT", "v")]),
        );
        let (env, _) = refresh(&baked, &[named("KEPT"), named("GONE")], &host(&[]));
        assert_eq!(
            unfilled_cli(&[named("KEPT"), named("GONE")], &env),
            vec!["GONE"]
        );
    }

    /// A secret's value appears on no screen: not the start line, not the
    /// table. Its name may.
    #[test]
    fn a_secret_value_is_masked_everywhere() {
        let env = resolve(
            &declared(&[(
                "KEY",
                EnvVar {
                    secret: true,
                    ..plain()
                },
            )]),
            &[],
            &host(&[("KEY", "hush")]),
        );
        let start = start_line(&env).expect("has variables");
        assert!(!start.contains("hush"), "{start}");
        assert!(start.contains("KEY•"), "{start}");
        let table = table(&env);
        assert!(!table.contains("hush"), "{table}");
        assert!(table.contains("••••"), "{table}");
    }

    #[test]
    fn the_start_line_counts_set_and_unset_apart() {
        let env = resolve(
            &declared(&[("A", plain()), ("B", plain())]),
            &[],
            &host(&[("A", "1")]),
        );
        assert_eq!(
            start_line(&env).expect("has variables"),
            "env: 1 set (A), 1 unset (B)"
        );
        assert_eq!(start_line(&BakedEnv::new()), None);
    }
}
