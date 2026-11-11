//! What a failed job's log says about flaky and failing tests: nextest (the shared reader [`crate::nextest_log`]), Playwright
//! (the `N flaky`, `N failed` and `N interrupted` lists of its summary) and vitest (the failed spec files, read by `plan_since`).
//!
//! A runner that started and did not finish in the log (no nextest summary, no Playwright final count, no vitest `Test Files`
//! line), or whose own counts the named tests do not meet, makes the log `broken`: the job is not measured, never a clean
//! non-test failure (ER-33).

use crate::log_text::plain;
use crate::nextest_log::{self, NextestError};
use crate::plan_since::vitest_failed_files;
use regex::Regex;
use std::collections::BTreeSet;
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
    /// Tests that ended failed (nextest `FAIL`, `TIMEOUT`, `TMT`, `SIG*`, or a last `TRY n FAIL`; Playwright's failed list).
    pub failed: BTreeSet<String>,
    /// The vitest spec files that failed.
    pub vitest_failed: BTreeSet<String>,
    /// Set when a runner started and did not finish in the log, or counts failures or flaky tests that the log does not name:
    /// the job is not measured (ER-33).
    pub broken: Option<String>,
}

impl Findings {
    /// Whether the log names any flaky or failing test.
    pub fn any(&self) -> bool {
        !(self.flaky.is_empty() && self.failed.is_empty() && self.vitest_failed.is_empty())
    }
}

static PW_RUNNING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Running \d+ tests? using"));
static PW_SECTION: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{1,4}(\d+) (flaky|failed|interrupted)\s*$"));
static PW_ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{2,}(?:\[[^\]]+\] › )?\S+:\d+:\d+ › \S.*$"));
static PW_PADDING: LazyLock<Regex> = LazyLock::new(|| re(r"\s+─+$"));
static PW_FINAL: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{1,4}\d+ (?:passed|skipped|did not run)\b"));
static VITEST_STARTED: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\s*RUN\s+v\d|^\s+[✓❯↓×]\s+(?:\S+\s+)?\S+\.(?:spec|test)\.[cm]?[jt]sx?\s+\(\d+ tests?"));
static VITEST_FILES: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Test Files\s+(?:(\d+) failed)?"));

/// A Playwright list being read: which list, how many names its header promised, how many were read.
struct PwList {
    flaky: bool,
    expected: usize,
    seen: usize,
}

/// Reads a job log.
pub fn read(log: &str) -> Findings {
    let mut f = Findings::default();
    match nextest_log::read(log) {
        Ok(n) => {
            f.failed.extend(n.failed.into_iter().map(|(b, t)| format!("{b} {t}")));
            f.flaky.extend(n.flaky.into_iter().map(|(b, t)| format!("{b} {t}")));
        }
        Err(NextestError::NoRun) => {}
        Err(NextestError::Partial(why)) => {
            f.broken.get_or_insert(why);
        }
    }
    read_playwright(log, &mut f);
    read_vitest(log, &mut f);
    f
}

fn read_playwright(log: &str, f: &mut Findings) {
    let (mut running, mut finished) = (false, false);
    let mut pw: Option<PwList> = None;
    let close = |f: &mut Findings, pw: &mut Option<PwList>| {
        if let Some(list) = pw.take().filter(|l| l.seen < l.expected) {
            let kind = if list.flaky { "flaky" } else { "failed" };
            f.broken
                .get_or_insert(format!("Playwright lists {} {kind} but the log names {}", list.expected, list.seen));
        }
    };
    for raw in log.lines() {
        let l = plain(raw);
        if PW_RUNNING.is_match(&l) {
            running = true;
        }
        if let Some(m) = PW_SECTION.captures(&l) {
            finished = true;
            close(f, &mut pw);
            if &m[2] == "interrupted" {
                f.broken.get_or_insert(format!("Playwright was interrupted ({} tests)", &m[1]));
                continue;
            }
            pw = Some(PwList {
                flaky: &m[2] == "flaky",
                expected: m[1].parse().unwrap_or(0),
                seen: 0,
            });
            continue;
        }
        if PW_FINAL.is_match(&l) {
            finished = true;
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
                close(f, &mut pw);
            }
        }
    }
    close(f, &mut pw);
    if running && !finished {
        f.broken
            .get_or_insert("Playwright started its tests and the log has no final count (it was cut short)".into());
    }
}

fn read_vitest(log: &str, f: &mut Findings) {
    let (mut started, mut files_line) = (false, None::<usize>);
    for raw in log.lines() {
        let l = plain(raw);
        if let Some(m) = VITEST_FILES.captures(&l) {
            files_line = Some(m.get(1).and_then(|n| n.as_str().parse().ok()).unwrap_or(0));
        } else if VITEST_STARTED.is_match(&l) {
            started = true;
        }
    }
    f.vitest_failed = vitest_failed_files(log).into_iter().collect();
    match files_line {
        None if started => {
            f.broken
                .get_or_insert("vitest started and the log has no `Test Files` line (it was cut short)".into());
        }
        Some(n) if n > f.vitest_failed.len() => {
            f.broken.get_or_insert(format!(
                "vitest counts {n} failed files but the log names {}",
                f.vitest_failed.len()
            ));
        }
        _ => {}
    }
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
        assert!(f.broken.as_deref().unwrap_or("").contains("counts 2 failures"), "{:?}", f.broken);
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
        assert!(f.broken.is_none());
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
        assert!(f.broken.is_none(), "{:?}", f.broken);
    }

    #[test]
    fn a_log_without_any_test_runner_output_shows_no_runner() {
        let f = read(CLIPPY);
        assert!(!f.any());
        assert!(
            f.broken.is_none(),
            "a clippy log shows no runner, so there is nothing to be broken: {:?}",
            f.broken
        );
    }

    /// The log up to (not including) the first line that starts with `needle`.
    fn cut_before(log: &str, needle: &str) -> String {
        log.lines()
            .take_while(|l| !plain(l).trim_start().starts_with(needle))
            .map(|l| format!("{l}\n"))
            .collect()
    }

    #[test]
    fn a_truncated_nextest_log_is_broken_not_a_clean_failure() {
        for (log, cut) in [
            (R1_FAIL, "Summary"),
            (R0_FAIL, "Summary"),
            (TIMEOUT_R1, "Summary"),
            (R0_PASS, "FAIL "),
        ] {
            let f = read(&cut_before(log, cut));
            assert!(f.broken.as_deref().is_some_and(|w| w.contains("nextest")), "{f:?}");
        }
    }

    #[test]
    fn a_truncated_playwright_log_is_broken() {
        for log in [PW_PROJECT, PW_PLAIN] {
            let f = read(&cut_before(log, "1 failed"));
            assert!(
                f.broken
                    .as_deref()
                    .is_some_and(|w| w.contains("Playwright") && w.contains("cut short")),
                "{f:?}"
            );
        }
    }

    #[test]
    fn a_playwright_run_that_was_interrupted_is_broken() {
        // Not captured from a real interrupted run: the `1 failed` header of the real capture is turned into `1 interrupted`.
        let f = read(&PW_PROJECT.replace("  1 failed", "  1 interrupted"));
        assert!(f.broken.as_deref().is_some_and(|w| w.contains("interrupted")), "{f:?}");
    }

    #[test]
    fn a_truncated_vitest_log_is_broken_and_so_is_a_count_above_the_named_files() {
        let f = read(&cut_before(VITEST, "Test Files"));
        assert!(f.broken.as_deref().is_some_and(|w| w.contains("Test Files")), "{f:?}");
        // The summary says two failed files; cut the section that names them.
        let unnamed: String = VITEST
            .lines()
            .filter(|l| !plain(l).trim_start().starts_with("FAIL "))
            .map(|l| format!("{l}\n"))
            .collect();
        let f = read(&unnamed);
        assert!(f.broken.as_deref().is_some_and(|w| w.contains("counts 2 failed files")), "{f:?}");
    }
}
