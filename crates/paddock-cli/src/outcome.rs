//! One answer for "did this check pass?" (ADR 0089, ER-33): a measurement that did not happen can never pass.
//!
//! A check, probe or test ends in exactly one [`Outcome`]. There is no conversion to `bool`, `Result` or a default: the only
//! ways out are [`Verdict::exit`] and [`Verdict::json`], so a missing tool, a crash or an empty run cannot be read as "clean".
//! [`Verdict::of`] folds a run's outcomes into one exit code:
//!
//! 1. any [`Outcome::CheckBroken`] gives [`Exit::CheckBroken`] (7);
//! 2. else any *failing* finding gives [`Exit::Findings`] (5); the JSON still lists every not-measured item;
//! 3. else any [`Outcome::NotMeasured`], or no outcome at all, gives [`Exit::NotMeasured`] (6), whatever report-only findings
//!    the run has: a report-only finding never masks a missing measurement;
//! 4. else 0: clean, not applicable, or report-only findings.
//!
//! Waivers and baselines work on [`Finding`]s before an `Outcome` is built, so they never see (and cannot cancel) a
//! `NotMeasured` or `CheckBroken`. Pure: nothing here reads the environment or the clock.

use crate::Exit;
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Measured, nothing found.
    Clean,
    /// A scoped run (`--files`, an app scope) reached nothing this check reads. Never produced by a whole-tree run.
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

    /// The name in the JSON.
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Clean => "clean",
            Outcome::NotApplicable(_) => "not-applicable",
            Outcome::Findings(_) => "findings",
            Outcome::NotMeasured(_) => "not-measured",
            Outcome::CheckBroken(_) => "check-broken",
        }
    }

    fn json(&self) -> Value {
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

/// A run's outcomes folded into one exit code. Built only by [`Verdict::of`].
#[must_use = "a verdict is turned into the process exit with Verdict::exit"]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    outcomes: Vec<Outcome>,
}

impl Verdict {
    /// Folds `outcomes`. No outcome at all is a run that measured nothing: [`Exit::NotMeasured`].
    pub fn of(outcomes: impl IntoIterator<Item = Outcome>) -> Verdict {
        Verdict {
            outcomes: outcomes.into_iter().collect(),
        }
    }

    /// The outcomes, in the order given.
    pub fn outcomes(&self) -> &[Outcome] {
        &self.outcomes
    }

    /// The exit code, by the precedence in the module doc.
    pub fn exit(&self) -> Exit {
        if self
            .outcomes
            .iter()
            .any(|o| matches!(o, Outcome::CheckBroken(_)))
        {
            Exit::CheckBroken
        } else if self
            .outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Findings(f) if f.failing()))
        {
            Exit::Findings
        } else if self.outcomes.is_empty()
            || self
                .outcomes
                .iter()
                .any(|o| matches!(o, Outcome::NotMeasured(_)))
        {
            Exit::NotMeasured
        } else {
            Exit::Ok
        }
    }

    /// The report: the exit, its name, and every outcome with its reason or findings (nothing is dropped, whatever the exit).
    pub fn json(&self) -> Value {
        let exit = self.exit();
        let mut v = json!({
            "exit": exit.code(),
            "status": exit.name(),
            "outcomes": self.outcomes.iter().map(Outcome::json).collect::<Vec<_>>(),
        });
        if self.outcomes.is_empty() {
            v["note"] = json!("no outcome was recorded: nothing was measured");
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason() -> Reason {
        Reason::new("why")
    }

    /// Every kind an outcome can take, with both severities and a mixed list.
    fn kinds() -> Vec<Outcome> {
        vec![
            Outcome::Clean,
            Outcome::NotApplicable(reason()),
            Outcome::from_findings(vec![Finding::failing("f")]),
            Outcome::from_findings(vec![Finding::report_only("r")]),
            Outcome::from_findings(vec![Finding::report_only("r"), Finding::failing("f")]),
            Outcome::NotMeasured(reason()),
            Outcome::CheckBroken(reason()),
        ]
    }

    /// The rules of the module doc, written a second time from the ADR (D3, D5), as the oracle for the property test.
    fn oracle(run: &[Outcome]) -> i32 {
        let broken = run.iter().any(|o| o.name() == "check-broken");
        let failing = run.iter().any(|o| match o {
            Outcome::Findings(f) => f.as_slice().iter().any(|x| x.severity == Severity::Failing),
            _ => false,
        });
        let not_measured = run.iter().any(|o| o.name() == "not-measured") || run.is_empty();
        if broken {
            7
        } else if failing {
            5
        } else if not_measured {
            6
        } else {
            0
        }
    }

    /// Calls `visit` with every sequence of up to `max` outcomes drawn from `kinds()`.
    fn every_run(max: usize, visit: &mut dyn FnMut(&[Outcome])) {
        fn go(prefix: &mut Vec<Outcome>, left: usize, visit: &mut dyn FnMut(&[Outcome])) {
            visit(prefix);
            if left == 0 {
                return;
            }
            for k in kinds() {
                prefix.push(k);
                go(prefix, left - 1, visit);
                prefix.pop();
            }
        }
        go(&mut Vec::new(), max, visit);
    }

    #[test]
    fn property_every_combination_folds_by_the_precedence() {
        let mut runs = 0;
        every_run(4, &mut |run| {
            runs += 1;
            let v = Verdict::of(run.to_vec());
            assert_eq!(v.exit().code(), oracle(run), "{run:?}");
            // The JSON never disagrees with the exit and never drops an outcome.
            let j = v.json();
            assert_eq!(j["exit"], v.exit().code());
            assert_eq!(j["outcomes"].as_array().unwrap().len(), run.len());
            // The two guarantees of ADR 0089 section 6, stated directly on every combination:
            let has = |name: &str| run.iter().any(|o| o.name() == name);
            if has("check-broken") {
                assert_eq!(v.exit(), Exit::CheckBroken);
            }
            if has("not-measured") {
                assert_ne!(
                    v.exit(),
                    Exit::Ok,
                    "a missing measurement must not pass: {run:?}"
                );
            }
            // Order never matters.
            let mut rev = run.to_vec();
            rev.reverse();
            assert_eq!(Verdict::of(rev).exit(), v.exit());
        });
        assert_eq!(runs, 1 + 7 + 49 + 343 + 2401);
    }

    #[test]
    fn positive_controls() {
        let report_only = Outcome::from_findings(vec![Finding::report_only("r")]);
        // D5: report-only findings plus not measured is 6, never 0.
        let v = Verdict::of([report_only.clone(), Outcome::NotMeasured(reason())]);
        assert_eq!(v.exit(), Exit::NotMeasured);
        // ... and plus check broken is 7.
        assert_eq!(
            Verdict::of([report_only.clone(), Outcome::CheckBroken(reason())]).exit(),
            Exit::CheckBroken
        );
        // Failing plus not measured is 5, with both listed.
        let v = Verdict::of([
            Outcome::from_findings(vec![Finding::failing("f")]),
            Outcome::NotMeasured(Reason::new("no ffmpeg")),
        ]);
        assert_eq!(v.exit(), Exit::Findings);
        let text = v.json().to_string();
        assert!(
            text.contains("no ffmpeg") && text.contains("\"failing\":true"),
            "{text}"
        );
        // Broken beats failing.
        let both = Verdict::of([
            Outcome::from_findings(vec![Finding::failing("f")]),
            Outcome::CheckBroken(reason()),
        ]);
        assert_eq!(both.exit(), Exit::CheckBroken);
        // The probe sees the clean cases: only these exit 0.
        assert_eq!(Verdict::of([Outcome::Clean]).exit(), Exit::Ok);
        assert_eq!(
            Verdict::of([Outcome::NotApplicable(reason())]).exit(),
            Exit::Ok
        );
        assert_eq!(Verdict::of([report_only]).exit(), Exit::Ok);
        // Nothing recorded is nothing measured.
        let none = Verdict::of(Vec::new());
        assert_eq!(none.exit(), Exit::NotMeasured);
        assert!(none.json()["note"].is_string());
        assert!(none.outcomes().is_empty());
    }

    #[test]
    fn findings_and_reasons_are_never_empty() {
        assert_eq!(Outcome::from_findings(Vec::new()), Outcome::Clean);
        assert!(Findings::new(Vec::new()).is_none());
        let blank = Reason::new("  ");
        assert!(blank.as_str().contains("no reason was given"));
        assert_eq!(Reason::new("tool missing").as_str(), "tool missing");
        let v = Verdict::of([Outcome::NotMeasured(Reason::new(""))]);
        assert!(v.json().to_string().contains("no reason was given"));
    }

    #[test]
    fn json_names_every_status() {
        let names: Vec<&str> = kinds().iter().map(|o| o.name()).collect();
        for n in [
            "clean",
            "not-applicable",
            "findings",
            "not-measured",
            "check-broken",
        ] {
            assert!(names.contains(&n), "{n}");
        }
        let j = Verdict::of(kinds()).json();
        assert_eq!(j["status"], "check-broken");
        assert_eq!(j["exit"], 7);
    }
}
