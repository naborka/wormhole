//! The broker's decisions: what a request carries on its way out, when a
//! credential must be renewed, and what may never appear in a message.
//!
//! The thesis of the whole design lives here. A box holds no credential
//! and has no route; one host-side process holds the token, injects it,
//! and is the only thing that can reach the API. Everything in this module
//! is the part that must be right *before* a single byte moves — CONCEPT.md
//! §8 assumes the agent is hostile, so the header set, the refresh rule and
//! the redaction rule are decided here, pure, and tested exhaustively.
//!
//! Carrying the bytes — the unix socket, the TLS leg, the streaming relay —
//! is the binary's, in `wormhole::broker`.

use std::fmt;

use serde_json::Value;

/// Headers a request from the box must never carry upstream.
///
/// The dummy key is what the agent was given so its client would start at
/// all; forwarding it would send a credential the account does not own and
/// leak that wormhole is in front. The rest are hop-by-hop or would let a
/// caller in the box choose its own identity upstream.
const STRIPPED: [&str; 5] = [
    "x-api-key",
    "authorization",
    "proxy-authorization",
    "connection",
    "host",
];

/// How close to expiry a token may get before the broker renews it ahead
/// of the request rather than after a rejection.
///
/// Reactive refresh is a fallback and a bad one: once response headers are
/// on the wire a retry is impossible, and the agent retries roughly seven
/// times on a 401 and would burn all of them silently. Renewing early is
/// what keeps that path unused.
pub const REFRESH_MARGIN_SECS: u64 = 300;

/// Where the broker's socket lands inside a box that uses it. Under
/// `/run`, which is a tmpfs the box already has, so nothing about the
/// image has to accommodate it.
pub const SOCKET_IN_BOX: &str = "/run/wormhole/broker.sock";

/// Where the forwarder helper is bound into such a box. Read-only, and
/// carried inside wormhole's own binary — there is nothing for the user
/// to install, which §12 promised. Built static against musl, so it runs
/// in any image regardless of the image's libc.
pub const FORWARD_IN_BOX: &str = "/run/wormhole/forward";

/// What the box's agent is pointed at. Loopback only; with
/// `network = "none"` loopback is the only thing that exists.
pub const IN_BOX_ADDR: &str = "127.0.0.1:8787";

/// The base URL an agent in the box uses instead of the API's own.
pub fn base_url_in_box() -> String {
    format!("http://{IN_BOX_ADDR}")
}

/// What the box's main process becomes when it brokers: the forwarder in
/// the background, then the agent replacing the shell.
///
/// The forwarder is started inside the box rather than bound in from
/// outside because it must listen on the box's loopback, and with
/// `network = "none"` that loopback exists nowhere else.
pub fn forwarder_line() -> String {
    format!("{FORWARD_IN_BOX} {IN_BOX_ADDR} {SOCKET_IN_BOX} &")
}

/// What the broker sends upstream for one request from the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Header name and value, in the order they go on the wire.
    pub headers: Vec<(String, String)>,
}

/// Rewrites a request's headers for the upstream leg: everything the box
/// chose that it may not choose, removed; the real credential, added.
///
/// The token is a parameter and never a field, so it exists in one place
/// for one call and there is nothing holding it to be logged later.
pub fn outgoing(from_box: &[(String, String)], token: &str) -> Outgoing {
    let mut headers: Vec<(String, String)> = from_box
        .iter()
        .filter(|(name, _)| !STRIPPED.contains(&name.to_ascii_lowercase().as_str()))
        .cloned()
        .collect();
    headers.push(("authorization".to_owned(), format!("Bearer {token}")));
    headers.push(("anthropic-beta".to_owned(), "oauth-2025-04-20".to_owned()));
    Outgoing { headers }
}

/// A request's head, as it arrived from the box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    /// The path and query, as written. Never a full URL: the box does not
    /// choose which host its request reaches.
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// How many bytes of body follow, when the box said. `None` means it
    /// did not — a streamed request — and the relay reads until the box
    /// closes its side.
    pub fn content_length(&self) -> Option<usize> {
        self.header("content-length")?.trim().parse().ok()
    }
}

/// Reads the head of an HTTP/1.1 request. Everything after the blank line
/// is the body and is never parsed — the broker relays it, it does not
/// read it.
///
/// A request from the box is data from something assumed hostile, so this
/// refuses anything it does not fully understand rather than guessing:
/// a guess here becomes a request wormhole makes on the account's behalf.
pub fn parse_head(text: &str) -> Result<Head, BrokerError> {
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or(BrokerError::BadRequest)?;
    let mut parts = request_line.split(' ');
    let method = parts
        .next()
        .filter(|m| !m.is_empty())
        .ok_or(BrokerError::BadRequest)?;
    let path = parts
        .next()
        .filter(|p| p.starts_with('/'))
        .ok_or(BrokerError::BadRequest)?;
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(BrokerError::BadRequest);
    }

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(BrokerError::BadRequest)?;
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(BrokerError::BadRequest);
        }
        headers.push((name.to_owned(), value.trim().to_owned()));
    }
    Ok(Head {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
    })
}

/// The upstream URL for a request from the box: the one host wormhole
/// talks to, plus the path the box asked for.
///
/// The box never names the host. That is the entire point — a broker that
/// forwarded to whatever `Host:` header arrived would be an open proxy
/// wearing the account's credential.
pub fn upstream_url(upstream: &str, path: &str) -> String {
    format!("{}{}", upstream.trim_end_matches('/'), path)
}

/// What the broker decided to do about the credential before forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// Good for this request and long enough after it.
    NotYet,
    /// Inside the margin: renew before forwarding, not after a rejection.
    Before,
    /// Already expired. Same action, named apart because a broker that
    /// only ever sees this has a proactive path that is not working.
    Expired,
}

impl Refresh {
    pub fn needed(self) -> bool {
        !matches!(self, Refresh::NotYet)
    }
}

/// Whether the token must be renewed before this request goes out.
///
/// `expires_at` is unix seconds. A credential that names no expiry is
/// treated as needing renewal: guessing that an unbounded token is fine is
/// the assumption that produces a 401 mid-stream, which cannot be retried.
pub fn refresh_before(expires_at: Option<u64>, now: u64) -> Refresh {
    match expires_at {
        None => Refresh::Expired,
        Some(at) if now >= at => Refresh::Expired,
        Some(at) if at.saturating_sub(now) <= REFRESH_MARGIN_SECS => Refresh::Before,
        Some(_) => Refresh::NotYet,
    }
}

/// Whether a 401 from upstream may be retried with a renewed token.
///
/// Only before the first response byte reaches the box. Once headers are
/// on the wire the response has begun and a retry would corrupt it, so a
/// mid-stream auth failure surfaces to the agent — the documented
/// behaviour, asserted rather than left to chance.
pub fn may_retry_after_401(response_bytes_flushed: usize) -> bool {
    response_bytes_flushed == 0
}

/// A credential file, as the agent's own login wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<u64>,
}

/// Reads a credential. The shape checks are not cosmetic: these values are
/// put into request headers, where a newline would end the header early
/// and let a caller append one of its own.
pub fn parse_credential(json: &str) -> Result<Credential, BrokerError> {
    let value: Value = serde_json::from_str(json).map_err(|_| BrokerError::Unreadable)?;
    let oauth = value.get("claudeAiOauth").ok_or(BrokerError::Unreadable)?;
    let field = |name: &str| -> Result<String, BrokerError> {
        let text = oauth
            .get(name)
            .and_then(Value::as_str)
            .ok_or(BrokerError::Unreadable)?;
        if text.is_empty() || text.contains(['\r', '\n', '"', '\\']) {
            return Err(BrokerError::BadShape(name.to_owned()));
        }
        Ok(text.to_owned())
    };
    Ok(Credential {
        access_token: field("accessToken")?,
        refresh_token: field("refreshToken")?,
        // Claude Code writes milliseconds; seconds are what everything
        // else here uses.
        expires_at: oauth
            .get("expiresAt")
            .and_then(Value::as_u64)
            .map(|ms| ms / 1000),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerError {
    Unreadable,
    BadShape(String),
    /// A request the broker will not guess at. Everything from the box is
    /// data from something assumed hostile, and a guess becomes a request
    /// made on the account's behalf.
    BadRequest,
    /// A refresh that failed, with the reason. Distinct from a blanket 401
    /// on purpose: the agent retries a 401 about seven times and would burn
    /// every one of them without ever learning what was wrong.
    RefreshFailed(String),
}

impl fmt::Display for BrokerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrokerError::Unreadable => write!(f, "the credential file cannot be read"),
            BrokerError::BadShape(field) => {
                write!(
                    f,
                    "the credential's {field} is not a shape wormhole will send"
                )
            }
            BrokerError::BadRequest => {
                write!(f, "the box sent a request wormhole will not forward")
            }
            BrokerError::RefreshFailed(why) => {
                write!(f, "the credential could not be renewed: {why}")
            }
        }
    }
}

impl std::error::Error for BrokerError {}

/// Whether this process should perform the refresh, having taken the lock
/// and re-read the file under it.
///
/// Two brokers starting together must produce one refresh, not two: the
/// second takes the lock after the first has written, re-reads, and finds
/// a token that is no longer inside the margin. Without the re-read *under
/// the lock* both would refresh, and the rotating refresh token admits
/// exactly one writer — the second would invalidate the first's.
pub fn still_needs_refresh(under_lock: &Credential, now: u64) -> bool {
    refresh_before(under_lock.expires_at, now).needed()
}

/// Redacts anything that looks like a credential out of a message.
///
/// No log line, error or panic may contain a token. Applied at the edge
/// rather than trusted at every call site, because the guarantee is
/// "never" and one forgotten `format!` would end it.
pub fn redact(message: &str, credential: &Credential) -> String {
    let mut safe = message.to_owned();
    for secret in [&credential.access_token, &credential.refresh_token] {
        if !secret.is_empty() {
            safe = safe.replace(secret.as_str(), "<redacted>");
        }
    }
    safe
}

/// The status line the box's client sees, whatever the upstream leg spoke.
///
/// `curl --include` writes the upstream head verbatim, and its version
/// token names the ALPN of *that* leg — `HTTP/2 200`. The box-side leg is
/// plain HTTP/1.1, and a client refuses a version its own connection never
/// negotiated. Only the version token changes; status and reason move
/// untouched, and a line that is not a status line moves verbatim.
pub fn client_status_line(upstream: &str) -> String {
    match upstream.split_once(' ') {
        Some((version, rest)) if version.starts_with("HTTP/") => format!("HTTP/1.1 {rest}"),
        _ => upstream.to_owned(),
    }
}

/// `PT_INTERP`: the program header that names a dynamic loader.
const PT_INTERP: u32 = 3;

/// Whether an ELF binary asks the image for a dynamic loader.
///
/// The bound-in forwarder runs inside whatever image the box uses. A
/// dynamically linked one names its build host's loader
/// (`/lib64/ld-linux-*` or musl's), which a foreign image does not have,
/// and the exec dies with a bare "not found" that blames the wrong thing.
/// Static (including static-pie) has no `PT_INTERP` and runs anywhere,
/// which is what the §12 "nothing to install" promise actually requires.
/// A test in `wormhole` holds the embedded helper to this.
pub fn requires_loader(elf: &[u8]) -> Result<bool, String> {
    let bad = |what: &str| format!("the forwarder binary is not a readable 64-bit ELF: {what}");
    let u16_at = |at: usize| Some(u16::from_le_bytes([*elf.get(at)?, *elf.get(at + 1)?]));
    if elf.get(..4) != Some(b"\x7fELF".as_slice()) {
        return Err(bad("wrong magic"));
    }
    if elf.get(4) != Some(&2) {
        return Err(bad("not 64-bit"));
    }
    if elf.get(5) != Some(&1) {
        return Err(bad("not little-endian"));
    }
    let phoff = elf
        .get(0x20..0x28)
        .and_then(|b| b.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| bad("truncated header"))?;
    let phentsize = u16_at(0x36).ok_or_else(|| bad("truncated header"))?;
    let phnum = u16_at(0x38).ok_or_else(|| bad("truncated header"))?;
    for i in 0..u64::from(phnum) {
        let at = phoff
            .checked_add(i * u64::from(phentsize))
            .and_then(|at| usize::try_from(at).ok())
            .ok_or_else(|| bad("program headers out of range"))?;
        let p_type = at
            .checked_add(4)
            .and_then(|end| elf.get(at..end))
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| bad("truncated program headers"))?;
        if p_type == PT_INTERP {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn names(out: &Outgoing) -> Vec<String> {
        out.headers
            .iter()
            .map(|(n, _)| n.to_ascii_lowercase())
            .collect()
    }

    /// The box-side leg is plain HTTP/1.1 whatever ALPN the upstream leg
    /// negotiated; a client refuses a version its connection never spoke.
    #[test]
    fn the_status_line_names_the_client_side_protocol_not_the_upstream_one() {
        for line in ["HTTP/2 400\r\n", "HTTP/2.0 400\r\n", "HTTP/3 400\r\n"] {
            assert_eq!(client_status_line(line), "HTTP/1.1 400\r\n");
        }
        assert_eq!(
            client_status_line("HTTP/1.1 200 OK\r\n"),
            "HTTP/1.1 200 OK\r\n"
        );
    }

    /// A line that is not a status line moves untouched: mangling bytes
    /// the broker does not understand is worse than relaying them.
    #[test]
    fn a_line_that_is_not_a_status_line_is_relayed_verbatim() {
        for line in ["", "\r\n", "not a status line\r\n", "HTTPS-ISH 200\r\n"] {
            assert_eq!(client_status_line(line), line);
        }
    }

    /// The dummy key exists so the agent's client starts at all. Sending
    /// it upstream would forward a credential the account does not own.
    #[test]
    fn the_dummy_key_is_stripped_and_never_forwarded() {
        let out = outgoing(
            &headers(&[
                ("x-api-key", "sk-dummy"),
                ("content-type", "application/json"),
            ]),
            "real-token",
        );
        assert!(!names(&out).contains(&"x-api-key".to_owned()), "{out:?}");
        for (_, value) in &out.headers {
            assert!(!value.contains("sk-dummy"), "{out:?}");
        }
        assert!(names(&out).contains(&"content-type".to_owned()));
    }

    /// Header names are case-insensitive on the wire, so a box that spells
    /// one differently must not slip it past the filter.
    #[test]
    fn a_stripped_header_is_stripped_whatever_its_case() {
        let out = outgoing(
            &headers(&[
                ("X-Api-Key", "sk-dummy"),
                ("AUTHORIZATION", "Bearer theirs"),
            ]),
            "real-token",
        );
        assert_eq!(
            out.headers,
            headers(&[
                ("authorization", "Bearer real-token"),
                ("anthropic-beta", "oauth-2025-04-20"),
            ])
        );
    }

    #[test]
    fn the_real_credential_is_injected_exactly_once() {
        let out = outgoing(&headers(&[("authorization", "Bearer theirs")]), "mine");
        let auth: Vec<_> = out
            .headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("authorization"))
            .collect();
        assert_eq!(auth.len(), 1, "{out:?}");
        assert_eq!(auth[0].1, "Bearer mine");
    }

    /// Renewing early is what keeps the un-retryable path unused.
    #[test]
    fn a_token_inside_the_margin_is_renewed_before_the_request() {
        assert_eq!(refresh_before(Some(10_000), 1_000), Refresh::NotYet);
        assert_eq!(
            refresh_before(Some(1_000 + REFRESH_MARGIN_SECS), 1_000),
            Refresh::Before
        );
        assert_eq!(refresh_before(Some(1_000), 1_000), Refresh::Expired);
        assert_eq!(refresh_before(Some(999), 1_000), Refresh::Expired);
        assert!(Refresh::Before.needed());
        assert!(!Refresh::NotYet.needed());
    }

    /// Assuming an unbounded token is fine is what produces the 401 that
    /// arrives mid-stream, where nothing can be done about it.
    #[test]
    fn a_credential_with_no_expiry_is_renewed_rather_than_trusted() {
        assert_eq!(refresh_before(None, 1_000), Refresh::Expired);
    }

    /// Once headers are on the wire the response has begun; a retry would
    /// corrupt it, so the failure reaches the agent instead.
    #[test]
    fn a_401_is_retried_only_before_the_first_byte_reaches_the_box() {
        assert!(may_retry_after_401(0));
        assert!(!may_retry_after_401(1));
    }

    fn credential(json: &str) -> Result<Credential, BrokerError> {
        parse_credential(json)
    }

    #[test]
    fn a_credential_round_trips_with_its_expiry_in_seconds() {
        let parsed = credential(
            r#"{"claudeAiOauth":{"accessToken":"acc","refreshToken":"ref","expiresAt":1786630000000}}"#,
        )
        .expect("valid");
        assert_eq!(parsed.access_token, "acc");
        assert_eq!(parsed.refresh_token, "ref");
        assert_eq!(parsed.expires_at, Some(1_786_630_000));
    }

    /// These values become header values. A newline in one would end the
    /// header early and let whatever follows be read as another header of
    /// the caller's choosing.
    ///
    /// Built through `serde_json` rather than by pasting into a string: a
    /// hand-written `"a\\b"` reaches the parser as a backspace escape, not
    /// as the backslash the test means to reject.
    #[test]
    fn a_token_that_could_forge_a_header_is_refused() {
        for bad in ["a\nb", "a\rb", "a\"b", "a\\b", ""] {
            let json = serde_json::json!({
                "claudeAiOauth": {
                    "accessToken": bad,
                    "refreshToken": "r",
                    "expiresAt": 1000,
                }
            })
            .to_string();
            assert_eq!(
                credential(&json),
                Err(BrokerError::BadShape("accessToken".to_owned())),
                "{bad:?} was accepted"
            );
        }
    }

    /// The same rule on the refresh token: it is what a renewal request
    /// carries, so it can forge a body the same way.
    #[test]
    fn a_refresh_token_that_could_forge_a_request_is_refused() {
        let json = serde_json::json!({
            "claudeAiOauth": { "accessToken": "a", "refreshToken": "r\nx", "expiresAt": 1000 }
        })
        .to_string();
        assert_eq!(
            credential(&json),
            Err(BrokerError::BadShape("refreshToken".to_owned()))
        );
    }

    /// The second broker takes the lock after the first has written and
    /// must find nothing left to do. The rotating refresh token admits one
    /// writer, so a second refresh would invalidate the first's.
    #[test]
    fn a_broker_that_takes_the_lock_second_finds_the_work_already_done() {
        let renewed = Credential {
            access_token: "new".to_owned(),
            refresh_token: "new-ref".to_owned(),
            expires_at: Some(100_000),
        };
        assert!(!still_needs_refresh(&renewed, 1_000));

        let stale = Credential {
            expires_at: Some(1_100),
            ..renewed
        };
        assert!(still_needs_refresh(&stale, 1_000));
    }

    /// The guarantee is "never", so it is enforced at the edge rather than
    /// trusted at every call site.
    #[test]
    fn no_message_can_carry_a_token_out_of_the_broker() {
        let credential = Credential {
            access_token: "sk-ant-secret".to_owned(),
            refresh_token: "rt-secret".to_owned(),
            expires_at: Some(1),
        };
        let leaked = "upstream said 401 for Bearer sk-ant-secret (refresh rt-secret)";
        let safe = redact(leaked, &credential);
        assert!(!safe.contains("sk-ant-secret"), "{safe}");
        assert!(!safe.contains("rt-secret"), "{safe}");
        assert!(safe.contains("<redacted>"), "{safe}");
    }

    #[test]
    fn a_request_head_reads_its_method_path_and_headers() {
        let head = parse_head(
            "POST /v1/messages HTTP/1.1\r\nHost: api\r\nContent-Length: 12\r\n\r\n{\"a\":1}",
        )
        .expect("valid");
        assert_eq!(head.method, "POST");
        assert_eq!(head.path, "/v1/messages");
        assert_eq!(head.header("content-length"), Some("12"));
        // Case-insensitive, because the wire is.
        assert_eq!(head.header("CONTENT-LENGTH"), Some("12"));
        assert_eq!(head.content_length(), Some(12));
    }

    /// A streamed request names no length; the relay reads until the box
    /// closes rather than assuming zero and truncating the body.
    #[test]
    fn a_request_without_a_length_is_not_a_request_of_length_zero() {
        let head = parse_head("GET /v1/models HTTP/1.1\r\n\r\n").expect("valid");
        assert_eq!(head.content_length(), None);
    }

    /// Everything from the box is data from something assumed hostile. A
    /// guess here becomes a request made on the account's credential.
    #[test]
    fn a_request_the_broker_does_not_understand_is_refused_not_guessed_at() {
        for bad in [
            "",
            "GET\r\n\r\n",
            "GET /v1 HTTP/9\r\n\r\n",
            // An absolute URL would let the box choose its own upstream.
            "GET http://elsewhere/v1 HTTP/1.1\r\n\r\n",
            "GET /v1 HTTP/1.1\r\nnot a header\r\n\r\n",
            "GET /v1 HTTP/1.1\r\nbad name: v\r\n\r\n",
        ] {
            assert_eq!(parse_head(bad), Err(BrokerError::BadRequest), "{bad:?}");
        }
    }

    /// The box never names the host. A broker that forwarded to whatever
    /// `Host:` arrived would be an open proxy wearing the credential.
    #[test]
    fn the_upstream_host_is_wormholes_and_never_the_boxs() {
        assert_eq!(
            upstream_url("https://api.anthropic.com/", "/v1/messages"),
            "https://api.anthropic.com/v1/messages"
        );
        let head = parse_head("GET /v1/m HTTP/1.1\r\nHost: evil.test\r\n\r\n").expect("valid");
        let url = upstream_url("https://api.anthropic.com", &head.path);
        assert!(!url.contains("evil.test"), "{url}");
        // And the header itself never reaches upstream.
        assert!(
            !names(&outgoing(&head.headers, "t")).contains(&"host".to_owned()),
            "host header forwarded"
        );
    }

    /// What the box is handed: a loopback URL and a dummy key. Neither is
    /// a credential, which is the whole claim.
    #[test]
    fn the_box_is_pointed_at_loopback_and_given_nothing_worth_stealing() {
        let url = base_url_in_box();
        assert!(url.starts_with("http://127.0.0.1"), "{url}");
        assert!(url.contains(IN_BOX_ADDR), "{url}");
    }

    /// The forwarder runs inside the box because it must listen on the
    /// box's loopback, and with `network = "none"` that loopback exists
    /// nowhere else.
    #[test]
    fn the_forwarder_is_started_in_the_box_against_the_bound_socket() {
        let line = forwarder_line();
        assert!(line.contains(FORWARD_IN_BOX), "{line}");
        assert!(line.contains(IN_BOX_ADDR), "{line}");
        assert!(line.contains(SOCKET_IN_BOX), "{line}");
        assert!(line.ends_with('&'), "the forwarder must not block: {line}");
    }

    /// Both live under `/run`, which is already a tmpfs in every box, so
    /// nothing about an image has to accommodate the broker.
    #[test]
    fn nothing_the_broker_needs_in_the_box_touches_the_image() {
        for path in [SOCKET_IN_BOX, FORWARD_IN_BOX] {
            assert!(path.starts_with("/run/"), "{path}");
        }
    }

    /// A minimal ELF64 whose program headers carry exactly `types`.
    fn elf_with(types: &[u32]) -> Vec<u8> {
        let mut elf = vec![0u8; 0x40];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2; // 64-bit
        elf[5] = 1; // little-endian
        elf[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes()); // e_phoff
        elf[0x36..0x38].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        let n = u16::try_from(types.len()).expect("few headers");
        elf[0x38..0x3a].copy_from_slice(&n.to_le_bytes()); // e_phnum
        for t in types {
            let mut phdr = [0u8; 56];
            phdr[..4].copy_from_slice(&t.to_le_bytes());
            elf.extend_from_slice(&phdr);
        }
        elf
    }

    /// `PT_INTERP` is the loader request; its absence — even with a
    /// `PT_DYNAMIC`, which static-pie keeps — means the binary runs in
    /// any image.
    #[test]
    fn a_binary_without_pt_interp_needs_no_loader_from_the_image() {
        assert_eq!(requires_loader(&elf_with(&[1, 2, 6])), Ok(false));
        assert_eq!(requires_loader(&elf_with(&[1, 3, 2])), Ok(true));
        assert_eq!(requires_loader(&elf_with(&[])), Ok(false));
    }

    /// Garbage is an error naming the binary, never a silent "static".
    /// Guessing "static" from unreadable bytes would wave a broken binary
    /// into the box.
    #[test]
    fn an_unreadable_binary_is_an_error_and_never_passes_as_static() {
        for bad in [
            &b""[..],
            &b"\x7fELF"[..],
            &b"not an elf at all, just text"[..],
        ] {
            let err = requires_loader(bad).expect_err("must refuse");
            assert!(err.contains("forwarder binary"), "{err}");
        }
        let mut truncated = elf_with(&[1, 3]);
        truncated.truncate(0x7a); // second phdr promised, its p_type cut off
        assert!(requires_loader(&truncated).is_err());
    }

    /// A refresh failure must say what went wrong. A blanket 401 gets
    /// retried about seven times by the agent and teaches it nothing.
    #[test]
    fn a_failed_refresh_is_its_own_actionable_error() {
        let err = BrokerError::RefreshFailed("upstream returned 400".to_owned());
        assert!(err.to_string().contains("could not be renewed"), "{err}");
        assert!(err.to_string().contains("upstream returned 400"), "{err}");
    }
}
