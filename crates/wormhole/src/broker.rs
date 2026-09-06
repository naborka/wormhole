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

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Command, Stdio};

use wormhole_core::broker::{self, BrokerError, Credential};

/// The one host wormhole talks to. The box never names it.
const UPSTREAM: &str = "https://api.anthropic.com";

/// Host side: serve the unix socket until killed.
///
/// One connection at a time is deliberate for now — the baton in a group is
/// turn-based and a single agent makes one request at a time — and it is
/// the shape that keeps the refresh lock honest while there is no
/// connection pool to reason about.
pub fn serve(socket: &Path, credential_file: &Path) -> ! {
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
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(e) = relay(stream, credential_file) {
                    // Redaction happens inside `relay`, where the
                    // credential is in scope; anything reaching here is
                    // already safe to print.
                    eprintln!("broker: {e}");
                }
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

/// One request from the box: read its head, renew the credential if it is
/// close to expiry, inject it, and stream the reply straight back.
fn relay(stream: UnixStream, credential_file: &Path) -> Result<(), String> {
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

    let mut body = Vec::new();
    if let Some(length) = head.content_length() {
        body.resize(length, 0);
        reader
            .read_exact(&mut body)
            .map_err(|e| format!("cannot read the request body: {e}"))?;
    }

    let credential = fresh_credential(credential_file)?;
    let outgoing = broker::outgoing(&head.headers, &credential.access_token);
    let url = broker::upstream_url(UPSTREAM, &head.path);

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

/// The credential to use for this request, renewed first if it is inside
/// the margin.
///
/// Re-read per request, so a rotation performed by anything else on the
/// host — another broker, a `/login` — is picked up rather than cached past
/// its usefulness.
fn fresh_credential(file: &Path) -> Result<Credential, String> {
    let credential = read_credential(file)?;
    if !broker::refresh_before(credential.expires_at, crate::now_unix()).needed() {
        return Ok(credential);
    }
    renew(file).or(Ok(credential))
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
    let credential = read_credential(file)?;
    if !broker::still_needs_refresh(&credential, crate::now_unix()) {
        drop(held);
        return Ok(credential); // the other writer already did it
    }
    let renewed = ask_for_a_new_token(&credential)?;
    // Atomic, and mode 600 before anything can read it: a killed process
    // must never leave a torn credential, and a token must never sit
    // world-readable even briefly.
    crate::replace_file_private(file, &renewed)?;
    drop(held);
    broker::parse_credential(&renewed).map_err(|e| e.to_string())
}

/// The refresh request itself. The token goes to `curl` on stdin, never in
/// argv — argv is world-readable on this host.
fn ask_for_a_new_token(credential: &Credential) -> Result<String, String> {
    let body = format!(
        "{{\"grant_type\":\"refresh_token\",\"refresh_token\":\"{}\"}}",
        credential.refresh_token
    );
    let config = format!(
        concat!(
            "url = \"{upstream}/v1/oauth/token\"\n",
            "header = \"Content-Type: application/json\"\n",
            "request = \"POST\"\n",
            "data-binary = \"{body}\"\n",
            "fail\nsilent\nshow-error\nmax-time = 20\n",
        ),
        upstream = UPSTREAM,
        body = body.replace('"', "\\\""),
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
        return Err(broker::redact(
            &BrokerError::RefreshFailed(String::from_utf8_lossy(&output.stderr).trim().to_owned())
                .to_string(),
            credential,
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("the renewal reply is not text: {e}"))
}
