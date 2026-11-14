//! The exit status of a conformance run (ADR 0089 section 2): a check that could not look is its own status, and no waiver, warning
//! or other finding turns it into a pass. The port of `conformance/src/outcome.mjs`.

use super::finding::{Finding, Severity};
use paddock_cli::{
    CheckId, Exit, Finding as SharedFinding, Outcome as SharedOutcome, Reason, Subject, ToolVersion, UnixSeconds, Unmeasured, Verdict,
    measure,
};
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

/// The exit status of a run, folded through the shared [`paddock_cli::Verdict`] (ADR 0089 amendment, WP-T2): each check that ran
/// (`checks`, plus any check that left a finding) is one shared outcome made through `measure` (a pass only with its evidence,
/// `subject` being the tree the run read), and the verdict gives the code: 7 when a check is broken, else 1 when an error finding is
/// left (or a warning of a check in `strict`), else 6 when a check did not look **or no check ran at all** (row 6), else 0: warnings
/// are report-only and never hide a missing measurement.
pub fn exit_status(findings: &[Finding], strict: &BTreeSet<String>, checks: &[String], subject: &Subject) -> i32 {
    let mut ids: Vec<&str> = checks.iter().map(String::as_str).collect();
    for f in findings {
        let counts = f.severity == Severity::Error || f.outcome.is_some() || strict.contains(&f.check);
        if counts && !ids.contains(&f.check.as_str()) {
            ids.push(f.check.as_str());
        }
    }
    let items: Vec<(String, SharedOutcome)> = ids
        .into_iter()
        .map(|id| {
            let mine: Vec<&Finding> = findings.iter().filter(|f| f.check == id).collect();
            let outcome = measure(CheckId::new(id), subject.clone(), tool(), UnixSeconds::now(), || {
                if let Some(f) = mine.iter().find(|f| f.outcome == Some(Outcome::CheckBroken)) {
                    return Err(Unmeasured::Broken(Reason::new(f.message.clone())));
                }
                if let Some(f) = mine.iter().find(|f| {
                    (f.severity == Severity::Error && f.outcome.is_none()) || (f.severity == Severity::Warn && strict.contains(&f.check))
                }) {
                    return Ok(vec![SharedFinding::failing(f.message.clone())]);
                }
                if let Some(f) = mine.iter().find(|f| f.outcome == Some(Outcome::NotMeasured)) {
                    return Err(Unmeasured::NotMeasured(Reason::new(f.message.clone())));
                }
                Ok(Vec::new())
            });
            (id.to_string(), outcome)
        })
        .collect();
    // The shared verdict says 5 for findings; the conformance hosts keep their documented 1.
    match Verdict::strict(items).exit() {
        Exit::Ok => CLEAN,
        Exit::Findings => FINDINGS,
        Exit::NotMeasured => NOT_MEASURED,
        Exit::CheckBroken => CHECK_BROKEN,
        other => other.code(),
    }
}

fn tool() -> ToolVersion {
    ToolVersion::new(concat!("paddock-dev check run ", env!("CARGO_PKG_VERSION")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject() -> Subject {
        Subject::Commit {
            sha: paddock_cli::outcome::Sha::new("a1"),
            tree: paddock_cli::outcome::TreeId::new("t1"),
        }
    }

    fn ran(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

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
        assert_eq!(exit_status(&[], &none, &ran(&["C0"]), &subject()), CLEAN);
        assert_eq!(
            exit_status(std::slice::from_ref(&warn), &none, &ran(&["C0"]), &subject()),
            CLEAN,
            "a warning is report-only"
        );
        assert_eq!(
            exit_status(std::slice::from_ref(&unseen), &none, &ran(&["C0"]), &subject()),
            NOT_MEASURED
        );
        assert_eq!(
            exit_status(&[unseen.clone(), err.clone()], &none, &ran(&["C0"]), &subject()),
            FINDINGS
        );
        assert_eq!(exit_status(&[unseen, err, broken], &none, &ran(&["C0"]), &subject()), CHECK_BROKEN);
    }

    #[test]
    fn strict_makes_the_named_checks_warnings_fail() {
        let warn = Finding::warn("C36", 1, "a", 1, "m", "f");
        assert_eq!(
            exit_status(std::slice::from_ref(&warn), &strict(&["C36"]), &ran(&["C36"]), &subject()),
            FINDINGS
        );
        assert_eq!(exit_status(&[warn], &strict(&["C30"]), &ran(&["C36"]), &subject()), CLEAN);
    }

    /// Row 6 of the ADR 0089 amendment: a run in which no check ran exits 6, never 0 ("delete the check": the registry or the
    /// selection is empty), and a check that ran and found nothing is still 0.
    #[test]
    fn a_run_in_which_no_check_ran_is_not_measured_never_clean() {
        let none = strict(&[]);
        assert_eq!(exit_status(&[], &none, &[], &subject()), NOT_MEASURED);
        let warn = Finding::warn("C4", 1, "a", 1, "m", "f");
        assert_eq!(
            exit_status(&[warn], &none, &[], &subject()),
            NOT_MEASURED,
            "a warning never turns an empty run into a pass"
        );
        assert_eq!(exit_status(&[], &none, &ran(&["C0"]), &subject()), CLEAN);
    }
}
