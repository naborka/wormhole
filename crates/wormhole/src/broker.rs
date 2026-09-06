//! The broker, carrying bytes.
//!
//! This is the host half: `serve` on a unix socket holds the token, renews
//! it, injects it, and is the only thing here that can reach the API. The
//! socket is bind-mounted into the box, so the box can speak to it without
//! a route to anywhere. The box half — the loopback forwarder — is the
//! `wormhole-forward` crate, a static musl binary carried inside this one
//! and written into every brokered box.
//!
//! The upstream leg is `curl`, the same choice `usage.rs` already makes:
//! the host's CA store, proxy settings and TLS are the host's business, and
//! wormhole owning a TLS stack would be a large thing to get wrong for no
//! gain. `curl` streams, which is the property that matters — a buffering
//! implementation would break every token of a streamed reply.
//!
//! Two legs, two scopes. The `CONNECT` tunnel is provider-neutral and
//! serves every agent. The credential-injection leg — this upstream, the
//! `claudeAiOauth` file, the OAuth beta header, the refresh endpoint — is
//! the Anthropic adapter, and only an agent whose registry entry names a
//! `brokered_api` is pointed at it (`manifest::declarations`). An agent
//! for another provider tunnels to its API with its own credential; a
//! second injection adapter would slot in beside this one, not into it.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use wormhole_core::broker::{self, BrokerError, Credential};

/// The one host wormhole talks to with the credential. Everything else a
/// box reaches goes through the `CONNECT` leg and its allowlist.
const UPSTREAM: &str = "https://api.anthropic.com";

/// How a broker knows what a tunnel may reach. A fixed list serves the
/// hand-run form; a file is what a box's broker gets, re-read on every
/// tunnel so `wormhole allow` acts without anything restarting.
pub struct Egress {
    fixed: Vec<String>,
    file: Option<PathBuf>,
    /// What the refusal calls this box, so a 403 can name the exact
    /// `wormhole allow` line that unblocks it.
    box_name: Option<String>,
}

impl Egress {
    /// Whether the allowlist admits this host, as of this moment. The
    /// fixed part answers without touching the disk; only a miss re-reads
    /// the live file, which is where an `allow` typed after this broker
    /// started shows up. A file that cannot be read is an empty
    /// contribution, never a crash — refusing extra hosts is the safe
    /// direction to fail in.
    fn allows(&self, host: &str) -> bool {
        if broker::egress_allows(&self.fixed, host) {
            return true;
        }
        let Some(file) = &self.file else {
            return false;
        };
        std::fs::read_to_string(file)
            .is_ok_and(|text| broker::egress_allows(&broker::parse_egress_file(&text), host))
    }
}

/// `wormhole broker [--socket PATH] [--egress H1,..] [--egress-file PATH]
/// [--name NAME]`: the standalone form, for running one by hand. A box
/// spawns its own with `--egress-file` and `--name`; this one defaults to
/// the shared path and an empty list.
pub fn broker_cmd(args: &[String]) -> ! {
    let parsed = broker::parse_broker_args(args).unwrap_or_else(|e| crate::usage(&e));
    let socket = parsed
        .socket
        .map_or_else(|| wormhole_core::paths::broker_socket(&crate::data_home()), PathBuf::from);
    let egress = Egress {
        fixed: parsed.fixed,
        file: parsed.file.map(PathBuf::from),
        box_name: parsed.box_name,
    };
    serve(
        &socket,
        &crate::host_home().join(".claude/.credentials.json"),
        egress,
    )
}

/// Host side: serve the unix socket until killed, one thread per
/// connection — a `CONNECT` tunnel lives as long as the TLS session in
/// it, and the agent's next API request must not queue behind it. The
/// refresh path stays single-writer under its own `flock`.
pub fn serve(socket: &Path, credential_file: &Path, egress: Egress) -> ! {
    if let Some(parent) = socket.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // A socket left by a killed broker is not a broker.
    let _ = std::fs::remove_file(socket);
    let listener = match at_socket(socket, |name| UnixListener::bind(name)) {
        Ok(listener) => listener,
        Err(e) => crate::fail(&format!("cannot listen on {}: {e}", socket.display())),
    };
    println!("broker: listening on {}", socket.display());
    let egress: Arc<Egress> = Arc::new(egress);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let credential_file = credential_file.to_owned();
                let egress = Arc::clone(&egress);
                std::thread::spawn(move || {
                    if let Err(e) = relay(stream, &credential_file, &egress) {
                        // Redaction happens inside `relay`, where the
                        // credential is in scope; anything reaching here
                        // is already safe to print.
                        eprintln!("broker: {e}");
                    }
                });
            }
            Err(e) => eprintln!("broker: cannot accept: {e}"),
        }
    }
    crate::fail("broker: the listener ended")
}

/// Reaches a socket without caring how deep its directory is.
///
/// A unix socket address is a fixed-size field — about 108 bytes — and a
/// data home nested a few directories further than usual overruns it, on
/// both ends: `bind` on the host and `connect` from the box. The kernel
/// resolves a *relative* address against the process's working directory,
/// so standing in the socket's own directory and naming it bare puts only
/// `broker.sock` in that field, and the limit stops being reachable.
///
/// One body for both ends, because a limit fixed on one side and not the
/// other is a failure that only shows up on somebody's deeper home.
///
/// Both callers are single-threaded at this point: the accept loop has not
/// spawned its relay thread yet, and those threads only copy between fds
/// that are already open. Nothing else here resolves a relative path.
fn at_socket<T>(
    socket: &Path,
    act: impl FnOnce(&Path) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let (Some(dir), Some(name)) = (socket.parent(), socket.file_name()) else {
        return act(socket);
    };
    let here = std::env::current_dir()?;
    std::env::set_current_dir(dir)?;
    let done = act(Path::new(name));
    // Back out whatever happened, so an error path leaves no surprise cwd.
    std::env::set_current_dir(here)?;
    done
}

/// One request from the box: read its head, then either open the tunnel
/// a `CONNECT` asks for, or renew the credential if it is close to
/// expiry, inject it, and stream the API reply straight back.
fn relay(stream: UnixStream, credential_file: &Path, egress: &Egress) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut raw = Vec::new();
    // Up to the blank line and not one byte further: everything after it
    // is the body, which the broker moves and never reads.
    loop {
        let mut line = Vec::new();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|e| format!("cannot read the request: {e}"))?;
        if read == 0 {
            return Ok(()); // the box hung up before finishing
        }
        let blank = line == b"\r\n" || line == b"\n";
        raw.extend_from_slice(&line);
        if blank {
            break;
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let head = broker::parse_head(&text).map_err(|e| e.to_string())?;
    if let Some(authority) = head.authority() {
        return tunnel(stream, reader, authority, egress);
    }

    let mut body = Vec::new();
    if let Some(length) = head.content_length() {
        body.resize(length, 0);
        reader
            .read_exact(&mut body)
            .map_err(|e| format!("cannot read the request body: {e}"))?;
    }

    let mut stream = stream;
    let credential = match fresh_credential(credential_file) {
        Ok(credential) => credential,
        // A real reply, not a dropped connection: the agent must see why
        // and where the fix is (the host), rather than retry a 401 seven
        // times or offer a `/login` the box cannot complete.
        Err(why) => {
            refuse(&mut stream, &broker::refresh_refused(&why))?;
            return Err(why);
        }
    };
    let outgoing = broker::outgoing(&head.headers, &credential.access_token);
    let url = broker::upstream_url(UPSTREAM, head.path().ok_or("no path in the request")?);

    let mut curl = Command::new("curl");
    curl.args(["--silent", "--show-error", "--no-buffer"])
        .args(["--request", &head.method])
        .arg("--include") // status line and headers, so the box sees a real reply
        .arg(&url);
    for (name, value) in &outgoing.headers {
        curl.args(["--header", &format!("{name}: {value}")]);
    }
    if !body.is_empty() {
        curl.args(["--data-binary", "@-"]);
    }

    let mut child = curl
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot reach upstream: {e}; is curl installed?"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&body);
    }

    // Streamed, not collected: the first chunk must reach the agent before
    // upstream has finished writing, or every streamed reply arrives as one
    // silent pause followed by a wall of text.
    let mut upstream = BufReader::new(child.stdout.take().ok_or("upstream produced no output")?);
    let mut out = stream;
    // The one line curl writes in the upstream leg's voice: its `--include`
    // head opens with that leg's ALPN version, `HTTP/2 200`, and the box's
    // HTTP/1.1 client refuses it. Rewritten before anything streams; the
    // decision is `client_status_line`, tested in wormhole-core.
    let mut status = Vec::new();
    upstream
        .read_until(b'\n', &mut status)
        .map_err(|e| format!("upstream read failed: {e}"))?;
    let status = match std::str::from_utf8(&status) {
        Ok(line) => broker::client_status_line(line).into_bytes(),
        Err(_) => status, // not a status line; relayed verbatim
    };
    if out.write_all(&status).is_err() {
        // The box hung up; nothing to report, but the child is still reaped.
        let _ = child.wait();
        return Ok(());
    }
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = match upstream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) => return Err(format!("upstream read failed: {e}")),
        };
        if out.write_all(&buffer[..read]).is_err() {
            break; // the box hung up; nothing to report
        }
        let _ = out.flush();
    }
    let finished = child.wait().map_err(|e| e.to_string())?;
    if !finished.success() {
        let mut why = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut why);
        }
        // The one place a message could carry the token; it cannot leave
        // here without going through the redactor.
        return Err(broker::redact(
            &format!("upstream failed: {}", why.trim()),
            &credential,
        ));
    }
    Ok(())
}

/// The `CONNECT` leg: judge the target against the allowlist, dial it
/// host-side, say `200 Connection established`, then move bytes both
/// ways until either side closes. The broker never sees inside the
/// tunnel — the TLS in it is the client's and the host's business.
///
/// A refusal is a real HTTP reply naming the host and the manifest line
/// that would allow it; a blocked host must never look like a network
/// failure.
fn tunnel(
    stream: UnixStream,
    mut from_box: BufReader<UnixStream>,
    target: &str,
    egress: &Egress,
) -> Result<(), String> {
    let mut out = stream;
    let name = egress.box_name.as_deref();
    let (host, port) = match broker::connect_target(target) {
        Ok(target) => target,
        Err(why) => return refuse(&mut out, &broker::connect_refused(target, &why, name)),
    };
    // The live file is read per tunnel, so an `allow` typed on the host
    // is already in force here — no restart, of anything.
    if !egress.allows(&host) {
        return refuse(
            &mut out,
            &broker::connect_refused(&host, "not in this box's egress allowlist", name),
        );
    }
    let upstream = match std::net::TcpStream::connect((host.as_str(), port)) {
        Ok(upstream) => upstream,
        Err(e) => return refuse(&mut out, &broker::connect_failed(&host, &e.to_string())),
    };
    out.write_all(broker::CONNECT_ESTABLISHED.as_bytes())
        .map_err(|e| format!("cannot answer the CONNECT: {e}"))?;
    // Both directions at once, same shape as the in-box forwarder: a
    // reply that begins before the request finishes uploading is normal.
    let mut up_in = upstream
        .try_clone()
        .map_err(|e| format!("cannot split the tunnel: {e}"))?;
    let mut up_out = upstream;
    let into_upstream = std::thread::spawn(move || {
        // The reader first: it may hold bytes the client sent right
        // behind its CONNECT, and they belong at the front.
        let _ = std::io::copy(&mut from_box, &mut up_out);
        let _ = up_out.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut up_in, &mut out);
    let _ = out.shutdown(std::net::Shutdown::Write);
    let _ = into_upstream.join();
    Ok(())
}

/// A whole policy reply written and done. Best-effort: a client that
/// hung up before reading its refusal lost nothing it wanted.
fn refuse(out: &mut UnixStream, reply: &str) -> Result<(), String> {
    let _ = out.write_all(reply.as_bytes());
    Ok(())
}

/// The credential to use for this request, renewed first if it is inside
/// the margin.
///
/// Re-read per request, so a rotation performed by anything else on the
/// host — another broker, a `/login` — is picked up rather than cached past
/// its usefulness.
///
/// A renewal that fails is not swallowed. Inside the margin the old token
/// still works, so the request goes out on it and the failure is logged;
/// past expiry there is nothing to send, and the error goes to the box
/// as a reply that names it. Falling back to an expired token — which
/// this once did — produces a 401 the agent can neither explain nor fix.
fn fresh_credential(file: &Path) -> Result<Credential, String> {
    let credential = read_credential(file)?;
    match broker::refresh_before(credential.expires_at, crate::now_unix()) {
        broker::Refresh::NotYet => Ok(credential),
        broker::Refresh::Before => renew(file).or_else(|why| {
            eprintln!("broker: {why}; forwarding on the current token while it lasts");
            Ok(credential)
        }),
        broker::Refresh::Expired => renew(file),
    }
}

fn read_credential(file: &Path) -> Result<Credential, String> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    broker::parse_credential(&text).map_err(|e: BrokerError| e.to_string())
}

/// Renews the token, at most once per host at a time.
///
/// The lock and the re-read under it are the whole of it: the refresh token
/// rotates, so it admits exactly one writer, and two brokers renewing
/// together would leave one of them holding a token the server has already
/// invalidated. The second takes the lock, re-reads, and finds nothing left
/// to do.
fn renew(file: &Path) -> Result<Credential, String> {
    let lock = file.with_extension("refresh.lock");
    let Some(held) = crate::try_lock(&lock)? else {
        // Someone else is renewing right now. Their answer is ours, and
        // waiting for the file is cheaper than racing them for it.
        return read_credential(file);
    };
    let existing = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let credential = broker::parse_credential(&existing).map_err(|e| e.to_string())?;
    if !broker::still_needs_refresh(&credential, crate::now_unix()) {
        drop(held);
        return Ok(credential); // the other writer already did it
    }
    let reply = ask_for_a_new_token(&credential)?;
    // The reply is in the endpoint's shape; the file is in Claude Code's,
    // and the host's own login reads it too. Merged, never written raw —
    // raw, a successful renewal would have logged the host out.
    let renewed = broker::merge_renewal(&existing, &reply, crate::now_unix())
        .map_err(|e| broker::redact(&e.to_string(), &credential))?;
    // Atomic, and mode 600 before anything can read it: a killed process
    // must never leave a torn credential, and a token must never sit
    // world-readable even briefly.
    crate::replace_file_private(file, &renewed)?;
    drop(held);
    broker::parse_credential(&renewed).map_err(|e| e.to_string())
}

/// The refresh request itself. The token goes to `curl` on stdin, never in
/// argv — argv is world-readable on this host. The body is the one Claude
/// Code's own login sends (`broker::refresh_request`); anything less is
/// refused as `Invalid request format`.
fn ask_for_a_new_token(credential: &Credential) -> Result<String, String> {
    let config = format!(
        concat!(
            "url = \"{url}\"\n",
            "header = \"Content-Type: application/json\"\n",
            "request = \"POST\"\n",
            "data-binary = \"{body}\"\n",
            "fail-with-body\nsilent\nshow-error\nmax-time = 20\n",
        ),
        url = broker::TOKEN_URL,
        body = broker::refresh_request(credential).replace('"', "\\\""),
    );
    let mut child = Command::new("curl")
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot renew the credential: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("cannot hand curl its options")?
        .write_all(config.as_bytes())
        .map_err(|e| format!("cannot hand curl its options: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("cannot renew the credential: {e}"))?;
    if !output.status.success() {
        // Specific, not a blanket 401: the agent retries a 401 about seven
        // times and would burn every one without learning what was wrong.
        // The reply body is the endpoint's own reason, when it gave one.
        let why = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let body = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let why = if body.is_empty() {
            why
        } else {
            format!("{why}: {body}")
        };
        return Err(broker::redact(
            &BrokerError::RefreshFailed(why).to_string(),
            credential,
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("the renewal reply is not text: {e}"))
}
