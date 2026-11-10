//! What a failed job's log says about flaky and failing tests: nextest (`FLAKY`, `TRY n FAIL`, `FAIL`), Playwright (the
//! `N flaky` and `N failed` lists of its summary) and vitest (the failed spec files, read by `plan_since`).

use crate::plan_since::{plain, vitest_failed_files};
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
    /// Tests the runner marked flaky (nextest `FLAKY`, or a `TRY n FAIL` that did not end failed; Playwright's flaky list).
    pub flaky: BTreeSet<String>,
    /// Tests that failed (nextest `FAIL`, `TIMEOUT`, `SIG*`; Playwright's failed list).
    pub failed: BTreeSet<String>,
    /// The vitest spec files that failed.
    pub vitest_failed: BTreeSet<String>,
    /// Whether any test runner's output was seen at all (a log that shows none cannot say "no flakes").
    pub saw_runner: bool,
}

impl Findings {
    /// Whether the log names any flaky or failing test.
    pub fn any(&self) -> bool {
        !(self.flaky.is_empty() && self.failed.is_empty() && self.vitest_failed.is_empty())
    }
}

static NEXTEST_STARTING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Starting \d+ tests? across"));
static NEXTEST_LINE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^\s*(TRY \d+ )?(FAIL|TIMEOUT|SIG[A-Z]+|ABORT|LEAK-FAIL|(?:FLAKY|FLKY)[-A-Z]*)(?: \d+/\d+)?\s+\[[^\]]*\]\s+(?:\(\s*\d+/\d+\)\s+)?(\S+)\s+(\S+)\s*$",
    )
});
static PW_SECTION: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{1,4}(\d+) (flaky|failed)\s*$"));
static PW_ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{2,}\[[^\]]+\] › \S.*$"));
static PW_SUMMARY: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{1,4}\d+ (?:passed|skipped|did not run|interrupted)\b"));

/// Reads a job log.
pub fn read(log: &str) -> Findings {
    let mut f = Findings::default();
    let mut retried: BTreeSet<String> = BTreeSet::new();
    let mut pw: Option<bool> = None; // Some(true): inside the flaky list, Some(false): inside the failed list
    for raw in log.lines() {
        let l = plain(raw);
        if NEXTEST_STARTING.is_match(&l) {
            f.saw_runner = true;
        }
        if let Some(m) = NEXTEST_LINE.captures(&l) {
            f.saw_runner = true;
            let name = format!("{} {}", &m[3], &m[4]);
            let label = &m[2];
            if label.starts_with("FLAKY") || label.starts_with("FLKY") {
                f.flaky.insert(name);
            } else if m.get(1).is_some() {
                retried.insert(name);
            } else {
                f.failed.insert(name);
            }
            continue;
        }
        if let Some(m) = PW_SECTION.captures(&l) {
            f.saw_runner = true;
            pw = Some(&m[2] == "flaky");
            continue;
        }
        if PW_SUMMARY.is_match(&l) {
            f.saw_runner = true;
        }
        if let Some(flaky) = pw {
            if PW_ITEM.is_match(&l) {
                let name = l.trim().to_string();
                if flaky {
                    f.flaky.insert(name);
                } else {
                    f.failed.insert(name);
                }
            } else if !l.trim().is_empty() {
                pw = None;
            }
        }
    }
    // A test that failed an attempt and did not end failed passed on a retry: flaky.
    f.flaky.extend(retried.into_iter().filter(|n| !f.failed.contains(n)));
    f.vitest_failed = vitest_failed_files(log).into_iter().collect();
    if !f.vitest_failed.is_empty() || log.lines().any(|l| plain(l).trim_start().starts_with("Test Files ")) {
        f.saw_runner = true;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEXTEST_FLAKY: &str = include_str!("../../tests/ci-runs/nextest-flaky.log");
    const NEXTEST_HARD: &str = include_str!("../../tests/ci-runs/nextest-hardfail.log");
    const PLAYWRIGHT: &str = include_str!("../../tests/ci-runs/playwright-flaky.log");
    const VITEST: &str = include_str!("../../tests/ci-runs/vitest-failed.log");
    const CLIPPY: &str = include_str!("../../tests/ci-runs/clippy-failed.log");

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nextest_marks_flaky_tests_by_the_flaky_line_and_by_a_failed_attempt_that_passed_on_retry() {
        let f = read(NEXTEST_FLAKY);
        assert_eq!(
            f.flaky,
            set(&[
                "dyno-app link::tuning::the_agent_follows_the_service",
                "dyno-app power::tests::a_pending_action_survives_a_restart",
            ])
        );
        assert!(f.failed.is_empty(), "{:?}", f.failed);
        assert!(f.saw_runner);
    }

    #[test]
    fn a_test_that_failed_every_attempt_is_failed_and_not_flaky() {
        let f = read(NEXTEST_HARD);
        assert_eq!(f.failed, set(&["dyno-app a::b::c"]));
        assert!(f.flaky.is_empty(), "{:?}", f.flaky);
    }

    #[test]
    fn playwright_lists_its_flaky_and_failed_tests_in_the_summary_only() {
        let f = read(PLAYWRIGHT);
        assert_eq!(f.flaky, set(&["[chromium] › e2e/tray.spec.ts:21:5 › tray › opens the popup"]));
        assert_eq!(f.failed, set(&["[chromium] › e2e/disks.spec.ts:8:3 › disk map › scans the folder"]));
        assert!(f.saw_runner);
    }

    #[test]
    fn vitest_failures_are_the_failed_spec_files() {
        let f = read(VITEST);
        assert_eq!(f.vitest_failed, set(&["src/app/watched/watched-folders.spec.ts"]));
        assert!(f.saw_runner);
    }

    #[test]
    fn a_log_without_any_test_runner_output_shows_no_runner() {
        let f = read(CLIPPY);
        assert!(!f.any());
        assert!(!f.saw_runner, "a clippy log is not evidence about tests");
    }
}
