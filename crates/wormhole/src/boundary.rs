//! `Namespaces` boundary: run a command in fresh user, mount, UTS, and
//! PID namespaces, pivoted into a root built from the mount plan.
//! Decisions (plan content, arg shape, exit-code mapping) live in
//! `wormhole_core`; this module only performs them.
//!
//! Three processes: `run` on the host spawns `__boxed`, which unshares
//! the user, UTS and PID namespaces and forks. That fork is the box's
//! PID 1: it unshares the mount namespace, builds the root, pivots into
//! it, and becomes the command. `__boxed` only waits and reports how the
//! box ended.
//!
//! Why the fork. `unshare` grants capabilities to the calling process
//! only, and an `execve` by a non-root user drops them, so every mount
//! must happen in a process that has not exec'd since the unshare. And
//! `proc` shows the PID namespace of whoever mounts it, while
//! `unshare(CLONE_NEWPID)` puts only the caller's *children* in the new
//! one. PID 1 doing all of it satisfies both at once.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::ffi::CString;
use std::fs;
use std::net::IpAddr;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use nix::libc;
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::sched::{CloneFlags, unshare};
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, execvp, fork};
use wormhole_core::ca;
use wormhole_core::launch;
use wormhole_core::limits_cgroup;
use wormhole_core::mount_plan::{self, BOX_HOSTNAME, BOX_PATH, MountOp, Root, User, home_in_box};
use wormhole_core::run::{
    self, Network, RootMode, RunArgs, WaitOutcome, exit_code, identity_map, root_map,
};

/// Host side of `wormhole run`. The scratch dir (box root mountpoint)
/// lives here so it is removed from the host after the box exits.
/// `env` replaces the box's environment entirely when given, so only what
/// the manifest declares reaches it. `None` inherits the host's, which is
/// what bare `wormhole run` does.
/// `home` is the host directory mounted as the box's `$HOME`. `wormhole
/// box` passes its workspace's kept home; `None` means a throwaway one,
/// which is what bare `wormhole run` gets.
pub fn run(
    args: &RunArgs,
    env: Option<&BTreeMap<String, String>>,
    home: Option<&Path>,
    limits: &limits_cgroup::Limits,
) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => fail(&format!("cannot find own binary: {e}")),
    };
    let scratch = tempdir("box root");
    let throwaway;
    let home = match home {
        Some(home) => home,
        None => {
            throwaway = tempdir("throwaway home");
            throwaway.path()
        }
    };
    let mut child = Command::new(exe);
    child.arg("__boxed").args([scratch.path(), home]);
    child.args(wormhole_core::run::to_argv(args));
    if let Some(env) = env {
        child.env_clear().envs(env);
    }
    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(e) => fail(&format!("cannot spawn boxed child: {e}")),
    };
    // The box goes in the cgroup, this process does not: an OOM kill must
    // take the box and leave the thing that reports how it died.
    //
    // A refusal here is loud and kills the box. A manifest that asked for a
    // memory ceiling and silently got none is the exact failure §10's claim
    // would otherwise be papering over.
    if let Err(e) = apply_limits(limits, child.id()) {
        let _ = child.kill();
        let _ = child.wait();
        fail(&e);
    }
    ignore_terminal_signals();
    match child.wait() {
        Ok(status) => exit_code(wait_outcome(status)),
        Err(e) => fail(&format!("cannot wait for boxed child: {e}")),
    }
}

/// Host side of `wormhole build`: runs the manifest's script inside the
/// image being built, as root in the box. Returns once it has finished, so
/// the caller can rename a successful image into place or throw it away.
///
/// `artifacts` are files the host already fetched and proved; each is
/// bound read-only where the recipe said. `ca` is the host's bundle, for
/// a network that intercepts TLS: the build box terminates its own TLS for
/// whatever the recipe did not pin, so it needs the same trust the host
/// has or those fetches fail with a verify error naming no cause.
pub fn build_in(
    image: &Path,
    script: &str,
    dns: Option<IpAddr>,
    artifacts: &[(String, String)],
    ca: Option<String>,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own binary: {e}"))?;
    let scratch = tempfile::tempdir().map_err(|e| format!("cannot create box root dir: {e}"))?;
    // Serialized by the one serializer, so a new flag is added in one file
    // and the round-trip test is what proves it survives the re-exec.
    let args = RunArgs {
        grants: Vec::new(),
        dns,
        image: None,
        pidfile: None,
        ca,
        artifacts: artifacts.to_vec(),
        broker: None,
        network: Network::default(),
        root: RootMode::default(),
        command: vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
    };
    let mut child = Command::new(exe);
    child.arg("__build").args([scratch.path(), image]);
    child.args(run::to_argv(&args));
    let status = child
        .status()
        .map_err(|e| format!("cannot start the build box: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "the manifest's setup failed inside the box (exit {})",
            status.code().unwrap_or(-1)
        ))
    }
}

/// Joins a running box through its PID 1's namespaces and runs a command
/// inside — a second terminal into the same box. The environment is the
/// attacher's own apart from `HOME`, `HOSTNAME` and `PATH`; the box's
/// declared environment lives in its PID 1, which nothing can read back.
pub fn attach(init: u32, workspace: &Path, command: &[String]) -> ! {
    let user = match current_user() {
        Ok(user) => user,
        Err(e) => fail(&e),
    };
    // All four handles are opened before the first setns: after joining
    // the mount namespace this `/proc` is the box's, and after joining
    // the pid namespace these paths name someone else entirely.
    let handles: Vec<(fs::File, CloneFlags)> = [
        ("user", CloneFlags::CLONE_NEWUSER),
        ("uts", CloneFlags::CLONE_NEWUTS),
        ("pid", CloneFlags::CLONE_NEWPID),
        ("mnt", CloneFlags::CLONE_NEWNS),
    ]
    .into_iter()
    .map(|(name, flag)| {
        let path = format!("/proc/{init}/ns/{name}");
        match fs::File::open(&path) {
            Ok(file) => (file, flag),
            Err(e) => fail(&format!("cannot open {path}: {e}; is the box gone?")),
        }
    })
    .collect();
    // The user namespace comes first: owning it is what authorizes
    // joining the others.
    for (handle, flag) in &handles {
        if let Err(e) = nix::sched::setns(handle, *flag) {
            fail(&format!("cannot join the box's {flag:?}: {e}"));
        }
    }
    // Only children enter the joined pid namespace, so the command needs
    // its own process, and someone must stay outside to report how it went.
    #[expect(
        unsafe_code,
        reason = "only fork can put a process inside the joined PID namespace"
    )]
    let forked = unsafe { fork() };
    match forked {
        Ok(ForkResult::Parent { child }) => {
            ignore_terminal_signals();
            report(child)
        }
        Ok(ForkResult::Child) => {
            if let Err(e) = std::env::set_current_dir(workspace) {
                fail(&format!("cannot enter {}: {e}", workspace.display()));
            }
            exec(command, &home_in_box(&user))
        }
        Err(e) => fail(&format!("cannot fork into the box: {e}")),
    }
}

/// The terminal sends `Ctrl-C` and `Ctrl-\` to every process in the
/// foreground group, and the box is the only one that should react: if
/// the processes waiting on it died, the shell would take the prompt back
/// while the box still owned the terminal. Installed only after the child
/// exists, because `SIG_IGN` survives `execve` and would otherwise
/// silence the command too.
fn ignore_terminal_signals() {
    let ignore = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
    for signal in [Signal::SIGINT, Signal::SIGQUIT] {
        #[expect(unsafe_code, reason = "SigIgn installs no handler code")]
        let done = unsafe { sigaction(signal, &ignore) };
        if let Err(e) = done {
            fail(&format!("cannot ignore {signal}: {e}"));
        }
    }
}

/// Box side: unshares the namespaces and forks the box's PID 1, then
/// waits for it and reports how it ended.
pub fn boxed_child(args: &[String]) -> ! {
    let (scratch, home, tail) = match args {
        [scratch, home, tail @ ..] => (Path::new(scratch), Path::new(home), tail),
        _ => fail("expected <scratch> <home> [--grant <path>]... -- <command...>"),
    };
    let parsed = match wormhole_core::run::parse_args(tail) {
        Ok(parsed) => parsed,
        Err(e) => fail(&e.to_string()),
    };
    fork_into_box(identity_map, parsed.network, parsed.pidfile.clone(), || {
        box_init(scratch, home, &parsed)
    })
}

/// Box side of `wormhole build`: the image directory is the root and we
/// are uid 0 inside it, so a package manager can install into it.
/// `<scratch> <image> [--dns <address>] -- <script>`
pub fn boxed_build(args: &[String]) -> ! {
    let (scratch, image, tail) = match args {
        [scratch, image, tail @ ..] => (Path::new(scratch), Path::new(image), tail),
        _ => fail("expected <scratch> <image> [--dns <address>] -- <script>"),
    };
    let parsed = match wormhole_core::run::parse_args(tail) {
        Ok(parsed) => parsed,
        Err(e) => fail(&e.to_string()),
    };
    fork_into_box(root_map, parsed.network, None, || {
        build_init(scratch, image, &parsed)
    })
}

/// Unshares the namespaces, then forks the process that will be PID 1 of
/// the box and runs `init` in it. The parent records PID 1's host pid
/// when asked — `attach` enters the box through its `/proc/<pid>/ns` —
/// then only waits and reports.
fn fork_into_box(
    map: fn(u32) -> String,
    network: Network,
    pidfile: Option<String>,
    init: impl FnOnce() -> Infallible,
) -> ! {
    if let Err(e) = enter_namespaces(map, network) {
        fail(&e);
    }
    // Nothing to inherit but the namespaces: `wormhole` is single-threaded
    // by design, so this fork holds no lock and no half-written buffer.
    #[expect(
        unsafe_code,
        reason = "only fork can put a process inside the new PID namespace"
    )]
    let forked = unsafe { fork() };
    match forked {
        Ok(ForkResult::Parent { child }) => {
            if let Some(pidfile) = pidfile
                && let Err(e) = fs::write(&pidfile, child.as_raw().to_string())
            {
                fail(&format!("cannot write pidfile {pidfile}: {e}"));
            }
            ignore_terminal_signals();
            report(child)
        }
        Ok(ForkResult::Child) => match init() {},
        Err(e) => fail(&format!("cannot fork the box: {e}")),
    }
}

/// PID 1 inside the box: own mount namespace, planned root, then become
/// the command. Never returns to `boxed_child`.
fn box_init(scratch: &Path, home: &Path, args: &RunArgs) -> ! {
    let user = match current_user() {
        Ok(user) => user,
        Err(e) => fail(&e),
    };
    if let Err(e) = build_box(scratch, home, &user, args) {
        fail(&e);
    }
    // Last, because the mounts above needed CAP_SYS_ADMIN and nothing
    // after this point needs any capability at all.
    if let Err(e) = drop_all_capabilities() {
        fail(&e);
    }
    exec(&args.command, &home_in_box(&user))
}

/// Empties the capability bounding set and sets no-new-privs, then proves
/// both from the box's own `/proc/self/status`.
///
/// Real depth against a kernel bug reachable from a user namespace, which
/// is the weakness the banner admits to: a capability out of the bounding
/// set cannot be regained by this process or any child, and no-new-privs
/// means an `execve` can never raise privilege — not through setuid, not
/// through file capabilities.
///
/// The build box does not do this: it installs packages as root, and
/// `apk` genuinely needs `CAP_CHOWN`, `CAP_FOWNER` and `CAP_MKNOD` to
/// unpack one. That box holds nothing of yours and is thrown away.
fn drop_all_capabilities() -> Result<(), String> {
    // The kernel's own answer, so a capability added after this build is
    // dropped too rather than left behind by a stale constant.
    let last: i32 = fs::read_to_string("/proc/sys/kernel/cap_last_cap")
        .map_err(|e| format!("cannot read the kernel's capability count: {e}"))?
        .trim()
        .parse()
        .map_err(|e| format!("the kernel's capability count is not a number: {e}"))?;
    for capability in 0..=last {
        #[expect(unsafe_code, reason = "prctl has no safe wrapper in nix")]
        let dropped = unsafe {
            nix::libc::prctl(
                nix::libc::PR_CAPBSET_DROP,
                nix::libc::c_ulong::try_from(capability).unwrap_or(0),
                0,
                0,
                0,
            )
        };
        if dropped != 0 {
            return Err(format!(
                "cannot drop capability {capability}: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    nix::sys::prctl::set_no_new_privs().map_err(|e| format!("cannot set no-new-privs: {e}"))?;

    // Asserted, not assumed: the point of the drop is that it happened.
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|e| format!("cannot read the box's own status: {e}"))?;
    let facts = launch::LaunchFacts {
        retained_capabilities: launch::retained_capabilities(&status),
        ..launch::LaunchFacts::default()
    };
    launch::assert_launch(&facts).map_err(|violations| {
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// PID 1 inside the build box: the image as a writable root, and the
/// manifest's script as the command. The host's environment is dropped
/// entirely — an install script must not read your `NODE_OPTIONS`, your
/// proxy settings or your tokens, and nothing it needs comes from there.
fn build_init(scratch: &Path, image: &Path, args: &RunArgs) -> ! {
    let mut bound: Vec<(PathBuf, PathBuf)> = args
        .artifacts
        .iter()
        .map(|(source, target)| (PathBuf::from(source), PathBuf::from(target)))
        .collect();
    if let Some(ca) = &args.ca {
        bound.push((PathBuf::from(ca), PathBuf::from(ca::CA_BUNDLE_IN_BUILD)));
    }
    let ops = mount_plan::build_ops(image, args.dns, &bound);
    if let Err(e) = enter_and_pivot(scratch, &ops, Path::new("/")) {
        fail(&e);
    }
    clear_host_environment();
    if args.ca.is_some() {
        trust_the_bundle_in_build();
    }
    exec(&args.command, Path::new("/root"))
}

/// Points the build's TLS clients at the host bundle bound beside the
/// scratch — never over `/etc/ssl/certs/ca-certificates.crt`, which
/// `apk add ca-certificates` writes while the build is running and which
/// a read-only bind would break.
///
/// `SSL_CERT_DIR` is deliberately left alone. OpenSSL takes the union of
/// the file and the directory, so the image's own roots keep working and
/// the host's are added to them.
fn trust_the_bundle_in_build() {
    for (name, bundle) in ca::readers_pointing_at(ca::CA_BUNDLE_IN_BUILD) {
        #[expect(unsafe_code, reason = "single-threaded, and the next call is execvp")]
        unsafe {
            std::env::set_var(name, bundle);
        }
    }
}

fn clear_host_environment() {
    for (name, _) in std::env::vars_os() {
        #[expect(unsafe_code, reason = "single-threaded, and the next call is execvp")]
        unsafe {
            std::env::remove_var(name);
        }
    }
}

/// Replaces PID 1 with the command. The capabilities the mounts needed
/// die here, with nothing left that requires them.
fn exec(command: &[String], home: &Path) -> ! {
    let Some(program) = command.first() else {
        fail("no command to exec");
    };
    let argv = match command
        .iter()
        .map(|arg| CString::new(arg.as_bytes()))
        .collect::<Result<Vec<CString>, _>>()
    {
        Ok(argv) => argv,
        Err(e) => fail(&format!("command argument is not a C string: {e}")),
    };

    #[expect(unsafe_code, reason = "single-threaded, and the next call is execvp")]
    unsafe {
        std::env::set_var("HOME", home);
        std::env::set_var("HOSTNAME", BOX_HOSTNAME);
        std::env::set_var("PATH", BOX_PATH);
    }

    // `execvp` returns only on failure, so the error is the whole result.
    let errno = execvp(&argv[0], &argv).unwrap_err();
    eprintln!("wormhole: cannot start {program}: {errno}");
    exit(if errno == nix::errno::Errno::ENOENT {
        127
    } else {
        126
    })
}

/// Turns the box's wait status into our exit code.
fn report(box_init: Pid) -> ! {
    loop {
        match waitpid(box_init, None) {
            Ok(WaitStatus::Exited(_, code)) => exit(exit_code(WaitOutcome::Exited(code))),
            Ok(WaitStatus::Signaled(_, signal, _)) => {
                exit(exit_code(WaitOutcome::Signaled(signal as i32)));
            }
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => {}
            Err(e) => fail(&format!("cannot wait for the box: {e}")),
        }
    }
}

fn tempdir(purpose: &str) -> tempfile::TempDir {
    match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(e) => fail(&format!("cannot create {purpose} dir: {e}")),
    }
}

fn wait_outcome(status: std::process::ExitStatus) -> WaitOutcome {
    match (status.code(), status.signal()) {
        (Some(code), _) => WaitOutcome::Exited(code),
        (None, Some(signal)) => WaitOutcome::Signaled(signal),
        (None, None) => WaitOutcome::Exited(1),
    }
}

fn enter_namespaces(map: fn(u32) -> String, network: Network) -> Result<(), String> {
    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();
    // The network namespace joins the same unshare when it is asked for:
    // it must exist before the fork, so the box's PID 1 and everything
    // under it inherits it and nothing can be left in the host's.
    let mut flags = CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWUTS | CloneFlags::CLONE_NEWPID;
    if network == Network::None {
        flags |= CloneFlags::CLONE_NEWNET;
    }
    unshare(flags)
        .map_err(|e| format!("unshare(user, uts, pid) failed: {e}; run `wormhole doctor`"))?;
    write("/proc/self/setgroups", "deny")?;
    write("/proc/self/uid_map", &map(uid))?;
    write("/proc/self/gid_map", &map(gid))?;
    if network == Network::None {
        bring_up_loopback()?;
    }
    nix::unistd::sethostname(BOX_HOSTNAME).map_err(|e| format!("cannot set hostname: {e}"))
}

/// Puts this process — and so the whole box, because everything in it
/// descends from here — under a cgroup carrying the manifest's limits.
///
/// Refuses rather than warns when the limits cannot be applied. A box that
/// was told to cap its memory and silently did not is the failure this
/// exists to prevent, and CONCEPT.md §10 already claims the protection.
///
/// The cgroup is a child of our own, which is the only place an
/// unprivileged process may make one. Its parent must delegate the
/// controllers first, or the child's limit files are simply absent —
/// another way for a limit to do nothing quietly.
fn apply_limits(limits: &limits_cgroup::Limits, pid: u32) -> Result<(), String> {
    let files = limits.files().map_err(|e| e.to_string())?;
    if files.is_empty() {
        return Ok(());
    }
    // `0::/user.slice/...` — the v2 line, and on a v2-only host the only
    // line. Anything else means no unified hierarchy to hang this on.
    let own = fs::read_to_string("/proc/self/cgroup")
        .map_err(|e| format!("cannot read this process's cgroup: {e}"))?
        .lines()
        .find_map(|line| line.strip_prefix("0::").map(str::to_owned))
        .ok_or_else(|| {
            "[limits] needs cgroup v2 and this host has no unified hierarchy; run `wormhole doctor`"
                .to_owned()
        })?;
    let parent = PathBuf::from("/sys/fs/cgroup").join(own.trim_start_matches('/'));

    // Enabling a controller in the parent is what makes its file exist in
    // the child. Written before the directory, because after it the parent
    // has a child and the kernel refuses the change.
    let wanted = limits.controllers();
    let enable: String = wanted.iter().map(|c| format!("+{c} ")).collect();
    fs::write(parent.join("cgroup.subtree_control"), enable.trim_end()).map_err(|e| {
        format!(
            "cannot delegate {} to {}: {e}; cgroup v2 delegation is what [limits] needs",
            wanted.join(", "),
            parent.display()
        )
    })?;

    let group = parent.join(format!("wormhole-{pid}"));
    fs::create_dir_all(&group).map_err(|e| format!("cannot create {}: {e}", group.display()))?;
    for (file, content) in files {
        fs::write(group.join(file), &content)
            .map_err(|e| format!("cannot write {file} = {content}: {e}"))?;
    }
    // Last: the limits are already in place when the box arrives, and
    // everything the box forks after this inherits the cgroup.
    fs::write(group.join("cgroup.procs"), pid.to_string())
        .map_err(|e| format!("cannot put the box into {}: {e}", group.display()))
}

/// A fresh network namespace has a loopback device and it is *down*, so
/// even `127.0.0.1` fails until it is raised. Plenty of tooling binds
/// loopback and would break for a reason that has nothing to do with the
/// isolation being asked for.
///
/// Done with an ioctl rather than by running `ip`: the box's image is not
/// required to contain one, and this is host-side code anyway.
fn bring_up_loopback() -> Result<(), String> {
    // `struct ifreq` is a 16-byte name followed by a union; only the first
    // `short` of that union — the flags — is touched here.
    #[repr(C)]
    struct IfReq {
        name: [libc::c_char; libc::IF_NAMESIZE],
        flags: libc::c_short,
        _pad: [u8; 22],
    }

    #[expect(unsafe_code, reason = "SIOCSIFFLAGS has no safe wrapper in nix")]
    unsafe {
        let socket = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0);
        if socket < 0 {
            return Err(format!(
                "cannot open a socket to raise loopback: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut request = IfReq {
            name: [0; libc::IF_NAMESIZE],
            flags: 0,
            _pad: [0; 22],
        };
        request.name[0] = b'l' as libc::c_char;
        request.name[1] = b'o' as libc::c_char;
        request.flags = libc::IFF_UP as libc::c_short | libc::IFF_RUNNING as libc::c_short;
        // The request argument is a `c_int` on musl and a `c_ulong` on
        // glibc; the cast takes whichever this target's `ioctl` declares.
        let raised = libc::ioctl(socket, libc::SIOCSIFFLAGS as _, &raw const request);
        let error = std::io::Error::last_os_error();
        libc::close(socket);
        if raised < 0 {
            return Err(format!("cannot raise loopback in the box: {error}"));
        }
    }
    Ok(())
}

/// Applies the plan into `scratch` and pivots into it.
fn build_box(scratch: &Path, home: &Path, user: &User, args: &RunArgs) -> Result<(), String> {
    let workspace =
        std::env::current_dir().map_err(|e| format!("cannot read working directory: {e}"))?;
    let mut grants = resolve_grants(&args.grants)?;
    if let Some(ca) = &args.ca {
        let real =
            fs::canonicalize(ca).map_err(|e| format!("cannot resolve the CA bundle {ca}: {e}"))?;
        grants.push(mount_plan::Grant::retargeted(
            real,
            wormhole_core::ca::CA_BUNDLE_IN_BOX,
        ));
    }
    if let Some(socket) = &args.broker {
        // The socket, and the binary that speaks to it. Both read-only —
        // the box needs to talk through the broker, never to change it.
        let real = fs::canonicalize(socket)
            .map_err(|e| format!("cannot resolve the broker socket {socket}: {e}"))?;
        grants.push(mount_plan::Grant::retargeted(
            real,
            wormhole_core::broker::SOCKET_IN_BOX,
        ));
        let exe = std::env::current_exe().map_err(|e| format!("cannot find own binary: {e}"))?;
        grants.push(mount_plan::Grant::retargeted(
            fs::canonicalize(&exe).unwrap_or(exe),
            wormhole_core::broker::WORMHOLE_IN_BOX,
        ));
    }
    let image = args.image.as_ref().map(PathBuf::from);
    let root = match (&image, args.root) {
        (Some(image), RootMode::Copy) => Root::Image(image),
        (Some(image), RootMode::Readonly) => Root::ImageReadOnly(image),
        (None, _) => Root::HostUsr,
    };
    let ops = mount_plan::compute(&workspace, home, root, user, &grants, args.dns)
        .map_err(|e| format!("mount plan refused: {e}"))?;
    enter_and_pivot(scratch, &ops, &workspace)
}

/// Own mount namespace, every op applied into `scratch`, pivot into it,
/// then stand in `cwd`. The one path both boxes take.
fn enter_and_pivot(scratch: &Path, ops: &[MountOp], cwd: &Path) -> Result<(), String> {
    unshare(CloneFlags::CLONE_NEWNS)
        .map_err(|e| format!("unshare(mount) failed: {e}; run `wormhole doctor`"))?;
    make_host_mounts_private()?;
    for op in ops {
        apply(scratch, op)?;
    }
    pivot(scratch)?;
    assert_no_unplanned_rw_mount(ops)?;
    std::env::set_current_dir(cwd).map_err(|e| format!("cannot enter {}: {e}", cwd.display()))
}

/// CONCEPT.md §8: every launch proves the boundary and refuses if it
/// cannot. The kernel's own mount table is read back after the pivot and
/// every read-write mount in it must be one the plan asked for.
///
/// This is the ratchet. The plan is exhaustively tested but the plan is
/// not what the box runs behind — the mounts are — and until now nothing
/// checked that those two agreed.
fn assert_no_unplanned_rw_mount(ops: &[MountOp]) -> Result<(), String> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|e| format!("cannot read the box's own mount table: {e}"))?;
    let planned: Vec<&Path> = ops.iter().filter_map(MountOp::mount_point).collect();
    let facts = launch::LaunchFacts {
        unplanned_rw_mounts: launch::rw_mount_points(&mountinfo)
            .into_iter()
            .filter(|point| !planned.contains(&point.as_path()))
            .collect(),
        ..launch::LaunchFacts::default()
    };
    launch::assert_launch(&facts).map_err(|violations| {
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })
}

/// A granted path is bound read-write at the same path inside the box.
/// The plan decides whether that is allowed; here we only report what the
/// host says the path really is, so a symlink cannot smuggle in a target.
fn resolve_grants(requested: &[String]) -> Result<Vec<mount_plan::Grant>, String> {
    requested
        .iter()
        .map(|path| {
            let requested = PathBuf::from(path);
            // A path the plan will reject anyway is passed through
            // untouched, so its own error is the one the user reads.
            let resolved = if requested.is_absolute() {
                fs::canonicalize(&requested)
                    .map_err(|e| format!("cannot resolve grant {path}: {e}"))?
            } else {
                requested.clone()
            };
            Ok(mount_plan::Grant {
                target: requested.clone(),
                requested,
                resolved,
                rw: true,
            })
        })
        .collect()
}

fn current_user() -> Result<User, String> {
    let uid = nix::unistd::getuid();
    let gid = nix::unistd::getgid();
    let name = nix::unistd::User::from_uid(uid)
        .map_err(|e| format!("cannot look up user {uid}: {e}"))?
        .ok_or_else(|| format!("no passwd entry for uid {uid}"))?
        .name;
    Ok(User {
        name,
        uid: uid.as_raw(),
        gid: gid.as_raw(),
    })
}

/// Nothing built inside the box may propagate back to the host.
fn make_host_mounts_private() -> Result<(), String> {
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .map_err(|e| format!("cannot make mounts private: {e}"))
}

fn apply(root: &Path, op: &MountOp) -> Result<(), String> {
    match op {
        MountOp::TmpfsRoot => mount_tmpfs(root, 0o755),
        MountOp::Tmpfs { target, mode } => {
            let target = make_dir(root, target)?;
            mount_tmpfs(&target, *mode)
        }
        MountOp::File { target, content } => {
            let target = in_root(root, target);
            create_parent(&target)?;
            fs::write(&target, content)
                .map_err(|e| format!("cannot write {}: {e}", target.display()))
        }
        MountOp::TmpfsFrom { target, seed, mode } => {
            let target = make_dir(root, target)?;
            mount_tmpfs(&target, *mode)?;
            // The seed is the image's own directory on the host, named
            // before the pivot, so it is still readable now that an empty
            // tmpfs covers where it will land.
            let copied = Command::new("cp")
                .args(["--archive", "--reflink=auto"])
                .arg(format!("{}/.", seed.display()))
                .arg(&target)
                .status()
                .map_err(|e| format!("cannot seed {}: {e}", target.display()))?;
            if copied.success() {
                Ok(())
            } else {
                Err(format!(
                    "cannot seed {} from {}: cp exited {}",
                    target.display(),
                    seed.display(),
                    copied.code().unwrap_or(-1)
                ))
            }
        }
        MountOp::Bind { source, target, rw } => bind(root, source, target, *rw),
        MountOp::Device { path } => bind(root, path, path, true),
        MountOp::Proc { target } => {
            let target = make_dir(root, target)?;
            mount(
                Some("proc"),
                &target,
                Some("proc"),
                MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC,
                None::<&str>,
            )
            .map_err(|e| format!("cannot mount proc on {}: {e}", target.display()))
        }
        // `newinstance` keeps the host's terminals out; `ptmxmode` lets
        // an unprivileged user open new ones through `pts/ptmx`.
        MountOp::DevPts { target } => {
            let target = make_dir(root, target)?;
            mount(
                Some("devpts"),
                &target,
                Some("devpts"),
                MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
                Some("newinstance,ptmxmode=0666,mode=0620"),
            )
            .map_err(|e| format!("cannot mount devpts on {}: {e}", target.display()))
        }
        MountOp::Symlink { link, to } => {
            let link = in_root(root, link);
            std::os::unix::fs::symlink(to, &link)
                .map_err(|e| format!("cannot symlink {}: {e}", link.display()))
        }
    }
}

fn bind(root: &Path, source: &Path, target: &Path, rw: bool) -> Result<(), String> {
    let target = in_root(root, target);
    if source.is_dir() {
        fs::create_dir_all(&target)
            .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
    } else {
        create_parent(&target)?;
        fs::File::create(&target)
            .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
    }
    mount(
        Some(source),
        &target,
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .map_err(|e| {
        format!(
            "cannot bind {} -> {}: {e}",
            source.display(),
            target.display()
        )
    })?;
    if rw { Ok(()) } else { remount_ro(&target) }
}

/// Read-only remount of a bind. The kernel refuses to drop flags a less
/// privileged userns inherited, so every flag the mount already carries
/// must be repeated alongside MS_RDONLY.
fn remount_ro(target: &Path) -> Result<(), String> {
    use nix::sys::statvfs::{FsFlags, statvfs};

    let held = statvfs(target)
        .map_err(|e| format!("cannot statvfs {}: {e}", target.display()))?
        .flags();
    // musl's statvfs has no ST_RELATIME bit, so nix omits the constant
    // there; the box binary only needs it on the glibc host.
    #[cfg(target_env = "gnu")]
    const RELATIME: &[(FsFlags, MsFlags)] = &[(FsFlags::ST_RELATIME, MsFlags::MS_RELATIME)];
    #[cfg(not(target_env = "gnu"))]
    const RELATIME: &[(FsFlags, MsFlags)] = &[];

    let mut flags = MsFlags::MS_BIND | MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY;
    for &(fs_flag, ms_flag) in [
        (FsFlags::ST_NOSUID, MsFlags::MS_NOSUID),
        (FsFlags::ST_NODEV, MsFlags::MS_NODEV),
        (FsFlags::ST_NOEXEC, MsFlags::MS_NOEXEC),
        (FsFlags::ST_NOATIME, MsFlags::MS_NOATIME),
        (FsFlags::ST_NODIRATIME, MsFlags::MS_NODIRATIME),
    ]
    .iter()
    .chain(RELATIME)
    {
        if held.contains(fs_flag) {
            flags |= ms_flag;
        }
    }
    mount(None::<&str>, target, None::<&str>, flags, None::<&str>)
        .map_err(|e| format!("cannot remount {} read-only: {e}", target.display()))
}

fn mount_tmpfs(target: &Path, mode: u32) -> Result<(), String> {
    mount(
        Some("tmpfs"),
        target,
        Some("tmpfs"),
        MsFlags::empty(),
        Some(format!("mode={mode:o}").as_str()),
    )
    .map_err(|e| format!("cannot mount tmpfs on {}: {e}", target.display()))
}

fn pivot(new_root: &Path) -> Result<(), String> {
    nix::unistd::chdir(new_root).map_err(|e| format!("cannot chdir to new root: {e}"))?;
    nix::unistd::pivot_root(".", ".").map_err(|e| format!("pivot_root failed: {e}"))?;
    umount2(".", MntFlags::MNT_DETACH).map_err(|e| format!("cannot detach old root: {e}"))?;
    nix::unistd::chdir("/").map_err(|e| format!("cannot chdir to /: {e}"))
}

fn make_dir(root: &Path, target: &Path) -> Result<PathBuf, String> {
    let target = in_root(root, target);
    fs::create_dir_all(&target).map_err(|e| format!("cannot create {}: {e}", target.display()))?;
    Ok(target)
}

fn in_root(root: &Path, absolute: &Path) -> PathBuf {
    let relative = absolute.strip_prefix("/").unwrap_or(absolute);
    root.join(relative)
}

fn create_parent(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display())),
        None => Ok(()),
    }
}

fn write(path: &str, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("cannot write {path}: {e}"))
}

/// Anything wormhole itself could not do: exit 125, the code `env` and
/// `docker run` use for "the tool failed, not your command".
fn fail(message: &str) -> ! {
    eprintln!("wormhole: {message}");
    exit(125)
}

/// `_exit`, never `exit`: after the fork two processes share one set of
/// buffers, and neither may flush what the other wrote.
fn exit(code: i32) -> ! {
    #[expect(unsafe_code, reason = "_exit is the only exit that flushes nothing")]
    unsafe {
        nix::libc::_exit(code)
    }
}
