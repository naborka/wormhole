//! Doctor verdict: probe facts in, verdict out. The `wormhole` crate
//! gathers facts (file reads, syscalls); every judgement about them —
//! including the failure message — is made and tested here.

pub mod outcome;
pub mod parse;

use core::fmt;

/// Every host probe `wormhole doctor` runs, in report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    UserNamespaces,
    MaxUserNamespaces,
    SubIdRanges,
    Landlock,
    Overlayfs,
    CgroupDelegation,
    Reflink,
    UidMapOverlap,
}

impl Probe {
    pub const ALL: [Probe; 8] = [
        Probe::UserNamespaces,
        Probe::MaxUserNamespaces,
        Probe::SubIdRanges,
        Probe::Landlock,
        Probe::Overlayfs,
        Probe::CgroupDelegation,
        Probe::Reflink,
        Probe::UidMapOverlap,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Probe::UserNamespaces => "unprivileged user namespaces",
            Probe::MaxUserNamespaces => "user.max_user_namespaces",
            Probe::SubIdRanges => "subuid/subgid ranges",
            Probe::Landlock => "Landlock",
            Probe::Overlayfs => "unprivileged overlayfs",
            Probe::CgroupDelegation => "cgroup v2 delegation",
            Probe::Reflink => "workspace reflink support",
            Probe::UidMapOverlap => "uid-map overlap",
        }
    }
}

/// `Fail` blocks wormhole on this host; `Info` records a fact that does
/// not (diagnostics). The constructors in [`outcome`]
/// are the only place that chooses between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail(String),
    Info(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub probe: Probe,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// At least one probe failed.
    Unusable,
}

impl Status {
    pub fn exit_code(self) -> i32 {
        match self {
            Status::Ok => 0,
            Status::Unusable => 1,
        }
    }
}

/// The decision (`status`) plus the facts it was made from. Rendering
/// lives in `Display`; the decision holds no text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub status: Status,
    pub results: Vec<ProbeResult>,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.results {
            match &r.outcome {
                Outcome::Pass => writeln!(f, "ok   {}", r.probe.name())?,
                Outcome::Fail(detail) => writeln!(f, "FAIL {}: {detail}", r.probe.name())?,
                Outcome::Info(detail) => writeln!(f, "info {}: {detail}", r.probe.name())?,
            }
        }
        match self.status {
            Status::Ok => writeln!(f, "verdict: ok"),
            Status::Unusable => writeln!(f, "verdict: this host cannot run wormhole"),
        }
    }
}

pub fn evaluate(results: Vec<ProbeResult>) -> Verdict {
    let failed = results
        .iter()
        .any(|r| matches!(r.outcome, Outcome::Fail(_)));
    Verdict {
        status: if failed { Status::Unusable } else { Status::Ok },
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_pass() -> Vec<ProbeResult> {
        Probe::ALL
            .iter()
            .map(|&probe| ProbeResult {
                probe,
                outcome: Outcome::Pass,
            })
            .collect()
    }

    fn with_outcome(probe: Probe, outcome: Outcome) -> Vec<ProbeResult> {
        all_pass()
            .into_iter()
            .map(|r| {
                if r.probe == probe {
                    ProbeResult {
                        probe,
                        outcome: outcome.clone(),
                    }
                } else {
                    r
                }
            })
            .collect()
    }

    #[test]
    fn all_probes_passing_is_ok_with_exit_zero() {
        let verdict = evaluate(all_pass());
        assert_eq!(verdict.status, Status::Ok);
        assert_eq!(verdict.status.exit_code(), 0);
        assert!(verdict.to_string().contains("verdict: ok\n"));
    }

    #[test]
    fn any_failed_probe_is_unusable_with_its_own_message() {
        for probe in Probe::ALL {
            let verdict = evaluate(with_outcome(probe, Outcome::Fail("boom".to_owned())));
            assert_eq!(verdict.status, Status::Unusable, "{}", probe.name());
            assert_eq!(verdict.status.exit_code(), 1);
            let fail_line = format!("FAIL {}: boom\n", probe.name());
            let rendered = verdict.to_string();
            assert!(
                rendered.contains(&fail_line),
                "missing {fail_line:?} in {rendered:?}"
            );
        }
    }

    #[test]
    fn informational_outcome_stays_ok_but_is_recorded() {
        let verdict = evaluate(with_outcome(
            Probe::Reflink,
            Outcome::Info("absent".to_owned()),
        ));
        assert_eq!(verdict.status, Status::Ok);
        assert!(
            verdict
                .to_string()
                .contains("info workspace reflink support: absent\n")
        );
    }

    #[test]
    fn two_failures_report_both_lines() {
        let mut results = with_outcome(Probe::Landlock, Outcome::Fail("ABI missing".to_owned()));
        for r in &mut results {
            if r.probe == Probe::Overlayfs {
                r.outcome = Outcome::Fail("EPERM".to_owned());
            }
        }
        let verdict = evaluate(results);
        assert_eq!(verdict.status, Status::Unusable);
        let rendered = verdict.to_string();
        assert!(rendered.contains("FAIL Landlock: ABI missing\n"));
        assert!(rendered.contains("FAIL unprivileged overlayfs: EPERM\n"));
    }
}
