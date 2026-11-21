//! C122: tests compare paths as paths (ADR 0099, rule 40). The same place is spelled differently on Windows (`\` and `/`, `\\?\`,
//! drive case, Git bash's `/c/x`), so test code that builds expected path text by hand fails there and only there. In Rust test
//! code (files under `tests/`, `tests.rs`, `*_tests.rs`, and the `#[cfg(test)] mod` of any other file), with comments and string
//! insides masked:
//!
//! - rule A: a `format!` that joins a `.display()` argument with a literal `/` or `\` next to a placeholder;
//! - rule B: a hand separator fold, `.replace('\\', "/")` (also with strings);
//! - rule C (a warning, a heuristic): `assert_eq!`/`assert_ne!` with an operand that is path text;
//! - rule D (a warning, new 2026-11-21): a path rendered with `.display()` or `.to_string_lossy()` and handed to a program as an
//!   argument (`.arg`, `.args`; not `.env`: a Rust child reads its own variables), formatted into a golden (`assert_golden`, `check_golden`) or into an `expected`/`want`/`golden`
//!   binding; the tools' spelling is `paddock_env::winpath::for_tools`;
//! - rule E (a warning, new 2026-11-21): a string comparison (`==`, `!=`, `contains`, `starts_with`) with a path rendered to text on
//!   one side and no fixed literal on the other; compare paths with `winpath::same`/`starts_with`. A statement that names `file_name()`,
//!   `extension()` or `file_stem()`, or checks for the `\\?\` prefix, is a name or a spelling check, not a place, and is left alone.
//!
//! A site that is not a path comparison carries `// conformance: path-text — <why>` on the line or the line above. Report-only
//! (ER-41): A and B are warnings until the first green run on the real tree; flip [`SEVERITY_AB`] then.
//!
//! The JavaScript check (`conformance/src/checks/C122-*.mjs`) was the original; it was retired with pair P3 (ADR 0022 amendment 2026-11-19) and the Rust host is the only one. The answers it gave are recorded in `tests/fixtures/check/recorded` and `units` and ``tests/it/check_c122.rs`` holds the check to them.
//! Positions are UTF-16 offsets, as in the JavaScript.

use super::finding::{Finding, Severity};
use super::jsre::{Positions, UnitIndex, has_reason, rx};
use super::mask::mask_rust;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// What rules A and B report as while the check is report-only.
const SEVERITY_AB: Severity = Severity::Warn;

static WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*path-text\s*([—–-]+)"));
static BARE_WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*path-text"));
static SKIP_PATH: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"(?:^|/)(?:node_modules|dist|target)/|(?:^|/)tests/fixtures/|^apps/[^/]+/vendor/|^conformance/(?:test|fixtures)/")
});
static TEST_FILE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)tests/|(?:^|/)tests\.rs$|_tests\.rs$"));
static CFG_TEST_MOD: LazyLock<Regex> =
    LazyLock::new(|| rx(r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{"));
static FORMAT_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\bformat!\s*\("));
static ASSERT_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\bassert_(?:eq|ne)!\s*\("));
static FORMAT_LITERAL: LazyLock<Regex> = LazyLock::new(|| rx(r"\}(?:/|\\\\)|(?:/|\\\\)\{"));
static FOLD: LazyLock<Regex> = LazyLock::new(|| rx(r##"\.replace\(\s*(?:'\\\\'|"\\\\"|r#*"\\"#*)\s*,\s*(?:"/"|'/')\s*\)"##));
static PREFILTER: LazyLock<Regex> = LazyLock::new(|| rx(r"format!|\.replace\(|assert_(?:eq|ne)!|\.display\(\)|\.to_string_lossy\(\)"));
static ARG_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(?:arg|args)\s*\("));
static GOLDEN_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:assert_golden|check_golden)(?:_with)?\s*\("));
static EXPECTED_BINDING: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:expected|want|golden)\w*\s*(?::[^=;]+)?=\s*$"));
static EQ_OP: LazyLock<Regex> = LazyLock::new(|| rx(r"[=!]="));
static COMPARE_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(?:contains|starts_with)\s*\("));
static NAME_ONLY: LazyLock<Regex> = LazyLock::new(|| rx(r"file_name\(\)|\.extension\(\)|file_stem\(\)"));
const PATH_TEXT: [&str; 3] = [".display().to_string()", ".to_str().unwrap()", ".to_string_lossy()"];

/// The rule of a site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// `format!` joining a `.display()` with a literal separator.
    A,
    /// A hand separator fold.
    B,
    /// `assert_eq!` over path text (warning).
    C,
    /// A path rendered to text and handed to a program, a golden or an expected value (warning).
    D,
    /// A string comparison with a path rendered to text (warning).
    E,
}

impl Rule {
    fn letter(self) -> char {
        match self {
            Rule::A => 'A',
            Rule::B => 'B',
            Rule::C => 'C',
            Rule::D => 'D',
            Rule::E => 'E',
        }
    }
}

/// One path-text site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// The rule.
    pub rule: Rule,
    /// What it does, in a sentence.
    pub what: &'static str,
    /// The waiver: `Some(true)` with a reason, `Some(false)` without one, `None` for no comment.
    pub waived: Option<bool>,
    index: usize,
}

fn slice(u: &[u16], a: usize, b: usize) -> String {
    String::from_utf16_lossy(&u[a.min(u.len())..b.min(u.len())])
}

/// Whether the text of an expression renders a path as text (`.display()` or `.to_string_lossy()`).
fn is_path_text(expr: &str) -> bool {
    expr.contains(".display()") || expr.contains(".to_string_lossy()")
}

/// Whether the text is a string or character literal (its quotes are kept by the mask): the other side of a comparison is then a fixed
/// text, a check of what the path ends in or names, not a comparison of two places.
fn is_literal(expr: &str) -> bool {
    let t = expr.trim().trim_start_matches('&');
    let t = t.strip_prefix('r').map_or(t, |r| r.trim_start_matches('#'));
    // a closure is a test of a character, not a place
    t.starts_with('"') && t.trim_end_matches('#').ends_with('"') || t.starts_with('\'') && t.ends_with('\'') || t.starts_with('|')
}

/// Whether two operands compare a path rendered to text with something that is not a fixed literal (either way round).
fn compares_path_text(a: &str, b: &str) -> bool {
    (is_path_text(a) && !is_literal(b)) || (is_path_text(b) && !is_literal(a))
}

/// The unit range of the statement around `at`: from the previous `;`, `{` or `}` to the next `;` or `}`.
fn statement(m: &[u16], at: usize) -> (usize, usize) {
    let is = |c: u16, set: &[u8]| set.iter().any(|&x| c == u16::from(x));
    let start = m[..at.min(m.len())].iter().rposition(|&c| is(c, b";{}")).map_or(0, |p| p + 1);
    let end = m[at.min(m.len())..].iter().position(|&c| is(c, b";}")).map_or(m.len(), |p| at + p);
    (start, end)
}

/// The operand that ends just before `end`: back to an opening bracket, a comma, a statement or block boundary, `=`, `|` or `&` at its own depth.
fn left_operand(m: &[u16], end: usize) -> (usize, usize) {
    let mut depth = 0usize;
    let mut i = end.min(m.len());
    while i > 0 {
        let c = m[i - 1];
        if c == u16::from(b')') || c == u16::from(b']') {
            depth += 1;
        } else if c == u16::from(b'(') || c == u16::from(b'[') {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if depth == 0 && b",;{}=|&".iter().any(|&x| c == u16::from(x)) {
            break;
        }
        i -= 1;
    }
    (i, end.min(m.len()))
}

/// The operand that starts at `start`: forward to a closing bracket, a comma, a statement or block boundary, `|` or `&` at its own depth.
fn right_operand(m: &[u16], start: usize) -> (usize, usize) {
    let mut depth = 0usize;
    let mut i = start.min(m.len());
    while i < m.len() {
        let c = m[i];
        if c == u16::from(b'(') || c == u16::from(b'[') {
            depth += 1;
        } else if c == u16::from(b')') || c == u16::from(b']') {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if depth == 0 && b",;{}|&".iter().any(|&x| c == u16::from(x)) {
            break;
        }
        i += 1;
    }
    (start.min(m.len()), i)
}

/// The unit index just past the `)` that closes the `(` at `open`.
fn close_of(masked: &[u16], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, &c) in masked.iter().enumerate().skip(open) {
        if c == b'(' as u16 {
            depth += 1;
        } else if c == b')' as u16 {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i + 1);
            }
        }
    }
    None
}

/// The first string literal of the call as written in the source, or empty.
fn first_literal(text: &[u16], masked: &[u16], open: usize, end: usize) -> String {
    let quote = b'"' as u16;
    let Some(a) = masked.iter().skip(open).position(|&c| c == quote).map(|p| p + open) else {
        return String::new();
    };
    if a >= end {
        return String::new();
    }
    let Some(b) = masked.iter().skip(a + 1).position(|&c| c == quote).map(|p| p + a + 1) else {
        return String::new();
    };
    slice(text, a + 1, b)
}

/// The path-text sites of one Rust file at the repository path `file`.
pub fn path_text_sites(text: &str, file: &str) -> Vec<Site> {
    let test_file = TEST_FILE.is_match(file);
    // Masking only blanks text, so a file whose text has no `#[cfg(test)]` has none in its masked text either: other code needs
    // no masking to be skipped.
    if SKIP_PATH.is_match(file) || (!test_file && !text.contains("#[cfg(test)]")) {
        return Vec::new();
    }
    let t: Vec<u16> = text.encode_utf16().collect();
    // `mask_units(&t)` as text and as units, without the round trip through UTF-16 and back (the same text: `text` is valid UTF-8).
    let masked = mask_rust(text);
    let m: Vec<u16> = masked.encode_utf16().collect();
    // JavaScript indexes of the matches, each text counted once (not once per match: quadratic in a test file with many calls).
    let in_masked = UnitIndex::new(&masked);
    let from = if test_file {
        0
    } else {
        match CFG_TEST_MOD.find(&masked) {
            Some(x) => in_masked.at(x.start()),
            None => return Vec::new(),
        }
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let newlines: Vec<usize> = t.iter().enumerate().filter(|&(_, &c)| c == b'\n' as u16).map(|(i, _)| i).collect();
    let line_at = |i: usize| newlines.partition_point(|&n| n < i) + 1;
    let mut out: Vec<Site> = Vec::new();
    let mut push = |index: usize, rule: Rule, what: &'static str| {
        let line = line_at(index);
        let near = format!(
            "{}\n{}",
            if line >= 2 {
                lines.get(line - 2).copied().unwrap_or("")
            } else {
                ""
            },
            lines.get(line - 1).copied().unwrap_or("")
        );
        let waived = if has_reason(&near, &WAIVER) {
            Some(true)
        } else if BARE_WAIVER.is_match(&near) {
            Some(false)
        } else {
            None
        };
        out.push(Site {
            line,
            rule,
            what,
            waived,
            index,
        });
    };
    for c in FORMAT_CALL.find_iter(&masked) {
        let start = in_masked.at(c.start());
        if start < from {
            continue;
        }
        let open = in_masked.at(c.end()) - 1;
        let Some(end) = close_of(&m, open) else { continue };
        if !slice(&m, open, end).contains(".display()") {
            continue;
        }
        if FORMAT_LITERAL.is_match(&first_literal(&t, &m, open, end)) {
            push(
                start,
                Rule::A,
                "joins a `.display()` argument with a literal `/` or `\\`: Windows spells the result `C:\\a/x`",
            );
        }
    }
    // Counted only when a fold is there (rare): most files have none.
    let in_text = Positions::new(text);
    for c in FOLD.find_iter(text) {
        let start = in_text.unit(c.start());
        if start < from || slice(&m, start, start + 9) != ".replace(" {
            continue;
        }
        push(start, Rule::B, "folds path separators by hand with `.replace`");
    }
    for c in ASSERT_CALL.find_iter(&masked) {
        let start = in_masked.at(c.start());
        if start < from {
            continue;
        }
        let open = in_masked.at(c.end()) - 1;
        let Some(end) = close_of(&m, open) else { continue };
        let args = slice(&m, open, end);
        if PATH_TEXT.iter().any(|p| args.contains(p)) && !args.contains("file_name()") {
            push(start, Rule::C, "compares path text, not paths");
        }
    }
    // Rules D and E judge one statement at a time, on its operands (narrowed after the review of de09dab8a: most of the first
    // sample were not path comparisons).
    let skip = |at: usize| {
        let (a, b) = statement(&m, at);
        let (masked_stmt, source) = (slice(&m, a, b), slice(&t, a, b));
        NAME_ONLY.is_match(&masked_stmt) || source.contains("?\\")
    };
    for (call, what) in [
        (
            &*ARG_CALL,
            "hands a path to a program as `.display()` text: use `paddock_env::winpath::for_tools`",
        ),
        (
            &*GOLDEN_CALL,
            "formats a path into a golden with `.display()`: the golden holds this machine's spelling",
        ),
    ] {
        for c in call.find_iter(&masked) {
            let start = in_masked.at(c.start());
            if start < from {
                continue;
            }
            let open = in_masked.at(c.end()) - 1;
            let Some(end) = close_of(&m, open) else { continue };
            if is_path_text(&slice(&m, open, end)) && !skip(start) {
                push(start, Rule::D, what);
            }
        }
    }
    for c in FORMAT_CALL.find_iter(&masked) {
        let start = in_masked.at(c.start());
        if start < from {
            continue;
        }
        let open = in_masked.at(c.end()) - 1;
        let Some(end) = close_of(&m, open) else { continue };
        let before_line = masked[..c.start()].rsplit('\n').next().unwrap_or("");
        if is_path_text(&slice(&m, open, end)) && EXPECTED_BINDING.is_match(before_line) && !skip(start) {
            push(
                start,
                Rule::D,
                "formats a path into an expected value with `.display()`: the text is this machine's spelling",
            );
        }
    }
    let mut e_lines: Vec<usize> = Vec::new();
    let mut e_sites: Vec<usize> = Vec::new();
    let mut note_e = |start: usize| {
        let line = line_at(start);
        if !e_lines.contains(&line) && !skip(start) {
            e_lines.push(line);
            e_sites.push(start);
        }
    };
    for c in EQ_OP.find_iter(&masked) {
        let at = in_masked.at(c.start());
        if at < from {
            continue;
        }
        let (left, right) = (left_operand(&m, at), right_operand(&m, in_masked.at(c.end())));
        if compares_path_text(&slice(&m, left.0, left.1), &slice(&m, right.0, right.1)) {
            note_e(at);
        }
    }
    for c in COMPARE_CALL.find_iter(&masked) {
        let at = in_masked.at(c.start());
        if at < from {
            continue;
        }
        let open = in_masked.at(c.end()) - 1;
        let Some(end) = close_of(&m, open) else { continue };
        let receiver = left_operand(&m, at);
        if compares_path_text(&slice(&m, receiver.0, receiver.1), &slice(&m, open + 1, end - 1)) {
            note_e(at);
        }
    }
    for s in e_sites {
        push(
            s,
            Rule::E,
            "compares a path rendered to text as a string: Windows spells the same place differently",
        );
    }
    out.sort_by(|a, b| a.index.cmp(&b.index).then(a.rule.letter().cmp(&b.rule.letter())));
    out
}

fn fix(rule: Rule) -> &'static str {
    match rule {
        Rule::A => {
            "Build the path with `Path::join` or `paddock_testkit::path::under`, or the expected text with `paddock_testkit::path::text`."
        }
        Rule::B => "Use `paddock_testkit::path::text` (the tools' spelling) or compare with `same_path` / `assert_same_path!`.",
        Rule::D => {
            "Pass `paddock_env::winpath::for_tools(path, os)` to a program, and normalise a golden's paths (`paddock_testkit::golden`)."
        }
        Rule::E => "Compare with `paddock_env::winpath::same` / `starts_with` (or `paddock_testkit::path::same_path`), not as strings.",
        Rule::C => {
            "Compare `Path`s, or use `paddock_testkit::assert_same_path!`; keep text only for what a program printed (`path::text`)."
        }
    }
}

/// The rule letter of a finding's message, as [`C122::run`] writes it (`(rule A: ...`): the one reader of that format, for the tests
/// that hold one rule apart from the others.
pub fn rule_of(message: &str) -> Option<char> {
    let rest = message.split("(rule ").nth(1)?;
    let mut chars = rest.chars();
    let letter = chars.next()?;
    (chars.next() == Some(':') && letter.is_ascii_uppercase()).then_some(letter)
}

/// The check.
pub struct C122;

impl Check for C122 {
    fn id(&self) -> &'static str {
        "C122"
    }

    fn rule(&self) -> u32 {
        40
    }

    fn title(&self) -> &'static str {
        "Tests compare paths as paths"
    }

    fn scope(&self) -> super::CheckScope {
        // As the JavaScript host declares it (`scope: 'root'`): run once over the whole tree, not again per app.
        super::CheckScope::Root
    }

    fn mode(&self) -> super::Mode {
        super::Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    "crates/c/tests/it/bad.rs",
                    "fn t() {\n    let e = format!(\"{}/x\", p.display());\n}\n",
                ),
                (
                    "crates/c/tests/it/good.rs",
                    "fn t() {\n    let e = format!(\"{}/x\", name);\n    // format!(\"{}/x\", p.display())\n}\n",
                ),
                ("crates/c/src/lib.rs", "fn f() {\n    let e = format!(\"{}/x\", p.display());\n}\n"),
                (
                    "apps/x/vendor/d/tests/w.rs",
                    "fn t() {\n    let e = format!(\"{}/x\", p.display());\n}\n",
                ),
            ],
            expect: vec![("crates/c/tests/it/bad.rs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut found: std::collections::BTreeMap<char, usize> = std::collections::BTreeMap::new();
        let mut probes = 0;
        let mut completed = true;
        for f in ctx.files().iter() {
            if !f.ends_with(".rs") {
                continue;
            }
            let Some(text) = ctx.read(f) else {
                completed = false;
                continue;
            };
            if text.is_empty() {
                continue;
            }
            probes += 1;
            if !PREFILTER.is_match(&text) {
                continue;
            }
            for s in path_text_sites(&text, f) {
                if s.waived == Some(true) {
                    continue;
                }
                *found.entry(s.rule.letter()).or_default() += 1;
                let bare = if s.waived == Some(false) {
                    " (the waiver comment has no reason)"
                } else {
                    ""
                };
                let severity = if matches!(s.rule, Rule::C | Rule::D | Rule::E) {
                    Severity::Warn
                } else {
                    SEVERITY_AB
                };
                let message = format!(
                    "{}{bare} (rule {}: the same place is spelled differently on Windows)",
                    s.what,
                    s.rule.letter()
                );
                let fixed = format!(
                    "{} A site that is not a path comparison carries `// conformance: path-text — <why>`.",
                    fix(s.rule)
                );
                findings.push(match severity {
                    Severity::Warn => Finding::warn("C122", 40, f, s.line, message, fixed),
                    Severity::Error => Finding::error("C122", 40, f, s.line, message, fixed),
                });
            }
        }
        // The baseline counts the whole tree: a run over some files only (a push gate's scope) cannot judge it.
        if ctx.files().len() == ctx.tree_files().len() {
            findings.extend(super::c122_baseline::judge(ctx, &found));
        }
        let has_rs = ctx.tree_files().iter().any(|f| f.ends_with(".rs"));
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: has_rs,
                detail: "the tree has a .rs file".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sites(text: &str) -> Vec<String> {
        sites_in(text, "crates/a/tests/it/x.rs")
    }

    fn sites_in(text: &str, file: &str) -> Vec<String> {
        path_text_sites(text, file)
            .iter()
            .map(|s| {
                let w = match s.waived {
                    Some(true) => ":reason",
                    Some(false) => ":no-reason",
                    None => "",
                };
                format!("{}{}{w}", s.rule.letter(), s.line)
            })
            .collect()
    }

    /// Each text is counted once, not once per call (asserted by the bytes walked, not by time, ER-12): counting the prefix of
    /// every match again was quadratic and took the live test (tests/it/check_c122.rs) past nextest's 60 s under load (2026-11-18).
    #[test]
    fn a_file_with_many_calls_is_counted_once_not_once_per_call() {
        let text: String = (0..2000)
            .map(|i| format!("    assert_eq!(a{i}, b);\n    let s = format!(\"{{}}\", x);\n"))
            .collect();
        super::super::jsre::WALKED.with(|w| w.set(0));
        assert!(path_text_sites(&text, "crates/a/tests/it/x.rs").is_empty());
        let walked = super::super::jsre::WALKED.with(|w| w.get());
        assert!(
            walked <= 4 * text.len(),
            "walked {walked} bytes of a {}-byte file: counted once per call, not once per file",
            text.len()
        );
    }

    #[test]
    fn a_format_that_joins_display_with_a_literal_separator_is_rule_a() {
        assert_eq!(sites("fn t() {\n    let e = format!(\"{}/x\", p.display());\n}\n"), ["A2"]);
        assert_eq!(sites("let e = format!(\"{}\\\\x\", p.display());"), ["A1"]);
        assert_eq!(sites("let e = format!(\"a/{}\", p.display());"), ["A1"]);
        assert_eq!(sites("let e = format!(\"{}/{}\", base.display(), name);"), ["A1"]);
    }

    #[test]
    fn no_display_no_separator_escapes_comments_and_strings_are_clean() {
        assert!(sites("let e = format!(\"{}/x\", name);").is_empty());
        assert!(sites("let e = format!(\"{}\\n\", p.display());").is_empty());
        assert!(sites("let e = format!(\"{} {}\", a.display(), b);").is_empty());
        assert!(sites("// let e = format!(\"{}/x\", p.display());\n").is_empty());
        assert!(sites("let s = \"format!(\\\"{}/x\\\", p.display())\";").is_empty());
    }

    #[test]
    fn a_hand_separator_fold_is_rule_b() {
        assert_eq!(sites("let t = p.to_string_lossy().replace('\\\\', \"/\");"), ["B1"]);
        assert_eq!(sites("let t = s.replace(\"\\\\\", \"/\");"), ["B1"]);
        assert_eq!(sites("let t = s.replace(r\"\\\", \"/\");"), ["B1"]);
        assert!(sites("let t = s.replace(\"a\", \"/\");").is_empty());
        assert!(sites("// s.replace('\\\\', \"/\")\n").is_empty());
    }

    #[test]
    fn assert_eq_over_path_text_is_rule_c_but_not_over_a_file_name() {
        assert_eq!(sites("assert_eq!(p.display().to_string(), \"x\");"), ["C1"]);
        assert_eq!(sites("assert_eq!(p.to_str().unwrap(), \"x\");"), ["C1"]);
        assert_eq!(sites("assert_ne!(p.to_string_lossy(), \"x\");"), ["C1"]);
        assert!(sites("assert_eq!(p.file_name().unwrap().to_string_lossy(), \"x\");").is_empty());
    }

    #[test]
    fn only_test_code_is_read() {
        let body = "fn f() { let e = format!(\"{}/x\", p.display()); }\n";
        assert!(sites_in(body, "crates/a/src/lib.rs").is_empty());
        assert_eq!(
            sites_in(&format!("{body}#[cfg(test)]\nmod tests {{\n{body}}}\n"), "crates/a/src/lib.rs"),
            ["A4"]
        );
        assert_eq!(sites_in(body, "crates/a/src/tests.rs"), ["A1"]);
        assert_eq!(sites_in(body, "crates/a/src/x_tests.rs"), ["A1"]);
        assert!(sites_in(body, "tools/t/tests/fixtures/x.rs").is_empty());
        assert!(sites_in(body, "apps/d/vendor/s/tests/x.rs").is_empty());
    }

    #[test]
    fn a_waiver_with_a_reason_covers_the_site_one_without_is_flagged() {
        let l = "let e = format!(\"{}/x\", p.display());";
        assert_eq!(
            sites(&format!("// conformance: path-text — a URL path, not a file\n{l}")),
            ["A2:reason"]
        );
        assert_eq!(sites(&format!("{l} // conformance: path-text — a URL path")), ["A1:reason"]);
        assert_eq!(sites(&format!("// conformance: path-text\n{l}")), ["A2:no-reason"]);
    }

    #[test]
    fn a_path_text_handed_to_a_program_or_a_golden_is_rule_d() {
        assert_eq!(
            sites("let o = Command::new(\"git\").arg(p.display().to_string()).output();"),
            ["D1"]
        );
        assert_eq!(sites("cmd.args([\"-C\", &dir.to_string_lossy()]);"), ["D1"]);
        assert_eq!(sites("assert_golden(&g, &format!(\"{}\", root.display()));"), ["D1"]);
        assert_eq!(sites("check_golden_with(&g, &x.display().to_string(), mode, paths);"), ["D1"]);
        assert_eq!(sites("let expected = format!(\"cache {}\", root.display());"), ["D1"]);
        // clean: a path handed over as a path, a file name, a literal, text that is no path binding
        assert!(sites("cmd.arg(&p);").is_empty());
        assert!(sites("cmd.arg(p.file_name().unwrap().to_string_lossy().to_string());").is_empty());
        assert!(sites("cmd.arg(\"x\");").is_empty());
        assert!(sites("let msg = format!(\"saw {}\", root.display());").is_empty());
        assert!(sites("// cmd.arg(p.display().to_string());\n").is_empty());
        // narrowed: `.env` is read by a Rust child; a `\\?\` check is a spelling check
        assert!(sites("cmd.env(\"HOME\", home.display().to_string());").is_empty());
        assert!(sites("cmd.arg(format!(\"{}\", p.display().to_string().trim_start_matches(r\"\\\\?\\\")));").is_empty());
    }

    #[test]
    fn a_string_comparison_of_path_text_is_rule_e() {
        assert_eq!(sites("assert!(out.contains(&p.display().to_string()));"), ["E1"]);
        assert_eq!(
            sites("assert!(p.display().to_string().starts_with(&root.display().to_string()));"),
            ["E1"]
        );
        assert_eq!(sites("if a.to_string_lossy() == b { f(); }"), ["E1"]);
        assert_eq!(sites("if a == b.to_string_lossy() { f(); }"), ["E1"]);
        assert_eq!(sites("assert!(s.starts_with(&root.to_string_lossy().to_string()));"), ["E1"]);
        assert_eq!(
            sites("assert!(\n    shown\n        .to_string_lossy()\n        .to_string() != other);"),
            ["E4"]
        );
        // clean: a name, a path comparison, a plain string
        assert!(sites("assert!(p.file_name().unwrap().to_string_lossy().contains(\"x\"));").is_empty());
        assert!(sites("assert!(p.extension().unwrap().to_string_lossy() == \"json\");").is_empty());
        assert!(sites("assert!(p.starts_with(&root));").is_empty());
        assert!(sites("assert!(out.contains(\"x\"));").is_empty());
    }

    /// The narrowing after the review of de09dab8a: most of the first sample were not comparisons of two places.
    #[test]
    fn a_fixed_literal_a_prefix_check_or_a_file_name_in_the_statement_is_not_rule_e() {
        // the other side is a fixed literal: what the path ends in, not which place
        assert!(sites("assert!(c[0].path.to_string_lossy().contains(\"msedge.exe\"));").is_empty());
        assert!(sites("assert!(p.display().to_string().starts_with(\"C:\"));").is_empty());
        assert!(sites("assert!(p.to_string_lossy() == \"x\");").is_empty());
        assert!(sites("assert!(\"abc\" != p.to_string_lossy());").is_empty());
        assert!(sites("assert!(p.to_string_lossy() == r\"E:\\Apps\\x.exe\");").is_empty());
        assert!(sites("assert!(p.to_string_lossy().starts_with(|c: char| c.is_ascii_digit()));").is_empty());
        // a spelling check for the verbatim prefix
        assert!(sites("assert!(!p.display().to_string().starts_with(r\"\\\\?\\\"));").is_empty());
        assert!(sites("assert!(!shown.to_string_lossy().contains(\"\\\\\\\\?\\\\\"), \"verbatim\");").is_empty());
        // a name earlier in the same statement (a multi-line chain)
        assert!(sites("assert!(\n    p.file_name()\n        .unwrap()\n        .to_string_lossy()\n        .contains(&name));").is_empty());
        // a line that merely holds the conversion is no comparison
        assert!(sites("let t = p\n    .to_string_lossy()\n    .to_string();").is_empty());
        // the sites that stay: the same two checks with a path on the other side
        assert_eq!(sites("assert!(p.to_string_lossy().contains(&q.to_string_lossy()));"), ["E1"]);
    }

    #[test]
    fn the_rule_of_a_message_is_read_from_the_text_the_check_writes() {
        assert_eq!(
            rule_of("compares path text, not paths (rule C: the same place is spelled differently on Windows)"),
            Some('C')
        );
        assert_eq!(rule_of("x (the waiver comment has no reason) (rule E: y)"), Some('E'));
        assert_eq!(rule_of("a finding of another check"), None);
    }

    #[test]
    fn a_waiver_covers_rules_d_and_e() {
        assert_eq!(
            sites("// conformance: path-text \u{2014} a URL, not a file\ncmd.arg(u.display().to_string());"),
            ["D2:reason"]
        );
    }

    #[test]
    fn non_ascii_text_before_a_site_does_not_shift_it() {
        assert_eq!(sites("let a = \"é😀\"; // é\nlet e = format!(\"{}/x\", p.display());"), ["A2"]);
    }
}
