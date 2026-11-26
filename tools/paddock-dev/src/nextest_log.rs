//! The one reader of cargo-nextest's output in a CI job log, shared by the rerun planner ([`crate::plan_since`]) and the flake
//! reader ([`crate::ci_runs`]).
//!
//! A log is a series of runs (`Starting N tests` ... `Summary [..] N tests run: ...`). A run is read only when it is provably
//! whole: it printed its summary, ran every test it started, was not cut short (fail-fast), and its summary's counts of failed
//! and flaky tests are met by the names the log gives. Anything else is [`NextestError::Partial`] with the reason, never a
//! quiet empty answer (ER-33). Within a run the **last status line of a test decides**: with `retries >= 1` a hard failure
//! prints only `TRY n FAIL` lines and never a bare `FAIL` (a timeout: `TRY n TMT`; `SIG*`, `ABORT` and `LEAK-FAIL` endings
//! have no captured log), and a test that passed on a retry ends `FLAKY a/b` (`flaky-result = "pass"`) or `FLKY-FL a/b`
//! (`"fail"`). The fixtures in `tests/ci-runs/nextest-*.log` were captured with cargo-nextest 0.9.149 (the CI pin,
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
// nextest prints `Cancelling due to` before the summary and `N/M tests were not run due to test failure` after it.
static NOT_RUN: LazyLock<Regex> = LazyLock::new(|| re(r"tests? (?:were |was )?not run"));
static CANCELLING: LazyLock<Regex> = LazyLock::new(|| re(r"Cancelling due to"));
// `ABORT` and `LEAK-FAIL` have no capture (nor `SIG*`): they are taken as failure labels without a real log to check them.
static LINE: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"^\s*(TRY \d+ )?(PASS|FAIL|TIMEOUT|TMT|SIG[A-Z]+|ABORT|LEAK-FAIL|(?:FLAKY|FLKY)[-A-Z]*)(?: \d+/\d+)?",
        r"\s+\[([^\]]*)\]\s+(?:\([^)]*\)\s+)?(\S+)\s+(\S+)\s*$"
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

/// How a status line ended its test: the label, and whether it was a retry (`TRY n`).
fn last_of(m: &regex::Captures) -> Last {
    let label = &m[2];
    if label.starts_with("FLAKY") || label.starts_with("FLKY") {
        Last::Flaky
    } else if label == "PASS" {
        if m.get(1).is_some() { Last::PassedOnRetry } else { Last::Passed }
    } else {
        Last::Failed
    }
}

static PANIC: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(thread '.*' (?:\(\d+\) )?panicked at .*?):?\s*$"));

/// What a failed step's nextest output shows so far: for the one-line cause of a push gate's row, never for a decision (that is
/// [`read`]'s, which refuses a run that was cut short).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The tests whose last status line is a failure, `binary test`, in the order nextest names them (sorted).
    pub failed: Vec<String>,
    /// The first panic line with its message (`thread 't' (163) panicked at src/lib.rs:7:9: no`).
    pub first_panic: Option<String>,
}

/// [`Seen`] for a step's output, also when the run was cut short (nextest's fail-fast prints `Cancelling due to test failure`, which
/// [`read`] refuses). `None` when the output has no nextest run.
pub fn seen(log: &str) -> Option<Seen> {
    let mut states: BTreeMap<TestId, Last> = BTreeMap::new();
    let (mut ran, mut first_panic, mut panic_at) = (false, None, None::<String>);
    for raw in log.lines() {
        let l = plain(raw);
        if let Some(head) = panic_at.take() {
            // The panic's message is the line after it; the backtrace note is not a message.
            let msg = l.trim();
            first_panic = Some(if msg.is_empty() || msg.starts_with("note:") {
                head
            } else {
                format!("{head}: {msg}")
            });
        }
        if STARTING.is_match(&l) {
            ran = true;
        } else if let Some(m) = LINE.captures(&l) {
            states.insert((m[4].to_string(), m[5].to_string()), last_of(&m));
        } else if first_panic.is_none()
            && let Some(m) = PANIC.captures(&l)
        {
            panic_at = Some(m[1].trim().to_string());
        }
    }
    if let Some(head) = panic_at {
        first_panic.get_or_insert(head);
    }
    ran.then(|| Seen {
        failed: states
            .into_iter()
            .filter(|(_, s)| *s == Last::Failed)
            .map(|((b, t), _)| format!("{b} {t}"))
            .collect(),
        first_panic,
    })
}

/// The binaries a log ran, by how their tests ended.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ran {
    /// Binary ids all of whose tests ended `PASS` on the first try.
    pub clean: BTreeSet<String>,
    /// Binary ids with a test that failed, ended flaky or passed on a retry (ADR 0116 §3: never a pass, ER-12).
    pub not_clean: BTreeSet<String>,
    /// How many tests each binary id ran.
    pub tests: BTreeMap<String, u64>,
}

/// The binaries of a log by how their tests ended, from each test's last status line. Binary ids come as nextest prints them:
/// `package` or `package::binary`. Whether the run was whole is [`read`]'s to say.
pub fn ran_binaries(log: &str) -> Ran {
    let mut states: BTreeMap<TestId, Last> = BTreeMap::new();
    for raw in log.lines() {
        let l = plain(raw);
        if let Some(m) = LINE.captures(&l) {
            states.insert((m[4].to_string(), m[5].to_string()), last_of(&m));
        }
    }
    let mut out = Ran::default();
    for (b, _) in states.keys() {
        *out.tests.entry(b.clone()).or_default() += 1;
    }
    for ((b, _), s) in &states {
        if *s != Last::Passed {
            out.not_clean.insert(b.clone());
        }
    }
    for (b, _) in states.keys() {
        if !out.not_clean.contains(b) {
            out.clean.insert(b.clone());
        }
    }
    out
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
        let Some(b) = block.as_mut() else {
            continue;
        };
        if NOT_RUN.is_match(&l) || (!b.summarised && CANCELLING.is_match(&l)) {
            return Err(partial(format!("a nextest run stopped before every test ran ({})", l.trim())));
        }
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
            b.states.insert((m[4].to_string(), m[5].to_string()), last_of(&m));
        }
    }
    match block {
        Some(b) => b.finish(&mut out)?,
        None if !seen => return Err(NextestError::NoRun),
        None => {}
    }
    Ok(out)
}

/// The time nextest gives a test on its last status line (`PASS [  12.345s]`, a timeout's `TMT [  60.003s]`): the try that decided.
#[derive(Debug, Clone, PartialEq)]
pub struct Timed {
    /// The test.
    pub id: TestId,
    /// Seconds of its last try.
    pub secs: f64,
    /// It ended failed (a failure, a timeout or a signal) rather than passed or flaky.
    pub failed: bool,
}

/// The duration of every test of a log's nextest runs, read only from runs [`read`] accepts as whole (a partial log is the same error,
/// never a shorter list). A test that runs in several runs of the log keeps its longest time.
pub fn durations(log: &str) -> Result<Vec<Timed>, NextestError> {
    let decided = read(log)?;
    let mut out: BTreeMap<TestId, Timed> = BTreeMap::new();
    let mut last: BTreeMap<TestId, f64> = BTreeMap::new();
    let mut in_run = false;
    for raw in log.lines() {
        let l = plain(raw);
        if STARTING.is_match(&l) {
            for (id, secs) in std::mem::take(&mut last) {
                keep(&mut out, &decided, id, secs);
            }
            in_run = true;
            continue;
        }
        if !in_run {
            continue;
        }
        if let Some(m) = LINE.captures(&l) {
            let secs = m[3].trim().trim_start_matches('>').trim().trim_end_matches('s').parse::<f64>();
            if let Ok(secs) = secs {
                last.insert((m[4].to_string(), m[5].to_string()), secs);
            }
        }
    }
    for (id, secs) in last {
        keep(&mut out, &decided, id, secs);
    }
    Ok(out.into_values().collect())
}

fn keep(out: &mut BTreeMap<TestId, Timed>, decided: &Nextest, id: TestId, secs: f64) {
    let failed = decided.failed.contains(&id);
    let e = out.entry(id.clone()).or_insert(Timed { id, secs, failed });
    if secs > e.secs {
        e.secs = secs;
    }
    e.failed |= failed;
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

    /// A push gate's failed nextest row (member-audits, fail-fast on): the names and the first panic, also when the run was cut short.
    #[test]
    fn seen_names_the_failed_tests_and_first_panic_also_of_a_run_cut_short() {
        let r0 = include_str!("../tests/ci-runs/nextest-r0-fail.log");
        let s = seen(r0).unwrap();
        assert_eq!(s.failed, ["nxflaky t::always_fails", "nxflaky t::flaky_once"]);
        assert_eq!(
            s.first_panic.as_deref(),
            Some("thread 't::always_fails' (163) panicked at src/lib.rs:7:9: no")
        );
        let cut = r0
            .lines()
            .take_while(|l| !l.contains("(3/3)"))
            .chain([
                "  Cancelling due to test failure: 1 test still running",
                "     Summary [   0.100s] 2/3 tests run: 1 passed, 1 failed",
                "warning: 1/3 tests were not run due to test failure",
            ])
            .collect::<Vec<_>>()
            .join("\n");
        assert!(read(&cut).is_err(), "the control: the decision reader refuses a cut run");
        assert_eq!(seen(&cut).unwrap().failed, ["nxflaky t::always_fails"]);
        // No nextest run: nothing is named, whatever panicked.
        assert_eq!(
            seen("error: could not compile `x`\nthread 'main' panicked at a.rs:1:1:\nboom\n"),
            None
        );
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
    fn durations_are_the_last_try_of_each_test_and_a_partial_log_gives_none() {
        let d = durations(R1_FAIL).unwrap();
        let of = |t: &str| d.iter().find(|x| x.id == id(t)).cloned().unwrap();
        assert_eq!(of("t::passes").secs, 0.063);
        assert!(!of("t::passes").failed);
        assert_eq!(of("t::flaky_once").secs, 0.057);
        assert!(of("t::always_fails").failed);
        assert_eq!(of("t::always_fails").secs, 0.060);
        let t = durations(TIMEOUT_R1).unwrap();
        assert_eq!(
            t,
            vec![Timed {
                id: id("slow::hangs"),
                secs: 1.017,
                failed: true
            }]
        );
        assert!(matches!(durations(&cut_before(R1_FAIL, "Summary")), Err(NextestError::Partial(_))));
        assert_eq!(durations("clippy output\n"), Err(NextestError::NoRun));
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
    fn a_cut_short_phrase_outside_a_nextest_run_is_not_a_partial_run() {
        // The text of another tool before the first `Starting` line says nothing about this run.
        let log = format!("some tool: 2 tests were not run\nCancelling due to something else\n{R1_FAIL}");
        assert!(read(&log).is_ok(), "{:?}", read(&log));
        // Nor is the same text in the output of a later step, after the run's summary.
        let log = format!("{R1_FAIL}Cancelling due to a later step\n");
        assert!(read(&log).is_ok(), "{:?}", read(&log));
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
        // Fail-fast: `Cancelling due to` before the summary, `tests were not run` after it.
        let before: String = R0_PASS
            .lines()
            .flat_map(|l| {
                let cancel = plain(l)
                    .trim_start()
                    .starts_with("Summary")
                    .then(|| "Cancelling due to test failure: 2 tests still running".to_string());
                cancel.into_iter().chain([l.to_string()])
            })
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(matches!(read(&before), Err(NextestError::Partial(w)) if w.contains("before every test ran")));
        let after = format!("{R0_PASS}warning: 2/3 tests were not run due to test failure\n");
        assert!(matches!(read(&after), Err(NextestError::Partial(w)) if w.contains("before every test ran")));
    }
}
