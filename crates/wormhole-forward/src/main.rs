//! The in-box half of the broker: a TCP listener on the box's loopback
//! that moves bytes to the broker's unix socket and back. It holds no
//! credential and can reach nothing else, so this is the only part of the
//! broker an untrusted agent ever touches.
//!
//! Its own binary on purpose. Wormhole itself links however the host
//! built it, but this helper runs inside whatever image the box uses, so
//! it must depend on nothing the image has — `wormhole`'s build script
//! compiles it for the musl target, where it is static and runs under any
//! libc. Std only, no dependencies: every crate added here is bytes baked
//! into every wormhole binary.
//!
//! Dumb by design: the address and socket path arrive as arguments, so
//! every decision stays in `wormhole-core` where it is tested.

use std::os::unix::net::UnixStream;
use std::path::Path;

fn main() -> ! {
    let mut args = std::env::args().skip(1);
    let (Some(addr), Some(socket)) = (args.next(), args.next()) else {
        fail("usage: wormhole-forward <addr> <socket>");
    };
    let socket = Path::new(&socket);
    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(listener) => listener,
        Err(e) => fail(&format!("cannot listen on {addr}: {e}")),
    };
    for stream in listener.incoming().flatten() {
        if let Err(e) = pipe_both_ways(stream, socket) {
            eprintln!("wormhole-forward: {e}");
        }
    }
    fail("the listener ended")
}

fn pipe_both_ways(tcp: std::net::TcpStream, socket: &Path) -> Result<(), String> {
    let unix = at_socket(socket, |name| UnixStream::connect(name))
        .map_err(|e| format!("cannot reach the broker at {}: {e}", socket.display()))?;
    let (mut tcp_in, mut tcp_out) = (
        tcp.try_clone().map_err(|e| e.to_string())?,
        tcp.try_clone().map_err(|e| e.to_string())?,
    );
    let (mut unix_in, mut unix_out) = (
        unix.try_clone().map_err(|e| e.to_string())?,
        unix.try_clone().map_err(|e| e.to_string())?,
    );
    // Both directions at once: a request whose reply begins before its body
    // has finished uploading is normal, and a one-way-at-a-time relay would
    // deadlock on it.
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut tcp_in, &mut unix_out);
        let _ = unix_out.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut unix_in, &mut tcp_out);
    let _ = tcp_out.shutdown(std::net::Shutdown::Write);
    let _ = up.join();
    Ok(())
}

/// A unix socket path is capped at 108 bytes; connecting by file name
/// from inside its directory sidesteps the cap for any path.
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

fn fail(message: &str) -> ! {
    eprintln!("wormhole-forward: {message}");
    std::process::exit(1)
}
