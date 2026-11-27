//! What a finished nextest run proves per target (ADR 0116 §3, ER-12, ER-33). A target is a pass only when the run was whole
//! ([`crate::nextest_log::read`] gives an answer, never `Partial`), and every test of its package ended `PASS` on the first try: a test
//! that failed, ended `FLAKY`/`FLKY-*` or passed on a retry makes the package's run not a pass, and nothing is written for it. A package
//! with no test in the log is not a pass either (not measured, never a pass).

use crate::nextest_log::{self, NextestError, Ran};
use std::collections::{BTreeMap, BTreeSet};

/// What a run proved about one package: the one outcome type of the test cache (user decision 106). Every reader matches it
/// exhaustively, so "not seen" can never be read as "failed" (a target outside the run's filter or without tests proves nothing about
/// its record: train x, e60b56d27 and c3a38a7ae).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    /// Every test of the package ended `PASS` on the first try, in a run read whole.
    Passed {
        /// How many tests the package ran.
        tests: u64,
    },
    /// The package ran and a test failed, ended flaky or passed on a retry.
    Failed {
        /// What happened, for a finding or a not-stored line.
        why: String,
    },
    /// The output has no verdict for the package: no test of it was seen (filtered out, no tests) or the run could not be read whole.
    NotSeen {
        /// Why nothing was seen.
        why: String,
    },
}

/// What the run said about the packages it was asked to run: one [`TestOutcome`] per asked package.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Judged {
    outcomes: BTreeMap<String, TestOutcome>,
}

impl Judged {
    /// The outcome of `package`; a package the run was not asked about is not seen.
    pub fn outcome(&self, package: &str) -> TestOutcome {
        self.outcomes.get(package).cloned().unwrap_or_else(|| TestOutcome::NotSeen {
            why: "the package was not part of the run".into(),
        })
    }
}

/// The package of a nextest binary id: `package` for its lib tests, `package::binary` for an integration test or a bin.
pub fn package_of(binary_id: &str) -> &str {
    binary_id.split("::").next().unwrap_or(binary_id)
}

/// Judges `log` (the run's output) for the `asked` packages.
pub fn judge(log: &str, asked: &[String]) -> Judged {
    let mut outcomes = BTreeMap::new();
    let not_seen_all = |why: String| Judged {
        outcomes: asked
            .iter()
            .map(|p| (p.clone(), TestOutcome::NotSeen { why: why.clone() }))
            .collect(),
    };
    match nextest_log::read(log) {
        Err(NextestError::NoRun) => return not_seen_all("no nextest run in the output".into()),
        Err(NextestError::Partial(w)) => return not_seen_all(w),
        Ok(_) => {}
    }
    let Ran { clean, not_clean, tests } = nextest_log::ran_binaries(log);
    let clean: BTreeSet<&str> = clean.iter().map(|b| package_of(b)).collect();
    let bad: BTreeSet<&str> = not_clean.iter().map(|b| package_of(b)).collect();
    for p in asked {
        let outcome = if bad.contains(p.as_str()) {
            TestOutcome::Failed {
                why: "a test failed, was flaky or passed on a retry".into(),
            }
        } else if clean.contains(p.as_str()) {
            let n = tests.iter().filter(|(b, _)| package_of(b) == p).map(|(_, n)| n).sum();
            TestOutcome::Passed { tests: n }
        } else {
            TestOutcome::NotSeen {
                why: "no test of the package was seen in the output".into(),
            }
        };
        outcomes.insert(p.clone(), outcome);
    }
    Judged { outcomes }
}
