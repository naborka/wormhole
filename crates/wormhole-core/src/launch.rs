//! Launch assertions and the banner. Facts come from real probes in the
//! impl crate; every refusal decision is made here. Failure is refusal,
//! never a warning — the agent is assumed hostile (CONCEPT.md §8).

use core::fmt;
use std::path::PathBuf;

/// What the pre-launch probes observed about the box about to start.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchFacts {
    /// Set when the base rootfs digest does not match its pin.
    pub base_digest_mismatch: Option<DigestMismatch>,
    /// Credential paths that turned out readable from inside the box.
    pub readable_credentials: Vec<PathBuf>,
    /// Network routes that exist besides the broker socket.
    pub unexpected_routes: Vec<String>,
    /// Host paths mounted rw that the mount plan did not put there.
    pub unplanned_rw_mounts: Vec<PathBuf>,
    /// Capabilities still in the bounding set that must be gone.
    pub retained_capabilities: Vec<String>,
    /// Pid of another wormhole holding this workspace's lockfile.
    pub lock_held_by: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestMismatch {
    pub expected: String,
    pub actual: String,
}

/// One violated launch assertion. Each variant is its own distinct,
/// actionable refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    BaseDigestMismatch { expected: String, actual: String },
    CredentialReadable(PathBuf),
    RouteExists(String),
    UnplannedRwMount(PathBuf),
    CapabilityRetained(String),
    WorkspaceLocked { pid: u32 },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::BaseDigestMismatch { expected, actual } => write!(
                f,
                "base rootfs digest mismatch: expected {expected}, got {actual}"
            ),
            Violation::CredentialReadable(p) => write!(
                f,
                "credential file {} is readable inside the box",
                p.display()
            ),
            Violation::RouteExists(r) => {
                write!(f, "network route exists beyond the broker: {r}")
            }
            Violation::UnplannedRwMount(p) => write!(
                f,
                "host path {} is mounted rw but is not in the plan",
                p.display()
            ),
            Violation::CapabilityRetained(c) => {
                write!(f, "capability {c} is still in the bounding set")
            }
            Violation::WorkspaceLocked { pid } => {
                write!(f, "workspace is locked by another wormhole (pid {pid})")
            }
        }
    }
}

/// Refuse to launch unless every assertion holds. All violations are
/// reported, not just the first.
pub fn assert_launch(facts: &LaunchFacts) -> Result<(), Vec<Violation>> {
    let mut violations = Vec::new();

    if let Some(m) = &facts.base_digest_mismatch {
        violations.push(Violation::BaseDigestMismatch {
            expected: m.expected.clone(),
            actual: m.actual.clone(),
        });
    }
    for p in &facts.readable_credentials {
        violations.push(Violation::CredentialReadable(p.clone()));
    }
    for r in &facts.unexpected_routes {
        violations.push(Violation::RouteExists(r.clone()));
    }
    for p in &facts.unplanned_rw_mounts {
        violations.push(Violation::UnplannedRwMount(p.clone()));
    }
    for c in &facts.retained_capabilities {
        violations.push(Violation::CapabilityRetained(c.clone()));
    }
    if let Some(pid) = facts.lock_held_by {
        violations.push(Violation::WorkspaceLocked { pid });
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Every mount point the box actually holds read-write, read out of its
/// own `/proc/self/mountinfo` after the pivot.
///
/// This is the one launch fact that is a real observation rather than a
/// restatement of what we just asked for: it is the kernel's answer, not
/// the plan's. Its job is to catch a mount the plan never asked for —
/// which today could only come from a regression in the apply loop, and
/// that is exactly what a ratchet is for.
///
/// Mount points are escaped in `mountinfo`, so a path with a space in it
/// arrives as `\040` and must be read back or it will never match a plan
/// target.
pub fn rw_mount_points(mountinfo: &str) -> Vec<PathBuf> {
    mountinfo
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let point = fields.nth(4)?;
            let options = fields.next()?;
            options
                .split(',')
                .any(|option| option == "rw")
                .then(|| PathBuf::from(unescape_mount_point(point)))
        })
        .collect()
}

fn unescape_mount_point(point: &str) -> String {
    let mut out = String::with_capacity(point.len());
    let mut chars = point.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let octal: String = chars.clone().take(3).collect();
        match u8::from_str_radix(&octal, 8) {
            Ok(byte) if octal.len() == 3 => {
                out.push(byte as char);
                chars.nth(2);
            }
            _ => out.push(c),
        }
    }
    out
}

/// The capability names Linux has had long enough to be worth naming in a
/// refusal. A bit past the end of this list is reported by its number
/// rather than guessed at, so a newer kernel's capability does not become
/// a wrong name.
const CAPABILITY_NAMES: [&str; 41] = [
    "CAP_CHOWN",
    "CAP_DAC_OVERRIDE",
    "CAP_DAC_READ_SEARCH",
    "CAP_FOWNER",
    "CAP_FSETID",
    "CAP_KILL",
    "CAP_SETGID",
    "CAP_SETUID",
    "CAP_SETPCAP",
    "CAP_LINUX_IMMUTABLE",
    "CAP_NET_BIND_SERVICE",
    "CAP_NET_BROADCAST",
    "CAP_NET_ADMIN",
    "CAP_NET_RAW",
    "CAP_IPC_LOCK",
    "CAP_IPC_OWNER",
    "CAP_SYS_MODULE",
    "CAP_SYS_RAWIO",
    "CAP_SYS_CHROOT",
    "CAP_SYS_PTRACE",
    "CAP_SYS_PACCT",
    "CAP_SYS_ADMIN",
    "CAP_SYS_BOOT",
    "CAP_SYS_NICE",
    "CAP_SYS_RESOURCE",
    "CAP_SYS_TIME",
    "CAP_SYS_TTY_CONFIG",
    "CAP_MKNOD",
    "CAP_LEASE",
    "CAP_AUDIT_WRITE",
    "CAP_AUDIT_CONTROL",
    "CAP_SETFCAP",
    "CAP_MAC_OVERRIDE",
    "CAP_MAC_ADMIN",
    "CAP_SYSLOG",
    "CAP_WAKE_ALARM",
    "CAP_BLOCK_SUSPEND",
    "CAP_AUDIT_READ",
    "CAP_PERFMON",
    "CAP_BPF",
    "CAP_CHECKPOINT_RESTORE",
];

/// Which capabilities are still in the bounding set, out of the box's own
/// `/proc/self/status`. Empty is what a box must reach: a capability that
/// is not in the bounding set cannot be regained, by this process or any
/// child, however it execs.
///
/// A status without the line is no capabilities rather than an error —
/// the caller is asserting emptiness, and a missing line cannot prove the
/// opposite.
pub fn retained_capabilities(status: &str) -> Vec<String> {
    let Some(mask) = status
        .lines()
        .find_map(|line| line.strip_prefix("CapBnd:"))
        .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
    else {
        return Vec::new();
    };
    (0..64)
        .filter(|bit| mask & (1u64 << bit) != 0)
        .map(|bit| {
            CAPABILITY_NAMES
                .get(bit)
                .map_or_else(|| format!("capability {bit}"), |name| (*name).to_owned())
        })
        .collect()
}

/// Which boundary stage this box runs behind. The banner names it on
/// every launch — a staged boundary that does not say which stage it is
/// in is a lie (CONCEPT.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    Namespaces,
    MicroVm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub boundary: Boundary,
    pub egress: Vec<String>,
    pub grant_count: usize,
}

impl Banner {
    /// The banner for a box behind the `Namespaces` boundary, read off the
    /// whole launch rather than a few fields of it.
    ///
    /// Taking `RunArgs` is the point. Assembling the banner from hand-picked
    /// fields is what let a brokered box print `egress: none` while it was
    /// reaching the model API through the broker's socket — a new way out
    /// was added and the banner was not told. Every way out and every host
    /// path bound in is decided here, so the next one cannot be added
    /// without passing through this function.
    pub fn for_run(args: &crate::run::RunArgs) -> Self {
        let mut egress = Vec::new();
        // Mediated and narrow, but a way out: the box speaks to a socket
        // the host serves, and what comes back is the model API — plus
        // every host the allowlist tunnels, counted so the banner never
        // understates what the box can reach.
        if args.broker.is_some() {
            egress.push(match args.egress.len() {
                0 => "model api via the broker".to_owned(),
                n => format!("model api + {n} allowed hosts via the broker"),
            });
        }
        match (args.network, args.dns) {
            // A namespace of its own with nothing but loopback in it takes
            // no route away that the box still has — it adds nothing to the
            // list, and an empty list is the one honest `none`.
            (crate::run::Network::None, _) => {}
            (crate::run::Network::Host, Some(dns)) => {
                egress.push(format!("host network (dns {dns})"))
            }
            (crate::run::Network::Host, None) => {
                egress.push("host network (no resolver)".to_owned())
            }
        }
        Banner {
            boundary: Boundary::Namespaces,
            egress,
            // A CA bundle is a host path bound into the box exactly like a
            // grant, and a count that leaves it out understates what was
            // handed in.
            grant_count: args.grants.len() + usize::from(args.ca.is_some()),
        }
    }
}

impl fmt::Display for Banner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let boundary = match self.boundary {
            Boundary::Namespaces => "namespaces (host kernel SHARED)",
            Boundary::MicroVm => "microvm (separate guest kernel)",
        };
        let egress = if self.egress.is_empty() {
            "none".to_owned()
        } else {
            self.egress.join(",")
        };
        write!(
            f,
            "boundary: {boundary} · egress: {egress} · workspace: rw · grants: {}",
            self.grant_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn clean_facts_launch() {
        assert_eq!(assert_launch(&LaunchFacts::default()), Ok(()));
    }

    /// Real `mountinfo` lines. Only the read-write ones matter: a
    /// read-only mount cannot be how an agent reaches back onto the host.
    #[test]
    fn only_the_read_write_mount_points_come_back() {
        let mountinfo = concat!(
            "23 1 0:21 / / rw,relatime - tmpfs tmpfs rw\n",
            "24 23 0:22 / /proc rw,nosuid,nodev,noexec - proc proc rw\n",
            "25 23 8:2 /home/me/proj /home/me/proj ro,relatime - ext4 /dev/sda2 rw\n",
            "26 23 8:2 /opt/data /opt/data rw,relatime - ext4 /dev/sda2 rw\n",
        );
        assert_eq!(
            rw_mount_points(mountinfo),
            vec![
                PathBuf::from("/"),
                PathBuf::from("/proc"),
                PathBuf::from("/opt/data"),
            ]
        );
    }

    /// `ro` is a prefix of nothing and a suffix of nothing, but `rw` does
    /// appear inside other option names. The option list is read as a list.
    #[test]
    fn an_option_that_merely_contains_rw_is_not_read_write() {
        let mountinfo = "23 1 0:21 / /x ro,relatime,errors=continue - ext4 /dev/sda rw\n";
        assert!(rw_mount_points(mountinfo).is_empty());
    }

    /// The kernel escapes a mount point's spaces, so a path with one
    /// arrives as `\040` and would never match a plan target unescaped.
    #[test]
    fn an_escaped_mount_point_reads_back_as_the_real_path() {
        let mountinfo = "23 1 0:21 / /home/me/my\\040proj rw,relatime - ext4 /dev/sda rw\n";
        assert_eq!(
            rw_mount_points(mountinfo),
            vec![PathBuf::from("/home/me/my proj")]
        );
    }

    #[test]
    fn a_short_or_empty_mount_table_is_no_mounts_not_a_panic() {
        assert!(rw_mount_points("").is_empty());
        assert!(rw_mount_points("23 1 0:21 /\n").is_empty());
    }

    /// What a box must reach: nothing left to regain. A capability out of
    /// the bounding set cannot come back, by this process or any child,
    /// however it execs.
    #[test]
    fn an_empty_bounding_set_is_no_capabilities() {
        let status = "Name:\tsh\nCapBnd:\t0000000000000000\nSeccomp:\t0\n";
        assert!(retained_capabilities(status).is_empty());
    }

    #[test]
    fn a_retained_capability_is_named_so_the_refusal_is_actionable() {
        // CAP_SYS_ADMIN is bit 21, CAP_CHOWN is bit 0.
        let status = format!("CapBnd:\t{:016x}\n", (1u64 << 21) | 1);
        assert_eq!(
            retained_capabilities(&status),
            vec!["CAP_CHOWN".to_owned(), "CAP_SYS_ADMIN".to_owned()]
        );
    }

    /// A newer kernel's capability must be reported by its number rather
    /// than given a name this build invented for it.
    #[test]
    fn a_capability_this_build_has_no_name_for_is_reported_by_number() {
        let status = format!("CapBnd:\t{:016x}\n", 1u64 << 60);
        assert_eq!(retained_capabilities(&status), vec!["capability 60"]);
    }

    /// The caller is asserting the set is empty; a status that does not
    /// say cannot be read as proof that it is not.
    #[test]
    fn a_status_without_the_line_reads_as_no_capabilities() {
        assert!(retained_capabilities("Name:\tsh\n").is_empty());
        assert!(retained_capabilities("CapBnd:\tnot hex\n").is_empty());
    }

    #[test]
    fn each_violated_precondition_refuses_with_its_own_error() {
        let cases: Vec<(LaunchFacts, Violation)> = vec![
            (
                LaunchFacts {
                    base_digest_mismatch: Some(DigestMismatch {
                        expected: "sha256:aa".into(),
                        actual: "sha256:bb".into(),
                    }),
                    ..Default::default()
                },
                Violation::BaseDigestMismatch {
                    expected: "sha256:aa".into(),
                    actual: "sha256:bb".into(),
                },
            ),
            (
                LaunchFacts {
                    readable_credentials: vec![PathBuf::from("/home/n/.aws/credentials")],
                    ..Default::default()
                },
                Violation::CredentialReadable(PathBuf::from("/home/n/.aws/credentials")),
            ),
            (
                LaunchFacts {
                    unexpected_routes: vec!["default via 10.0.0.1".into()],
                    ..Default::default()
                },
                Violation::RouteExists("default via 10.0.0.1".into()),
            ),
            (
                LaunchFacts {
                    unplanned_rw_mounts: vec![PathBuf::from("/etc")],
                    ..Default::default()
                },
                Violation::UnplannedRwMount(PathBuf::from("/etc")),
            ),
            (
                LaunchFacts {
                    retained_capabilities: vec!["CAP_SYS_RAWIO".into()],
                    ..Default::default()
                },
                Violation::CapabilityRetained("CAP_SYS_RAWIO".into()),
            ),
            (
                LaunchFacts {
                    lock_held_by: Some(4242),
                    ..Default::default()
                },
                Violation::WorkspaceLocked { pid: 4242 },
            ),
        ];

        let mut seen = Vec::new();
        for (facts, expected) in cases {
            let violations = assert_launch(&facts).unwrap_err();
            assert_eq!(violations, vec![expected.clone()]);
            let msg = expected.to_string();
            assert!(!seen.contains(&msg), "duplicate message {msg:?}");
            seen.push(msg);
        }
    }

    #[test]
    fn all_violations_are_reported_together() {
        let facts = LaunchFacts {
            readable_credentials: vec![PathBuf::from("/x")],
            lock_held_by: Some(1),
            ..Default::default()
        };
        assert_eq!(assert_launch(&facts).unwrap_err().len(), 2);
    }

    #[test]
    fn violation_messages_name_the_offending_thing() {
        let v = Violation::CredentialReadable(Path::new("/home/n/.ssh/id_ed25519").to_owned());
        assert_eq!(
            v.to_string(),
            "credential file /home/n/.ssh/id_ed25519 is readable inside the box"
        );
    }

    #[test]
    fn banner_snapshot_namespaces_no_grants() {
        let banner = Banner {
            boundary: Boundary::Namespaces,
            egress: vec!["model-api".into()],
            grant_count: 0,
        };
        assert_eq!(
            banner.to_string(),
            "boundary: namespaces (host kernel SHARED) · egress: model-api · workspace: rw · grants: 0"
        );
    }

    #[test]
    fn banner_snapshot_microvm_multiple_egress_and_grants() {
        let banner = Banner {
            boundary: Boundary::MicroVm,
            egress: vec!["model-api".into(), "git".into(), "github-api".into()],
            grant_count: 3,
        };
        assert_eq!(
            banner.to_string(),
            "boundary: microvm (separate guest kernel) · egress: model-api,git,github-api · workspace: rw · grants: 3"
        );
    }

    /// A box in the host's network namespace reaches everything the host
    /// does. The banner exists to stop the boundary overstating itself, so
    /// that case must never render as `none`.
    #[test]
    fn a_namespaces_banner_never_claims_an_egress_the_box_does_not_have() {
        let mut args = run_args();
        args.network = crate::run::Network::Host;
        let without = Banner::for_run(&args);
        assert_eq!(
            without.to_string(),
            "boundary: namespaces (host kernel SHARED) · egress: host network (no resolver) · workspace: rw · grants: 0"
        );
        args.dns = Some("1.1.1.1".parse().expect("address"));
        args.grants = vec!["/home/n/.ssh".to_owned(), "/home/n/.gnupg".to_owned()];
        let with = Banner::for_run(&args);
        assert!(with.to_string().contains("dns 1.1.1.1"), "{with}");
        assert!(with.to_string().ends_with("grants: 2"), "{with}");
        for banner in [without, with] {
            assert!(!banner.to_string().contains("egress: none"), "{banner}");
        }
    }

    /// And the one case where `none` is the truth: a network namespace of
    /// the box's own, holding nothing but loopback. There is no route to
    /// take, rather than a filter in front of one — which is why this is
    /// the only shape allowed to say so.
    #[test]
    fn a_box_with_its_own_network_namespace_says_none_because_it_is_true() {
        let mut args = run_args();
        args.grants = vec!["/home/n/.ssh".to_owned()];
        let alone = Banner::for_run(&args);
        assert!(alone.to_string().contains("egress: none"), "{alone}");
        // A resolver named for a box that has no route to it is noise.
        args.grants.clear();
        args.dns = Some("1.1.1.1".parse().expect("address"));
        let ignored = Banner::for_run(&args);
        assert!(!ignored.to_string().contains("1.1.1.1"), "{ignored}");
    }

    #[test]
    fn banner_with_no_egress_says_none() {
        let banner = Banner {
            boundary: Boundary::Namespaces,
            egress: vec![],
            grant_count: 1,
        };
        assert!(banner.to_string().contains("egress: none"));
    }

    fn run_args() -> crate::run::RunArgs {
        crate::run::RunArgs {
            grants: Vec::new(),
            dns: None,
            image: None,
            pidfile: None,
            ca: None,
            artifacts: Vec::new(),
            broker: None,
            egress: Vec::new(),
            network: crate::run::Network::None,
            root: crate::run::RootMode::default(),
            command: vec!["sh".to_owned()],
        }
    }

    /// The pairing the design aims at — no network namespace of its own,
    /// the broker as the only way out — is exactly the one the banner used
    /// to call `none`. The box reaches the model API through that socket,
    /// so `none` is as false here as it is on the host network.
    #[test]
    fn a_brokered_box_names_the_one_way_out_it_has() {
        let mut args = run_args();
        args.broker = Some("/data/wormhole/broker.sock".to_owned());
        let banner = Banner::for_run(&args);
        assert!(!banner.to_string().contains("egress: none"), "{banner}");
        assert!(
            banner.to_string().contains("model api via the broker"),
            "{banner}"
        );
    }

    /// A broker does not take the host network away. A box that has both
    /// says both, in the order that puts the wider reach last.
    #[test]
    fn a_brokered_box_on_the_host_network_says_both() {
        let mut args = run_args();
        args.broker = Some("/data/wormhole/broker.sock".to_owned());
        args.network = crate::run::Network::Host;
        args.dns = Some("1.1.1.1".parse().expect("address"));
        assert_eq!(
            Banner::for_run(&args).egress,
            vec![
                "model api via the broker".to_owned(),
                "host network (dns 1.1.1.1)".to_owned()
            ]
        );
    }

    /// Every host path bound into the box counts, not just the ones the
    /// manifest calls `grants`: a CA bundle is a host path the box can
    /// read.
    #[test]
    fn the_grant_count_covers_every_host_path_bound_in() {
        let mut args = run_args();
        args.grants = vec!["/home/n/.ssh".to_owned()];
        args.ca = Some("/etc/ssl/certs/ca-certificates.crt".to_owned());
        assert_eq!(Banner::for_run(&args).grant_count, 2);
    }
}
