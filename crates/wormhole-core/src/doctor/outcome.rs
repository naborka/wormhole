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

/// Informational: wormhole maps only your own uid into a box — a single
/// `{uid} {uid} 1` line, or `0 {uid} 1` while building — which needs no
/// `/etc/subuid` range and no `newuidmap`. A range is for mapping *other*
/// or *several* uids, which wormhole never does, so its absence stops
/// nothing. Reported because a person expecting the usual container
/// prerequisite should hear it is simply not one here.
pub fn subid(content: &str, user: &str, uid: u32, path: &str) -> Outcome {
    if parse::has_subid_range(content, user, uid) {
        Outcome::Pass
    } else {
        Outcome::Info(format!(
            "no range for {user} (uid {uid}) in {path}; not needed — \
             a box maps only your own uid"
        ))
    }
}

/// Informational: Landlock is a probed prerequisite the boundary does not
/// yet apply (CONCEPT.md §10), so its absence blocks no box today. Kept as
/// a probe so the day the boundary adds it, a host that cannot is already
/// visible.
pub fn landlock(create: Result<(), String>) -> Outcome {
    match create {
        Ok(()) => Outcome::Pass,
        Err(e) => Outcome::Info(format!(
            "Landlock ABI v1 unavailable ({e}); the boundary does not yet apply it"
        )),
    }
}

/// Informational: wormhole builds a box from a copied (or read-only bound)
/// image tree, never an overlay mount, so overlayfs is not a prerequisite.
/// The probe stays because digest-pinned layers would want it, and a host
/// that has it is worth knowing.
pub fn overlayfs(mount: Result<(), String>, fuse_present: bool) -> Outcome {
    match mount {
        Ok(()) => Outcome::Pass,
        Err(_) if fuse_present => Outcome::Pass,
        Err(e) => Outcome::Info(format!(
            "unprivileged overlay unavailable ({e}) and no fuse-overlayfs; \
             not needed — a box copies its image"
        )),
    }
}

/// Informational: only `[limits]` needs a delegated cgroup hierarchy, and a
/// start that asks for a limit it cannot apply already refuses loudly. A
/// box with no `[limits]` runs without it, so its absence is not a reason
/// the host cannot run wormhole.
pub fn cgroup_delegation(content: &str) -> Outcome {
    let missing = parse::missing_cgroup_controllers(content);
    if missing.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Info(format!(
            "controllers not delegated: {}; needed only for [limits]",
            missing.join(", ")
        ))
    }
}

/// `copied` is whether `cp --reflink=always` succeeded; `Err` is a
/// failure to run the experiment at all. Informational either way:
/// without reflink a box still starts, from a full copy of its image.
pub fn reflink(copied: Result<bool, String>) -> Outcome {
    match copied {
        Ok(true) => Outcome::Pass,
        Ok(false) => Outcome::Info(
            "this filesystem does not support reflink; every box start copies its whole image"
                .to_owned(),
        ),
        Err(e) => Outcome::Info(format!("probe could not run: {e}")),
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

    /// A missing subid range is informational, not a failure: wormhole
    /// maps only the invoking uid, which needs none. Proven by boxes that
    /// run on a host with no `/etc/subuid` at all.
    #[test]
    fn a_missing_subid_range_does_not_block_the_host() {
        assert_eq!(
            subid("nabor:100000:65536\n", "nabor", 1000, "/etc/subuid"),
            Outcome::Pass
        );
        assert!(matches!(
            subid("", "nabor", 1000, "/etc/subgid"),
            Outcome::Info(_)
        ));
    }

    /// Landlock is not applied yet, so its absence informs rather than
    /// blocks — a Fail here would say the host cannot run a box it can.
    #[test]
    fn landlock_is_informational_until_the_boundary_applies_it() {
        assert_eq!(landlock(Ok(())), Outcome::Pass);
        assert!(matches!(landlock(err()), Outcome::Info(_)));
    }

    #[test]
    fn overlayfs_mount_success_passes_regardless_of_fuse() {
        assert_eq!(overlayfs(Ok(()), false), Outcome::Pass);
        assert_eq!(overlayfs(Ok(()), true), Outcome::Pass);
    }

    /// wormhole copies its image rather than stacking it, so a host with
    /// no unprivileged overlay still runs boxes: the probe informs.
    #[test]
    fn overlayfs_absence_is_informational_not_a_failure() {
        assert_eq!(overlayfs(err(), true), Outcome::Pass);
        assert!(matches!(overlayfs(err(), false), Outcome::Info(_)));
    }

    /// Only `[limits]` needs delegation, and a start that cannot apply a
    /// limit refuses on its own, so the missing controllers inform here.
    #[test]
    fn cgroup_delegation_informs_because_only_limits_need_it() {
        assert_eq!(
            cgroup_delegation("cpuset cpu io memory pids\n"),
            Outcome::Pass
        );
        assert!(matches!(
            cgroup_delegation("cpuset io memory\n"),
            Outcome::Info(_)
        ));
    }

    #[test]
    fn reflink_is_informational_in_every_branch() {
        assert_eq!(reflink(Ok(true)), Outcome::Pass);
        assert!(matches!(reflink(Ok(false)), Outcome::Info(_)));
        assert!(matches!(reflink(Err("no cp".to_owned())), Outcome::Info(_)));
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
