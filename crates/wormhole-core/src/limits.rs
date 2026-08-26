//! What is left of the account's usage windows, and the one line the panel
//! shows for them. Parsing and rendering only; fetching lives in the
//! binary.
//!
//! The windows are the *account's*, not this box's: every box on this host
//! draws from the same session and weekly allowance, so the line says
//! `account` and never `this box`.

use serde_json::Value;

/// A reading older than this is shown with its age. A window only changes
/// when a request is made, so a quiet host holds a true-but-old number —
/// and an old number presented as current is the one failure that would
/// make the panel worse than no panel.
const STALE_AFTER: u64 = 120;

/// One usage window: how much of it is gone, and when it starts over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// What the user calls it: `session`, `weekly`, `weekly Fable`.
    pub label: String,
    pub percent: u16,
    /// Unix seconds, absent when the reply gives no reset for a window.
    pub resets_at: Option<u64>,
}

/// Every window in one reading, with the time it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub windows: Vec<Window>,
    pub observed_at: u64,
}

/// Reads a usage reply. `limits` is preferred because it carries every
/// window in one shape — including the scoped weekly one that binds first
/// and that the top-level `five_hour`/`seven_day` pair does not mention.
pub fn parse(json: &str, observed_at: u64) -> Result<Limits, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| format!("cannot read the usage reply: {e}"))?;
    let mut windows = from_limits(&value);
    if windows.is_empty() {
        windows = from_pair(&value);
    }
    if windows.is_empty() {
        return Err("the usage reply names no windows".to_owned());
    }
    windows.sort_by(|a, b| {
        rank(&a.label)
            .cmp(&rank(&b.label))
            .then(b.percent.cmp(&a.percent))
            .then(a.label.cmp(&b.label))
    });
    Ok(Limits {
        windows,
        observed_at,
    })
}

/// The subscription token that may ask for these windows, out of the
/// credential file the host's own agent login wrote.
///
/// The shape check is not cosmetic: the token is handed to `curl` inside a
/// config file, where a quote, a backslash or a newline would end the value
/// early and change the request that carries it.
pub fn token_from_credential(json: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| format!("cannot read the credential: {e}"))?;
    let token = value
        .get("claudeAiOauth")
        .and_then(|oauth| oauth.get("accessToken"))
        .and_then(Value::as_str)
        .ok_or_else(|| "the credential holds no subscription token".to_owned())?;
    if token.is_empty() || token.contains(['"', '\\', '\n', '\r']) {
        return Err("the subscription token is not a shape wormhole will send".to_owned());
    }
    Ok(token.to_owned())
}

/// Whether a reading is old enough to be worth asking for again. The
/// windows belong to the account, so every box on the host draws the same
/// numbers from one cache file: without this the endpoint — which rate
/// limits hard — would be asked once per box per interval instead of once
/// per host. A reading from the future is a clock that moved, not a fresh
/// reason to ask.
pub fn needs_refresh(cached: Option<&Limits>, now: u64, every: u64) -> bool {
    match cached {
        None => true,
        Some(limits) => now
            .checked_sub(limits.observed_at)
            .is_some_and(|age| age >= every),
    }
}

/// The status line under a running conversation: every window at a
/// glance, nothing else. `SESSION: 13%  FABLE: 78%  WEEKLY: 47%` — resets
/// live in `render`, which `wormhole usage` shows when asked. A stale
/// reading still says its age: that one suffix is never traded for looks.
pub fn render_short(limits: Option<&Limits>, now: u64) -> String {
    let Some(limits) = limits else {
        return "LIMITS: unknown".to_owned();
    };
    let mut line = limits
        .windows
        .iter()
        .map(|window| format!("{}: {}%", short_label(&window.label), window.percent))
        .collect::<Vec<_>>()
        .join("  ");
    if now.saturating_sub(limits.observed_at) > STALE_AFTER {
        line.push_str(&format!("  ({} ago)", elapsed(now - limits.observed_at)));
    }
    line
}

/// One word per window, capitals so the eye can skip the numbers it does
/// not need. A scoped weekly window goes by its model's name alone —
/// `FABLE` says more in one word than `WEEKLY FABLE` in two.
fn short_label(label: &str) -> String {
    match label.strip_prefix("weekly ") {
        Some(model) => model.to_uppercase(),
        None => label.to_uppercase(),
    }
}

/// The panel's line. `None` reads as unknown rather than as zero — a
/// missing reading must never look like an empty window.
pub fn render(limits: Option<&Limits>, now: u64) -> String {
    let Some(limits) = limits else {
        return "account limits: unknown".to_owned();
    };
    let mut line = String::from("account limits:");
    for (index, window) in limits.windows.iter().enumerate() {
        if index > 0 {
            line.push_str(" ·");
        }
        line.push_str(&format!(" {} {}%", window.label, window.percent));
        if let Some(at) = window.resets_at {
            line.push_str(&format!(" resets {}", until(now, at)));
        }
    }
    if now.saturating_sub(limits.observed_at) > STALE_AFTER {
        line.push_str(&format!(" ({} ago)", elapsed(now - limits.observed_at)));
    }
    line
}

/// Session before weekly, then the fullest first: the window that binds
/// soonest reads first.
fn rank(label: &str) -> u8 {
    match label {
        "session" => 0,
        _ if label.starts_with("weekly") => 1,
        _ => 2,
    }
}

fn from_limits(value: &Value) -> Vec<Window> {
    let Some(entries) = value.get("limits").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            Some(Window {
                label: label(entry.get("kind")?.as_str()?, entry.get("scope")),
                percent: u16::try_from(entry.get("percent")?.as_u64()?).ok()?,
                resets_at: entry
                    .get("resets_at")
                    .and_then(Value::as_str)
                    .and_then(unix_from_iso),
            })
        })
        .collect()
}

/// The older shape, kept as a fallback: if `limits` ever goes away the
/// panel degrades to two windows rather than to nothing.
fn from_pair(value: &Value) -> Vec<Window> {
    [("five_hour", "session"), ("seven_day", "weekly")]
        .iter()
        .filter_map(|(field, label)| {
            let window = value.get(field)?;
            Some(Window {
                label: (*label).to_owned(),
                percent: window.get("utilization")?.as_f64()?.round() as u16,
                resets_at: window
                    .get("resets_at")
                    .and_then(Value::as_str)
                    .and_then(unix_from_iso),
            })
        })
        .collect()
}

/// The window's name in the user's words. An unknown kind is shown as
/// itself: a limit wormhole cannot name must still be visible.
fn label(kind: &str, scope: Option<&Value>) -> String {
    let model = scope
        .and_then(|scope| scope.get("model"))
        .and_then(|model| model.get("display_name"))
        .and_then(Value::as_str);
    match (kind, model) {
        ("session", _) => "session".to_owned(),
        ("weekly_all", _) => "weekly".to_owned(),
        ("weekly_scoped", Some(model)) => format!("weekly {model}"),
        ("weekly_scoped", None) => "weekly scoped".to_owned(),
        (other, _) => other.to_owned(),
    }
}

/// How long until a reset, coarsely — a countdown to the minute would
/// redraw the panel every second for no gain.
fn until(now: u64, at: u64) -> String {
    if at <= now {
        return "now".to_owned();
    }
    elapsed(at - now)
}

fn elapsed(secs: u64) -> String {
    let (days, hours, minutes) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if days > 0 {
        format!("{days}d{hours:02}h")
    } else if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

/// `2026-08-12T21:49:59.917043+00:00` to unix seconds. Written here rather
/// than taken from a date crate because the core stays dependency-thin,
/// and because this is the only date shape wormhole ever reads.
fn unix_from_iso(text: &str) -> Option<u64> {
    let field = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (field(0..4)?, field(5..7)?, field(8..10)?);
    let (hour, minute, second) = (field(11..13)?, field(14..16)?, field(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second
        - offset_seconds(text.get(19..)?)?;
    u64::try_from(seconds).ok()
}

/// The fractional seconds and zone that follow `HH:MM:SS`, as an offset in
/// seconds. `Z`, `+HH:MM` and a bare end all mean UTC-relative.
fn offset_seconds(tail: &str) -> Option<i64> {
    let tail = match tail.strip_prefix('.') {
        Some(fraction) => {
            let end = fraction
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(fraction.len());
            &fraction[end..]
        }
        None => tail,
    };
    match tail.chars().next() {
        None | Some('Z' | 'z') => Some(0),
        Some(sign) if sign == '+' || sign == '-' => {
            let hours = tail.get(1..3)?.parse::<i64>().ok()?;
            let minutes = tail.get(4..6)?.parse::<i64>().ok()?;
            let magnitude = hours * 3600 + minutes * 60;
            Some(if sign == '-' { -magnitude } else { magnitude })
        }
        Some(_) => None,
    }
}

/// Days from 1970-01-01 to a civil date, by Howard Hinnant's algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a live reply, keeping every field the parser reads.
    const REPLY: &str = r#"{
        "five_hour": {"utilization": 13.0, "resets_at": "2026-08-12T21:49:59.917043+00:00"},
        "seven_day": {"utilization": 47.0, "resets_at": "2026-08-16T02:59:59.917069+00:00"},
        "limits": [
          {"kind": "session", "group": "session", "percent": 13, "severity": "normal",
           "resets_at": "2026-08-12T21:49:59.917043+00:00", "scope": null, "is_active": false},
          {"kind": "weekly_all", "group": "weekly", "percent": 47, "severity": "normal",
           "resets_at": "2026-08-16T02:59:59.917069+00:00", "scope": null, "is_active": false},
          {"kind": "weekly_scoped", "group": "weekly", "percent": 78, "severity": "warning",
           "resets_at": "2026-08-16T02:59:59.917343+00:00",
           "scope": {"model": {"id": null, "display_name": "Fable"}}, "is_active": true}
        ]
    }"#;

    /// 2026-08-12T21:49:59+00:00
    const RESET: u64 = 1786571399;

    /// The scoped weekly window is the one that binds first here. A reading
    /// that dropped it would show 47% while the account is at 78%.
    #[test]
    fn every_window_in_the_reply_is_read() {
        let limits = parse(REPLY, 0).expect("a live reply parses");
        let seen: Vec<(&str, u16)> = limits
            .windows
            .iter()
            .map(|w| (w.label.as_str(), w.percent))
            .collect();
        assert_eq!(
            seen,
            vec![("session", 13), ("weekly Fable", 78), ("weekly", 47)]
        );
    }

    /// If `limits` ever goes away the panel must degrade to two windows,
    /// not to nothing.
    #[test]
    fn the_older_two_field_shape_still_parses() {
        let without = REPLY.replace("\"limits\"", "\"limits_renamed\"");
        let limits = parse(&without, 0).expect("the fallback shape parses");
        let seen: Vec<&str> = limits.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(seen, vec!["session", "weekly"]);
        assert_eq!(limits.windows[0].percent, 13);
    }

    #[test]
    fn the_subscription_token_is_read_out_of_the_credential() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abc","expiresAt":1}}"#;
        assert_eq!(
            token_from_credential(json).expect("a token"),
            "sk-ant-oat01-abc"
        );
    }

    /// A token wormhole cannot send safely is refused here, not quoted into
    /// a request that would then carry something else.
    #[test]
    fn a_token_that_could_break_out_of_the_request_is_refused() {
        for bad in [
            r#"{}"#,
            r#"{"claudeAiOauth":{}}"#,
            r#"{"claudeAiOauth":{"accessToken":""}}"#,
            r#"{"claudeAiOauth":{"accessToken":"a\"b"}}"#,
            r#"{"claudeAiOauth":{"accessToken":"a\\b"}}"#,
            r#"{"claudeAiOauth":{"accessToken":"a\nheader = \"x\""}}"#,
            "not json",
        ] {
            assert!(token_from_credential(bad).is_err(), "{bad} must be refused");
        }
    }

    /// One account, one set of windows, one cache — so a host running six
    /// boxes must still ask once per interval, not six times.
    #[test]
    fn a_reading_is_asked_for_again_only_once_its_interval_is_up() {
        let limits = parse(REPLY, RESET).expect("parses");
        assert!(needs_refresh(None, 0, 60), "no reading must be fetched");
        assert!(!needs_refresh(Some(&limits), RESET, 60));
        assert!(!needs_refresh(Some(&limits), RESET + 59, 60));
        assert!(needs_refresh(Some(&limits), RESET + 60, 60));
    }

    /// A clock that moved backwards must not turn the feed into a fetch
    /// loop against a rate-limited endpoint.
    #[test]
    fn a_reading_from_the_future_is_not_a_reason_to_ask_again() {
        let limits = parse(REPLY, RESET).expect("parses");
        assert!(!needs_refresh(Some(&limits), RESET - 1, 60));
        assert!(!needs_refresh(Some(&limits), 0, 60));
    }

    #[test]
    fn a_reply_naming_no_windows_is_an_error() {
        assert!(parse("{}", 0).is_err());
        assert!(parse("{\"limits\": []}", 0).is_err());
        assert!(parse("not json", 0).is_err());
    }

    /// A limit wormhole cannot name must still be visible.
    #[test]
    fn an_unknown_kind_is_shown_as_itself() {
        let json = r#"{"limits":[{"kind":"nimbus_quill","percent":5}]}"#;
        let limits = parse(json, 0).expect("parses");
        assert_eq!(limits.windows[0].label, "nimbus_quill");
        assert_eq!(limits.windows[0].resets_at, None);
    }

    #[test]
    fn the_line_names_each_window_its_share_and_its_reset() {
        let limits = parse(REPLY, RESET).expect("parses");
        let line = render(Some(&limits), RESET);
        assert_eq!(
            line,
            "account limits: session 13% resets now · weekly Fable 78% resets 3d05h · weekly 47% resets 3d05h"
        );
    }

    /// The conversation's line: one word and one number per window.
    #[test]
    fn the_short_line_is_labels_and_percentages_only() {
        let limits = parse(REPLY, RESET).expect("parses");
        assert_eq!(
            render_short(Some(&limits), RESET),
            "SESSION: 13%  FABLE: 78%  WEEKLY: 47%"
        );
    }

    /// Brevity never buys silence about age or absence.
    #[test]
    fn the_short_line_still_says_stale_and_unknown() {
        let limits = parse(REPLY, RESET).expect("parses");
        let stale = render_short(Some(&limits), RESET + 600);
        assert!(stale.ends_with("(10m ago)"), "{stale}");
        assert_eq!(render_short(None, 0), "LIMITS: unknown");
    }

    /// An old number presented as current is worse than no number.
    #[test]
    fn a_stale_reading_shows_its_age() {
        let limits = parse(REPLY, RESET).expect("parses");
        assert!(!render(Some(&limits), RESET + STALE_AFTER).contains("ago"));
        let stale = render(Some(&limits), RESET + 600);
        assert!(stale.contains("(10m ago)"), "{stale}");
    }

    /// No reading must read as unknown, never as an empty window.
    #[test]
    fn no_reading_reads_as_unknown() {
        assert_eq!(render(None, 0), "account limits: unknown");
    }

    #[test]
    fn a_reset_in_the_past_reads_as_now() {
        assert_eq!(until(RESET + 1, RESET), "now");
        assert_eq!(until(RESET, RESET), "now");
    }

    #[test]
    fn a_countdown_is_coarse_and_ordered() {
        assert_eq!(until(0, 90), "1m");
        assert_eq!(until(0, 3 * 3600 + 4 * 60), "3h04m");
        assert_eq!(until(0, 3 * 86400 + 5 * 3600), "3d05h");
    }

    #[test]
    fn an_iso_timestamp_becomes_unix_seconds() {
        assert_eq!(unix_from_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            unix_from_iso("2026-08-12T21:49:59.917043+00:00"),
            Some(RESET)
        );
        assert_eq!(unix_from_iso("2026-08-12T21:49:59"), Some(RESET));
        // A zone is honoured, not ignored.
        assert_eq!(
            unix_from_iso("2026-08-12T23:49:59+02:00"),
            Some(RESET),
            "a +02:00 stamp is two hours earlier in unix time"
        );
        assert_eq!(unix_from_iso("2026-08-12T19:49:59-02:00"), Some(RESET));
    }

    #[test]
    fn a_timestamp_wormhole_cannot_read_is_absent_not_wrong() {
        for bad in [
            "",
            "2026-08-12",
            "2026-13-12T00:00:00Z",
            "2026-08-32T00:00:00Z",
            "2026-08-12T25:00:00Z",
            "2026-08-12T00:00:00 CEST",
            "1969-12-31T23:59:59Z",
            "not-a-date-at-all",
        ] {
            assert_eq!(unix_from_iso(bad), None, "{bad:?} must not parse");
        }
    }
}
