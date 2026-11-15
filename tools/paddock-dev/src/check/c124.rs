//! C124: a gate's result is built through the shared outcome, never by hand (ADR 0089 amendment 2026-11-13, WP-T6; ER-21: the class
//! "a gate gives its verdict from an indirect signal" recurred at rows 2 to 7 of the amendment's table).
//!
//! Flagged, one site at a time, in the gate and check tools' Rust (`tools/paddock-dev/src/`, `crates/paddock-bench/src/`) outside
//! test code (a `tests/` folder, `tests.rs`, `*_test.rs`, and everything after a file's first `#[cfg(test)]`):
//!
//! - the `NotMeasured`, `Clean`, `Pass`, `Passed`, `Success`, `Current` or `Green` variants of a local enum that is a result type, one
//!   with a `NotMeasured` variant or two or more result words (those, and `Red`, `Stale`, `Fail`, `Failed`, `Findings`, `Dirty`,
//!   `Outdated`, `Broken`, `Error`, `Mismatch`, `Missing`, `Skipped`): a second result type; one such word among others names a phase or a command (`PhaseId::Clean`);
//! - a JSON object literal with a `"status"`, `"verdict"` or `"outcome"` key: a result a consumer would read without evidence;
//! - `fn fails(&self) -> bool` or `fn passed(&self) -> bool`: the bool conversion the shared type forbids;
//! - a call of `Evidence::ci_run(` or `Evidence::adopt(` outside the modules that own them;
//! - `measure(.., || Ok(Vec::new()))` (also `vec![]`, `Vec::default()`, `Default::default()`): a measurement that always finds
//!   nothing reads as a pass.
//!
//! In JavaScript, outside `conformance/src/outcome.mjs`: an object literal `status: 'clean'` (a pass made by hand).
//!
//! The shared reader owns the error paths: an allow file that cannot be read makes the check broken (one finding, nothing else),
//! and a file the check cannot read is not measured (an error finding at that file).
//!
//! Pre-existing sites are listed one by one in `conformance/c124-allow.json`: a path, a `site` (a piece of the flagged line) and a
//! reason (shrink only), read and counted by the shared [`super::allow_list`] (also C121's): an entry that matches no site is stale
//! (a warning) and a site beyond an entry's `count` is reported at its own line. **Report-only (warnings)
//! until its first green run on the real tree is recorded here (ER-41).** Recorded: not yet.
//!
//! The JavaScript check `conformance/src/checks/C124-gate-outcomes.mjs` is the original; the `c124` tests of `tests/it/check.rs`
//! compare the two finding for finding.

use super::allow_list::{self, Admit, Entry, Spec};
use super::finding::Finding;
use super::jsre::rx;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

/// The allow list both hosts read.
pub const ALLOW: &str = "conformance/c124-allow.json";

static RUST: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:tools/paddock-dev/src|crates/paddock-bench/src)/.*\.rs$"));
static OWNER: LazyLock<Regex> = LazyLock::new(|| rx(r"^crates/paddock-cli/"));
static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:tests?|__tests__)/|(?:^|/)tests\.rs$|_test\.rs$"));
static VARIANT: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(NotMeasured|Clean|Pass|Passed|Success|Current|Green)\b"));
/// The words of a result: an enum is a second result type when it has a `NotMeasured` variant or two or more of these (`Earlier {
/// Clean, Failed }`), never for one word among others (`PhaseId { Resolve, Clean, Fetch }`: a phase, not a result).
static RESULT_WORD: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"^\s*(NotMeasured|Clean|Pass|Passed|Success|Current|Green|Red|Stale|Fail|Failed|Findings|Dirty|Outdated|Broken|Error|Mismatch|Missing|Skipped)\b",
    )
});
static ENUM: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+\w+"));
static JSON_KEY: LazyLock<Regex> = LazyLock::new(|| rx(r#""(?:status|verdict|outcome)"\s*:"#));
static BOOL_FN: LazyLock<Regex> = LazyLock::new(|| rx(r"\bfn\s+(?:fails|passed)\s*\(\s*&self\s*\)\s*->\s*bool\b"));
static CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:Evidence|Proof)::(?:ci_run|adopt)\s*\("));
static EMPTY_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"\|\|\s*(?:\{\s*)?Ok\(\s*(?:Vec::new\(\)|vec!\s*\[\s*\]|Vec::default\(\)|Default::default\(\))\s*\)\s*;?\s*\}?\s*,?\s*$")
});
static MEASURE_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\bmeasure\s*\("));
static JSON_USE: LazyLock<Regex> = LazyLock::new(|| rx(r"json!|serde_json"));
static CFG_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*#\[cfg\(test\)\]"));
static JS_FILE: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:conformance/(?:src|bin)|scripts|tools)/.*\.mjs$"));
static JS_PASS: LazyLock<Regex> = LazyLock::new(|| rx(r#"\bstatus\s*:\s*['"]clean['"]"#));
static JS_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"\.test\.mjs$"));

const CALL_OWNERS: [&str; 2] = [
    "tools/paddock-dev/src/ci_runs/verdict.rs",
    "tools/paddock-dev/src/pre_push/known.rs",
];
static SPEC: Spec = Spec {
    id: "C124",
    rule: 21,
    file: ALLOW,
    unseen: "a gate result made by hand",
    restore: "Restore conformance/c124-allow.json: {\"allow\": [{\"path\": \"...\", \"site\": \"...\", \"reason\": \"...\"}]}.",
};
const FIX: &str = "Build the result with `paddock_cli::measure` (or `Evidence::adopt`) and print it with `Outcome::json()`; read another gate's result with `Outcome::from_wire`, never its exit code (ADR 0089 amendment 2026-11-13).";

/// One flagged site: its line, the text the allow list's `site` is looked for in, and what is wrong with it.
pub type Site = (usize, String, String);

/// Every site in the Rust text `text` of `path`.
pub fn flagged_rust(path: &str, text: &str) -> Vec<Site> {
    if OWNER.is_match(path) || TEST_PATH.is_match(path) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let end = lines.iter().position(|l| CFG_TEST.is_match(l)).unwrap_or(lines.len());
    let json_used = JSON_USE.is_match(text);
    let mut depth: i64 = -1; // brace depth inside an enum, -1 outside
    let mut header = String::new(); // the enum's own line, so the `NotMeasured,` of two enums are two sites
    // The enum's flaggable variants, kept until it closes and shows whether it is a result type, and its result words.
    let mut candidates: Vec<Site> = Vec::new();
    let mut words = std::collections::BTreeSet::new();
    for (i, line) in lines[..end].iter().enumerate() {
        if line.trim().starts_with("//") {
            continue;
        }
        if depth < 0 && ENUM.is_match(line) && line.contains('{') {
            depth = 0;
            header = line.trim().to_string();
            candidates.clear();
            words.clear();
        }
        if depth >= 0 {
            if depth == 1 {
                if let Some(c) = VARIANT.captures(line) {
                    candidates.push((
                        i + 1,
                        format!("{header} {}", line.trim()),
                        format!("an enum variant `{}` (a second result type)", &c[1]),
                    ));
                }
                if let Some(w) = RESULT_WORD.captures(line) {
                    words.insert(w[1].to_string());
                }
            }
            for ch in line.chars() {
                depth += match ch {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                };
            }
            if depth <= 0 {
                depth = -1;
                if words.contains("NotMeasured") || words.len() >= 2 {
                    out.append(&mut candidates);
                }
            }
        }
        if json_used && JSON_KEY.is_match(line) {
            out.push((
                i + 1,
                line.to_string(),
                "a JSON result object with a status, verdict or outcome key".to_string(),
            ));
        }
        if BOOL_FN.is_match(line) {
            out.push((i + 1, line.to_string(), "a `fails`/`passed` bool on a result type".to_string()));
        }
        if CALL.is_match(line) && !CALL_OWNERS.contains(&path) {
            out.push((
                i + 1,
                line.to_string(),
                "a call of `Evidence::ci_run`/`adopt` outside its owner".to_string(),
            ));
        }
    }
    // Comment lines are blanked (line numbers stay), then each `measure(` call is cut at its balanced closing parenthesis.
    let code = lines[..end]
        .iter()
        .map(|l| if l.trim().starts_with("//") { "" } else { *l })
        .collect::<Vec<_>>()
        .join("\n");
    for m in MEASURE_CALL.find_iter(&code) {
        let (mut depth, mut k) = (1, m.end());
        for (off, ch) in code[m.end()..].char_indices() {
            depth += match ch {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            k = m.end() + off;
            if depth == 0 {
                break;
            }
        }
        if depth != 0 || !EMPTY_TAIL.is_match(&code[m.end()..k]) {
            continue;
        }
        let line = code[..m.start()].matches('\n').count() + 1;
        out.push((
            line,
            lines[line - 1].to_string(),
            "a `measure` whose closure always returns an empty list (an empty measurement reads as a pass)".to_string(),
        ));
    }
    // An enum's variants are known only when it closes: back in line order (stable, as the JavaScript sorts).
    out.sort_by_key(|s| s.0);
    out
}

/// Every hand-made pass in the JavaScript text of `path`.
pub fn flagged_js(path: &str, text: &str) -> Vec<Site> {
    if path == "conformance/src/outcome.mjs"
        || path == "conformance/src/checks/C124-gate-outcomes.mjs"
        || TEST_PATH.is_match(path)
        || JS_TEST.is_match(path)
    {
        return Vec::new();
    }
    text.split('\n')
        .enumerate()
        .filter(|(_, l)| !l.trim().starts_with("//") && JS_PASS.is_match(l))
        .map(|(i, l)| {
            (
                i + 1,
                l.to_string(),
                "an object literal `status: 'clean'` (a pass made by hand)".to_string(),
            )
        })
        .collect()
}

/// Reads the allow list: its entries, or why it cannot be read (the shared parser, [`super::allow_list`]).
pub fn parse_allow(text: &str) -> Result<Vec<Entry>, String> {
    allow_list::parse(text, ALLOW)
}

/// The check.
pub struct C124;

impl Check for C124 {
    fn id(&self) -> &'static str {
        "C124"
    }

    fn rule(&self) -> u32 {
        21
    }

    fn title(&self) -> &'static str {
        "A gate's result is built through the shared outcome, never by hand"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn allow_list(&self) -> Option<&'static str> {
        Some(ALLOW)
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ALLOW,
                    "{\"allow\":[{\"path\":\"tools/paddock-dev/src/ok.rs\",\"site\":\"NotMeasured\",\"reason\":\"the seed, shrink only\"}]}",
                ),
                ("tools/paddock-dev/src/x/mod.rs", "pub enum V {\n    NotMeasured,\n}\n"),
                ("tools/paddock-dev/src/ok.rs", "pub enum V {\n    NotMeasured,\n}\n"),
                ("tools/paddock-dev/src/y.rs", "// json!({\"status\": 1})\nfn f() {}\n"),
            ],
            expect: vec![("tools/paddock-dev/src/x/mod.rs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut allow = match allow_list::read(ctx, &SPEC, allow_list::entries_only(ALLOW)) {
            Ok((allow, ())) => allow,
            Err(broken) => return *broken,
        };
        let mut findings = Vec::new();
        for f in ctx.files().iter() {
            let rust = RUST.is_match(f);
            if !rust && !JS_FILE.is_match(f) {
                continue;
            }
            let text = match allow.read(ctx, f) {
                Ok(text) => text,
                Err(unread) => {
                    findings.push(*unread);
                    continue;
                }
            };
            let sites = if rust { flagged_rust(f, &text) } else { flagged_js(f, &text) };
            for (line, at, what) in sites {
                let finding = match allow.admit(f, |site| at.contains(site)) {
                    Admit::Allowed => continue,
                    Admit::Beyond => Finding::warn(
                        "C124",
                        21,
                        f,
                        line,
                        format!("builds a gate result by hand beyond the count of its allow entry: {what}"),
                        format!("Migrate it, or raise the entry's `count` in {ALLOW} if this site is meant too."),
                    ),
                    Admit::Unlisted => Finding::warn("C124", 21, f, line, format!("builds a gate result by hand: {what}"), FIX),
                };
                findings.push(finding);
            }
        }
        allow.output(ctx, findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "tools/paddock-dev/src/x/mod.rs";

    fn lines(text: &str) -> Vec<usize> {
        flagged_rust(DEV, text).into_iter().map(|s| s.0).collect()
    }

    /// One shape per result word, as the JavaScript tests list them: the enum, then the lines flagged.
    #[test]
    fn each_result_word_makes_an_enum_with_a_flaggable_variant_a_result_type() {
        for (text, want) in [
            ("enum Tree {\n    Clean,\n    Dirty,\n}\n", vec![2]),
            ("enum Lock {\n    Current,\n    Outdated,\n}\n", vec![2]),
            ("enum Probe {\n    Pass,\n    Broken(String),\n}\n", vec![2]),
            ("enum Job {\n    Green,\n    Error(String),\n}\n", vec![2]),
            ("enum Golden {\n    Clean,\n    Mismatch { diff: String },\n}\n", vec![2]),
            ("enum Pin {\n    Current,\n    Missing,\n}\n", vec![2]),
            ("enum Step {\n    Pass,\n    Skipped,\n}\n", vec![2]),
            ("enum JobState {\n    Passed,\n    Failed(String),\n}\n", vec![2]),
            ("enum RunState {\n    Running,\n    Success,\n    Failed(String),\n}\n", vec![3]),
            (
                "enum Last {\n    Passed,\n    PassedOnRetry,\n    Flaky,\n    Failed,\n}\n",
                vec![2],
            ),
        ] {
            assert_eq!(lines(text), want, "{text}");
        }
    }

    #[test]
    fn an_enum_with_one_result_word_or_no_flaggable_variant_is_not_flagged() {
        for text in [
            "enum PhaseId {\n    Resolve,\n    Clean,\n    Fetch,\n}\n",
            "enum Cmd {\n    Missing,\n    Error,\n}\n",
            "enum Disk {\n    Success,\n    Run,\n}\n",
        ] {
            assert!(lines(text).is_empty(), "{text}");
        }
    }
}
