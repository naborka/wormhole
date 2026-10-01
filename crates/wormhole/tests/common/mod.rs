//! What the tests that read this repository's own files need to find it,
//! and the one way they drive wormhole on a real terminal.
//! One module, several test binaries; each uses part of it.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Retries an attempt until it answers or two seconds pass — the one
/// wait-until-listening loop, however a test's server comes up.
pub fn await_ready<T>(mut attempt: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..100 {
        if let Some(ready) = attempt() {
            return Some(ready);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

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
pub fn on_a_terminal(command: Command, wait_for: &str, answer: &[u8]) -> String {
    let (seen, status) = drive_terminal(command, wait_for, answer);
    assert!(status.success(), "exited {status}:\n{seen}");
    seen
}

/// The same drive, for a question whose answer is a refusal.
pub fn on_a_refused_terminal(command: Command, wait_for: &str, answer: &[u8]) -> String {
    let (seen, status) = drive_terminal(command, wait_for, answer);
    assert!(
        !status.success(),
        "exited {status}, should have refused:\n{seen}"
    );
    seen
}

/// The words a terminal would be showing, from the bytes drawn on it.
///
/// A drawn screen is not a stream of lines: cells are written where they
/// changed and the cursor is moved over the ones that did not, so `x` and
/// `remove` arrive with an escape sequence between them and the space
/// between them is never written at all. Each escape becomes one space
/// and runs of blanks collapse, which turns the stream back into the
/// words on the screen — which is what a test about a screen should be
/// asserting on.
pub fn shown(drawn: &str) -> String {
    let mut said = String::new();
    let mut rest = drawn.chars().peekable();
    while let Some(c) = rest.next() {
        if c != '\u{1b}' {
            said.push(c);
            continue;
        }
        said.push(' ');
        // CSI and OSC alike: parameter bytes, then one final byte.
        if rest.peek() == Some(&'[') || rest.peek() == Some(&']') {
            rest.next();
        }
        for byte in rest.by_ref() {
            if ('@'..='~').contains(&byte) {
                break;
            }
        }
    }
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The size every test terminal has.
///
/// Given rather than left unset: `openpty(None, ..)` makes a terminal that
/// reports zero by zero, which is a shape no real terminal has and which
/// hid the one thing these tests exist to drive — a panel drawn for a
/// surface it has measured.
pub const TERMINAL: nix::pty::Winsize = nix::pty::Winsize {
    ws_row: 24,
    ws_col: 80,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

nix::ioctl_write_ptr_bad!(set_terminal_size, nix::libc::TIOCSWINSZ, nix::pty::Winsize);

/// Drives a screen, resizes the terminal under it, and returns what was
/// drawn before the resize and what was drawn after.
///
/// The only way to prove a panel reacts to a resize: the kernel raises
/// `SIGWINCH` on the foreground group when the size is set, which is
/// where a redraw has to come from. Everything before the resize is the
/// first answer, everything after it the second.
pub fn resized(
    command: Command,
    wait_for: &str,
    to: (u16, u16),
    answer: &[u8],
) -> (String, String) {
    let (before, after, status) = drive(command, wait_for, Some(to), answer);
    assert!(status.success(), "exited {status}:\n{before}{after}");
    (before, after)
}

/// The pty loop both wrappers share. The caller judges the exit.
pub fn drive_terminal(
    command: Command,
    wait_for: &str,
    answer: &[u8],
) -> (String, std::process::ExitStatus) {
    let (before, after, status) = drive(command, wait_for, None, answer);
    (before + &after, status)
}

fn drive(
    mut command: Command,
    wait_for: &str,
    resize: Option<(u16, u16)>,
    answer: &[u8],
) -> (String, String, std::process::ExitStatus) {
    let pty = nix::pty::openpty(Some(&TERMINAL), None).expect("a pty");
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
    let mut after = String::new();
    let mut buffer = [0u8; 4096];
    let mut answered = false;
    let mut resizing = false;
    loop {
        let read = match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => String::from_utf8_lossy(&buffer[..bytes]).into_owned(),
            // The child closed the last slave fd: nothing more is coming.
            Err(e) if e.raw_os_error() == Some(nix::errno::Errno::EIO as i32) => break,
            Err(e) => panic!("cannot read the pty: {e}"),
        };
        if resizing || answered {
            after.push_str(&read);
        } else {
            seen.push_str(&read);
        }
        // Resized, and now waiting for the redraw it causes. Answering
        // here instead would race it: the panel could read the key and
        // leave before it ever looked at the new size, and a test that
        // cannot tell those apart proves nothing.
        if resizing && !after.is_empty() {
            master.write_all(answer).expect("answer the screen");
            (resizing, answered) = (false, true);
            continue;
        }
        if !answered && !resizing && shown(&seen).contains(wait_for) {
            if let Some((width, height)) = resize {
                let size = nix::pty::Winsize {
                    ws_col: width,
                    ws_row: height,
                    ..TERMINAL
                };
                #[expect(
                    unsafe_code,
                    reason = "an ioctl on a pty this test owns, with a size it owns"
                )]
                unsafe {
                    use std::os::fd::AsRawFd;
                    set_terminal_size(master.as_raw_fd(), &size).expect("resize the terminal");
                }
                resizing = true;
                continue;
            }
            master.write_all(answer).expect("answer the screen");
            answered = true;
        }
    }
    let status = child.lock().expect("the child").wait().expect("it exits");
    let _ = done.send(());
    assert!(
        answered,
        "never drew {wait_for:?} or never redrew:\n{seen}{after}"
    );
    (seen, after, status)
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

/// `wormhole <args>` in `cwd`, against a data home of its own.
///
/// The one spawner, so a variable every test needs — `XDG_DATA_HOME`
/// today, and the config home the moment a test cares about roles — is
/// added in one place rather than in each file that forgot.
pub fn wormhole(data_home: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(args)
        .current_dir(cwd)
        .env("XDG_DATA_HOME", data_home)
        .output()
        .expect("wormhole binary should spawn")
}

/// Everything a run wrote, in the order a person would read it.
pub fn said(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A box record built through the real type, so a field that changes is a
/// compile error in the fixture rather than a test that quietly stops
/// covering it.
pub fn a_record(workspace: &Path, id: &str, alias: Option<&str>) -> wormhole_core::home::Record {
    wormhole_core::home::Record {
        key: wormhole_core::paths::box_key(workspace, id),
        id: id.to_owned(),
        workspace: workspace.to_owned(),
        earlier: Vec::new(),
        role: None,
        source: None,
        alias: alias.map(str::to_owned),
        name: Some("architect".to_owned()),
        agent: Some("claude".to_owned()),
        created_unix: 1,
        started_unix: 2,
    }
}

/// A kept box on disk: its home, its record, and one file standing in for
/// everything an agent leaves behind. Returns the home.
pub fn keep_box(data_home: &Path, record: &wormhole_core::home::Record) -> std::path::PathBuf {
    let home = wormhole_core::paths::home_dir(data_home, &record.key);
    std::fs::create_dir_all(&home).expect("home");
    let file = wormhole_core::paths::record_file(data_home, &record.key);
    std::fs::create_dir_all(file.parent().expect("records dir")).expect("records dir");
    std::fs::write(file, wormhole_core::home::to_toml(record).expect("toml")).expect("record");
    std::fs::write(home.join("history.jsonl"), "what the agent knew\n").expect("history");
    home
}

/// A directory under `under`, made and canonical, so it compares equal to
/// the workspace path a box records.
pub fn a_dir(under: &Path, name: &str) -> std::path::PathBuf {
    let dir = under.join(name);
    std::fs::create_dir_all(&dir).expect("dir");
    dir.canonicalize().expect("canonical dir")
}

/// A role whose recipe names a base that does not exist, so a start that
/// reads it says so by the URL it then fails to fetch. `resume` is a whole
/// `[agent]` line, or nothing.
pub fn a_role(dir: &Path, resume: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).expect("role dir");
    std::fs::write(
        dir.join("wormhole.toml"),
        format!(
            "version = 1\n[agent]\nrun = \"claude\"\n{resume}\
             [image]\nbase = \"file:///nothing/role-recipe.tar.gz\"\n\
             base_sha256 = \"{}\"\n",
            "0".repeat(64)
        ),
    )
    .expect("role manifest");
    dir.canonicalize().expect("canonical role")
}

/// A box of `role`, kept as having last run in `ran_in`: its id and home.
pub fn a_role_box(data_home: &Path, ran_in: &Path, role: &Path) -> (String, std::path::PathBuf) {
    let id = wormhole_core::paths::box_id(ran_in, 0);
    let mut record = a_record(ran_in, &id, None);
    record.role = Some(role.display().to_string());
    record.source = Some(wormhole_core::source::dir_source(role));
    let home = keep_box(data_home, &record);
    (id, home)
}

/// The record the host keeps for the box living in `home`.
pub fn kept_record(data_home: &Path, home: &Path) -> wormhole_core::home::Record {
    let key = home
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a home is named by its key");
    let file = wormhole_core::paths::record_file(data_home, key);
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("no host-side record at {}: {e}", file.display()));
    wormhole_core::home::parse(&text, key).expect("a valid record")
}
