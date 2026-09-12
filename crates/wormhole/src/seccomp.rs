//! The last narrowing before the agent runs: a seccomp filter that shrinks
//! the kernel surface an assumed-hostile agent can reach.
//!
//! CONCEPT.md §1 names the one row the boundary does not fill: a kernel LPE
//! reachable from an unprivileged user namespace. Two things make that row
//! wide open without this filter, and both are proven, not assumed:
//!
//! - **Nested user namespaces hand back every capability.** A new user
//!   namespace resets the bounding set to full (`cap_bset = CAP_FULL_SET`
//!   in the kernel's `set_cred_user_ns`), so an agent that calls
//!   `unshare(CLONE_NEWUSER)` — or `clone` with the flag — undoes the empty
//!   bounding set the box started with and reaches `mount`, overlayfs and
//!   the rest of the admin-only surface.
//! - **`io_uring` is the single largest LPE surface Linux has.** Google's
//!   kCTF work put it in 60% of winning kernel exploits; Docker and ChromeOS
//!   turn it off by default.
//!
//! So the filter denies exactly the syscalls that buy an agent a new
//! namespace or a known escalation primitive, and nothing an ordinary tool
//! needs. It is a denylist, not an allowlist: the box exists to run an
//! autonomous agent reliably, and a filter that broke `git`, `node` or a
//! build the moment it met an unlisted syscall would be paid for every hour
//! against a threat this design has already chosen its ground on.
//!
//! Every denied syscall returns **ENOSYS**, "the kernel does not have it",
//! not EPERM. That is the gentlest signal a caller can get — `liburing`,
//! `keyctl` probes and feature checks all fall back on it — and it is what
//! `clone3` in particular *requires*: its flags live behind a pointer a BPF
//! filter cannot read, so it cannot be inspected; forcing ENOSYS makes
//! glibc fall back to plain `clone`, whose flags the filter *can* read
//! (this is the moby #42681 lesson).
//!
//! Applied after `pivot_root` and the capability drop, immediately before
//! the agent's `execve`. A seccomp filter is inherited across `execve` and
//! by every child, so one application covers the agent and everything it
//! spawns. The build box is deliberately left unfiltered: it installs
//! packages as root-in-userns, holds nothing of the host, and is thrown
//! away.

use nix::libc;
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};

/// The namespace flags to `clone`. Any one of them buys a new namespace —
/// and a new user namespace is what hands back the capabilities the box
/// dropped. `unshare` and `setns` are denied outright below; `clone` is
/// denied only when it carries one of these, so ordinary process and
/// thread creation (which sets none) is untouched.
const CLONE_NAMESPACE_FLAGS: [libc::c_int; 7] = [
    libc::CLONE_NEWNS,
    libc::CLONE_NEWCGROUP,
    libc::CLONE_NEWUTS,
    libc::CLONE_NEWIPC,
    libc::CLONE_NEWUSER,
    libc::CLONE_NEWPID,
    libc::CLONE_NEWNET,
];

/// Syscalls denied whatever their arguments: a new namespace, a known
/// container-escape primitive, or a kernel surface too broad to reason
/// about. The reason beside each is why it is here and not merely why it
/// is unusual.
const DENIED: &[(libc::c_long, &str)] = &[
    // A new namespace undoes the empty capability bounding set.
    (libc::SYS_unshare, "new namespaces"),
    (libc::SYS_setns, "join a namespace"),
    // clone3's flags hide behind a pointer the filter cannot read, so it
    // cannot be inspected like clone; ENOSYS pushes glibc onto clone.
    (libc::SYS_clone3, "the uninspectable clone"),
    // The largest kernel-LPE surface Linux has.
    (libc::SYS_io_uring_setup, "io_uring"),
    (libc::SYS_io_uring_enter, "io_uring"),
    (libc::SYS_io_uring_register, "io_uring"),
    // The kernel keyring is not namespaced.
    (libc::SYS_keyctl, "the kernel keyring"),
    (libc::SYS_add_key, "the kernel keyring"),
    (libc::SYS_request_key, "the kernel keyring"),
    // Persistent kernel programs and host-wide profiling.
    (libc::SYS_bpf, "loading BPF"),
    (libc::SYS_perf_event_open, "kernel profiling"),
    // Handing the kernel a page-fault handler in userspace.
    (libc::SYS_userfaultfd, "userspace page faults"),
    // Replacing or extending the running kernel.
    (libc::SYS_kexec_load, "loading a kernel"),
    (libc::SYS_kexec_file_load, "loading a kernel"),
    (libc::SYS_init_module, "kernel modules"),
    (libc::SYS_finit_module, "kernel modules"),
    (libc::SYS_delete_module, "kernel modules"),
    // A file handle sidesteps the mount namespace — an old breakout.
    (libc::SYS_open_by_handle_at, "opening by file handle"),
];

/// ENOSYS: "the kernel does not have it". See the module note for why this
/// and not EPERM.
const NOT_THERE: u32 = libc::ENOSYS as u32;

/// Builds the agent's seccomp filter. Pure: it reads no state and touches
/// nothing, so the policy can be checked without a kernel and only
/// [`narrow`] performs it.
pub fn filter() -> Result<BpfProgram, String> {
    let mut rules: std::collections::BTreeMap<i64, Vec<SeccompRule>> =
        std::collections::BTreeMap::new();

    // The outright denials carry no argument conditions: an empty rule
    // vector means "this syscall, whatever its arguments".
    for (syscall, _) in DENIED {
        rules.insert(*syscall, Vec::new());
    }

    // clone is denied only when it asks for a namespace. One rule per flag,
    // ORed by the filter, each true exactly when that bit is set — so a
    // clone that sets none (every thread and ordinary fork) is allowed and
    // a clone that sets any is not.
    let clone_rules = CLONE_NAMESPACE_FLAGS
        .iter()
        .map(|flag| {
            let bit = *flag as u64;
            SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(bit), bit)
                .and_then(|condition| SeccompRule::new(vec![condition]))
                .map_err(|e| format!("cannot build the clone rule: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    rules.insert(libc::SYS_clone, clone_rules);

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(NOT_THERE),
        target_arch()?,
    )
    .map_err(|e| format!("cannot build the seccomp filter: {e}"))?;

    filter
        .try_into()
        .map_err(|e| format!("cannot compile the seccomp filter: {e}"))
}

/// Installs the filter on the calling thread and every thread it later
/// spawns. `seccompiler` sets `no_new_privs` itself, so this needs no
/// capability — which is the point, since it runs after the box has
/// dropped them all.
pub fn narrow() -> Result<(), String> {
    let program = filter()?;
    seccompiler::apply_filter(&program).map_err(|e| format!("cannot apply the seccomp filter: {e}"))
}

/// This build's architecture, as seccompiler names it. A filter is a
/// program of syscall numbers, and those differ per architecture, so a
/// filter built for the wrong one would deny by number what it never meant
/// to.
fn target_arch() -> Result<seccompiler::TargetArch, String> {
    #[cfg(target_arch = "x86_64")]
    {
        Ok(seccompiler::TargetArch::x86_64)
    }
    #[cfg(target_arch = "aarch64")]
    {
        Ok(seccompiler::TargetArch::aarch64)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        Err("wormhole has no seccomp filter for this architecture".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter compiles to a real BPF program on this architecture. A
    /// filter that only builds in principle is one nothing has proven the
    /// kernel will take.
    #[test]
    fn the_filter_compiles() {
        assert!(!filter().expect("the filter builds").is_empty());
    }

    /// The two syscalls a nested user namespace needs are both denied — the
    /// hole the whole filter exists to close.
    #[test]
    fn the_namespace_syscalls_are_denied() {
        for wanted in [libc::SYS_unshare, libc::SYS_setns, libc::SYS_clone3] {
            assert!(
                DENIED.iter().any(|(syscall, _)| *syscall == wanted),
                "syscall {wanted} is not denied"
            );
        }
    }
}
