//! The broker's CONNECT leg, driven through the real binary and its real
//! socket: policy answers arrive as HTTP replies, never as hangs or
//! errors that blame the network. The allowed-and-reachable tunnel needs
//! a listener on port 443 or 80, which no test may bind unprivileged, so
//! what these hold is every decision up to the dial and the dial's own
//! failure; the byte relay is the same shape `tests/forwarder.rs` proves.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};

mod common;

struct Broker {
    child: Child,
    socket: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn broker(egress: &str) -> Broker {
    broker_with(egress, None)
}

fn broker_with(egress: &str, egress_file: Option<&std::path::Path>) -> Broker {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("broker.sock");
    let mut args = vec!["broker", "--socket", socket.to_str().expect("utf8 path")];
    if !egress.is_empty() {
        args.extend(["--egress", egress]);
    }
    if let Some(file) = egress_file {
        args.extend(["--egress-file", file.to_str().expect("utf8 path")]);
        args.extend(["--name", "api"]);
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn broker");
    if common::await_ready(|| UnixStream::connect(&socket).ok()).is_some() {
        return Broker {
            child,
            socket,
            _dir: dir,
        };
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("the broker never came up on {}", socket.display());
}

fn connect(broker: &Broker, target: &str) -> String {
    let mut stream = UnixStream::connect(&broker.socket).expect("connect");
    stream
        .write_all(format!("CONNECT {target} HTTP/1.1\r\n\r\n").as_bytes())
        .expect("send CONNECT");
    let mut reply = String::new();
    stream.read_to_string(&mut reply).expect("read reply");
    reply
}

#[test]
fn a_host_off_the_allowlist_is_refused_by_name_with_the_fix() {
    let broker = broker("crates.io");
    let reply = connect(&broker, "evil.com:443");
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    assert!(reply.contains("evil.com"), "{reply}");
    assert!(reply.contains("egress"), "{reply}");
}

#[test]
fn an_empty_allowlist_admits_nothing() {
    let broker = broker("");
    let reply = connect(&broker, "crates.io:443");
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
}

#[test]
fn a_port_that_is_not_the_web_is_refused_before_any_policy() {
    let broker = broker("crates.io");
    let reply = connect(&broker, "crates.io:22");
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    assert!(reply.contains("443"), "{reply}");
}

/// The point of the live file: a host allowed after the broker started
/// is admitted on the very next tunnel — nothing restarted. The refusal
/// before it names the exact `wormhole allow` line.
#[test]
fn a_host_allowed_while_the_broker_runs_is_admitted_without_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("egress");
    std::fs::write(&file, "").expect("empty list");
    let broker = broker_with("", Some(&file));
    let refused = connect(&broker, "127.0.0.1:443");
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    assert!(refused.contains("wormhole allow api 127.0.0.1"), "{refused}");
    std::fs::write(&file, "127.0.0.1\n").expect("allow the host");
    // 502, not 403: admitted now, and only the dial (nothing listens on
    // loopback 443) fails.
    let allowed = connect(&broker, "127.0.0.1:443");
    assert!(allowed.starts_with("HTTP/1.1 502"), "{allowed}");
}

/// The whole tunnel against the real internet: admitted, dialed, and the
/// 200 that tells a client to begin TLS. Ignored by default because it
/// needs a route out; run it with `--ignored` on a connected host.
#[test]
#[ignore = "needs real network egress"]
fn an_allowed_host_tunnels_end_to_end() {
    let broker = broker("crates.io");
    let mut stream = UnixStream::connect(&broker.socket).expect("connect");
    stream
        .write_all(b"CONNECT crates.io:443 HTTP/1.1\r\n\r\n")
        .expect("send CONNECT");
    let mut reply = [0u8; 39];
    stream.read_exact(&mut reply).expect("read the answer");
    assert_eq!(&reply, b"HTTP/1.1 200 Connection established\r\n\r\n");
}

/// The allow branch, proven to the dial: the host is admitted, nothing
/// listens on it, and the reply says upstream failed — a 502, not the
/// 403 a policy refusal wears.
#[test]
fn an_allowed_host_that_cannot_be_dialed_is_a_gateway_error_not_a_refusal() {
    let broker = broker("127.0.0.1");
    let reply = connect(&broker, "127.0.0.1:443");
    assert!(reply.starts_with("HTTP/1.1 502"), "{reply}");
    assert!(reply.contains("cannot reach"), "{reply}");
}
