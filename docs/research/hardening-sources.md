# Hardening: primary-source facts

Research for adding seccomp (and maybe Landlock) to the wormhole sandbox.
Checked 2026-09-12. Every claim has its source. "Inference" marks my own
reasoning, not a quote from a source.

## 0. A fact to know first: nested user namespaces get all capabilities back

- A new user namespace resets the capability bounding set to full: `cred->cap_bset = CAP_FULL_SET;` in `set_cred_user_ns`.
  https://github.com/torvalds/linux/blob/master/kernel/user_namespace.c
- "The child process created by clone(2) with the CLONE_NEWUSER flag starts out with a complete set of capabilities in the new user namespace."
  https://man7.org/linux/man-pages/man7/user_namespaces.7.html
- Inference: our empty bounding set does not stop an agent from calling `unshare(CLONE_NEWUSER)` and then using `CAP_SYS_ADMIN`-gated code (mount, overlayfs, netfilter, and so on) inside that nested namespace. Only seccomp, an LSM, or a sysctl can close this.

## 1. Docker default seccomp profile

**How it works**
- `defaultAction` is `SCMP_ACT_ERRNO` with `defaultErrnoRet: 1`. It is an allowlist, so anything not listed is denied.
  https://github.com/moby/profiles/blob/main/seccomp/default.json
- Errno 1 is `EPERM` ("Operation not permitted"). 38 is `ENOSYS`.
  https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/errno-base.h ,
  https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/errno.h
- Docker's docs say the profile "disables around 44 system calls out of 300+" and that the effect is a "Permission Denied" error. That wording is loose: the errno is EPERM, not EACCES.
  https://docs.docker.com/engine/security/seccomp/
- Rules can depend on capabilities (`includes.caps` / `excludes.caps`). A container without the capability gets the default EPERM.
  https://github.com/moby/profiles/blob/main/seccomp/default.json

**Status with no extra capabilities (our case: empty bounding set).** Reasons are quoted from the Docker docs table.
Reason source: https://docs.docker.com/engine/security/seccomp/ . Status source: https://github.com/moby/profiles/blob/main/seccomp/default.json

| Syscall | Status | Allowed only with | Docker's stated reason |
|---|---|---|---|
| keyctl, add_key, request_key | EPERM | never | "kernel keyring, which is not namespaced" |
| bpf | EPERM | CAP_SYS_ADMIN or CAP_BPF | "potentially persistent BPF programs" |
| perf_event_open | EPERM | CAP_SYS_ADMIN or CAP_PERFMON | "could leak a lot of information on the host" |
| userfaultfd | EPERM | never | "largely needed for process migration" |
| io_uring_setup/enter/register | EPERM | never | "security vulnerabilities that can be exploited to break out of containers" |
| unshare | EPERM | CAP_SYS_ADMIN | "Deny cloning new namespaces" |
| clone | allowed only if `flags & 0x7E020000 == 0` | CAP_SYS_ADMIN for any flags | "Deny cloning new namespaces" |
| clone3 | **ENOSYS** (errnoRet 38) | CAP_SYS_ADMIN | see below |
| setns | EPERM | CAP_SYS_ADMIN | "Deny associating a thread with a namespace" |
| open_by_handle_at | EPERM | CAP_DAC_READ_SEARCH | "Cause of an old container breakout" |
| name_to_handle_at | **allowed** | - | (not in table) |
| kexec_load | EPERM | never | "Deny loading a new kernel" |
| init_module (and finit/delete) | EPERM | CAP_SYS_MODULE | "kernel modules" |
| ptrace | **allowed on kernel >= 4.8** | - | "Blocked in Linux kernel versions before 4.8 to avoid seccomp bypass" |
| personality | allowed only for arg 0, 0x8, 0x20000, 0x20008, 0xffffffff | - | "Prevent container from enabling BSD emulation" |
| mount, umount2 | EPERM | CAP_SYS_ADMIN | "already gated by CAP_SYS_ADMIN" |
| pivot_root | EPERM | never | "should be privileged operation" |
| chroot | EPERM | CAP_SYS_CHROOT | (not in table) |
| acct | EPERM | CAP_SYS_PACCT | "disable their own resource limits or process accounting" |
| swapon | EPERM | never | "Deny start/stop swapping" |
| reboot | EPERM | CAP_SYS_BOOT | "Don't let containers reboot the host" |
| lookup_dcookie | EPERM | CAP_SYS_ADMIN | "could leak a lot of information on the host" |
| move_pages | EPERM | never | "modifies kernel memory and NUMA settings" |
| get_mempolicy, set_mempolicy, mbind | EPERM | CAP_SYS_NICE | "modifies kernel memory and NUMA settings" |
| nfsservctl | EPERM | never | "Obsolete since Linux 3.1" |
| vm86 | EPERM | never | "In kernel x86 real mode virtual machine" |
| uselib | EPERM | never | "unused for a long time" |
| ustat, sysfs | EPERM | never | "Obsolete syscall" |
| _sysctl | EPERM | never | "Obsolete, replaced by /proc/sys" |
| quotactl | EPERM | CAP_SYS_ADMIN | "disable their own resource limits" |
| fanotify_init | EPERM | CAP_SYS_ADMIN | (not in table) |

- The clone mask `0x7E020000` is exactly CLONE_NEWNS, NEWCGROUP, NEWUTS, NEWIPC, NEWUSER, NEWPID and NEWNET.
  https://github.com/torvalds/linux/blob/master/include/uapi/linux/sched.h
- The personality values are PER_LINUX (0), PER_LINUX32 (0x8), UNAME26 (0x20000), their OR, and 0xffffffff (query).
  https://github.com/torvalds/linux/blob/master/include/uapi/linux/personality.h
- The same `CAP_SYS_ADMIN` rule also covers the new mount API (fsopen, fsmount, fsconfig, fspick, move_mount, open_tree, mount_setattr) plus sethostname and setdomainname.
  https://github.com/moby/profiles/blob/main/seccomp/default.json

**Why clone3 returns ENOSYS and not EPERM**
- "If it sees ENOSYS then it will automatically fallback to using clone." With EPERM, new glibc treats the failure as fatal.
  https://github.com/moby/moby/pull/42681 , https://github.com/moby/moby/issues/42680
- clone3 flags "are hidden inside a struct. This means that seccomp filters are unable to apply policy based on values seen in flags."
  https://github.com/moby/moby/pull/42681
- Kernel: "BPF programs may not dereference pointers which constrains all filters to solely evaluating the system call arguments directly."
  https://docs.kernel.org/userspace-api/seccomp_filter.html

## 2. Unprivileged user namespaces and io_uring as exploit surface

1. Google kCTF VRP: "60% of the submissions exploited the io_uring component", about 1M USD paid for io_uring, and "io_uring vulnerabilities were used in all the submissions which bypassed our mitigations". Google turned io_uring off in ChromeOS, made it unreachable for Android apps with seccomp, and disabled it on production servers. It is considered "safe only for use by trusted components".
   https://security.googleblog.com/2023/06/learnings-from-kctf-vrps-42-linux.html
2. Docker blocked io_uring_* by default (merged 2023-11-02), citing container breakouts and Google's move.
   https://github.com/moby/moby/pull/46762
3. Ubuntu: "In a report from Google, 44% of the exploits they saw required unprivileged user namespaces". Caveat: the text is Ubuntu's summary. The linked Google post's HTML does not contain "44%", so the number probably comes from its linked data.
   https://ubuntu.com/blog/ubuntu-23-10-restricted-unprivileged-user-namespaces
4. Ubuntu: "Unprivileged user namespaces have been repeatedly used to exploit kernel vulnerabilities."
   https://discourse.ubuntu.com/t/understanding-apparmor-user-namespace-restriction/58007
5. GameOver(lay), CVE-2023-2640 and CVE-2023-32629: Ubuntu overlayfs local privilege escalations, CVSS 7.8. Ubuntu's listed mitigation is to disable unprivileged user namespace creation.
   https://ubuntu.com/security/CVE-2023-2640 , https://ubuntu.com/security/CVE-2023-32629
6. CVE-2023-0386: overlayfs copy-up of a file with capabilities, local privilege escalation. The fix commit says `Fixes: 459c7c565ac3 ("ovl: unprivieged mounts")` and is marked for stable v5.11. The bug came in with the feature that lets overlayfs be mounted inside a user namespace.
   https://nvd.nist.gov/vuln/detail/CVE-2023-0386 ,
   https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/commit/?id=4f11ada10d0a

## 3. Tools that break when nested user namespaces are denied

**Chromium / Chrome**
- The primary sandbox uses user namespaces. When they are blocked, developer builds fail. Without root, "run developer builds with the `--no-sandbox` command line flag", which "disables critical security features". Another fallback is the setuid helper (`CHROME_DEVEL_SANDBOX=/opt/google/chrome/chrome-sandbox`).
  https://chromium.googlesource.com/chromium/src/+/main/docs/security/apparmor-userns-restrictions.md
- Inference: the setuid helper cannot work in wormhole. `no_new_privs` turns off setuid, as bubblewrap's README notes ("uses `PR_SET_NO_NEW_PRIVS` to turn off setuid binaries").
  https://github.com/containers/bubblewrap/blob/main/README.md

**Puppeteer**
- The symptom is the `No usable sandbox!` error. The workaround is `puppeteer.launch({ args: ['--no-sandbox'] })`.
  https://pptr.dev/troubleshooting (source: https://github.com/puppeteer/puppeteer/blob/main/docs/troubleshooting.md)

**Playwright**
- `chromiumSandbox`: "Enable Chromium sandboxing. Defaults to `false`." Playwright runs Chromium without its sandbox unless asked, so it is not affected by default.
  https://playwright.dev/docs/api/class-browsertype

**bubblewrap** (and Flatpak, which uses it)
- It uses user namespaces. "Historically, bubblewrap also supported a setuid mode ... However, this has been removed."
  https://github.com/containers/bubblewrap/blob/main/README.md
- The failure message is `No permissions to create a new namespace, likely because the kernel does not allow non-privileged user namespaces.`
  https://github.com/containers/bubblewrap/blob/main/bubblewrap.c

**Podman rootless**
- "Podman makes use of a user namespace to shift the UIDs and GIDs ... (via the `newuidmap` and `newgidmap` executables)".
  https://github.com/containers/podman/blob/main/docs/tutorials/rootless_tutorial.md
- Inference: it breaks without nested user namespaces. Multi-UID mapping also needs the privileged `newuidmap`, which gains nothing under no_new_privs.

## 4. Ubuntu `kernel.apparmor_restrict_unprivileged_userns`

- Sysctl path: `/proc/sys/kernel/apparmor_restrict_unprivileged_userns`. Related knobs: `..._force` and `..._complain` (6.2+), plus `kernel.apparmor_restrict_unprivileged_unconfined`, which blocks the `aa-exec` bypass.
  https://gitlab.com/apparmor/apparmor/-/wikis/unprivileged_userns_restriction ,
  https://discourse.ubuntu.com/t/understanding-apparmor-user-namespace-restriction/58007
- It was added in 23.10, off by default.
  https://discourse.ubuntu.com/t/mantic-minotaur-release-notes/35534
- **24.04 behaviour (default on): the call succeeds but has no capabilities.** "A default AppArmor profile is provided that allows the use of user namespaces for unprivileged and unconfined applications but will deny the subsequent use of any capabilities within the user namespace."
  https://discourse.ubuntu.com/t/ubuntu-24-04-lts-noble-numbat-release-notes/39890
- That default profile is `profile unprivileged_userns { allow all, deny capability, allow pix /**, }`. If it is not loaded, "apparmor will fallback to denying" creation. The audit log shows `error=-13`, which is EACCES.
  https://gitlab.com/apparmor/apparmor/-/wikis/unprivileged_userns_restriction
- Minimal official allow profile, from the 24.04 release notes. The file is `/etc/apparmor.d/chrome`:
  ```
  abi <abi/4.0>,
  include <tunables/global>

  /opt/google/chrome/chrome flags=(unconfined) {
    userns,

    # Site-specific additions and overrides. See local/README for details.
    include if exists <local/chrome>
  }
  ```
  https://discourse.ubuntu.com/t/ubuntu-24-04-lts-noble-numbat-release-notes/39890
- The reload command is `sudo service apparmor reload`, shown with a `profile chrome <path> flags=(unconfined) { userns, ... }` example.
  https://chromium.googlesource.com/chromium/src/+/main/docs/security/apparmor-userns-restrictions.md
- Inference for wormhole: on 24.04, wormhole itself needs such a profile, or its own `unshare(CLONE_NEWUSER)` gets no capabilities, so `pivot_root` and `mount` fail.

## 5. Unprivileged overlayfs in a user namespace

- **Minimum kernel 5.11.** `.fs_flags = FS_USERNS_MOUNT` is present in overlayfs `super.c` at v5.11 and absent at v5.10.
  https://github.com/torvalds/linux/blob/v5.11/fs/overlayfs/super.c ,
  https://github.com/torvalds/linux/blob/v5.10/fs/overlayfs/super.c
- **`userxattr`** also first shows up in the overlayfs doc at v5.11. It "forces overlayfs to use the "user.overlay." xattr namespace instead of "trusted.overlay.". This is useful for unprivileged mounting".
  https://github.com/torvalds/linux/blob/v5.11/Documentation/filesystems/overlayfs.rst ,
  https://docs.kernel.org/filesystems/overlayfs.html
- With `userxattr`, `redirect_dir` (other than nofollow) and `metacopy=on` are rejected with EINVAL. Without `userxattr` and without CAP_SYS_ADMIN, requesting those features fails with EPERM.
  https://github.com/torvalds/linux/blob/master/fs/overlayfs/params.c
- The upper fs "must support the creation of trusted.* and/or user.* extended attributes, and must provide valid d_type in readdir responses, so NFS is not suitable."
  https://docs.kernel.org/filesystems/overlayfs.html
- If the upper fs rejects the xattr, the kernel only warns ("failed to set xattr on upper"), turns off index, metacopy and xino, and on EPERM hints "try mounting with 'userxattr' option". A missing RENAME_WHITEOUT or O_TMPFILE also only warns.
  https://github.com/torvalds/linux/blob/master/fs/overlayfs/super.c
- **tmpfs as upper** supports `user.*` xattrs only since 6.6 (v6.5 doc lists trusted.* and security.* only). Inference: a tmpfs upper with `userxattr` needs 6.6+.
  https://github.com/torvalds/linux/blob/v6.5/Documentation/filesystems/tmpfs.rst ,
  https://github.com/torvalds/linux/blob/v6.6/Documentation/filesystems/tmpfs.rst
- **Overlay on overlay**: the lower layer may be overlayfs ("The lower filesystem can even be another overlayfs"). The max stacking depth is `FILESYSTEM_MAX_STACK_DEPTH 2`, and going past it gives "maximum fs stacking depth exceeded".
  https://docs.kernel.org/filesystems/overlayfs.html ,
  https://github.com/torvalds/linux/blob/master/include/linux/fs.h ,
  https://github.com/torvalds/linux/blob/master/fs/overlayfs/super.c
- An overlayfs upper is not documented as supported. overlayfs `rename` rejects any flag other than RENAME_EXCHANGE and RENAME_NOREPLACE, so it has no RENAME_WHITEOUT, and its own `overlay.*` xattrs are private unless escaped. Treat an overlay upper as unsupported until tested.
  https://github.com/torvalds/linux/blob/master/fs/overlayfs/dir.c ,
  https://github.com/torvalds/linux/blob/master/fs/overlayfs/xattrs.c
- Reusing an upper or work dir that another overlay mount already uses "may fail with EBUSY".
  https://docs.kernel.org/filesystems/overlayfs.html

## 6. rust-vmm `seccompiler`

- The current version is **0.5.0** (published 2025-03-07). It is also the `main` branch version.
  https://crates.io/api/v1/crates/seccompiler , https://github.com/rust-vmm/seccompiler/blob/main/Cargo.toml
- It is **pure Rust, with no libseccomp**. Its dependencies are `libc` plus optional `serde`/`serde_json` for the `json` feature.
  https://github.com/rust-vmm/seccompiler/blob/main/Cargo.toml
- Architectures: little-endian x86_64, aarch64 and riscv64 (`TargetArch`).
  https://github.com/rust-vmm/seccompiler/blob/main/README.md ,
  https://github.com/rust-vmm/seccompiler/blob/main/src/backend/mod.rs
- Build and apply: `SeccompFilter::new(rules, mismatch_action, match_action, TargetArch)`, then `let prog: BpfProgram = filter.try_into()?;`, then `seccompiler::apply_filter(&prog)?`. JSON input goes through `compile_from_json` (feature `json`).
  https://github.com/rust-vmm/seccompiler/blob/main/README.md , https://docs.rs/seccompiler/0.5.0
- `apply_filter` calls `prctl(PR_SET_NO_NEW_PRIVS, 1)` itself, then `seccomp(SECCOMP_SET_MODE_FILTER, flags, prog)`. `apply_filter_all_threads` passes `SECCOMP_FILTER_FLAG_TSYNC`. An empty program returns `Error::EmptyFilter`.
  https://github.com/rust-vmm/seccompiler/blob/main/src/lib.rs
- **Argument matching: yes.** `SeccompCondition(index, SeccompCmpArgLen, SeccompCmpOp, value)` with ops `Eq, Ge, Gt, Le, Lt, MaskedEq(mask), Ne`. Conditions inside a `SeccompRule` are ANDed; rules for one syscall are ORed. `MaskedEq` can filter `clone` namespace flags. It cannot filter clone3 (flags sit behind a pointer, see §1).
  https://github.com/rust-vmm/seccompiler/blob/main/src/backend/mod.rs ,
  https://github.com/rust-vmm/seccompiler/blob/main/README.md
- `SECCOMP_FILTER_FLAG_TSYNC`: "When adding a new filter, synchronize all other threads of the calling process to the same seccomp filter tree". If any thread cannot sync, the filter is not attached and the call returns that thread's ID.
  https://man7.org/linux/man-pages/man2/seccomp.2.html
- **Children and exec inherit the filter**: "If fork(2) or clone(2) is allowed by the filter, any child processes will be constrained to the same system call filters as the parent. If execve(2) is allowed, the existing filters will be preserved across a call to execve(2)."
  https://man7.org/linux/man-pages/man2/seccomp.2.html
- Installing needs CAP_SYS_ADMIN in the user namespace or `no_new_privs`, otherwise EACCES.
  https://man7.org/linux/man-pages/man2/seccomp.2.html

## 7. Landlock

- It was introduced in **Linux 5.13** (ABI 1). ABI table: v2 5.19 (REFER), v3 6.2 (TRUNCATE), v4 6.7 (TCP bind/connect), v5 6.10 (IOCTL_DEV), v6 6.12 (scope abstract UNIX sockets, signals), v7 6.15 (logging), v8 7.0 (TSYNC), v9 7.1 (RESOLVE_UNIX). The man page says to use the ABI version, not the kernel version.
  https://man7.org/linux/man-pages/man7/landlock.7.html
- Neither the Landlock doc nor the man page compares it with a pivot_root'ed mount namespace. What it adds beyond "only planned paths are visible" follows.
  - **Per-path rights inside visible paths**: execute, write, truncate, remove, make device/socket/fifo/symlink, ioctl on devices. A mount namespace only gives visibility plus ro/nosuid/noexec per mount.
    https://docs.kernel.org/userspace-api/landlock.html
  - **Abstract UNIX sockets**: `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET` limits `connect(2)` to sockets made in the same domain. Inference: abstract sockets belong to the network namespace, not the filesystem, so without a netns the sandbox can reach host abstract sockets (for example X11, D-Bus). Landlock ABI 6 is one fix. A private netns is the other.
    https://docs.kernel.org/userspace-api/landlock.html
  - **TCP bind/connect by port** (ABI 4).
    https://man7.org/linux/man-pages/man7/landlock.7.html
  - **No topology changes**: "Threads sandboxed with filesystem restrictions cannot modify filesystem topology, whether via mount(2) or pivot_root(2). However, chroot(2) calls are not denied." This holds even inside a nested user namespace, which covers the §0 gap for mounts.
    https://docs.kernel.org/userspace-api/landlock.html
- Limits:
  - Rules on an overlayfs layer do not apply to the merged view ("A policy restricting an OverlayFS layer will not restrict the resulted merged hierarchy"), so rules must target the mounted paths.
  - Pipes, sockets via `/proc/<pid>/fd` and nsfs cannot be restricted explicitly.
  https://docs.kernel.org/userspace-api/landlock.html
