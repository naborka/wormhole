//! Fact gathering for `wormhole doctor`. Each probe reads a file, runs a
//! syscall, or spawns a child; the judgement — message and severity —
//! lives in `wormhole_core::doctor::outcome`.

use std::fs;
use std::process::Command;

use wormhole_core::doctor::{Outcome, Probe, ProbeResult, outcome};

pub fn run_all() -> Vec<ProbeResult> {
    Probe::ALL
        .into_iter()
        .map(|probe| ProbeResult {
            probe,
            outcome: run(probe),
        })
        .collect()
}

fn run(probe: Probe) -> Outcome {
    match probe {
        Probe::UserNamespaces => outcome::userns(self_probe(ChildProbe::Userns)),
        Probe::MaxUserNamespaces => max_user_namespaces(),
        Probe::SubIdRanges => subid_ranges(),
        Probe::Landlock => outcome::landlock(landlock_ruleset()),
        Probe::Overlayfs => {
            outcome::overlayfs(self_probe(ChildProbe::Overlayfs), which("fuse-overlayfs"))
        }
        Probe::CgroupDelegation => cgroup_delegation(),
        Probe::Reflink => outcome::reflink(reflink_copy()),
        Probe::UidMapOverlap => outcome::uid_map_overlap(
            self_probe(ChildProbe::UidMap),
            nix::unistd::geteuid().is_root(),
        ),
    }
}

/// Read `path` and judge its content; a read error is its own failure.
fn read_and_check(path: &str, check: impl FnOnce(&str) -> Outcome) -> Outcome {
    match fs::read_to_string(path) {
        Ok(content) => check(&content),
        Err(e) => Outcome::Fail(format!("cannot read {path}: {e}")),
    }
}

fn max_user_namespaces() -> Outcome {
    read_and_check(
        "/proc/sys/user/max_user_namespaces",
        outcome::max_user_namespaces,
    )
}

fn subid_ranges() -> Outcome {
    let uid = nix::unistd::getuid();
    let user = nix::unistd::User::from_uid(uid)
        .ok()
        .flatten()
        .map(|u| u.name)
        .unwrap_or_default();

    for path in ["/etc/subuid", "/etc/subgid"] {
        let result = read_and_check(path, |content| {
            outcome::subid(content, &user, uid.as_raw(), path)
        });
        if result != Outcome::Pass {
            return result;
        }
    }
    Outcome::Pass
}

/// A real `landlock_create_ruleset` for ABI v1 — LSM presence alone would
/// pass on a kernel whose Landlock is enabled but too old.
fn landlock_ruleset() -> Result<(), String> {
    use landlock::{ABI, Access, AccessFs, Ruleset, RulesetAttr};
    Ruleset::default()
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|r| r.create())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn cgroup_delegation() -> Outcome {
    let uid = nix::unistd::getuid();
    let path =
        format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/cgroup.controllers");
    read_and_check(&path, outcome::cgroup_delegation)
}

/// `cp --reflink=always` in the invocation directory tells us whether a
/// box root copy here is free or a full copy of the image.
fn reflink_copy() -> Result<bool, String> {
    let dir = tempfile::tempdir_in(".").map_err(|e| format!("cannot create temp dir here: {e}"))?;
    let src = dir.path().join("src");
    fs::write(&src, b"probe").map_err(|e| format!("cannot write probe file: {e}"))?;
    let output = Command::new("cp")
        .arg("--reflink=always")
        .arg(&src)
        .arg(dir.path().join("dst"))
        .output()
        .map_err(|e| format!("cannot run cp: {e}"))?;
    Ok(output.status.success())
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
}

/// The parent/child probe protocol: one name per experiment that must run
/// in its own process so a failed unshare cannot pollute this one.
#[derive(Clone, Copy)]
enum ChildProbe {
    Userns,
    Overlayfs,
    UidMap,
}

impl ChildProbe {
    fn name(self) -> &'static str {
        match self {
            ChildProbe::Userns => "userns",
            ChildProbe::Overlayfs => "overlayfs",
            ChildProbe::UidMap => "uidmap",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::Userns, Self::Overlayfs, Self::UidMap]
            .into_iter()
            .find(|p| p.name() == name)
    }

    fn run(self) -> Result<(), String> {
        match self {
            ChildProbe::Userns => enter_userns(nix::sched::CloneFlags::empty()),
            ChildProbe::Overlayfs => child_overlayfs(),
            ChildProbe::UidMap => child_uidmap(),
        }
    }
}

/// Runs `wormhole __probe <name>` and reports the child's outcome.
fn self_probe(probe: ChildProbe) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = Command::new(exe)
        .args(["__probe", probe.name()])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Child side of `self_probe`. Exits the process.
pub fn child_probe(name: &str) -> ! {
    let result = match ChildProbe::parse(name) {
        Some(probe) => probe.run(),
        None => Err(format!("unknown probe {name}")),
    };
    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// Unshare into a new user namespace (plus `extra` namespaces) and map
/// our own uid/gid to 0 — the preamble every child experiment shares.
fn enter_userns(extra: nix::sched::CloneFlags) -> Result<(), String> {
    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();
    nix::sched::unshare(nix::sched::CloneFlags::CLONE_NEWUSER | extra)
        .map_err(|e| e.to_string())?;
    fs::write("/proc/self/setgroups", "deny").map_err(|e| e.to_string())?;
    fs::write("/proc/self/uid_map", format!("0 {uid} 1")).map_err(|e| e.to_string())?;
    fs::write("/proc/self/gid_map", format!("0 {gid} 1")).map_err(|e| e.to_string())?;
    Ok(())
}

/// Two inside uids mapped to one outside uid — kernel `mappings_overlap()`
/// should reject the write.
fn child_uidmap() -> Result<(), String> {
    let uid = nix::unistd::getuid().as_raw();
    nix::sched::unshare(nix::sched::CloneFlags::CLONE_NEWUSER).map_err(|e| e.to_string())?;
    fs::write("/proc/self/uid_map", format!("0 {uid} 1\n1 {uid} 1\n")).map_err(|e| e.to_string())
}

fn child_overlayfs() -> Result<(), String> {
    use nix::mount::{MsFlags, mount};
    use nix::sched::CloneFlags;

    enter_userns(CloneFlags::CLONE_NEWNS)?;

    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (lower, upper, work, merged) = (
        dir.path().join("lower"),
        dir.path().join("upper"),
        dir.path().join("work"),
        dir.path().join("merged"),
    );
    for d in [&lower, &upper, &work, &merged] {
        fs::create_dir(d).map_err(|e| e.to_string())?;
    }
    let opts = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower.display(),
        upper.display(),
        work.display()
    );
    mount(
        Some("overlay"),
        &merged,
        Some("overlay"),
        MsFlags::empty(),
        Some(opts.as_str()),
    )
    .map_err(|e| e.to_string())
}
