//! The account's usage windows: fetched on the host, cached in one file,
//! and fed into every running box's home for the agent's status line.
//!
//! The host credential never enters a box, so the fetch belongs here and
//! not in one. It is also never put on a command line — argv is
//! world-readable on this host — so `curl` is handed its options on stdin.
//!
//! The endpoint is the one Claude Code's own `/usage` uses. It is
//! undocumented: it can change or disappear without notice, and it rate
//! limits an unrecognised `User-Agent` hard. Every failure here degrades to
//! the last reading with its age shown, never to a wrong number.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use wormhole_core::limits::{self, Limits};
use wormhole_core::paths;

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";

/// Claude Code's own shape. A `User-Agent` the endpoint does not recognise
/// lands in an aggressively rate-limited bucket, so this is not cosmetic.
const USER_AGENT: &str = "claude-code/2.1.228";

/// How often a running box gets a fresh reading. The windows move slowly
/// and the endpoint rate limits, so a minute is the floor.
const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// Where the host's OAuth credential lives, relative to the host home.
/// Read here and nowhere else.
const CREDENTIAL: &str = ".claude/.credentials.json";

/// The rendered line inside the box home; the status line only reads it.
pub const LIMITS_FILE: &str = ".claude/wormhole-limits";

/// Keeps `<box home>/.claude/wormhole-limits` fresh for the box's whole
/// life: read the shared reading, fetch only when it is past its interval,
/// render, write, sleep. Spawned as a thread by `wormhole box` and gone
/// with it. Every failure leaves the last line in place — its age is baked
/// into the text, so it goes stale visibly, not wrongly.
///
/// The fetch is gated on the *shared* reading's age, not on this thread's
/// own sleep: the windows belong to the account, so a host running a group
/// of six boxes asks the rate-limited endpoint once per interval rather
/// than six times.
pub fn feed_box(data_home: &Path, host_home: &Path, box_home: &Path) {
    loop {
        let mut reading = cached(data_home);
        if limits::needs_refresh(reading.as_ref(), crate::now_unix(), REFRESH_EVERY.as_secs())
            && let Some(fresh) = poll_once(data_home, host_home)
        {
            reading = Some(fresh);
        }
        let line = limits::render_short(reading.as_ref(), crate::now_unix());
        let _ = crate::replace_file(&box_home.join(LIMITS_FILE), &line);
        std::thread::sleep(REFRESH_EVERY);
    }
}

/// One poll of the endpoint, at most one per host at a time.
///
/// Age alone was not enough. `needs_refresh` reads the shared cache and
/// then acts on it, and between those two a peer can do the same: a group
/// of six boxes starts in one instant, wakes in one instant, and all six
/// see the same stale reading and all six ask. On the very first tick
/// there is no cache at all, so all six ask however old anything is. That
/// is the herd the gate was meant to stop, against an endpoint whose own
/// docstring says it rate limits hard.
///
/// The lock closes it: whoever takes it does the asking, and everyone else
/// re-reads the cache the winner just wrote. A box that loses is not an
/// error — it is a box that did not need to ask.
fn poll_once(data_home: &Path, host_home: &Path) -> Option<Limits> {
    let lock = paths::usage_lock(data_home);
    let held = match crate::try_lock(&lock) {
        Ok(Some(held)) => held,
        // Someone else is asking right now; their answer is ours too.
        Ok(None) => return cached(data_home),
        Err(_) => return None,
    };
    let fresh = refresh(data_home, host_home).ok();
    drop(held);
    fresh
}

/// The last reading, aged by the cache file's own timestamp. A file that
/// cannot be read or parsed is no reading at all, which the panel says.
pub fn cached(data_home: &Path) -> Option<Limits> {
    let file = paths::usage_file(data_home);
    let text = std::fs::read_to_string(&file).ok()?;
    let observed_at = std::fs::metadata(&file)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    limits::parse(&text, observed_at).ok()
}

/// Fetches a reading and caches it. The reply is parsed before it is
/// stored, so a body wormhole cannot read never replaces a good one.
pub fn refresh(data_home: &Path, home: &Path) -> Result<Limits, String> {
    let reply = fetch(&access_token(home)?)?;
    let limits = limits::parse(&reply, crate::now_unix())?;
    store(data_home, &reply)?;
    Ok(limits)
}

/// The subscription's access token, from the credential file the host's
/// own agent login wrote. The file is named here; what counts as a usable
/// token is decided in the core.
fn access_token(home: &Path) -> Result<String, String> {
    let file = home.join(CREDENTIAL);
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    limits::token_from_credential(&text).map_err(|e| format!("{e} ({})", file.display()))
}

/// Asks the endpoint, with the token on stdin rather than in argv.
fn fetch(token: &str) -> Result<String, String> {
    let config = format!(
        concat!(
            "url = \"{endpoint}\"\n",
            "header = \"Authorization: Bearer {token}\"\n",
            "header = \"anthropic-beta: oauth-2025-04-20\"\n",
            "header = \"Content-Type: application/json\"\n",
            "user-agent = \"{agent}\"\n",
            "fail\nsilent\nshow-error\nlocation\nmax-time = 10\n",
        ),
        endpoint = ENDPOINT,
        token = token,
        agent = USER_AGENT,
    );
    let mut child = Command::new("curl")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot ask for the usage limits: {e}; is curl installed?"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "cannot hand curl its options".to_owned())?
        .write_all(config.as_bytes())
        .map_err(|e| format!("cannot hand curl its options: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("cannot ask for the usage limits: {e}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "cannot ask for the usage limits: {}",
            detail.trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("the usage reply is not text: {e}"))
}

/// Replaces the cache in one step: a half-written file must never be read
/// as a reading, and two hosts' polls must not interleave into one body.
fn store(data_home: &Path, reply: &str) -> Result<(), String> {
    crate::replace_file(&paths::usage_file(data_home), reply)
}
