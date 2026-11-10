//! What a failed job's log says about flaky and failing tests: nextest (the last status line of each test decides), Playwright
//! (the `N flaky` and `N failed` lists of its summary) and vitest (the failed spec files, read by `plan_since`).
//!
//! The nextest rules come from captured output (cargo-nextest 0.9.151, `tests/ci-runs/nextest-r*-*.log`): with `retries >= 1` a
//! hard failure prints only `TRY n FAIL` lines (a timeout: `TRY n TMT`; SIG* endings have no captured log) and never a bare `FAIL`; a test that passed on a retry ends with `FLAKY a/b`
//! (`flaky-result = "pass"`) or `FLKY-FL a/b` (`"fail"`).

use crate::log_text::plain;
use crate::plan_since::vitest_failed_files;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// What one log shows.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Findings {
    /// Tests the runner marked flaky (nextest `FLAKY`/`FLKY-*`, or a retry that passed; Playwright's flaky list).
    pub flaky: BTreeSet<String>,
    /// Tests that ended failed (nextest `FAIL`, `TIMEOUT`, `SIG*`, or a last `TRY n FAIL`; Playwright's failed list).
    pub failed: BTreeSet<String>,
    /// The vitest spec files that failed.
    pub vitest_failed: BTreeSet<String>,
    /// Whether any test runner's output was seen at all (a log that shows none cannot say "no flakes").
    pub saw_runner: bool,
    /// Set when a runner's own summary counts failures or flaky tests that the log does not name: the reader is broken for
    /// this log, and the job is not measured (ER-33).
    pub broken: Option<String>,
}

impl Findings {
    /// Whether the log names any flaky or failing test.
    pub fn any(&self) -> bool {
        !(self.flaky.is_empty() && self.failed.is_empty() && self.vitest_failed.is_empty())
    }
}

/// The last thing nextest said about a test.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    Passed,
    PassedOnRetry,
    Flaky,
    Failed,
}

static NEXTEST_STARTING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Starting \d+ tests? across"));
static NEXTEST_SUMMARY: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Summary \[[^\]]*\]\s+\d+ tests? run:(.*)$"));
static NEXTEST_FAILED_COUNT: LazyLock<Regex> = LazyLock::new(|| re(r"(\d+) (?:[a-z]+ )?(?:failed|timed out)"));
static NEXTEST_FLAKY_COUNT: LazyLock<Regex> = LazyLock::new(|| re(r"\((\d+) flaky\)"));
static NEXTEST_LINE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^\s*(TRY \d+ )?(PASS|FAIL|TIMEOUT|TMT|SIG[A-Z]+|ABORT|LEAK-FAIL|(?:FLAKY|FLKY)[-A-Z]*)(?: \d+/\d+)?\s+\[[^\]]*\]\s+(?:\([^)]*\)\s+)?(\S+)\s+(\S+)\s*$",
    )
});
static PW_SECTION: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{1,4}(\d+) (flaky|failed)\s*$"));
static PW_ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{2,}(?:\[[^\]]+\] › )?\S+:\d+:\d+ › \S.*$"));
static PW_PADDING: LazyLock<Regex> = LazyLock::new(|| re(r"\s+─+$"));
static PW_SUMMARY: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{1,4}\d+ (?:passed|skipped|did not run|interrupted)\b"));

/// A Playwright list being read: which list, how many names its header promised, how many were read.
struct PwList {
    flaky: bool,
    expected: usize,
    seen: usize,
}

/// Reads a job log.
pub fn read(log: &str) -> Findings {
    let mut f = Findings::default();
    let mut last: BTreeMap<String, Last> = BTreeMap::new();
    let mut block: BTreeSet<String> = BTreeSet::new();
    let mut pw: Option<PwList> = None;
    let broken = |f: &mut Findings, why: String| {
        f.broken.get_or_insert(why);
    };
    for raw in log.lines() {
        let l = plain(raw);
        if NEXTEST_STARTING.is_match(&l) {
            f.saw_runner = true;
            block.clear();
        }
        if let Some(m) = NEXTEST_LINE.captures(&l) {
            f.saw_runner = true;
            let name = format!("{} {}", &m[3], &m[4]);
            let label = &m[2];
            let state = if label.starts_with("FLAKY") || label.starts_with("FLKY") {
                Last::Flaky
            } else if label == "PASS" {
                if m.get(1).is_some() { Last::PassedOnRetry } else { Last::Passed }
            } else {
                Last::Failed
            };
            block.insert(name.clone());
            last.insert(name, state);
            continue;
        }
        if let Some(m) = NEXTEST_SUMMARY.captures(&l) {
            f.saw_runner = true;
            let count = |r: &Regex| r.captures_iter(&m[1]).filter_map(|c| c[1].parse::<usize>().ok()).sum::<usize>();
            let (failed, flaky) = (count(&NEXTEST_FAILED_COUNT), count(&NEXTEST_FLAKY_COUNT));
            let named = |want: &[Last]| block.iter().filter(|n| last.get(*n).is_some_and(|s| want.contains(s))).count();
            // With `flaky-result = "fail"` nextest counts a flaky test among the failed ones.
            if named(&[Last::Failed, Last::Flaky, Last::PassedOnRetry]) < failed {
                broken(
                    &mut f,
                    format!(
                        "a nextest summary counts {failed} failed but the log names {}",
                        named(&[Last::Failed])
                    ),
                );
            }
            if named(&[Last::Flaky, Last::PassedOnRetry]) < flaky {
                broken(
                    &mut f,
                    format!(
                        "a nextest summary counts {flaky} flaky but the log names {}",
                        named(&[Last::Flaky, Last::PassedOnRetry])
                    ),
                );
            }
            continue;
        }
        let close = |f: &mut Findings, pw: &mut Option<PwList>| {
            if let Some(list) = pw.take().filter(|l| l.seen < l.expected) {
                let kind = if list.flaky { "flaky" } else { "failed" };
                f.broken
                    .get_or_insert(format!("Playwright lists {} {kind} but the log names {}", list.expected, list.seen));
            }
        };
        if let Some(m) = PW_SECTION.captures(&l) {
            f.saw_runner = true;
            close(&mut f, &mut pw);
            pw = Some(PwList {
                flaky: &m[2] == "flaky",
                expected: m[1].parse().unwrap_or(0),
                seen: 0,
            });
            continue;
        }
        if PW_SUMMARY.is_match(&l) {
            f.saw_runner = true;
        }
        if let Some(list) = pw.as_mut() {
            if PW_ITEM.is_match(&l) {
                let name = PW_PADDING.replace(l.trim(), "").to_string();
                list.seen += 1;
                if list.flaky {
                    f.flaky.insert(name);
                } else {
                    f.failed.insert(name);
                }
            } else if !l.trim().is_empty() {
                close(&mut f, &mut pw);
            }
        }
    }
    if let Some(list) = pw.take().filter(|l| l.seen < l.expected) {
        let kind = if list.flaky { "flaky" } else { "failed" };
        broken(
            &mut f,
            format!("Playwright lists {} {kind} but the log names {}", list.expected, list.seen),
        );
    }
    for (name, state) in last {
        match state {
            Last::Failed => {
                f.failed.insert(name);
            }
            Last::Flaky | Last::PassedOnRetry => {
                f.flaky.insert(name);
            }
            Last::Passed => {}
        }
    }
    f.vitest_failed = vitest_failed_files(log).into_iter().collect();
    if !f.vitest_failed.is_empty() || log.lines().any(|l| plain(l).trim_start().starts_with("Test Files ")) {
        f.saw_runner = true;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from cargo-nextest 0.9.151 and @playwright/test 1.64.1 on scratch projects (see each file's first line).
    const R0_PASS: &str = include_str!("../../tests/ci-runs/nextest-r0-pass.log");
    const R0_FAIL: &str = include_str!("../../tests/ci-runs/nextest-r0-fail.log");
    const R1_PASS: &str = include_str!("../../tests/ci-runs/nextest-r1-pass.log");
    const R1_FAIL: &str = include_str!("../../tests/ci-runs/nextest-r1-fail.log");
    const TIMEOUT_R0: &str = include_str!("../../tests/ci-runs/nextest-timeout-r0.log");
    const TIMEOUT_R1: &str = include_str!("../../tests/ci-runs/nextest-timeout-r1.log");
    const PW_PROJECT: &str = include_str!("../../tests/ci-runs/playwright-project.log");
    const PW_PLAIN: &str = include_str!("../../tests/ci-runs/playwright-noproject.log");
    const VITEST: &str = include_str!("../../tests/since-run/vitest-frontend.log");
    const CLIPPY: &str = include_str!("../../tests/ci-runs/clippy-failed.log");

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    const ALWAYS: &str = "nxflaky t::always_fails";
    const ONCE: &str = "nxflaky t::flaky_once";

    #[test]
    fn without_retries_a_failed_test_is_failed_whatever_flaky_result_says() {
        for log in [R0_PASS, R0_FAIL] {
            let f = read(log);
            assert_eq!(f.failed, set(&[ALWAYS, ONCE]));
            assert!(f.flaky.is_empty(), "{:?}", f.flaky);
            assert!(f.broken.is_none(), "{:?}", f.broken);
        }
    }

    #[test]
    fn with_retries_the_last_line_of_a_test_decides_for_both_flaky_results() {
        // `flaky-result = "pass"` ends the flaky test with `FLAKY 2/2`, `"fail"` with `FLKY-FL 2/2`; the hard failure ends
        // with its last `TRY 2 FAIL` and never a bare FAIL.
        for log in [R1_PASS, R1_FAIL] {
            let f = read(log);
            assert_eq!(f.flaky, set(&[ONCE]), "{f:?}");
            assert_eq!(f.failed, set(&[ALWAYS]), "{f:?}");
            assert!(f.broken.is_none(), "{:?}", f.broken);
        }
    }

    #[test]
    fn a_timed_out_test_is_failed_with_and_without_retries() {
        // Captured: without retries the final line is `TIMEOUT`; with retries each attempt ends `TRY n TMT` (nextest's short
        // label for a timeout; `TRMNTG` is the progress line while it is killed). SIG* endings are not captured: they are
        // matched by the same rule as TIMEOUT but have no real log here.
        for log in [TIMEOUT_R0, TIMEOUT_R1] {
            let f = read(log);
            assert_eq!(f.failed, set(&["nxflaky slow::hangs"]), "{f:?}");
            assert!(f.flaky.is_empty() && f.broken.is_none(), "{f:?}");
        }
    }

    #[test]
    fn a_test_that_passed_on_a_retry_is_flaky_even_when_its_final_line_is_missing() {
        let cut: String = R1_PASS.lines().filter(|l| !l.contains("FLAKY")).map(|l| format!("{l}\n")).collect();
        let f = read(&cut);
        assert_eq!(f.flaky, set(&[ONCE]), "{f:?}");
        assert_eq!(f.failed, set(&[ALWAYS]));
    }

    #[test]
    fn a_summary_that_counts_failures_nobody_named_is_a_broken_check() {
        let nameless: String = R0_PASS
            .lines()
            .filter(|l| !plain(l).trim_start().starts_with("FAIL "))
            .map(|l| format!("{l}\n"))
            .collect();
        let f = read(&nameless);
        assert!(f.failed.is_empty());
        assert!(f.broken.as_deref().unwrap_or("").contains("2 failed"), "{:?}", f.broken);
    }

    #[test]
    fn playwright_lists_flaky_and_failed_tests_without_the_padding_with_or_without_a_project() {
        let f = read(PW_PROJECT);
        assert_eq!(
            f.flaky,
            set(&[
                "[chromium] › e2e/tray.spec.js:4:3 › tray › opens the popup",
                "[chromium] › e2e/tray.spec.js:7:1 › top level test"
            ])
        );
        assert_eq!(f.failed, set(&["[chromium] › e2e/tray.spec.js:5:3 › tray › always fails"]));
        assert!(f.saw_runner && f.broken.is_none());
        let f = read(PW_PLAIN);
        assert_eq!(
            f.flaky,
            set(&[
                "e2e/tray.spec.js:4:3 › tray › opens the popup",
                "e2e/tray.spec.js:7:1 › top level test"
            ])
        );
        assert_eq!(f.failed, set(&["e2e/tray.spec.js:5:3 › tray › always fails"]));
    }

    #[test]
    fn a_playwright_count_with_fewer_names_is_a_broken_check() {
        let cut: String = PW_PROJECT
            .lines()
            .filter(|l| !l.contains("top level test ─"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(read(&cut).broken.as_deref().unwrap_or("").contains("2 flaky"));
    }

    #[test]
    fn vitest_failures_are_the_failed_spec_files() {
        let f = read(VITEST);
        assert_eq!(
            f.vitest_failed,
            set(&["src/app/analysis/outliers-view.spec.ts", "src/app/watched/watched-folders.spec.ts"])
        );
        assert!(f.saw_runner);
    }

    #[test]
    fn a_log_without_any_test_runner_output_shows_no_runner() {
        let f = read(CLIPPY);
        assert!(!f.any());
        assert!(!f.saw_runner, "a clippy log is not evidence about tests");
    }
}
