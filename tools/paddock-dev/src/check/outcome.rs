//! The exit status of a conformance run (ADR 0089 section 2): a check that could not look is its own status, and no waiver, warning
//! or other finding turns it into a pass. The port of `conformance/src/outcome.mjs`.

use super::finding::{Finding, Severity};
use std::collections::BTreeSet;

/// Why an error finding cannot be waived: the check did not look (ER-33).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The check crashed, or its positive control failed: it fails closed with the higher exit status.
    CheckBroken,
    /// The check did not look (incomplete, tool missing, nothing probed).
    NotMeasured,
}

impl Outcome {
    /// The name in the report (`check-broken`, `not-measured`).
    pub fn name(self) -> &'static str {
        match self {
            Outcome::CheckBroken => "check-broken",
            Outcome::NotMeasured => "not-measured",
        }
    }
}

/// Exit status 0: clean.
pub const CLEAN: i32 = 0;
/// Exit status 1: an error finding is left (or a warning of a check named in `--strict`).
pub const FINDINGS: i32 = 1;
/// Exit status 2: a usage error.
pub const USAGE: i32 = 2;
/// Exit status 8: a report file (`--out`, `--summary`, the job summary) could not be written; the run's own result is still printed.
pub const IO_ERROR: i32 = 8;
/// Exit status 6: a check did not look.
pub const NOT_MEASURED: i32 = 6;
/// Exit status 7: a check is broken.
pub const CHECK_BROKEN: i32 = 7;

/// 7 when a check is broken, else 1 when an error finding is left (or a warning of a check in `strict`), else 6 when a check did
/// not look, else 0: warnings are report-only and never hide a missing measurement.
pub fn exit_status(findings: &[Finding], strict: &BTreeSet<String>) -> i32 {
    if findings.iter().any(|f| f.outcome == Some(Outcome::CheckBroken)) {
        return CHECK_BROKEN;
    }
    if findings
        .iter()
        .any(|f| (f.severity == Severity::Error && f.outcome.is_none()) || (f.severity == Severity::Warn && strict.contains(&f.check)))
    {
        return FINDINGS;
    }
    if findings.iter().any(|f| f.outcome == Some(Outcome::NotMeasured)) {
        return NOT_MEASURED;
    }
    CLEAN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strict(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn broken_beats_findings_beats_not_measured_beats_clean() {
        let err = Finding::error("C1", 1, "a", 1, "m", "f");
        let broken = Finding::error("C2", 1, "", 1, "m", "f")
            .unwaivable()
            .with_outcome(Outcome::CheckBroken);
        let unseen = Finding::error("C3", 1, "", 1, "m", "f")
            .unwaivable()
            .with_outcome(Outcome::NotMeasured);
        let warn = Finding::warn("C4", 1, "a", 1, "m", "f");
        let none = strict(&[]);
        assert_eq!(exit_status(&[], &none), CLEAN);
        assert_eq!(exit_status(std::slice::from_ref(&warn), &none), CLEAN, "a warning is report-only");
        assert_eq!(exit_status(std::slice::from_ref(&unseen), &none), NOT_MEASURED);
        assert_eq!(exit_status(&[unseen.clone(), err.clone()], &none), FINDINGS);
        assert_eq!(exit_status(&[unseen, err, broken], &none), CHECK_BROKEN);
    }

    #[test]
    fn strict_makes_the_named_checks_warnings_fail() {
        let warn = Finding::warn("C36", 1, "a", 1, "m", "f");
        assert_eq!(exit_status(std::slice::from_ref(&warn), &strict(&["C36"])), FINDINGS);
        assert_eq!(exit_status(&[warn], &strict(&["C30"])), CLEAN);
    }
}
