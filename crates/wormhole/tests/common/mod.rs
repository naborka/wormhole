//! What the tests that read this repository's own files need to find it,
//! and the one way they drive wormhole on a real terminal.
//! One module, several test binaries; each uses part of it.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

pub fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/wormhole sits two levels under the repository root")
}

/// A screen that never appears must fail the test, not hold it. Without
/// this the read loop below blocks on a child that will never write again
/// and never exit, and the suite hangs instead of reporting.
const PATIENCE: Duration = Duration::from_secs(30);

/// Runs a command on a real pty, answers one prompt, and returns
/// everything it wrote — keystrokes and all.
///
/// Some of wormhole only exists on a terminal: the panel holds one in raw
/// mode, and the confirm that stands between a fetched role and this
/// machine is drawn on one. Both are driven the same way — wait for the
/// screen to say it is ready, then send the one key that answers it — so
/// there is one body for it rather than a copy per test.
pub fn on_a_terminal(mut command: Command, wait_for: &str, answer: &[u8]) -> String {
    let pty = nix::pty::openpty(None, None).expect("a pty");
    let child = command
        .env("TERM", "xterm")
        .stdin(Stdio::from(pty.slave.try_clone().expect("slave fd")))
        .stdout(Stdio::from(pty.slave.try_clone().expect("slave fd")))
        .stderr(Stdio::from(pty.slave))
        .spawn()
        .expect("wormhole binary should spawn");
    // A `Command` keeps the stdio handles it was given. Held past the
    // spawn, its copies of the slave keep the pty open from this side, so
    // the read below never reaches end-of-file and the test hangs instead
    // of finishing. Taken by value for exactly this reason.
    drop(command);

    // Killed through the `Child`, never by pid: a pid the system has
    // already handed to somebody else must not be signalled.
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    let (done, waiting) = std::sync::mpsc::channel::<()>();
    let watched = std::sync::Arc::clone(&child);
    std::thread::spawn(move || {
        if waiting.recv_timeout(PATIENCE).is_err() {
            let _ = watched.lock().expect("the child").kill();
        }
    });

    let mut master = std::fs::File::from(pty.master);
    let mut seen = String::new();
    let mut buffer = [0u8; 4096];
    let mut answered = false;
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => seen.push_str(&String::from_utf8_lossy(&buffer[..bytes])),
            // The child closed the last slave fd: nothing more is coming.
            Err(e) if e.raw_os_error() == Some(nix::errno::Errno::EIO as i32) => break,
            Err(e) => panic!("cannot read the pty: {e}"),
        }
        if !answered && seen.contains(wait_for) {
            master.write_all(answer).expect("answer the screen");
            answered = true;
        }
    }
    let status = child.lock().expect("the child").wait().expect("it exits");
    let _ = done.send(());
    assert!(answered, "never drew {wait_for:?}:\n{seen}");
    assert!(status.success(), "exited {status}:\n{seen}");
    seen
}

/// A gzipped tarball of `content`, and its sha256 — what `[image] base`
/// wants. One body, because every test that builds an image needs it and
/// two spellings would drift on what `--exclude` or `--auto-compress`
/// mean here.
pub fn tarball_of(dir: &Path, content: &Path) -> (String, String) {
    let tarball = dir.join("rootfs.tar.gz");
    let tar = Command::new("tar")
        .arg("--create")
        .arg("--gzip")
        .arg("--file")
        .arg(&tarball)
        .arg("--directory")
        .arg(content)
        .arg(".")
        .status()
        .expect("tar should run");
    assert!(tar.success(), "tar failed");

    let bytes = std::fs::read(&tarball).expect("tarball readable");
    (
        tarball.display().to_string(),
        wormhole_core::sha256_hex(&bytes),
    )
}

/// The smallest rootfs that is a rootfs: one file, no shell. Enough for
/// every test whose recipe installs nothing, and so starts no build box.
pub fn rootfs_tarball(dir: &Path) -> (String, String) {
    let content = dir.join("content");
    std::fs::create_dir_all(content.join("bin")).expect("rootfs layout");
    std::fs::write(content.join("bin/hello"), "#!/bin/sh\necho hi\n").expect("rootfs file");
    tarball_of(dir, &content)
}

/// A rootfs holding a working `/bin/sh` and the libraries it needs, so a
/// build box started from it can actually run a script.
///
/// The libraries come from `ldd`, not from a list: the host may be glibc
/// or musl and the test must not care which.
pub fn rootfs_with_a_shell(dir: &Path) -> (String, String) {
    let content = dir.join("content");
    std::fs::create_dir_all(content.join("bin")).expect("rootfs layout");
    std::fs::copy("/bin/sh", content.join("bin/sh")).expect("a shell to put in it");

    let ldd = Command::new("ldd")
        .arg("/bin/sh")
        .output()
        .expect("ldd should run");
    for line in String::from_utf8_lossy(&ldd.stdout).lines() {
        let library = match line.split_once("=> ") {
            Some((_, rest)) => rest.split_whitespace().next(),
            None => line.split_whitespace().next(),
        };
        let Some(library) = library.filter(|path| path.starts_with('/')) else {
            continue;
        };
        let target = content.join(library.trim_start_matches('/'));
        std::fs::create_dir_all(target.parent().expect("a directory")).expect("library dir");
        std::fs::copy(library, &target).expect("library");
    }
    tarball_of(dir, &content)
}

/// `wormhole build` in `workspace`, against a data home of its own.
pub fn build_in(workspace: &Path, data_home: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .arg("build")
        .current_dir(workspace)
        .env("XDG_DATA_HOME", data_home)
        .output()
        .expect("wormhole binary should spawn")
}

/// The one image a test built.
pub fn one_image(data_home: &Path) -> std::path::PathBuf {
    let mut images: Vec<std::path::PathBuf> = std::fs::read_dir(data_home.join("wormhole/images"))
        .expect("images dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(images.len(), 1, "{images:?}");
    images.pop().expect("one image")
}
