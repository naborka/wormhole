//! The forwarder runs inside whatever image a box uses, so the one
//! property it must never lose is being static: a `PT_INTERP` would ask
//! the image for a loader it may not have, and the exec would die with a
//! bare "not found". This holds the binary `build.rs` actually embedded
//! to that, so a toolchain or profile change that sneaks a loader back in
//! fails here instead of inside someone's box.

mod common;

/// The same bytes `boundary.rs` writes into every brokered box.
static FORWARDER: &[u8] = include_bytes!(env!("WORMHOLE_FORWARD_BIN"));

#[test]
fn the_embedded_forwarder_needs_no_loader_from_the_image() {
    assert_eq!(
        wormhole_core::broker::requires_loader(FORWARDER),
        Ok(false),
        "the embedded forwarder must be a static ELF"
    );
}

/// The embedded bytes must be a program, not just a static ELF: written
/// to disk the way `boundary.rs` writes them, it runs and moves bytes
/// both ways between its TCP side and the broker's socket.
#[test]
fn the_embedded_forwarder_relays_between_tcp_and_the_socket() {
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let binary = dir.path().join("forward");
    std::fs::write(&binary, FORWARDER).expect("write forwarder");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let socket = dir.path().join("broker.sock");
    let broker = std::os::unix::net::UnixListener::bind(&socket).expect("bind socket");
    // A port the kernel says is free right now; the forwarder takes no
    // ":0", because in the box its address is fixed.
    let addr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe port");
        probe.local_addr().expect("addr").to_string()
    };
    let mut child = std::process::Command::new(&binary)
        .args([&addr, &socket.display().to_string()])
        .spawn()
        .expect("spawn forwarder");

    let server = std::thread::spawn(move || {
        let (mut peer, _) = broker.accept().expect("accept");
        let mut buf = [0u8; 4];
        peer.read_exact(&mut buf).expect("read request");
        assert_eq!(&buf, b"ping");
        peer.write_all(b"pong").expect("write reply");
    });
    // The listener comes up when it comes up; retry rather than sleep.
    let mut tcp = common::await_ready(|| std::net::TcpStream::connect(&addr).ok())
        .expect("the forwarder never started listening");
    tcp.write_all(b"ping").expect("write request");
    let mut reply = [0u8; 4];
    tcp.read_exact(&mut reply).expect("read reply");
    assert_eq!(&reply, b"pong");
    server.join().expect("broker side");
    child.kill().expect("kill forwarder");
    let _ = child.wait();
}
