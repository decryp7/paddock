//! The one reader of cargo-nextest's output in a CI job log, shared by the rerun planner ([`crate::plan_since`]) and the flake
//! reader ([`crate::ci_runs`]).
//!
//! A log is a series of runs (`Starting N tests` ... `Summary [..] N tests run: ...`). A run is read only when it is provably
//! whole: it printed its summary, ran every test it started, was not cut short (fail-fast), and its summary's counts of failed
//! and flaky tests are met by the names the log gives. Anything else is [`NextestError::Partial`] with the reason, never a
//! quiet empty answer (ER-33). Within a run the **last status line of a test decides** (captured cargo-nextest 0.9.151, see
//! `tests/ci-runs/nextest-*.log`): with `retries >= 1` a hard failure prints only `TRY n FAIL` lines and never a bare `FAIL`
//! (a timeout: `TRY n TMT`; `SIG*`, `ABORT` and `LEAK-FAIL` endings have no captured log), a test that passed on a retry ends `FLAKY a/b` (`flaky-result =
//! "pass"`) or `FLKY-FL a/b` (`"fail"`). The fixtures were captured with cargo-nextest 0.9.149 (the CI pin,
//! `.gitea/actions/host-tools/action.yml`); 0.9.151 prints the same status lines.

use crate::log_text::plain;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// A test as nextest names it: the binary id and the test name.
pub type TestId = (String, String);

/// The tests of a log that ended failed or flaky.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Nextest {
    /// Ended failed (`FAIL`, `TIMEOUT`, `TMT`, `SIG*`, or a last `TRY n FAIL`).
    pub failed: BTreeSet<TestId>,
    /// Ended flaky (`FLAKY`, `FLKY-*`, or a retry that passed).
    pub flaky: BTreeSet<TestId>,
}

/// Why a log gives no answer.
#[derive(Debug, PartialEq, Eq)]
pub enum NextestError {
    /// No nextest run is in the log.
    NoRun,
    /// A run is in the log but cannot be read whole; the reason.
    Partial(String),
}

impl std::fmt::Display for NextestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NextestError::NoRun => f.write_str("no nextest run was found in the log"),
            NextestError::Partial(why) => f.write_str(why),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    Passed,
    PassedOnRetry,
    Flaky,
    Failed,
}

static STARTING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Starting (\d+) tests? across"));
static SUMMARY: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Summary \[[^\]]*\]\s+(\d+) tests? run:(.*)$"));
static FAILED_COUNT: LazyLock<Regex> = LazyLock::new(|| re(r"(\d+) (?:[a-z]+ )?(?:failed|timed out)"));
static FLAKY_COUNT: LazyLock<Regex> = LazyLock::new(|| re(r"\((\d+) flaky\)"));
static NOT_RUN: LazyLock<Regex> = LazyLock::new(|| re(r"tests? (?:were |was )?not run|Cancelling due to"));
// `ABORT` and `LEAK-FAIL` have no capture (nor `SIG*`): they are taken as failure labels without a real log to check them.
static LINE: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"^\s*(TRY \d+ )?(PASS|FAIL|TIMEOUT|TMT|SIG[A-Z]+|ABORT|LEAK-FAIL|(?:FLAKY|FLKY)[-A-Z]*)(?: \d+/\d+)?",
        r"\s+\[[^\]]*\]\s+(?:\([^)]*\)\s+)?(\S+)\s+(\S+)\s*$"
    ))
});

#[derive(Default)]
struct Block {
    started: u64,
    summarised: bool,
    states: BTreeMap<TestId, Last>,
}

impl Block {
    fn named(&self, want: &[Last]) -> usize {
        self.states.values().filter(|s| want.contains(s)).count()
    }

    fn finish(self, out: &mut Nextest) -> Result<(), NextestError> {
        if !self.summarised {
            return Err(NextestError::Partial(
                "a nextest run ended without its summary (it crashed or was stopped)".into(),
            ));
        }
        for (id, state) in self.states {
            match state {
                Last::Failed => out.failed.insert(id),
                Last::Flaky | Last::PassedOnRetry => out.flaky.insert(id),
                Last::Passed => false,
            };
        }
        Ok(())
    }
}

/// Reads every nextest run of a job log.
pub fn read(log: &str) -> Result<Nextest, NextestError> {
    let partial = |s: String| NextestError::Partial(s);
    let mut out = Nextest::default();
    let mut block: Option<Block> = None;
    let mut seen = false;
    for raw in log.lines() {
        let l = plain(raw);
        if let Some(m) = STARTING.captures(&l) {
            if let Some(b) = block.take() {
                b.finish(&mut out)?;
            }
            seen = true;
            block = Some(Block {
                started: m[1].parse().unwrap_or(u64::MAX),
                ..Block::default()
            });
            continue;
        }
        if NOT_RUN.is_match(&l) {
            return Err(partial(format!("a nextest run stopped before every test ran ({})", l.trim())));
        }
        let Some(b) = block.as_mut() else {
            continue;
        };
        if let Some(m) = SUMMARY.captures(&l) {
            let ran: u64 = m[1].parse().unwrap_or(0);
            b.summarised = true;
            if ran != b.started {
                return Err(partial(format!("a nextest run started {} tests but ran {ran}", b.started)));
            }
            let count = |r: &Regex| r.captures_iter(&m[2]).filter_map(|c| c[1].parse::<usize>().ok()).sum::<usize>();
            let (failed, flaky) = (count(&FAILED_COUNT), count(&FLAKY_COUNT));
            // With `flaky-result = "fail"` nextest counts a flaky test among the failed ones.
            let problems = b.named(&[Last::Failed, Last::Flaky, Last::PassedOnRetry]);
            if problems < failed {
                return Err(partial(format!(
                    "a nextest summary counts {failed} failures but the log names {problems}"
                )));
            }
            let flakes = b.named(&[Last::Flaky, Last::PassedOnRetry]);
            if flakes < flaky {
                return Err(partial(format!(
                    "a nextest summary counts {flaky} flaky but the log names {flakes}"
                )));
            }
            continue;
        }
        if let Some(m) = LINE.captures(&l) {
            let label = &m[2];
            let state = if label.starts_with("FLAKY") || label.starts_with("FLKY") {
                Last::Flaky
            } else if label == "PASS" {
                if m.get(1).is_some() { Last::PassedOnRetry } else { Last::Passed }
            } else {
                Last::Failed
            };
            b.states.insert((m[3].to_string(), m[4].to_string()), state);
        }
    }
    match block {
        Some(b) => b.finish(&mut out)?,
        None if !seen => return Err(NextestError::NoRun),
        None => {}
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R0_PASS: &str = include_str!("../tests/ci-runs/nextest-r0-pass.log");
    const R1_PASS: &str = include_str!("../tests/ci-runs/nextest-r1-pass.log");
    const R1_FAIL: &str = include_str!("../tests/ci-runs/nextest-r1-fail.log");
    const TIMEOUT_R1: &str = include_str!("../tests/ci-runs/nextest-timeout-r1.log");

    fn id(t: &str) -> TestId {
        ("nxflaky".into(), t.into())
    }

    fn cut_before(log: &str, needle: &str) -> String {
        log.lines()
            .take_while(|l| !plain(l).trim_start().starts_with(needle))
            .map(|l| format!("{l}\n"))
            .collect()
    }

    #[test]
    fn the_last_line_of_a_test_decides_in_every_captured_configuration() {
        for log in [R1_PASS, R1_FAIL] {
            let n = read(log).unwrap();
            assert_eq!(n.failed, [id("t::always_fails")].into());
            assert_eq!(n.flaky, [id("t::flaky_once")].into());
        }
        assert_eq!(read(R0_PASS).unwrap().failed.len(), 2);
        assert_eq!(read(TIMEOUT_R1).unwrap().failed, [id("slow::hangs")].into());
    }

    #[test]
    fn a_run_without_its_summary_is_partial_not_empty() {
        let cut = cut_before(R1_FAIL, "Summary");
        assert!(
            matches!(read(&cut), Err(NextestError::Partial(w)) if w.contains("without its summary")),
            "{:?}",
            read(&cut)
        );
        let cut = cut_before(R0_PASS, "FAIL");
        assert!(matches!(read(&cut), Err(NextestError::Partial(_))));
    }

    #[test]
    fn a_log_with_no_run_says_so() {
        assert_eq!(read("clippy output\n"), Err(NextestError::NoRun));
    }

    #[test]
    fn counted_failures_and_flaky_tests_the_log_does_not_name_are_partial() {
        let nameless: String = R0_PASS
            .lines()
            .filter(|l| !plain(l).trim_start().starts_with("FAIL "))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(matches!(read(&nameless), Err(NextestError::Partial(w)) if w.contains("counts 2 failures")));
        // `(1 flaky)` in the summary of the `flaky-result = "pass"` capture, with every line that names the test removed.
        let nameless: String = R1_PASS
            .lines()
            .filter(|l| !plain(l).contains("flaky_once"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(
            matches!(read(&nameless), Err(NextestError::Partial(w)) if w.contains("counts 1 flaky")),
            "{:?}",
            read(&nameless)
        );
    }

    #[test]
    fn a_run_cut_short_or_with_a_different_count_is_partial() {
        let started: String = R0_PASS
            .lines()
            .map(|l| {
                if plain(l).contains("Starting 3 tests") {
                    plain(l).replace("Starting 3", "Starting 4")
                } else {
                    l.to_string()
                }
            })
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(matches!(read(&started), Err(NextestError::Partial(w)) if w.contains("started 4 tests but ran 3")));
        let failfast = format!("{R0_PASS}Cancelling due to test failure: 2 tests still running\n");
        assert!(matches!(read(&failfast), Err(NextestError::Partial(w)) if w.contains("before every test ran")));
    }
}
