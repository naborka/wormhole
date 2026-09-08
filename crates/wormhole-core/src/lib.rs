//! Pure core: every decision, no syscalls, no I/O.

pub mod boxenv;
pub mod ca;
pub mod doctor;
pub mod gc;
pub mod help;
pub mod home;
pub mod launch;
pub mod limits_cgroup;
pub mod manifest;
pub mod mount_plan;
pub mod paths;
pub mod receipt;
pub mod registry;
pub mod run;
pub mod secrets;
pub mod seed;
pub mod source;
pub mod table;
pub mod terminfo;
pub mod tui;

/// The one spelling of a SHA-256 digest everywhere in wormhole:
/// lowercase hex, full length. Cache names, recipe digests and rootfs
/// verification all compare these strings, so one body owns the format.
/// Lowercase hex of exactly `len` characters. A digest, a box id and a
/// commit are all this shape, and one body is what keeps them agreeing on
/// what "hex" means.
#[must_use]
pub fn is_lowercase_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod purity {
    /// The purity rule from PLAN.md: this crate makes decisions only —
    /// no OS, no I/O, no async. Enforced against the manifest itself.
    #[test]
    fn core_has_no_os_or_io_dependencies() {
        let manifest = include_str!("../Cargo.toml");
        let deps = manifest
            .split("[dependencies]")
            .nth(1)
            .and_then(|rest| rest.split("[dev-dependencies]").next())
            .unwrap_or_default();
        for forbidden in ["libc", "nix", "rustix", "tokio", "async-std"] {
            assert!(
                !deps.contains(forbidden),
                "wormhole-core must stay pure; found {forbidden} in [dependencies]"
            );
        }
    }
}
