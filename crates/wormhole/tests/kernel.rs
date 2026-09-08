//! Integration tests that need a real kernel with unprivileged user
//! namespaces. Run on the host: `cargo test -p wormhole --features kernel-tests`
#![cfg(feature = "kernel-tests")]

use std::collections::BTreeSet;
use std::process::{Command, Output};

mod common;
use common::{build_in, one_image, rootfs_with_a_shell};
use std::os::unix::fs::PermissionsExt;

fn wormhole(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(args)
        .output()
        .expect("wormhole binary should spawn")
}

/// Spawns `wormhole run` with piped stdout and waits for the box to say
/// `ready` — its command must start with `echo ready`.
fn spawn_until_ready(args: &[&str]) -> std::process::Child {
    use std::io::{BufRead, BufReader};

    let mut child = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(args)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("wormhole binary should spawn");
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("box should say ready");
    assert_eq!(ready.trim(), "ready");
    child
}

fn run_stdout(args: &[&str]) -> String {
    let output = wormhole(args);
    assert!(
        output.status.success(),
        "expected success, got {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn true_exits_zero() {
    assert_eq!(wormhole(&["run", "--", "/bin/true"]).status.code(), Some(0));
}

#[test]
fn false_exits_one() {
    assert_eq!(
        wormhole(&["run", "--", "/bin/false"]).status.code(),
        Some(1)
    );
}

#[test]
fn child_exit_code_propagates() {
    let output = wormhole(&["run", "--", "sh", "-c", "exit 7"]);
    assert_eq!(output.status.code(), Some(7));
}

/// PID 1 with no handler for a signal ignores it when it comes from
/// inside its own namespace — the same kernel rule that makes `kill -9 1`
/// do nothing in a container. The 128+signal mapping itself is covered by
/// the pure test `signal_death_becomes_128_plus_signal`.
#[test]
fn pid_one_survives_a_sigkill_from_inside_the_box() {
    let output = wormhole(&["run", "--", "sh", "-c", "kill -9 $$; echo alive"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "alive");
}

/// `Ctrl-C` goes to every process in the terminal's foreground group.
/// Only the box may react to it: if `wormhole` died there, the shell would
/// take the prompt back while the box still held the terminal.
#[test]
fn an_interrupt_does_not_kill_wormhole_while_the_box_runs() {
    let mut child = spawn_until_ready(&["run", "--", "sh", "-c", "echo ready; sleep 1; exit 7"]);

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(i32::try_from(child.id()).expect("pid fits")),
        nix::sys::signal::Signal::SIGINT,
    )
    .expect("interrupt should be deliverable");

    let status = child.wait().expect("wormhole should exit on its own");
    assert_eq!(status.code(), Some(7));
}

#[test]
fn uid_inside_the_box_is_host_identical() {
    let inside = run_stdout(&["run", "--", "id", "-u"]);
    let host = nix::unistd::getuid().as_raw().to_string();
    assert_eq!(inside, host);
}

#[test]
fn missing_command_exits_127() {
    let output = wormhole(&["run", "--", "/no/such/binary"]);
    assert_eq!(output.status.code(), Some(127));
}

#[test]
fn run_without_separator_is_a_usage_error() {
    let output = wormhole(&["run", "/bin/true"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn root_shows_exactly_the_plan() {
    let listing = run_stdout(&["run", "--", "ls", "-a", "/"]);
    let seen: BTreeSet<&str> = listing.split_whitespace().collect();
    let expected: BTreeSet<&str> = [
        ".", "..", "bin", "dev", "etc", "home", "lib", "lib64", "proc", "run", "sbin", "tmp", "usr",
    ]
    .into();
    assert_eq!(seen, expected);
}

#[test]
fn etc_holds_only_the_synthetic_files() {
    let listing = run_stdout(&["run", "--", "ls", "-a", "/etc"]);
    let seen: BTreeSet<&str> = listing.split_whitespace().collect();
    let expected: BTreeSet<&str> = [".", "..", "group", "hosts", "passwd", "resolv.conf"].into();
    assert_eq!(seen, expected);
}

/// The box is on the host's network, so with no resolver named it reads
/// the host's own — the file behind `/etc/resolv.conf`, symlink followed.
#[test]
fn without_dns_the_box_reads_the_hosts_resolver() {
    let host = std::fs::read_to_string("/etc/resolv.conf").expect("the host has a resolver");
    let seen = run_stdout(&["run", "--", "cat", "/etc/resolv.conf"]);
    assert_eq!(seen, host.trim());
}

#[test]
fn a_named_resolver_is_the_only_one_the_box_sees() {
    let seen = run_stdout(&["run", "--dns", "1.1.1.1", "--", "cat", "/etc/resolv.conf"]);
    assert_eq!(seen, "nameserver 1.1.1.1");
}

#[test]
fn workspace_is_visible_at_the_host_identical_path() {
    let cwd = std::env::current_dir().expect("test cwd");
    let manifest = cwd.join("Cargo.toml");
    let inside = run_stdout(&[
        "run",
        "--",
        "sh",
        "-c",
        &format!("test -f {} && pwd", manifest.display()),
    ]);
    assert_eq!(inside, cwd.display().to_string());
}

#[test]
fn usr_is_read_only() {
    let output = wormhole(&["run", "--", "touch", "/usr/wormhole-probe"]);
    assert!(!output.status.success());
    assert!(!std::path::Path::new("/usr/wormhole-probe").exists());
}

#[test]
fn host_etc_is_not_visible() {
    let output = wormhole(&["run", "--", "test", "-e", "/etc/fstab"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn proc_shows_only_this_box() {
    let listing = run_stdout(&["run", "--", "ls", "/proc"]);
    let pids: BTreeSet<&str> = listing
        .split_whitespace()
        .filter(|entry| entry.chars().all(|c| c.is_ascii_digit()))
        .collect();
    assert_eq!(pids, ["1"].into(), "expected only PID 1, got {listing}");
}

#[test]
fn the_command_is_pid_one() {
    assert_eq!(run_stdout(&["run", "--", "sh", "-c", "echo $$"]), "1");
}

#[test]
fn host_processes_are_invisible() {
    let host_pid = std::process::id().to_string();
    let output = wormhole(&[
        "run",
        "--",
        "sh",
        "-c",
        &format!("test ! -e /proc/{host_pid}"),
    ]);
    assert!(output.status.success(), "host pid {host_pid} leaked");
}

#[test]
fn the_fixed_device_nodes_are_usable() {
    let output = wormhole(&[
        "run",
        "--",
        "sh",
        "-c",
        "echo x > /dev/null && head -c 4 /dev/urandom > /dev/null",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dev_holds_only_the_planned_nodes() {
    let listing = run_stdout(&["run", "--", "ls", "-a", "/dev"]);
    let seen: BTreeSet<&str> = listing.split_whitespace().collect();
    let expected: BTreeSet<&str> = [
        ".", "..", "full", "null", "pts", "ptmx", "random", "shm", "tty", "urandom", "zero",
    ]
    .into();
    assert_eq!(seen, expected);
}

/// The box has its own `devpts`, so tools that open pseudo-terminals work
/// and never see the host's.
#[test]
fn a_pseudo_terminal_can_be_opened_through_ptmx() {
    let listing = run_stdout(&[
        "run",
        "--",
        "sh",
        "-c",
        "ls /dev/pts && test -c /dev/ptmx && echo ok",
    ]);
    assert!(listing.ends_with("ok"), "{listing}");
}

#[test]
fn a_file_written_in_the_box_lands_on_the_host_with_our_uid() {
    use std::os::unix::fs::MetadataExt;

    let workspace = std::env::current_dir().expect("test cwd");
    let marker = workspace.join("wormhole-write-probe");
    let _ = std::fs::remove_file(&marker);

    let output = wormhole(&["run", "--", "sh", "-c", "echo boxed > wormhole-write-probe"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata = std::fs::metadata(&marker).expect("host-side file at the identical path");
    assert_eq!(metadata.uid(), nix::unistd::getuid().as_raw());
    assert_eq!(
        std::fs::read_to_string(&marker).expect("readable"),
        "boxed\n"
    );
    std::fs::remove_file(&marker).expect("cleanup");
}

#[test]
fn home_is_set_to_the_box_home() {
    let user = nix::unistd::User::from_uid(nix::unistd::getuid())
        .expect("passwd lookup")
        .expect("passwd entry");
    let inside = run_stdout(&["run", "--", "sh", "-c", "echo $HOME"]);
    assert_eq!(inside, format!("/home/{}", user.name));
}

/// The host's `PATH` describes the host's filesystem, not the box's. Arch
/// has no `/bin`, Alpine keeps busybox there, so an inherited `PATH` finds
/// neither `mkdir` nor `apk`.
#[test]
fn path_is_the_boxs_own_not_the_hosts() {
    let inside = run_stdout(&["run", "--", "sh", "-c", "echo $PATH"]);
    assert!(
        inside.ends_with(wormhole_core::mount_plan::BOX_PATH),
        "{inside}"
    );
    assert!(inside.contains("/.local/bin:"), "{inside}");
}

#[test]
fn hostname_is_wormhole() {
    assert_eq!(run_stdout(&["run", "--", "uname", "-n"]), "wormhole");
}

#[test]
fn tmp_and_home_are_writable() {
    let output = wormhole(&[
        "run",
        "--",
        "sh",
        "-c",
        "touch /tmp/probe && touch \"$HOME/probe\"",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_granted_path_is_readable_and_writable_in_the_box() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().to_str().expect("utf-8 temp path");
    std::fs::write(dir.path().join("in"), "granted\n").expect("write");

    let seen = run_stdout(&[
        "run",
        "--grant",
        path,
        "--",
        "sh",
        "-c",
        &format!("cat {path}/in && touch {path}/out"),
    ]);

    assert_eq!(seen, "granted");
    assert!(dir.path().join("out").exists(), "write did not reach host");
}

#[test]
fn an_ungranted_host_path_stays_invisible() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().to_str().expect("utf-8 temp path");
    let output = wormhole(&["run", "--", "test", "-e", path]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn a_relative_grant_is_refused() {
    let output = wormhole(&["run", "--grant", "relative/path", "--", "/bin/true"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("grant must be an absolute path"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The box `attach` is asked for. A box is named by its id everywhere in
/// the CLI, so that is what the hand-laid entry carries.
const BOX_ID: &str = "0123456789ab";

/// `attach` joins a running box's namespaces by its box id and runs a
/// command inside: same hostname, same isolated pid view. The registry
/// entry is laid out by hand around a bare `run --pidfile`, which is
/// exactly what `wormhole box` writes for real.
#[test]
fn attach_joins_a_running_box() {
    let temp = tempfile::tempdir().expect("temp dir");
    let mut running = spawn_until_ready(&[
        "run",
        "--pidfile",
        &temp.path().join("init.pid").display().to_string(),
        "--",
        "sh",
        "-c",
        "echo ready; sleep 5",
    ]);

    let box_dir = temp
        .path()
        .join("wormhole/boxes")
        .join(running.id().to_string());
    std::fs::create_dir_all(&box_dir).expect("box dir");
    std::fs::copy(temp.path().join("init.pid"), box_dir.join("init.pid")).expect("init.pid");
    let entry = wormhole_core::registry::Entry {
        box_id: BOX_ID.to_owned(),
        pid: running.id(),
        workspace: std::env::current_dir().expect("cwd"),
        image: "/i".to_owned(),
        agent: None,
        name: None,
        alias: None,
        started_unix: 1,
    };
    std::fs::write(
        box_dir.join("box.toml"),
        wormhole_core::registry::to_toml(&entry).expect("toml"),
    )
    .expect("entry written");

    let attached = Command::new(env!("CARGO_BIN_EXE_wormhole"))
        .args(["attach", BOX_ID, "--", "sh", "-c", "uname -n"])
        .env("XDG_DATA_HOME", temp.path())
        .output()
        .expect("attach should run");
    assert!(
        attached.status.success(),
        "{}",
        String::from_utf8_lossy(&attached.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&attached.stdout).trim(), "wormhole");
    running.wait().expect("box should exit");
}

/// Only `wormhole box` keeps a home per workspace (N5). Bare `run` is the
/// boundary on its own, and everything it makes stays throwaway.
#[test]
fn bare_run_home_is_throwaway() {
    let first = wormhole(&["run", "--", "sh", "-c", "touch \"$HOME/marker\""]);
    assert!(first.status.success());
    let second = wormhole(&["run", "--", "sh", "-c", "test ! -e \"$HOME/marker\""]);
    assert!(second.status.success());
}

/// Builds an image from a recipe body and returns what the build said and
/// the data home it built into. The tests below differ only in that body
/// and in what they assert about it.
fn built_from(temp: &std::path::Path, image_body: &str) -> (String, std::path::PathBuf) {
    let (url, digest) = rootfs_with_a_shell(temp);
    let workspace = temp.join("workspace");
    let data_home = temp.join("data");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let version = wormhole_core::manifest::VERSION;
    std::fs::write(
        workspace.join("wormhole.toml"),
        format!(
            "version = {version}\n[image]\nbase = \"file://{url}\"\n\
             base_sha256 = \"{digest}\"\n{image_body}"
        ),
    )
    .expect("manifest written");

    let built = build_in(&workspace, &data_home);
    let stdout = String::from_utf8_lossy(&built.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert!(built.status.success(), "{stdout}\n{stderr}");
    (stdout, data_home)
}

/// The whole point of an artifact, end to end: the host fetches it, proves
/// its digest, and the build box reads the bytes without opening a single
/// connection of its own. A network that intercepts TLS has nothing to
/// intercept.
#[test]
fn a_build_box_reads_the_artifact_the_host_proved() {
    let temp = tempfile::tempdir().expect("temp dir");
    let artifact = temp.path().join("tool");
    std::fs::write(&artifact, "the real bytes\n").expect("artifact written");

    let digest = wormhole_core::sha256_hex(b"the real bytes\n");
    let (stdout, data_home) = built_from(
        temp.path(),
        &format!(
            "build = [\"read line < /tmp/tool; echo \\\"the box read: $line\\\"\"]\n\
             [[image.artifact]]\nurl = \"file://{}\"\nsha256 = \"{digest}\"\ninto = \"/tmp/tool\"\n",
            artifact.display()
        ),
    );
    assert!(stdout.contains("the box read: the real bytes"), "{stdout}");

    // The artifact served the build and is not in the image: it lived on
    // the scratch tmpfs, which the finished image never carried.
    let image = one_image(&data_home);
    assert!(
        !image.join("tmp/tool").exists(),
        "{image:?} kept the artifact"
    );

    // Sealed read-only and not executable where its identity is
    // established, so "an artifact is not something a build runs by
    // accident" is a fact about the cache rather than about curl's umask.
    let cached = data_home.join("wormhole/artifacts").join(&digest);
    let mode = std::fs::metadata(&cached)
        .expect("cached")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o444, "{cached:?} is {:o}", mode & 0o777);
}

/// What the build box could not have before: the host's trust.
///
/// A recipe cannot pin what `npm` or `apk` resolve for themselves, so a
/// build still terminates some TLS of its own. On a network that
/// intercepts it — Cloudflare WARP, a corporate proxy — the image's own
/// roots refuse the interceptor's certificate and the build dies naming no
/// cause. `host_ca` now reaches the build, at a path that is neither the
/// one `apk` writes nor one an artifact could be asked for.
#[test]
fn the_build_box_is_pointed_at_the_host_bundle_it_was_given() {
    let temp = tempfile::tempdir().expect("temp dir");
    let bundle = wormhole_core::ca::CA_BUNDLE_IN_BUILD;
    let (stdout, data_home) = built_from(
        temp.path(),
        &format!(
            "build = [\"echo \\\"points at $SSL_CERT_FILE\\\"; \
             read first < {bundle}; echo \\\"holds $first\\\"\"]\n\
             [access]\nhost_ca = true\n"
        ),
    );
    assert!(stdout.contains(&format!("points at {bundle}")), "{stdout}");

    // Not merely a file at that path: the host's own, proved by reading
    // the same first line the build box read.
    let named = stdout
        .lines()
        .find_map(|line| line.strip_prefix("the build trusts host CA bundle "))
        .expect("the build says which bundle it trusted");
    let first = std::fs::read_to_string(named)
        .expect("the host bundle is readable")
        .lines()
        .next()
        .expect("it is not empty")
        .to_owned();
    assert!(stdout.contains(&format!("holds {first}")), "{stdout}");

    assert!(
        !one_image(&data_home)
            .join(bundle.trim_start_matches('/'))
            .exists(),
        "the image kept the host's bundle"
    );
}
