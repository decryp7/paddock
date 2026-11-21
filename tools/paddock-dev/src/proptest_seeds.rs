//! The failing proptest cases in a nextest job log, as CI annotations.
//!
//! proptest records a failing case as a `cc <hex>` line in the crate's `proptest-regressions/<module>.txt`, but a CI checkout is
//! thrown away, so the case is lost and the next run (random seed) may pass. The policy (rule 89): the fix commits the line.
//! This reader finds each `cc` line proptest printed under a failed test of a nextest log and says where to commit it.
//! Read-only; the log reader is the one in [`crate::log_text`]; the failed-test headers are nextest's status lines.

use crate::log_text::plain;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// A nextest status line: `FAIL [ 0.4s] (2/3) binary test`, `TRY 1 FAIL [ 0.4s] binary test`, `TMT`, `SIGABRT` ...
static STATUS: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\s*(?:TRY \d+ )?([A-Z][A-Z-]+)\s+\[\s*[0-9.]+s\]\s+(?:\(\s*\d+/\s*\d+\)\s+)?(\S+)\s+(\S+)\s*$"));
/// The line proptest prints with the file it saves to (`proptest: Saving this and future failures in <path>`).
static SAVING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*proptest: Saving this and future failures in (\S+)"));
/// The seed line itself.
static CC: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(cc [0-9a-f]{16,}(?: #.*)?)$"));

/// One failing proptest case.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seed {
    /// The crate (the part of nextest's binary id before `::`).
    pub krate: String,
    /// The test's name.
    pub test: String,
    /// The line to commit, as printed.
    pub cc: String,
    /// The regressions file, relative to the crate (`proptest-regressions/<module>.txt`).
    pub file: String,
}

/// The failing proptest cases of a log, each once (nextest prints a failed test's output twice), in a stable order.
pub fn find(log: &str) -> Vec<Seed> {
    let mut found = BTreeSet::new();
    let mut current: Option<(String, String)> = None;
    let mut saving: Option<String> = None;
    for raw in log.lines() {
        let line = plain(raw);
        if let Some(c) = STATUS.captures(&line) {
            let failed = !matches!(&c[1], "PASS" | "SKIP" | "LEAK" | "SLOW" | "START" | "FLAKY" | "FLKY-PS");
            current = failed.then(|| (c[2].split("::").next().unwrap_or(&c[2]).to_string(), c[3].to_string()));
            saving = None;
        } else if let Some(c) = SAVING.captures(&line) {
            saving = Some(file_of(&c[1]));
        } else if let (Some((krate, test)), Some(c)) = (&current, CC.captures(&line)) {
            let file = saving.take().unwrap_or_else(|| default_file(test));
            found.insert(Seed {
                krate: krate.clone(),
                test: test.clone(),
                cc: c[1].to_string(),
                file,
            });
        }
    }
    found.into_iter().collect()
}

/// `proptest-regressions/<module>.txt` out of the path proptest printed (relative or absolute).
fn file_of(path: &str) -> String {
    match path.rfind("proptest-regressions/") {
        Some(at) => path[at..].to_string(),
        None => format!("proptest-regressions/{}", path.rsplit('/').next().unwrap_or(path)),
    }
}

/// proptest's default (`SourceParallel`): the file is named after the test's module, `<first module>.txt` for a test in a
/// `src` file; used only when the log did not print the path.
fn default_file(test: &str) -> String {
    format!("proptest-regressions/{}.txt", test.split("::").next().unwrap_or(test))
}

/// The annotation for one case: the cause and the next step (ER-27).
pub fn annotation(s: &Seed) -> String {
    format!(
        "::error::proptest case failed in {} {}: {} | Next step: commit this line to {}/{} in the fix, so the case is checked on every run (rule 89), then fix the code.",
        s.krate, s.test, s.cc, s.krate, s.file
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPTEST_FAIL: &str = include_str!("../tests/ci-runs/nextest-proptest-fail.log");
    const PLAIN_FAIL: &str = include_str!("../tests/ci-runs/nextest-r0-fail.log");
    const PASS: &str = include_str!("../tests/ci-runs/nextest-r0-pass.log");

    #[test]
    fn a_failed_proptest_gives_its_seed_line_and_where_to_commit_it() {
        let seeds = find(PROPTEST_FAIL);
        assert_eq!(seeds.len(), 1, "{seeds:?}");
        let s = &seeds[0];
        assert_eq!(s.krate, "paddock-demo");
        assert_eq!(s.test, "size::tests::any_interleaving_keeps_the_size_rules");
        assert!(s.cc.starts_with("cc 7b2f0c1de45a9a1c"), "{}", s.cc);
        assert_eq!(s.file, "proptest-regressions/size.txt");
        let a = annotation(s);
        assert!(a.starts_with("::error::"), "{a}");
        assert!(a.contains("paddock-demo/proptest-regressions/size.txt"), "{a}");
        assert!(a.contains(&s.cc) && a.contains("size::tests::any_interleaving"), "{a}");
    }

    #[test]
    fn positive_control_no_proptest_failure_no_annotation() {
        assert!(find(PLAIN_FAIL).is_empty(), "a plain failed test has no seed");
        assert!(find(PASS).is_empty());
        assert!(find("").is_empty());
    }

    #[test]
    fn a_seed_line_under_a_passing_test_is_ignored() {
        let log = PROPTEST_FAIL.replace("FAIL\u{1b}[0m [   0.412s]", "PASS\u{1b}[0m [   0.412s]");
        assert!(find(&log).is_empty());
    }

    #[test]
    fn a_log_without_the_saving_line_names_the_default_file() {
        let log: String = PROPTEST_FAIL
            .lines()
            .filter(|l| !l.contains("Saving this"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(find(&log)[0].file, "proptest-regressions/size.txt");
    }

    #[test]
    fn an_absolute_saving_path_is_cut_to_the_crate_relative_file() {
        assert_eq!(
            file_of("/work/crates/paddock-demo/proptest-regressions/size.txt"),
            "proptest-regressions/size.txt"
        );
        assert_eq!(file_of("tests/proptest-regressions/props.txt"), "proptest-regressions/props.txt");
    }

    #[test]
    fn a_repeated_output_block_gives_one_case() {
        let twice = format!("{PROPTEST_FAIL}{PROPTEST_FAIL}");
        assert_eq!(find(&twice).len(), 1);
    }
}
