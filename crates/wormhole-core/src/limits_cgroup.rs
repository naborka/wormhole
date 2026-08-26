//! Resource limits on a box, as cgroup v2 files.
//!
//! CONCEPT.md §10 claims runaway resource use is enforced. Until these are
//! applied that claim is false, and a document asserting a protection the
//! code does not implement is worse than no claim at all.
//!
//! Everything here is the translation from what a person writes in a
//! manifest to what the kernel reads in a cgroup file. Creating the cgroup
//! and moving the box into it is the binary's.

use std::fmt;

use serde::Deserialize;

/// The scheduling period cgroup v2 measures cpu against, in microseconds.
/// The kernel's own default; naming it keeps the quota arithmetic readable.
const CPU_PERIOD_US: u64 = 100_000;

/// What a manifest may limit. Absent means the kernel's own default, which
/// is no limit — said plainly rather than implied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Cores, as a decimal: `1.5` means one and a half cores' worth of
    /// runtime per period. Not a pinning, a share of time.
    #[serde(default)]
    pub cpu: Option<String>,
    /// Memory ceiling: `512M`, `2G`, or plain bytes. The box meets the OOM
    /// killer at this, rather than the host being pushed into swap.
    #[serde(default)]
    pub memory: Option<String>,
    /// How many processes and threads the box may have at once. The cheap
    /// answer to a fork bomb, which no memory limit catches in time.
    #[serde(default)]
    pub pids: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitError {
    Cpu(String),
    Memory(String),
}

impl fmt::Display for LimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LimitError::Cpu(value) => write!(
                f,
                "[limits] cpu wants a number of cores like \"1.5\", not {value:?}"
            ),
            LimitError::Memory(value) => write!(
                f,
                "[limits] memory wants a size like \"512M\" or \"2G\", not {value:?}"
            ),
        }
    }
}

impl std::error::Error for LimitError {}

impl Limits {
    /// Every cgroup file this box needs written, in the order they are
    /// written. Empty when the manifest limits nothing, so a box with no
    /// limits creates no cgroup at all rather than an empty one.
    pub fn files(&self) -> Result<Vec<(&'static str, String)>, LimitError> {
        let mut files = Vec::new();
        if let Some(cpu) = &self.cpu {
            files.push(("cpu.max", format!("{} {CPU_PERIOD_US}", quota(cpu)?)));
        }
        if let Some(memory) = &self.memory {
            files.push(("memory.max", bytes(memory)?.to_string()));
        }
        if let Some(pids) = self.pids {
            files.push(("pids.max", pids.to_string()));
        }
        Ok(files)
    }

    /// The controllers these limits need, for the `cgroup.subtree_control`
    /// line the parent cgroup must carry. A controller the parent has not
    /// enabled makes its file absent in the child, so the limit would be
    /// written nowhere and nobody would know.
    pub fn controllers(&self) -> Vec<&'static str> {
        let mut controllers = Vec::new();
        if self.cpu.is_some() {
            controllers.push("cpu");
        }
        if self.memory.is_some() {
            controllers.push("memory");
        }
        if self.pids.is_some() {
            controllers.push("pids");
        }
        controllers
    }
}

/// Cores as a person writes them, as microseconds of runtime per period.
fn quota(cpu: &str) -> Result<u64, LimitError> {
    let cores: f64 = cpu.parse().map_err(|_| LimitError::Cpu(cpu.to_owned()))?;
    if !cores.is_finite() || cores <= 0.0 {
        return Err(LimitError::Cpu(cpu.to_owned()));
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a quota in microseconds is a whole number by definition"
    )]
    Ok((cores * CPU_PERIOD_US as f64).round() as u64)
}

/// `512M`, `2G`, `1500` — the sizes a person writes, as the bytes the
/// kernel reads. Binary multiples, because that is what a memory limit
/// means everywhere else on a Linux system.
fn bytes(value: &str) -> Result<u64, LimitError> {
    let text = value.trim();
    let (digits, scale) = match text.chars().last() {
        Some('K' | 'k') => (&text[..text.len() - 1], 1024),
        Some('M' | 'm') => (&text[..text.len() - 1], 1024 * 1024),
        Some('G' | 'g') => (&text[..text.len() - 1], 1024 * 1024 * 1024),
        _ => (text, 1),
    };
    digits
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .and_then(|n| n.checked_mul(scale))
        .ok_or_else(|| LimitError::Memory(value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(cpu: Option<&str>, memory: Option<&str>, pids: Option<u32>) -> Limits {
        Limits {
            cpu: cpu.map(str::to_owned),
            memory: memory.map(str::to_owned),
            pids,
        }
    }

    /// A box that limits nothing creates no cgroup: an empty one is a
    /// directory that exists to say nothing.
    #[test]
    fn a_manifest_that_limits_nothing_writes_nothing() {
        assert_eq!(Limits::default().files(), Ok(Vec::new()));
        assert!(Limits::default().controllers().is_empty());
    }

    /// Cores as a person writes them, against the kernel's own period.
    #[test]
    fn cpu_cores_become_a_quota_over_the_scheduling_period() {
        for (written, expected) in [
            ("1.5", "150000 100000"),
            ("1", "100000 100000"),
            ("0.25", "25000 100000"),
        ] {
            assert_eq!(
                limits(Some(written), None, None).files(),
                Ok(vec![("cpu.max", expected.to_owned())]),
                "{written}"
            );
        }
    }

    #[test]
    fn sizes_are_binary_multiples_like_everywhere_else_on_linux() {
        for (written, expected) in [
            ("512M", 512 * 1024 * 1024_u64),
            ("2G", 2 * 1024 * 1024 * 1024),
            ("64k", 64 * 1024),
            ("1500", 1500),
        ] {
            assert_eq!(
                limits(None, Some(written), None).files(),
                Ok(vec![("memory.max", expected.to_string())]),
                "{written}"
            );
        }
    }

    /// A limit that cannot be read must stop the box, not become an
    /// accidental no-limit — which is exactly the silent failure this
    /// module exists to remove.
    #[test]
    fn a_limit_that_cannot_be_read_is_refused_naming_what_was_written() {
        for bad in ["", "lots", "-1", "0", "1.5G"] {
            let err = limits(None, Some(bad), None).files().unwrap_err();
            assert_eq!(err, LimitError::Memory(bad.to_owned()), "{bad}");
            assert!(err.to_string().contains("512M"), "{err}");
        }
        for bad in ["", "fast", "-1", "0", "abc"] {
            assert_eq!(
                limits(Some(bad), None, None).files().unwrap_err(),
                LimitError::Cpu(bad.to_owned()),
                "{bad}"
            );
        }
    }

    /// Only what is asked for is enabled: delegating a controller nothing
    /// uses widens what the box's cgroup can do for no reason.
    #[test]
    fn only_the_controllers_the_limits_need_are_asked_for() {
        assert_eq!(
            limits(Some("1"), None, Some(64)).controllers(),
            ["cpu", "pids"]
        );
        assert_eq!(
            limits(Some("1"), Some("1G"), Some(64)).controllers(),
            ["cpu", "memory", "pids"]
        );
    }

    #[test]
    fn all_three_limits_are_written_together() {
        assert_eq!(
            limits(Some("2"), Some("1G"), Some(512)).files(),
            Ok(vec![
                ("cpu.max", "200000 100000".to_owned()),
                ("memory.max", (1024 * 1024 * 1024_u64).to_string()),
                ("pids.max", "512".to_owned()),
            ])
        );
    }
}
