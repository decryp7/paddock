//! One answer for "did this check pass?" (ADR 0089, ER-33): a measurement that did not happen can never pass.
//!
//! A check, probe or test ends in exactly one [`Outcome`]. There is no conversion to `bool`, `Result` or a default: the only
//! ways out are [`Verdict::exit`], [`Verdict::json`] and [`Verdict::table`], so a missing tool, a crash or an empty run
//! cannot be read as "clean". [`Verdict::of`] folds a run's outcomes into one exit code:
//!
//! 1. any [`Outcome::CheckBroken`] gives [`crate::Exit::CheckBroken`] (7); it is never optional;
//! 2. else any *failing* finding, or an expired optional entry, gives [`crate::Exit::Findings`] (5); the JSON still lists every
//!    not-measured item;
//! 3. else any [`Outcome::NotMeasured`] that no live [`Optional`] entry covers, an [`Outcome::NotApplicable`] in a whole-tree
//!    run (only a scoped run can have nothing to do), or no outcome at all, gives [`crate::Exit::NotMeasured`] (6), whatever
//!    report-only findings the run has: a report-only finding never masks a missing measurement;
//! 4. else 0: clean, not applicable in a scoped run, report-only findings, and not-measured items all declared optional.
//!
//! Waivers and baselines work on [`Finding`]s before an `Outcome` is built, so they never see (and cannot cancel) a
//! `NotMeasured` or `CheckBroken`. Pure: nothing here reads the environment or the clock; the run's kind, leg, OS and date are
//! passed in as a [`Run`].

mod date;
mod optional;
mod verdict;

pub use date::Date;
pub use optional::{Entry, Optional, Run, RunKind, Scope};
pub use verdict::Verdict;

use serde_json::{Value, json};

/// Why a check did not measure, or is broken, or has nothing to do. Never empty: a missing reason is itself named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason(String);

impl Reason {
    /// A reason; blank text becomes a sentence that says the check gave none (a bug in the check, not a silent pass).
    pub fn new(text: impl Into<String>) -> Reason {
        let text = text.into();
        if text.trim().is_empty() {
            Reason(
                "no reason was given (a bug in the check: say what could not be looked at)".into(),
            )
        } else {
            Reason(text)
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether a finding fails the run or is only reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Fails the run (exit 5).
    Failing,
    /// Listed, never fails the run, never hides a missing measurement.
    ReportOnly,
}

/// One thing a measurement found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// What was found and where.
    pub message: String,
    /// Whether it fails the run.
    pub severity: Severity,
}

impl Finding {
    /// A finding that fails the run.
    pub fn failing(message: impl Into<String>) -> Finding {
        Finding {
            message: message.into(),
            severity: Severity::Failing,
        }
    }

    /// A finding that is only reported.
    pub fn report_only(message: impl Into<String>) -> Finding {
        Finding {
            message: message.into(),
            severity: Severity::ReportOnly,
        }
    }
}

/// A non-empty list of findings: "measured, found something" cannot hold zero findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Findings(Vec<Finding>);

impl Findings {
    /// The findings, or `None` when there are none (then the outcome is [`Outcome::Clean`]).
    pub fn new(list: Vec<Finding>) -> Option<Findings> {
        if list.is_empty() {
            None
        } else {
            Some(Findings(list))
        }
    }

    /// The findings.
    pub fn as_slice(&self) -> &[Finding] {
        &self.0
    }

    fn failing(&self) -> bool {
        self.0.iter().any(|f| f.severity == Severity::Failing)
    }
}

/// How one measurement ended. Deliberately not convertible to `bool` or `Result`, and without a `Default`.
#[must_use = "an outcome must reach a Verdict; dropping it turns a missing measurement into a pass"]
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Measured, nothing found.
    Clean,
    /// A scoped run (`--files`, an app scope) reached nothing this check reads. In a whole-tree run it counts as not measured.
    NotApplicable(Reason),
    /// Measured; each finding failing or report-only.
    Findings(Findings),
    /// Did not look: tool missing, precondition false, incomplete, zero probes in its scope.
    NotMeasured(Reason),
    /// Looked, but its positive control failed, or it crashed.
    CheckBroken(Reason),
}

impl Outcome {
    /// Measured, with these findings: [`Outcome::Clean`] when the list is empty.
    pub fn from_findings(list: Vec<Finding>) -> Outcome {
        match Findings::new(list) {
            Some(f) => Outcome::Findings(f),
            None => Outcome::Clean,
        }
    }

    /// The name in the JSON and the table.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Outcome::Clean => "clean",
            Outcome::NotApplicable(_) => "not-applicable",
            Outcome::Findings(_) => "findings",
            Outcome::NotMeasured(_) => "not-measured",
            Outcome::CheckBroken(_) => "check-broken",
        }
    }

    pub(crate) fn failing(&self) -> bool {
        matches!(self, Outcome::Findings(f) if f.failing())
    }

    /// The reason or the findings, as one line for the table.
    pub(crate) fn detail(&self) -> String {
        match self {
            Outcome::Clean => String::new(),
            Outcome::NotApplicable(r) | Outcome::NotMeasured(r) | Outcome::CheckBroken(r) => {
                r.as_str().to_string()
            }
            Outcome::Findings(f) => f
                .as_slice()
                .iter()
                .map(|x| {
                    format!(
                        "{}{}",
                        if x.severity == Severity::Failing {
                            ""
                        } else {
                            "(report only) "
                        },
                        x.message
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
        }
    }

    pub(crate) fn json(&self) -> Value {
        match self {
            Outcome::Clean => json!({"status": self.name()}),
            Outcome::NotApplicable(r) | Outcome::NotMeasured(r) | Outcome::CheckBroken(r) => {
                json!({"status": self.name(), "reason": r.as_str()})
            }
            Outcome::Findings(f) => {
                let list: Vec<Value> = f
                    .as_slice()
                    .iter()
                    .map(|x| json!({"message": x.message, "failing": x.severity == Severity::Failing}))
                    .collect();
                json!({"status": self.name(), "findings": list})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_and_reasons_are_never_empty() {
        assert!(matches!(Outcome::from_findings(Vec::new()), Outcome::Clean));
        assert!(Findings::new(Vec::new()).is_none());
        let blank = Reason::new("  ");
        assert!(blank.as_str().contains("no reason was given"));
        assert_eq!(Reason::new("tool missing").as_str(), "tool missing");
    }

    #[test]
    fn detail_and_json_say_what_each_outcome_holds() {
        let mixed = Outcome::from_findings(vec![Finding::report_only("r"), Finding::failing("f")]);
        assert_eq!(mixed.detail(), "(report only) r; f");
        let j = mixed.json();
        assert_eq!(j["findings"][0]["message"], "r");
        assert_eq!(j["findings"][0]["failing"], false);
        assert_eq!(j["findings"][1]["message"], "f");
        assert_eq!(j["findings"][1]["failing"], true);
        assert_eq!(Outcome::Clean.detail(), "");
        let nm = Outcome::NotMeasured(Reason::new("no ffmpeg"));
        assert_eq!(nm.detail(), "no ffmpeg");
        assert_eq!(nm.json()["reason"], "no ffmpeg");
        assert_eq!(Outcome::Clean.json()["status"], "clean");
        assert!(!Outcome::Clean.failing() && mixed.failing());
        assert!(!Outcome::from_findings(vec![Finding::report_only("r")]).failing());
    }
}
