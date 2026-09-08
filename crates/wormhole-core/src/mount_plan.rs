//! Default-deny mount planning. Takes already-resolved paths (symlink
//! resolution happens behind the `PathResolver` port) and never touches
//! the filesystem.

use core::fmt;
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};

/// The box's fixed hostname; also what the synthetic `/etc/hosts` names.
pub const BOX_HOSTNAME: &str = "wormhole";

/// The box's own `PATH`, never the host's. A host `PATH` describes a
/// filesystem the box does not have: Arch drops `/bin`, Alpine keeps
/// busybox there, and the box would find neither `mkdir` nor `apk`.
pub const BOX_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// The `PATH` a box runs under: the kept home's `.local/bin` first, so a
/// tool installed there outlives the throwaway root, then the image's.
#[must_use]
pub fn box_path(home: &Path) -> String {
    format!("{}/.local/bin:{BOX_PATH}", home.display())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
}

/// A grant as the user asked for it plus what the resolver found.
/// `resolved` differing from `requested` means the path is a symlink.
/// `target` is where the source lands in the box — the same path for a
/// plain grant, somewhere else for a retargeted one like the CA bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub requested: PathBuf,
    pub resolved: PathBuf,
    pub target: PathBuf,
    pub rw: bool,
}

impl Grant {
    pub fn direct(path: impl Into<PathBuf>, rw: bool) -> Self {
        let path = path.into();
        Grant {
            requested: path.clone(),
            resolved: path.clone(),
            target: path,
            rw,
        }
    }

    /// An already-resolved host file bound at a different box path: the
    /// CA bundle read-only, a shared login read-write.
    pub fn bound(resolved: impl Into<PathBuf>, target: impl Into<PathBuf>, rw: bool) -> Self {
        let resolved = resolved.into();
        Grant {
            requested: resolved.clone(),
            resolved,
            target: target.into(),
            rw,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountOp {
    /// An empty tmpfs as the root — the interim rootfs for a bare
    /// `__run`, filled by the host-`/usr` binds. Step 4 replaces
    /// this with real overlay layers.
    TmpfsRoot,
    Tmpfs {
        target: PathBuf,
        /// Permission bits for the mount point, as passed in `mode=`.
        mode: u32,
    },
    /// A tmpfs that starts out holding a copy of `seed`. What makes a
    /// read-only root usable: `/etc` and `/var` must be writable, and a
    /// bare tmpfs over either would hide the image's own contents — the CA
    /// bundle, `apk`'s configuration — rather than let the box change them.
    /// Both are small, so the copy is cheap where copying the whole image
    /// is not.
    TmpfsFrom {
        target: PathBuf,
        seed: PathBuf,
        mode: u32,
    },
    /// Synthetic file written into the box, never bound from the host.
    File { target: PathBuf, content: String },
    Bind {
        source: PathBuf,
        target: PathBuf,
        rw: bool,
    },
    /// Relative symlink created inside the box root.
    Symlink { link: PathBuf, to: PathBuf },
    /// One of the fixed device nodes from `DEVICES`, bound at the same
    /// path. Kept apart from `Bind` so the rule "a bind source is the
    /// workspace, the home, or a grant" stays true of every `Bind`.
    Device { path: PathBuf },
    /// A fresh `proc` for this box's PID namespace. Must be mounted by a
    /// process already inside that namespace, or it shows the host's.
    Proc { target: PathBuf },
    /// A `devpts` of the box's own, so tools can open pseudo-terminals
    /// without seeing the host's.
    DevPts { target: PathBuf },
}

impl MountOp {
    /// Where this op puts something inside the box, when it puts it
    /// somewhere a mount table can name. `File` and `Symlink` write into
    /// the root rather than mounting, so neither has one.
    ///
    /// The launch assertion compares the kernel's rw mount list against
    /// these, so a new variant that does mount something and forgets to
    /// answer here fails the assertion rather than slipping past it.
    pub fn mount_point(&self) -> Option<&Path> {
        match self {
            MountOp::TmpfsRoot => Some(Path::new("/")),
            MountOp::Tmpfs { target, .. }
            | MountOp::TmpfsFrom { target, .. }
            | MountOp::Bind { target, .. }
            | MountOp::Proc { target }
            | MountOp::DevPts { target } => Some(target),
            MountOp::Device { path } => Some(path),
            MountOp::File { .. } | MountOp::Symlink { .. } => None,
        }
    }
}

/// The build box's throwaway directory, and the only place an artifact
/// may land.
///
/// It is a tmpfs, so a file bound there leaves nothing in the image when
/// the mount goes. Anywhere else is the image itself, where the bind
/// would have to create the mount point first and an empty file would
/// survive into the finished image.
pub const BUILD_SCRATCH: &str = "/tmp";

/// Whether a path names a file in the build box's scratch: absolute,
/// stepping through no parent directory, and under the scratch itself.
///
/// The `..` rule is checked before the prefix is believed, because
/// `/tmp/../etc/passwd` starts with the scratch and is not in it.
///
/// It lives here, beside the mount it decides, rather than beside the
/// parser that calls it: this is the rule that keeps an artifact from
/// being bound over the image being built, and a rule stated a module
/// away from the mechanism it protects is one that can be forgotten.
#[must_use]
pub fn is_in_build_scratch(path: &str) -> bool {
    let path = Path::new(path);
    if path.components().any(|part| part == Component::ParentDir) {
        return false;
    }
    path.strip_prefix(BUILD_SCRATCH)
        .is_ok_and(|under| under.file_name().is_some())
}

/// The only host device nodes a box sees. Every one is characterless —
/// no disk, no input, nothing that identifies the machine. `/dev/tty` is
/// the terminal the box was started from, which it already holds as
/// inherited stdio; interactive tools open it by name.
pub const DEVICES: [&str; 6] = [
    "/dev/null",
    "/dev/zero",
    "/dev/full",
    "/dev/random",
    "/dev/urandom",
    "/dev/tty",
];

/// The box that builds an image: the image directory itself as a writable
/// root, because installing is the point, plus only what a package manager
/// needs. No workspace, no home, no grants — nothing of yours is reachable
/// while third-party install scripts run.
pub fn build_ops(image: &Path, resolver: &Resolver, bound: &[(PathBuf, PathBuf)]) -> Vec<MountOp> {
    let mut ops = vec![image_root(image, true)];
    ops.extend(device_ops());
    ops.push(MountOp::Proc {
        target: PathBuf::from("/proc"),
    });
    ops.push(MountOp::Tmpfs {
        target: PathBuf::from(BUILD_SCRATCH),
        mode: 0o1777,
    });
    // Wormhole's own, kept apart from the scratch a recipe writes into:
    // the host CA bundle lands here, where no artifact can be asked for.
    ops.push(MountOp::Tmpfs {
        target: PathBuf::from("/run"),
        mode: 0o755,
    });
    // After both tmpfs, never before: binding a file creates its mount
    // point, and a mount point created before the tmpfs covers it is a
    // file left behind in the finished image.
    ops.extend(bound.iter().map(|(source, target)| MountOp::Bind {
        source: source.clone(),
        target: target.clone(),
        rw: false,
    }));
    ops.push(hosts_op());
    ops.push(resolv_op(resolver));
    ops
}

fn image_root(image: &Path, rw: bool) -> MountOp {
    MountOp::Bind {
        source: image.to_owned(),
        target: PathBuf::from("/"),
        rw,
    }
}

/// What a read-only root must still be able to write to.
///
/// `/etc` because a synthetic `passwd`, `group`, `hosts` and `resolv.conf`
/// are written into it on every start, and because the image's own
/// contents — the CA bundle above all — must stay visible, which a bare
/// tmpfs would hide. `/var` because `apk`'s state, locks and caches live
/// there and a package manager that cannot write its own database fails in
/// ways that read as anything but a read-only root.
///
/// Both are small. Copying them is what makes not copying the other
/// gigabyte possible.
const WRITABLE_OVER_READONLY: [&str; 2] = ["/etc", "/var"];

/// `/dev` and the fixed nodes inside it — one body, so the run box and
/// the build box can never disagree on what a box's `/dev` holds.
fn device_ops() -> Vec<MountOp> {
    let mut ops = vec![MountOp::Tmpfs {
        target: PathBuf::from("/dev"),
        mode: 0o755,
    }];
    for device in DEVICES {
        ops.push(MountOp::Device {
            path: PathBuf::from(device),
        });
    }
    ops
}

fn hosts_op() -> MountOp {
    MountOp::File {
        target: PathBuf::from("/etc/hosts"),
        content: format!("127.0.0.1 localhost\n127.0.1.1 {BOX_HOSTNAME}\n"),
    }
}

/// What answers the box's name lookups. A box on the host's network
/// always has one: the resolver named, or the host's own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolver {
    /// One nameserver, the only line of the box's `resolv.conf`.
    Named(IpAddr),
    /// The host's `resolv.conf`, symlinks followed, bound read-only.
    Host(PathBuf),
}

fn resolv_op(resolver: &Resolver) -> MountOp {
    let target = PathBuf::from("/etc/resolv.conf");
    match resolver {
        Resolver::Named(dns) => MountOp::File {
            target,
            content: format!("nameserver {dns}\n"),
        },
        Resolver::Host(file) => MountOp::Bind {
            source: file.clone(),
            target,
            rw: false,
        },
    }
}

/// Interim rootfs until Step 4 ships digest-pinned layers: the host's
/// `/usr` read-only plus the usr-merge symlinks, mirroring an Arch-style
/// host. Deleted when real layers land.
fn interim_host_usr() -> Vec<MountOp> {
    let mut ops = vec![MountOp::Bind {
        source: PathBuf::from("/usr"),
        target: PathBuf::from("/usr"),
        rw: false,
    }];
    for (link, to) in [
        ("/bin", "usr/bin"),
        ("/sbin", "usr/bin"),
        ("/lib", "usr/lib"),
        ("/lib64", "usr/lib"),
    ] {
        ops.push(MountOp::Symlink {
            link: PathBuf::from(link),
            to: PathBuf::from(to),
        });
    }
    ops
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    WorkspaceNotAbsolute(PathBuf),
    HomeNotAbsolute(PathBuf),
    GrantNotAbsolute(PathBuf),
    GrantHasDotDot(PathBuf),
    GrantIsSymlink {
        requested: PathBuf,
        resolved: PathBuf,
    },
    GrantOverlapsWorkspace(PathBuf),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::WorkspaceNotAbsolute(p) => {
                write!(f, "workspace path must be absolute: {}", p.display())
            }
            PlanError::HomeNotAbsolute(p) => {
                write!(f, "home path must be absolute: {}", p.display())
            }
            PlanError::GrantNotAbsolute(p) => {
                write!(f, "grant must be an absolute path: {}", p.display())
            }
            PlanError::GrantHasDotDot(p) => {
                write!(f, "grant must not contain '..': {}", p.display())
            }
            PlanError::GrantIsSymlink {
                requested,
                resolved,
            } => write!(
                f,
                "grant {} is a symlink to {}; grant the real path instead",
                requested.display(),
                resolved.display()
            ),
            PlanError::GrantOverlapsWorkspace(p) => write!(
                f,
                "grant {} overlaps the workspace, which is already mounted rw",
                p.display()
            ),
        }
    }
}

impl std::error::Error for PlanError {}

/// What the box's root is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root<'a> {
    /// A built image directory, writable, thrown away when the box exits.
    /// One `cp --reflink` away from the cached image, so writes inside the
    /// box can never reach it.
    Image(&'a Path),
    /// The cached image itself, bound read-only, with a tmpfs over the few
    /// paths that must be writable. No copy is made at all, so the box
    /// starts in the time it takes to mount instead of the time it takes
    /// to copy a gigabyte — which on a host without reflink is every start.
    ///
    /// Because nothing is copied, the image is shared by every box using
    /// it, and read-only is what makes that safe rather than merely fast.
    ImageReadOnly(&'a Path),
    /// The interim rootfs for a bare `__run`: the host's `/usr`
    /// read-only over a tmpfs root. Goes away when nothing needs it.
    HostUsr,
}

/// Where the box's home lands, decided beside the passwd entry and the
/// bind that both encode it.
pub fn home_in_box(user: &User) -> PathBuf {
    PathBuf::from(format!("/home/{}", user.name))
}

pub fn compute(
    workspace: &Path,
    home_src: &Path,
    root: Root<'_>,
    user: &User,
    grants: &[Grant],
    resolver: &Resolver,
) -> Result<Vec<MountOp>, PlanError> {
    if !workspace.is_absolute() {
        return Err(PlanError::WorkspaceNotAbsolute(workspace.to_owned()));
    }
    if !home_src.is_absolute() {
        return Err(PlanError::HomeNotAbsolute(home_src.to_owned()));
    }
    let grants = validate_and_normalize(workspace, grants)?;

    let mut ops = Vec::new();
    match root {
        Root::Image(image) => ops.push(image_root(image, true)),
        Root::ImageReadOnly(image) => {
            ops.push(image_root(image, false));
            // Before everything else that writes into the root: the
            // synthetic `/etc` files below land in this tmpfs, not in the
            // shared image, which is the whole reason the image can be
            // read-only and shared at all.
            for target in WRITABLE_OVER_READONLY {
                ops.push(MountOp::TmpfsFrom {
                    target: PathBuf::from(target),
                    seed: image.join(target.trim_start_matches('/')),
                    mode: 0o755,
                });
            }
        }
        Root::HostUsr => {
            ops.push(MountOp::TmpfsRoot);
            ops.extend(interim_host_usr());
        }
    }
    ops.extend(device_ops());
    ops.push(MountOp::DevPts {
        target: PathBuf::from("/dev/pts"),
    });
    ops.push(MountOp::Symlink {
        link: PathBuf::from("/dev/ptmx"),
        to: PathBuf::from("pts/ptmx"),
    });
    for target in ["/dev/shm", "/tmp", "/run"] {
        ops.push(MountOp::Tmpfs {
            target: PathBuf::from(target),
            mode: 0o1777,
        });
    }
    ops.push(MountOp::Proc {
        target: PathBuf::from("/proc"),
    });
    ops.push(hosts_op());
    ops.push(MountOp::File {
        target: PathBuf::from("/etc/passwd"),
        content: format!(
            "{name}:x:{uid}:{gid}::/home/{name}:/bin/sh\n",
            name = user.name,
            uid = user.uid,
            gid = user.gid
        ),
    });
    ops.push(MountOp::File {
        target: PathBuf::from("/etc/group"),
        content: format!("{name}:x:{gid}:\n", name = user.name, gid = user.gid),
    });
    ops.push(resolv_op(resolver));
    ops.push(MountOp::Bind {
        source: home_src.to_owned(),
        target: home_in_box(user),
        rw: true,
    });
    ops.push(MountOp::Bind {
        source: workspace.to_owned(),
        target: workspace.to_owned(),
        rw: true,
    });
    for g in grants {
        ops.push(MountOp::Bind {
            source: g.resolved.clone(),
            target: g.target.clone(),
            rw: g.rw,
        });
    }
    Ok(ops)
}

/// Rejects escapes, collapses duplicates and covered nested grants, and
/// orders parents before children so binds mount inside their parents.
fn validate_and_normalize(workspace: &Path, grants: &[Grant]) -> Result<Vec<Grant>, PlanError> {
    for g in grants {
        if !g.requested.is_absolute() {
            return Err(PlanError::GrantNotAbsolute(g.requested.clone()));
        }
        if g.requested.components().any(|c| c == Component::ParentDir) {
            return Err(PlanError::GrantHasDotDot(g.requested.clone()));
        }
        if !g.target.is_absolute() {
            return Err(PlanError::GrantNotAbsolute(g.target.clone()));
        }
        if g.resolved != g.requested {
            return Err(PlanError::GrantIsSymlink {
                requested: g.requested.clone(),
                resolved: g.resolved.clone(),
            });
        }
        if g.requested.starts_with(workspace) || workspace.starts_with(&g.requested) {
            return Err(PlanError::GrantOverlapsWorkspace(g.requested.clone()));
        }
    }

    let mut sorted: Vec<Grant> = grants.to_vec();
    sorted.sort_by(|a, b| (&a.requested, &a.target).cmp(&(&b.requested, &b.target)));
    // The same source at the same target merges to one mount; rw wins
    // because the user explicitly granted it.
    sorted.dedup_by(|dup, kept| {
        if dup.requested == kept.requested && dup.target == kept.target {
            kept.rw |= dup.rw;
            true
        } else {
            false
        }
    });

    // A grant is covered when an ancestor grant already mounts it with the
    // same mode, or rw (an rw ancestor makes the child reachable rw either
    // way — the user chose that when granting the ancestor). An rw child
    // under an ro ancestor stays: it needs its own bind. Only same-path
    // grants take part: a retargeted grant mounts somewhere no ancestor
    // reaches, so it neither covers nor is covered.
    let same_path = |g: &Grant| g.target == g.requested;
    let mut kept: Vec<Grant> = Vec::with_capacity(sorted.len());
    for g in sorted {
        let covered = same_path(&g)
            && kept.iter().any(|k| {
                same_path(k)
                    && g.requested != k.requested
                    && g.requested.starts_with(&k.requested)
                    && (k.rw || !g.rw)
            });
        if !covered {
            kept.push(g);
        }
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tool the agent installs into its kept home is found before the
    /// image's copy, and the image's directories are all still there.
    #[test]
    fn the_box_path_puts_the_homes_local_bin_first_and_keeps_the_images() {
        let path = box_path(Path::new("/home/me"));
        assert!(path.starts_with("/home/me/.local/bin:"), "{path}");
        assert!(path.ends_with(BOX_PATH), "{path}");
    }

    fn user() -> User {
        User {
            name: "nabor".to_owned(),
            uid: 1000,
            gid: 1000,
        }
    }

    fn host_resolver() -> Resolver {
        Resolver::Host(PathBuf::from("/run/systemd/resolve/stub-resolv.conf"))
    }

    fn plan(grants: &[Grant]) -> Result<Vec<MountOp>, PlanError> {
        plan_with(grants, &host_resolver())
    }

    fn plan_with(grants: &[Grant], resolver: &Resolver) -> Result<Vec<MountOp>, PlanError> {
        compute(
            Path::new("/home/nabor/proj"),
            Path::new("/home/nabor/.local/share/wormhole/workspaces/abc/home"),
            Root::HostUsr,
            &user(),
            grants,
            resolver,
        )
    }

    fn resolv_conf(ops: &[MountOp]) -> Vec<&MountOp> {
        ops.iter()
            .filter(|op| match op {
                MountOp::File { target, .. } | MountOp::Bind { target, .. } => {
                    target == Path::new("/etc/resolv.conf")
                }
                _ => false,
            })
            .collect()
    }

    /// The host bundle lands read-only at the box's canonical path, so
    /// every TLS client inside finds it without being told where to look.
    #[test]
    fn a_trusted_host_ca_is_bound_read_only_at_the_canonical_path() {
        let host_bundle = Path::new("/etc/ca-certificates/extracted/tls-ca-bundle.pem");
        let ops = plan(&[Grant::bound(
            host_bundle,
            crate::ca::CA_BUNDLE_IN_BOX,
            false,
        )])
        .unwrap();
        assert!(
            binds(&ops).contains(&(host_bundle, Path::new(crate::ca::CA_BUNDLE_IN_BOX), false)),
            "{:?}",
            binds(&ops)
        );
    }

    #[test]
    fn no_ca_grant_means_no_ca_bind() {
        let ops = plan(&[]).unwrap();
        assert!(
            !binds(&ops)
                .iter()
                .any(|(_, t, _)| *t == Path::new(crate::ca::CA_BUNDLE_IN_BOX)),
            "{:?}",
            binds(&ops)
        );
    }

    /// A retargeted grant mounts where no ancestor reaches, so an
    /// enclosing grant must not swallow it.
    #[test]
    fn a_retargeted_grant_is_never_covered_by_an_ancestor() {
        let ops = plan(&[
            Grant::direct("/etc", true),
            Grant::bound("/etc/ca/bundle.pem", crate::ca::CA_BUNDLE_IN_BOX, false),
        ])
        .unwrap();
        assert!(
            binds(&ops)
                .iter()
                .any(|(_, t, _)| *t == Path::new(crate::ca::CA_BUNDLE_IN_BOX)),
            "{:?}",
            binds(&ops)
        );
    }

    #[test]
    fn a_relative_grant_target_is_refused() {
        let mut g = Grant::direct("/etc/ca/bundle.pem", false);
        g.target = PathBuf::from("certs/bundle.pem");
        assert_eq!(
            plan(&[g]),
            Err(PlanError::GrantNotAbsolute(PathBuf::from(
                "certs/bundle.pem"
            )))
        );
    }

    fn binds(ops: &[MountOp]) -> Vec<(&Path, &Path, bool)> {
        ops.iter()
            .filter_map(|op| match op {
                MountOp::Bind { source, target, rw } => {
                    Some((source.as_path(), target.as_path(), *rw))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn workspace_appears_exactly_once_rw_at_host_identical_path() {
        let ops = plan(&[]).unwrap();
        let ws: Vec<_> = binds(&ops)
            .into_iter()
            .filter(|(_, t, _)| *t == Path::new("/home/nabor/proj"))
            .collect();
        assert_eq!(
            ws,
            vec![(
                Path::new("/home/nabor/proj"),
                Path::new("/home/nabor/proj"),
                true
            )]
        );
    }

    /// The interim host-`/usr` root is the one deliberate exception; an
    /// image root has no host sources beyond the image itself and the
    /// host's resolver file.
    #[test]
    fn no_host_path_beyond_workspace_home_grants_and_resolver() {
        let grant = Grant::direct("/opt/data", false);
        let ops = compute(
            Path::new("/home/nabor/proj"),
            Path::new("/home/nabor/.local/share/wormhole/workspaces/abc/home"),
            Root::Image(Path::new("/box-image")),
            &user(),
            &[grant],
            &host_resolver(),
        )
        .unwrap();
        let allowed = [
            Path::new("/box-image"),
            Path::new("/home/nabor/proj"),
            Path::new("/home/nabor/.local/share/wormhole/workspaces/abc/home"),
            Path::new("/opt/data"),
            Path::new("/run/systemd/resolve/stub-resolv.conf"),
        ];
        for (source, _, _) in binds(&ops) {
            assert!(allowed.contains(&source), "unplanned source {source:?}");
        }
    }

    /// The point of a read-only root: the image is bound, never copied,
    /// and the box cannot write to the thing every other box shares.
    #[test]
    fn a_readonly_root_binds_the_image_itself_and_binds_it_read_only() {
        let image = Path::new("/data/images/abc");
        let ops = compute(
            Path::new("/w"),
            Path::new("/data/home"),
            Root::ImageReadOnly(image),
            &user(),
            &[],
            &host_resolver(),
        )
        .expect("plan");
        assert_eq!(
            ops.first(),
            Some(&MountOp::Bind {
                source: image.to_owned(),
                target: PathBuf::from("/"),
                rw: false,
            })
        );
    }

    /// A bare tmpfs over `/etc` would hide the image's own — the CA bundle
    /// above all — so the box would lose TLS to gain writability. Seeding
    /// it from the image keeps both.
    #[test]
    fn a_readonly_root_gets_writable_etc_and_var_seeded_from_the_image() {
        let image = Path::new("/data/images/abc");
        let ops = compute(
            Path::new("/w"),
            Path::new("/data/home"),
            Root::ImageReadOnly(image),
            &user(),
            &[],
            &host_resolver(),
        )
        .expect("plan");
        for (target, seed) in [
            ("/etc", "/data/images/abc/etc"),
            ("/var", "/data/images/abc/var"),
        ] {
            assert!(
                ops.contains(&MountOp::TmpfsFrom {
                    target: PathBuf::from(target),
                    seed: PathBuf::from(seed),
                    mode: 0o755,
                }),
                "{target} missing from {ops:#?}"
            );
        }
    }

    /// The synthetic `/etc` files are written after the tmpfs that must
    /// hold them. The other order writes them into the shared image, which
    /// a read-only bind refuses — the box would not start.
    #[test]
    fn the_etc_tmpfs_is_mounted_before_anything_is_written_into_etc() {
        let ops = compute(
            Path::new("/w"),
            Path::new("/data/home"),
            Root::ImageReadOnly(Path::new("/data/images/abc")),
            &user(),
            &[],
            &Resolver::Named("1.1.1.1".parse().expect("address")),
        )
        .expect("plan");
        let tmpfs = ops
            .iter()
            .position(
                |op| matches!(op, MountOp::TmpfsFrom { target, .. } if target == Path::new("/etc")),
            )
            .expect("etc tmpfs");
        for (index, op) in ops.iter().enumerate() {
            if let MountOp::File { target, .. } = op
                && target.starts_with("/etc")
            {
                assert!(
                    tmpfs < index,
                    "{} written before its tmpfs",
                    target.display()
                );
            }
        }
    }

    /// A writable root still gets no seeded tmpfs: it is a throwaway copy
    /// already, so `/etc` and `/var` are its own to write.
    #[test]
    fn a_copied_root_needs_no_seeded_tmpfs() {
        let ops = plan(&[]).expect("plan");
        assert!(!ops.iter().any(|op| matches!(op, MountOp::TmpfsFrom { .. })));
    }

    /// No resolver named: the host's own file, read-only, and nothing
    /// else at that path.
    #[test]
    fn without_a_named_resolver_the_hosts_file_is_bound_read_only() {
        let ops = plan(&[]).unwrap();
        assert_eq!(
            resolv_conf(&ops),
            vec![&MountOp::Bind {
                source: PathBuf::from("/run/systemd/resolve/stub-resolv.conf"),
                target: PathBuf::from("/etc/resolv.conf"),
                rw: false,
            }]
        );
    }

    #[test]
    fn the_build_box_writes_into_the_image_and_reaches_nothing_of_yours() {
        let dns = Resolver::Named("1.1.1.1".parse().expect("address"));
        let ops = build_ops(Path::new("/data/images/abc"), &dns, &[]);
        let sources: Vec<&Path> = ops
            .iter()
            .filter_map(|op| match op {
                MountOp::Bind { source, .. } => Some(source.as_path()),
                _ => None,
            })
            .collect();
        assert_eq!(sources, vec![Path::new("/data/images/abc")]);
        assert!(matches!(
            ops.first(),
            Some(MountOp::Bind { target, rw: true, .. }) if target == Path::new("/")
        ));
    }

    /// The build box resolves names the same way a running box does.
    #[test]
    fn the_build_box_gets_the_same_resolver_as_a_running_box() {
        let ops = build_ops(Path::new("/i"), &host_resolver(), &[]);
        assert_eq!(resolv_conf(&ops), resolv_conf(&plan(&[]).unwrap()));
    }

    /// A named resolver is the only one the box can reach; the host's
    /// own setup never leaks in beside it.
    #[test]
    fn a_named_resolver_becomes_the_only_line_of_resolv_conf() {
        let dns = Resolver::Named("1.1.1.1".parse().expect("valid address"));
        let ops = plan_with(&[], &dns).unwrap();
        let resolv: Vec<&str> = ops
            .iter()
            .filter_map(|op| match op {
                MountOp::File { target, content } if target == Path::new("/etc/resolv.conf") => {
                    Some(content.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(resolv, vec!["nameserver 1.1.1.1\n"]);
    }

    #[test]
    fn etc_hosts_is_synthetic_with_localhost_and_hostname() {
        let ops = plan(&[]).unwrap();
        let hosts = ops.iter().find_map(|op| match op {
            MountOp::File { target, content } if target == Path::new("/etc/hosts") => {
                Some(content.as_str())
            }
            _ => None,
        });
        assert_eq!(hosts, Some("127.0.0.1 localhost\n127.0.1.1 wormhole\n"));
    }

    #[test]
    fn etc_passwd_and_group_are_synthetic_with_only_our_user() {
        let ops = plan(&[]).unwrap();
        let file = |name: &str| {
            ops.iter().find_map(|op| match op {
                MountOp::File { target, content } if target == Path::new(name) => {
                    Some(content.as_str())
                }
                _ => None,
            })
        };
        assert_eq!(
            file("/etc/passwd"),
            Some("nabor:x:1000:1000::/home/nabor:/bin/sh\n")
        );
        assert_eq!(file("/etc/group"), Some("nabor:x:1000:\n"));
        for content in [file("/etc/passwd").unwrap(), file("/etc/group").unwrap()] {
            assert_eq!(content.lines().count(), 1);
        }
    }

    #[test]
    fn the_root_comes_first_and_parents_mount_before_children() {
        let grants = [
            Grant::direct("/opt/data/sub", true),
            Grant::direct("/opt/data", false),
        ];
        let ops = plan(&grants).unwrap();
        assert!(matches!(ops[0], MountOp::TmpfsRoot));
        let targets: Vec<&Path> = binds(&ops).into_iter().map(|(_, t, _)| t).collect();
        let parent = targets
            .iter()
            .position(|t| *t == Path::new("/opt/data"))
            .unwrap();
        let child = targets
            .iter()
            .position(|t| *t == Path::new("/opt/data/sub"))
            .unwrap();
        assert!(parent < child);
    }

    #[test]
    fn ephemeral_tmpfs_covers_dev_shm_tmp_and_run() {
        let ops = plan(&[]).unwrap();
        let tmpfs: Vec<(&Path, u32)> = ops
            .iter()
            .filter_map(|op| match op {
                MountOp::Tmpfs { target, mode } => Some((target.as_path(), *mode)),
                _ => None,
            })
            .collect();
        assert_eq!(
            tmpfs,
            vec![
                (Path::new("/dev"), 0o755),
                (Path::new("/dev/shm"), 0o1777),
                (Path::new("/tmp"), 0o1777),
                (Path::new("/run"), 0o1777),
            ]
        );
    }

    #[test]
    fn only_the_fixed_device_nodes_are_planned() {
        let ops = plan(&[]).unwrap();
        let devices: Vec<&Path> = ops
            .iter()
            .filter_map(|op| match op {
                MountOp::Device { path } => Some(path.as_path()),
                _ => None,
            })
            .collect();
        let expected: Vec<&Path> = DEVICES.iter().map(Path::new).collect();
        assert_eq!(devices, expected);
    }

    /// `/dev` is a tmpfs, so the device nodes and `/dev/shm` inside it
    /// only survive if they are created after it is mounted.
    #[test]
    fn dev_is_mounted_before_anything_inside_it() {
        let ops = plan(&[]).unwrap();
        let dev = ops
            .iter()
            .position(
                |op| matches!(op, MountOp::Tmpfs { target, .. } if target == Path::new("/dev")),
            )
            .unwrap();
        for (index, op) in ops.iter().enumerate() {
            let inside = match op {
                MountOp::Device { path } => Some(path.as_path()),
                MountOp::Tmpfs { target, .. } | MountOp::DevPts { target } => {
                    Some(target.as_path())
                }
                _ => None,
            };
            if let Some(path) = inside
                && path != Path::new("/dev")
                && path.starts_with("/dev")
            {
                assert!(dev < index, "{} planned before /dev", path.display());
            }
        }
    }

    /// Tools that open their own pseudo-terminal — `tmux`, `script`, an
    /// attached agent — need a `devpts` of the box's own, plus the
    /// `/dev/ptmx` entry glibc's `openpty` starts from.
    #[test]
    fn the_box_gets_its_own_devpts_and_ptmx_link() {
        let ops = plan(&[]).unwrap();
        let pts = ops
            .iter()
            .position(
                |op| matches!(op, MountOp::DevPts { target } if target == Path::new("/dev/pts")),
            )
            .expect("devpts planned");
        let dev = ops
            .iter()
            .position(
                |op| matches!(op, MountOp::Tmpfs { target, .. } if target == Path::new("/dev")),
            )
            .expect("dev planned");
        assert!(dev < pts, "devpts must mount after /dev");
        assert!(
            ops.iter().any(|op| matches!(
                op,
                MountOp::Symlink { link, to }
                    if link == Path::new("/dev/ptmx") && to == Path::new("pts/ptmx")
            )),
            "ptmx link missing"
        );
    }

    /// An artifact is bound after the scratch tmpfs is mounted, so the
    /// mount point the bind creates lands on the tmpfs and not in the
    /// image being built. Read-only, because a build that could write
    /// back through one would be changing a file the digest already named.
    #[test]
    fn an_artifact_is_bound_read_only_onto_the_scratch_after_it_exists() {
        let artifact = (
            PathBuf::from("/data/artifacts/abc"),
            PathBuf::from("/tmp/rustup-init"),
        );
        let ops = build_ops(Path::new("/i"), &host_resolver(), &[artifact]);
        let scratch = ops
            .iter()
            .position(|op| matches!(op, MountOp::Tmpfs { target, .. } if target == Path::new(BUILD_SCRATCH)))
            .expect("scratch tmpfs");
        let bound = ops
            .iter()
            .position(|op| matches!(op, MountOp::Bind { target, .. } if target == Path::new("/tmp/rustup-init")))
            .expect("artifact bound");
        assert!(scratch < bound, "{ops:#?}");
        assert!(
            binds(&ops).contains(&(
                Path::new("/data/artifacts/abc"),
                Path::new("/tmp/rustup-init"),
                false
            )),
            "{ops:#?}"
        );
    }

    #[test]
    fn the_build_box_needs_no_devpts() {
        let ops = build_ops(Path::new("/i"), &host_resolver(), &[]);
        assert!(!ops.iter().any(|op| matches!(op, MountOp::DevPts { .. })));
    }

    #[test]
    fn proc_is_planned_once_at_slash_proc() {
        let ops = plan(&[]).unwrap();
        let proc: Vec<&Path> = ops
            .iter()
            .filter_map(|op| match op {
                MountOp::Proc { target } => Some(target.as_path()),
                _ => None,
            })
            .collect();
        assert_eq!(proc, vec![Path::new("/proc")]);
    }

    #[test]
    fn interim_rootfs_binds_only_usr_read_only() {
        let sources: Vec<_> = interim_host_usr()
            .iter()
            .filter_map(|op| match op {
                MountOp::Bind { source, rw, .. } => Some((source.clone(), *rw)),
                _ => None,
            })
            .collect();
        assert_eq!(sources, vec![(PathBuf::from("/usr"), false)]);
    }

    #[test]
    fn interim_rootfs_symlinks_point_into_usr() {
        let links: Vec<_> = interim_host_usr()
            .iter()
            .filter_map(|op| match op {
                MountOp::Symlink { link, to } => Some((link.clone(), to.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            links,
            vec![
                (PathBuf::from("/bin"), PathBuf::from("usr/bin")),
                (PathBuf::from("/sbin"), PathBuf::from("usr/bin")),
                (PathBuf::from("/lib"), PathBuf::from("usr/lib")),
                (PathBuf::from("/lib64"), PathBuf::from("usr/lib")),
            ]
        );
        for (_, to) in links {
            assert!(to.is_relative(), "symlink targets must be relative");
        }
    }

    #[test]
    fn grant_with_dotdot_is_rejected() {
        let g = Grant::direct("/opt/../etc", false);
        assert_eq!(
            plan(&[g]),
            Err(PlanError::GrantHasDotDot(PathBuf::from("/opt/../etc")))
        );
    }

    #[test]
    fn relative_grant_is_rejected() {
        let g = Grant::direct("opt/data", false);
        assert_eq!(
            plan(&[g]),
            Err(PlanError::GrantNotAbsolute(PathBuf::from("opt/data")))
        );
    }

    #[test]
    fn symlink_grant_is_rejected_naming_the_target() {
        let g = Grant {
            requested: PathBuf::from("/home/nabor/link"),
            resolved: PathBuf::from("/etc"),
            target: PathBuf::from("/home/nabor/link"),
            rw: false,
        };
        assert_eq!(
            plan(&[g]),
            Err(PlanError::GrantIsSymlink {
                requested: PathBuf::from("/home/nabor/link"),
                resolved: PathBuf::from("/etc"),
            })
        );
    }

    #[test]
    fn grant_overlapping_workspace_is_rejected_both_directions() {
        for path in ["/home/nabor/proj/sub", "/home/nabor"] {
            let g = Grant::direct(path, false);
            assert_eq!(
                plan(&[g]),
                Err(PlanError::GrantOverlapsWorkspace(PathBuf::from(path))),
                "{path}"
            );
        }
    }

    #[test]
    fn duplicate_grants_normalize_to_one_mount() {
        let g = Grant::direct("/opt/data", false);
        let ops = plan(&[g.clone(), g]).unwrap();
        let count = binds(&ops)
            .into_iter()
            .filter(|(_, t, _)| *t == Path::new("/opt/data"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn duplicate_grants_with_conflicting_modes_normalize_to_one_rw_mount() {
        let grants = [
            Grant::direct("/opt/data", false),
            Grant::direct("/opt/data", true),
        ];
        for order in [[0, 1], [1, 0]] {
            let ops = plan(&[grants[order[0]].clone(), grants[order[1]].clone()]).unwrap();
            let data_binds: Vec<_> = binds(&ops)
                .into_iter()
                .filter(|(_, t, _)| *t == Path::new("/opt/data"))
                .collect();
            assert_eq!(
                data_binds,
                vec![(Path::new("/opt/data"), Path::new("/opt/data"), true)],
                "order {order:?}"
            );
        }
    }

    #[test]
    fn nested_grant_same_mode_collapses_into_parent() {
        let grants = [
            Grant::direct("/opt/data", false),
            Grant::direct("/opt/data/sub", false),
        ];
        let ops = plan(&grants).unwrap();
        let targets: Vec<&Path> = binds(&ops).into_iter().map(|(_, t, _)| t).collect();
        assert!(targets.contains(&Path::new("/opt/data")));
        assert!(!targets.contains(&Path::new("/opt/data/sub")));
    }

    #[test]
    fn nested_rw_grant_under_ro_parent_keeps_its_own_mount() {
        let grants = [
            Grant::direct("/opt/data", false),
            Grant::direct("/opt/data/sub", true),
        ];
        let ops = plan(&grants).unwrap();
        assert!(binds(&ops).contains(&(
            Path::new("/opt/data/sub"),
            Path::new("/opt/data/sub"),
            true
        )));
    }

    #[test]
    fn grants_default_ro_and_rw_is_explicit() {
        let ops = plan(&[
            Grant::direct("/opt/ro", false),
            Grant::direct("/opt/rw", true),
        ])
        .unwrap();
        let b = binds(&ops);
        assert!(b.contains(&(Path::new("/opt/ro"), Path::new("/opt/ro"), false)));
        assert!(b.contains(&(Path::new("/opt/rw"), Path::new("/opt/rw"), true)));
    }

    mod properties {
        use super::*;
        use proptest::prelude::*;

        fn arb_grant() -> impl Strategy<Value = Grant> {
            let seg = "[a-z]{1,8}";
            (
                proptest::collection::vec(seg, 1..4),
                any::<bool>(),
                proptest::option::of(proptest::collection::vec(seg, 1..3)),
            )
                .prop_map(|(segs, rw, link)| {
                    let requested = PathBuf::from(format!("/{}", segs.join("/")));
                    let resolved = match link {
                        None => requested.clone(),
                        Some(l) => PathBuf::from(format!("/{}", l.join("/"))),
                    };
                    Grant {
                        target: requested.clone(),
                        requested,
                        resolved,
                        rw,
                    }
                })
        }

        proptest! {
            #[test]
            fn no_planned_source_lies_outside_workspace_home_and_grants(
                grants in proptest::collection::vec(arb_grant(), 0..8)
            ) {
                let workspace = Path::new("/home/nabor/proj");
                let home = Path::new("/data/home");
                let image = Path::new("/box-image");
                let resolver = host_resolver();
                let Ok(ops) = compute(workspace, home, Root::Image(image), &user(), &grants, &resolver) else {
                    return Ok(()); // rejection is always a safe outcome
                };
                for op in &ops {
                    if let MountOp::Bind { source, .. } = op {
                        let allowed = source == workspace
                            || source == home
                            || source == image
                            || resolver == Resolver::Host(source.clone())
                            || grants.iter().any(|g| g.resolved == *source);
                        prop_assert!(allowed, "unplanned source {source:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn relative_workspace_is_rejected() {
        let err = compute(
            Path::new("proj"),
            Path::new("/data/home"),
            Root::HostUsr,
            &user(),
            &[],
            &host_resolver(),
        )
        .unwrap_err();
        assert_eq!(err, PlanError::WorkspaceNotAbsolute(PathBuf::from("proj")));
    }

    #[test]
    fn relative_home_is_rejected() {
        let err = compute(
            Path::new("/proj"),
            Path::new("home"),
            Root::HostUsr,
            &user(),
            &[],
            &host_resolver(),
        )
        .unwrap_err();
        assert_eq!(err, PlanError::HomeNotAbsolute(PathBuf::from("home")));
    }
}
