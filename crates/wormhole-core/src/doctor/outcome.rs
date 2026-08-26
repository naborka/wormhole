//! One constructor per probe: facts in, judged `Outcome` out. The impl
//! crate gathers the facts and never writes a message or picks a
//! severity — required probes fail here, informational ones inform here.

use super::Outcome;
use super::parse;

pub fn userns(unshare: Result<(), String>) -> Outcome {
    match unshare {
        Ok(()) => Outcome::Pass,
        Err(e) => Outcome::Fail(format!("unshare(CLONE_NEWUSER) failed: {e}")),
    }
}

pub fn max_user_namespaces(content: &str) -> Outcome {
    if parse::max_user_namespaces_ok(content) {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "/proc/sys/user/max_user_namespaces is {}",
            content.trim()
        ))
    }
}

pub fn subid(content: &str, user: &str, uid: u32, path: &str) -> Outcome {
    if parse::has_subid_range(content, user, uid) {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("no range for {user} (uid {uid}) in {path}"))
    }
}

pub fn landlock(create: Result<(), String>) -> Outcome {
    match create {
        Ok(()) => Outcome::Pass,
        Err(e) => Outcome::Fail(format!("Landlock ABI v1 ruleset creation failed: {e}")),
    }
}

/// Passes when an unprivileged overlay mount works, or when
/// `fuse-overlayfs` is present as the fallback.
pub fn overlayfs(mount: Result<(), String>, fuse_present: bool) -> Outcome {
    match mount {
        Ok(()) => Outcome::Pass,
        Err(_) if fuse_present => Outcome::Pass,
        Err(e) => Outcome::Fail(format!(
            "unprivileged overlay mount failed ({e}) and fuse-overlayfs is not in PATH"
        )),
    }
}

pub fn cgroup_delegation(content: &str) -> Outcome {
    let missing = parse::missing_cgroup_controllers(content);
    if missing.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail(format!("controllers not delegated: {}", missing.join(", ")))
    }
}

/// `copied` is whether `cp --reflink=always` succeeded; `Err` is a
/// failure to run the experiment at all. Informational either way —
/// reflink only decides stage-2 snapshot feasibility.
pub fn reflink(copied: Result<bool, String>) -> Outcome {
    match copied {
        Ok(true) => Outcome::Pass,
        Ok(false) => Outcome::Info(
            "this filesystem does not support reflink; stage-2 snapshots would copy".to_owned(),
        ),
        Err(e) => Outcome::Info(format!("probe could not run: {e}")),
    }
}

pub fn microvm_devices(missing: &[&str]) -> Outcome {
    if missing.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Info(format!(
            "missing: {} — MicroVM boundary unavailable",
            missing.join(", ")
        ))
    }
}

/// Assumption #1: can a uid map point two inside uids at one outside
/// uid? Without CAP_SETUID the write fails EPERM either way, so the
/// result is conclusive only as root. Informational always — it decides
/// a design alternative, not whether this host works.
pub fn uid_map_overlap(write: Result<(), String>, is_root: bool) -> Outcome {
    match write {
        Ok(()) => Outcome::Info(
            "overlapping uid map ACCEPTED — assumption #1 is false; \
             uid-per-agent separation is possible"
                .to_owned(),
        ),
        Err(e) if is_root => Outcome::Info(format!(
            "overlapping uid map rejected as root ({e}) — assumption #1 holds"
        )),
        Err(e) => Outcome::Info(format!(
            "inconclusive without root ({e}); to decide assumption #1 run: sudo wormhole doctor"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err() -> Result<(), String> {
        Err("EPERM".to_owned())
    }

    #[test]
    fn userns_judges_unshare_result() {
        assert_eq!(userns(Ok(())), Outcome::Pass);
        assert_eq!(
            userns(err()),
            Outcome::Fail("unshare(CLONE_NEWUSER) failed: EPERM".to_owned())
        );
    }

    #[test]
    fn max_user_namespaces_names_the_bad_value() {
        assert_eq!(max_user_namespaces("63414\n"), Outcome::Pass);
        assert_eq!(
            max_user_namespaces("0\n"),
            Outcome::Fail("/proc/sys/user/max_user_namespaces is 0".to_owned())
        );
    }

    #[test]
    fn subid_names_user_and_file() {
        assert_eq!(
            subid("nabor:100000:65536\n", "nabor", 1000, "/etc/subuid"),
            Outcome::Pass
        );
        assert_eq!(
            subid("", "nabor", 1000, "/etc/subgid"),
            Outcome::Fail("no range for nabor (uid 1000) in /etc/subgid".to_owned())
        );
    }

    #[test]
    fn landlock_judges_ruleset_creation() {
        assert_eq!(landlock(Ok(())), Outcome::Pass);
        assert_eq!(
            landlock(err()),
            Outcome::Fail("Landlock ABI v1 ruleset creation failed: EPERM".to_owned())
        );
    }

    #[test]
    fn overlayfs_mount_success_passes_regardless_of_fuse() {
        assert_eq!(overlayfs(Ok(()), false), Outcome::Pass);
        assert_eq!(overlayfs(Ok(()), true), Outcome::Pass);
    }

    #[test]
    fn overlayfs_mount_failure_passes_only_with_fuse_fallback() {
        assert_eq!(overlayfs(err(), true), Outcome::Pass);
        assert_eq!(
            overlayfs(err(), false),
            Outcome::Fail(
                "unprivileged overlay mount failed (EPERM) and fuse-overlayfs is not in PATH"
                    .to_owned()
            )
        );
    }

    #[test]
    fn cgroup_delegation_lists_missing_controllers() {
        assert_eq!(
            cgroup_delegation("cpuset cpu io memory pids\n"),
            Outcome::Pass
        );
        assert_eq!(
            cgroup_delegation("cpuset io memory\n"),
            Outcome::Fail("controllers not delegated: cpu, pids".to_owned())
        );
    }

    #[test]
    fn reflink_is_informational_in_every_branch() {
        assert_eq!(reflink(Ok(true)), Outcome::Pass);
        assert!(matches!(reflink(Ok(false)), Outcome::Info(_)));
        assert!(matches!(reflink(Err("no cp".to_owned())), Outcome::Info(_)));
    }

    #[test]
    fn microvm_devices_reports_what_is_missing_as_info() {
        assert_eq!(microvm_devices(&[]), Outcome::Pass);
        assert_eq!(
            microvm_devices(&["/dev/kvm"]),
            Outcome::Info("missing: /dev/kvm — MicroVM boundary unavailable".to_owned())
        );
    }

    #[test]
    fn uid_map_overlap_is_informational_and_conclusive_only_as_root() {
        assert!(
            uid_map_overlap(Ok(()), false).eq(&Outcome::Info(
                "overlapping uid map ACCEPTED — assumption #1 is false; \
                     uid-per-agent separation is possible"
                    .to_owned()
            ))
        );
        assert_eq!(
            uid_map_overlap(err(), true),
            Outcome::Info(
                "overlapping uid map rejected as root (EPERM) — assumption #1 holds".to_owned()
            )
        );
        assert_eq!(
            uid_map_overlap(err(), false),
            Outcome::Info(
                "inconclusive without root (EPERM); to decide assumption #1 run: \
                 sudo wormhole doctor"
                    .to_owned()
            )
        );
    }
}
