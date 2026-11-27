//! What a finished nextest run proves per target (ADR 0116 §3, ER-12, ER-33). A target is a pass only when the run was whole
//! ([`crate::nextest_log::read`] gives an answer, never `Partial`), and every test of its package ended `PASS` on the first try: a test
//! that failed, ended `FLAKY`/`FLKY-*` or passed on a retry makes the package's run not a pass, and nothing is written for it. A package
//! with no test in the log is not a pass either (not measured, never a pass).

use crate::nextest_log::{self, NextestError, Ran};
use std::collections::BTreeSet;

/// What the run said about the packages it was asked to run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Judged {
    /// Packages whose tests all passed on the first try.
    pub passed: BTreeSet<String>,
    /// Why a package is not in `passed`, by package (a failure, a flake, no test seen), and why the run itself could not be read.
    pub not_passed: Vec<(String, String)>,
    /// Packages that ran and had a test fail, end flaky or pass on a retry (a subset of `not_passed`: not "never seen" or "log cut short").
    pub failed: BTreeSet<String>,
    /// Why the whole run could not be read (no nextest run in the output, or a log cut short): nothing was judged, every asked package is
    /// not measured. `None` when the log was read whole.
    pub unreadable: Option<String>,
    /// How many tests each passed package ran.
    pub tests: std::collections::BTreeMap<String, u64>,
}

/// The package of a nextest binary id: `package` for its lib tests, `package::binary` for an integration test or a bin.
pub fn package_of(binary_id: &str) -> &str {
    binary_id.split("::").next().unwrap_or(binary_id)
}

/// Judges `log` (the run's output) for the `asked` packages.
pub fn judge(log: &str, asked: &[String]) -> Judged {
    let mut out = Judged::default();
    let why = |out: &mut Judged, why: String| {
        for p in asked {
            out.not_passed.push((p.clone(), why.clone()));
        }
    };
    match nextest_log::read(log) {
        Err(NextestError::NoRun) => {
            out.unreadable = Some("no nextest run in the output".into());
            why(&mut out, "no nextest run in the output".into());
            return out;
        }
        Err(NextestError::Partial(w)) => {
            out.unreadable = Some(w.clone());
            why(&mut out, w);
            return out;
        }
        Ok(_) => {}
    }
    let Ran { clean, not_clean, tests } = nextest_log::ran_binaries(log);
    let clean: BTreeSet<&str> = clean.iter().map(|b| package_of(b)).collect();
    let bad: BTreeSet<&str> = not_clean.iter().map(|b| package_of(b)).collect();
    for p in asked {
        if bad.contains(p.as_str()) {
            out.failed.insert(p.clone());
            out.not_passed
                .push((p.clone(), "a test failed, was flaky or passed on a retry".into()));
        } else if clean.contains(p.as_str()) {
            out.passed.insert(p.clone());
            let n = tests.iter().filter(|(b, _)| package_of(b) == p).map(|(_, n)| n).sum();
            out.tests.insert(p.clone(), n);
        } else {
            out.not_passed
                .push((p.clone(), "no test of the package was seen in the output".into()));
        }
    }
    out
}
