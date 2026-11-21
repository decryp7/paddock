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

/// A nextest status line: `FAIL [ 0.4s] (2/3) binary test`, `TRY 1 FAIL [ 0.4s] (───) binary test` (the count is dashes on a first try
/// that will be retried), `TMT`, `SIGABRT` ...
static STATUS: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\s*(?:TRY \d+ )?([A-Z][A-Z-]+)\s+\[\s*[0-9.]+s\]\s+(?:\([^)]*\)\s+)?(\S+)\s+(\S+)\s*$"));
/// The line proptest prints with the file it saves to (`proptest: Saving this and future failures in <path>`).
static SAVING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*proptest: Saving this and future failures in (\S+)"));
/// The sentence that introduces the seed line.
static FOLLOWING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*proptest: If this test was run on a CI system"));
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
    /// The file proptest said it saves to, as printed (absolute or relative to the crate), or a guess from the test's module path when
    /// the log did not print one.
    pub file: String,
    /// Whether `file` is the guess.
    pub guessed: bool,
}

/// What a log gives: the cases, and the failed tests whose output shows proptest's seed text but no `cc` line (a changed format).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The failing cases, each once (nextest prints a failed test's output twice or more), in a stable order.
    pub seeds: Vec<Seed>,
    /// `(crate, test)` of each failed test that printed proptest's text without a seed line.
    pub misses: Vec<(String, String)>,
}

/// The failing proptest cases of a log.
pub fn find(log: &str) -> Found {
    struct Block {
        krate: String,
        test: String,
        saving: Option<String>,
        marker: bool,
        cc: Vec<String>,
    }
    let mut seeds = BTreeSet::new();
    let mut misses = BTreeSet::new();
    let mut close = |b: Option<Block>| {
        let Some(b) = b else { return };
        if b.marker && b.cc.is_empty() {
            misses.insert((b.krate.clone(), b.test.clone()));
        }
        for cc in b.cc {
            let (file, guessed) = match &b.saving {
                Some(f) => (f.clone(), false),
                None => (default_file(&b.test), true),
            };
            seeds.insert(Seed {
                krate: b.krate.clone(),
                test: b.test.clone(),
                cc,
                file,
                guessed,
            });
        }
    };
    let mut current: Option<Block> = None;
    for raw in log.lines() {
        let line = plain(raw);
        if let Some(c) = STATUS.captures(&line) {
            close(current.take());
            let failed = !matches!(&c[1], "PASS" | "SKIP" | "LEAK" | "SLOW" | "START" | "FLAKY" | "FLKY-PS");
            current = failed.then(|| Block {
                krate: c[2].split("::").next().unwrap_or(&c[2]).to_string(),
                test: c[3].to_string(),
                saving: None,
                marker: false,
                cc: Vec::new(),
            });
        } else if let Some(b) = current.as_mut() {
            if let Some(c) = SAVING.captures(&line) {
                b.saving = Some(c[1].to_string());
                b.marker = true;
            } else if FOLLOWING.is_match(&line) {
                b.marker = true;
            } else if let Some(c) = CC.captures(&line) {
                b.cc.push(c[1].to_string());
            }
        }
    }
    close(current.take());
    Found {
        seeds: seeds.into_iter().collect(),
        misses: misses.into_iter().collect(),
    }
}

/// proptest's default (`SourceParallel`) file for a test that printed no path: the source file's module path under
/// `proptest-regressions/` (`foo::bar::tests::f` is `foo/bar.txt`); a guess, since a test's name does not say which file holds it.
fn default_file(test: &str) -> String {
    let mut parts: Vec<&str> = test.split("::").collect();
    parts.pop();
    if parts.last() == Some(&"tests") {
        parts.pop();
    }
    let module = if parts.is_empty() { "lib".to_string() } else { parts.join("/") };
    format!("proptest-regressions/{module}.txt")
}

/// The file to name in the annotation, relative to the repository root: the path proptest printed with `root` taken off; a path that
/// is relative (to the crate) or outside `root` is named under the crate (`<crate>/<path>`), as far as the log says.
fn shown_file(s: &Seed, root: Option<&str>) -> String {
    let root = root.map(|r| r.trim_end_matches('/')).filter(|r| !r.is_empty());
    if let Some(rest) = root.and_then(|r| s.file.strip_prefix(r)).and_then(|r| r.strip_prefix('/')) {
        return rest.to_string();
    }
    if s.file.starts_with('/') || s.file.get(1..3) == Some(":\\") {
        // Absolute but not under the root: the part below the crate folder, which is `tests/` or `proptest-regressions/` first.
        let at = s
            .file
            .find("/tests/")
            .map(|i| i + 1)
            .or_else(|| s.file.find("/proptest-regressions/").map(|i| i + 1));
        return format!("{}/{}", s.krate, at.map_or(s.file.as_str(), |i| &s.file[i..]));
    }
    format!("{}/{}", s.krate, s.file)
}

/// The annotation for one case: the cause and the next step (ER-27). `root`: the repository root the printed path is made relative to.
pub fn annotation(s: &Seed, root: Option<&str>) -> String {
    let guess = if s.guessed {
        " (the log printed no path: this file name is a guess from the test's module)"
    } else {
        ""
    };
    format!(
        "::error::proptest case failed in {} {}: {} | Next step: commit this line to {}{guess} in the fix, so the case is checked on every run (rule 89), then fix the code.",
        s.krate,
        s.test,
        s.cc,
        shown_file(s, root)
    )
}

/// The warning for a failed test that printed proptest's text without a seed line (ER-33: a parse miss is never silent).
pub fn miss_warning(krate: &str, test: &str) -> String {
    format!(
        "::warning::proptest seeds NOT MEASURED for {krate} {test}: its output has proptest's `Saving this and future failures` text but no `cc <hex>` line was found (the output format may have changed). Next step: read the test's output in this log and commit its `cc` line to the crate's proptest-regressions file by hand (rule 89), then fix the reader in tools/paddock-dev/src/proptest_seeds.rs."
    )
}

/// Every annotation line for a log: one error per case, one warning per miss.
pub fn report(log: &str, root: Option<&str>) -> Vec<String> {
    let f = find(log);
    f.seeds
        .iter()
        .map(|s| annotation(s, root))
        .chain(f.misses.iter().map(|(k, t)| miss_warning(k, t)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPTEST_FAIL: &str = include_str!("../tests/ci-runs/nextest-proptest-fail.log");
    const REAL: &str = include_str!("../tests/ci-runs/nextest-proptest-real.log");
    const PLAIN_FAIL: &str = include_str!("../tests/ci-runs/nextest-r0-fail.log");
    const PASS: &str = include_str!("../tests/ci-runs/nextest-r0-pass.log");

    #[test]
    fn a_failed_proptest_gives_its_seed_line_and_where_to_commit_it() {
        let seeds = find(PROPTEST_FAIL).seeds;
        assert_eq!(seeds.len(), 1, "{seeds:?}");
        let s = &seeds[0];
        assert_eq!(s.krate, "paddock-demo");
        assert_eq!(s.test, "size::tests::any_interleaving_keeps_the_size_rules");
        assert!(s.cc.starts_with("cc 7b2f0c1de45a9a1c"), "{}", s.cc);
        assert_eq!(s.file, "proptest-regressions/size.txt");
        let a = annotation(s, None);
        assert!(a.starts_with("::error::"), "{a}");
        assert!(a.contains("paddock-demo/proptest-regressions/size.txt"), "{a}");
        assert!(a.contains(&s.cc) && a.contains("size::tests::any_interleaving"), "{a}");
    }

    #[test]
    fn a_real_proptest_log_with_two_lib_failures_a_retry_and_an_integration_test_gives_each_printed_case_once() {
        let f = find(REAL);
        assert!(f.misses.is_empty(), "{:?}", f.misses);
        let got: Vec<(&str, &str, &str)> = f
            .seeds
            .iter()
            .map(|s| (s.krate.as_str(), s.test.as_str(), s.file.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("ptdemo", "it_fails", "/work/ptdemo/tests/props.proptest-regressions"),
                ("ptdemo", "tests::always_tiny", "/work/ptdemo/proptest-regressions/lib.txt"),
            ],
            "{:?}",
            f.seeds
        );
        // Real proptest printed no seed text at all for `always_small` (two tests of one binary save to the same file at once, and the
        // text shows for one of them), so there is nothing to read and no warning: that case is in the file the log's other test names.
        // The retries printed no second line, and a repeated line would be kept once.
        assert!(f.seeds.iter().all(|s| s.cc.starts_with("cc ") && !s.guessed));
    }

    #[test]
    fn the_annotation_names_the_path_proptest_printed_relative_to_the_repository_root() {
        let seeds = find(REAL).seeds;
        let integ = &seeds[0];
        let a = annotation(integ, Some("/work"));
        assert!(a.contains("commit this line to ptdemo/tests/props.proptest-regressions in"), "{a}");
        // An integration test's file is not under `proptest-regressions/` of the crate: nothing is cut.
        assert!(!a.contains("ptdemo/proptest-regressions/props"), "{a}");
        // An app crate whose folder differs from its name is right once the root is known.
        let app = Seed {
            krate: "debrief-core".into(),
            test: "t".into(),
            cc: "cc ab".into(),
            file: "/ws/apps/debrief/crates/debrief-core/tests/proptest-regressions/properties.txt".into(),
            guessed: false,
        };
        assert!(
            annotation(&app, Some("/ws/"))
                .contains("commit this line to apps/debrief/crates/debrief-core/tests/proptest-regressions/properties.txt in"),
            "{}",
            annotation(&app, Some("/ws/"))
        );
        // Without the root it falls back to the crate name and the part below the crate folder.
        assert!(annotation(&app, None).contains("to debrief-core/tests/proptest-regressions/properties.txt in"));
    }

    #[test]
    fn a_nested_module_without_a_printed_path_maps_to_its_folder_and_says_it_is_a_guess() {
        assert_eq!(default_file("foo::bar::tests::f"), "proptest-regressions/foo/bar.txt");
        assert_eq!(default_file("size::tests::f"), "proptest-regressions/size.txt");
        assert_eq!(default_file("f"), "proptest-regressions/lib.txt");
        let log: String = PROPTEST_FAIL
            .lines()
            .filter(|l| !l.contains("Saving this"))
            .collect::<Vec<_>>()
            .join("\n");
        let s = &find(&log).seeds[0];
        assert!(s.guessed && s.file == "proptest-regressions/size.txt");
        assert!(annotation(s, None).contains("a guess"));
    }

    #[test]
    fn positive_control_no_proptest_failure_no_annotation() {
        for log in [PLAIN_FAIL, PASS, ""] {
            assert!(report(log, None).is_empty());
        }
    }

    #[test]
    fn a_seed_line_under_a_passing_test_is_ignored() {
        let log = PROPTEST_FAIL.replace("FAIL\u{1b}[0m [   0.412s]", "PASS\u{1b}[0m [   0.412s]");
        assert_eq!(find(&log), Found::default());
    }

    #[test]
    fn proptest_text_without_a_seed_line_is_a_warning_never_silence() {
        let log: String = PROPTEST_FAIL.lines().filter(|l| !l.contains(" cc ")).collect::<Vec<_>>().join("\n");
        let out = report(&log, None);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(
            out[0].starts_with("::warning::proptest seeds NOT MEASURED for paddock-demo size::tests::any_interleaving"),
            "{out:?}"
        );
        // Positive control: with the line back, the error comes and no warning.
        let out = report(PROPTEST_FAIL, None);
        assert!(out.len() == 1 && out[0].starts_with("::error::"), "{out:?}");
    }

    #[test]
    fn a_repeated_output_block_gives_one_case() {
        let twice = format!("{PROPTEST_FAIL}{PROPTEST_FAIL}");
        assert_eq!(find(&twice).seeds.len(), 1);
    }
}
