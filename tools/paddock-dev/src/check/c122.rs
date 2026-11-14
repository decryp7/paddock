//! C122: tests compare paths as paths (ADR 0099, rule 40). The same place is spelled differently on Windows (`\` and `/`, `\\?\`,
//! drive case, Git bash's `/c/x`), so test code that builds expected path text by hand fails there and only there. In Rust test
//! code (files under `tests/`, `tests.rs`, `*_tests.rs`, and the `#[cfg(test)] mod` of any other file), with comments and string
//! insides masked:
//!
//! - rule A: a `format!` that joins a `.display()` argument with a literal `/` or `\` next to a placeholder;
//! - rule B: a hand separator fold, `.replace('\\', "/")` (also with strings);
//! - rule C (a warning, a heuristic): `assert_eq!`/`assert_ne!` with an operand that is path text.
//!
//! A site that is not a path comparison carries `// conformance: path-text — <why>` on the line or the line above. Report-only
//! (ER-41): A and B are warnings until the first green run on the real tree; flip [`SEVERITY_AB`] then.
//!
//! The port of `conformance/src/checks/C122-test-path-text.mjs`, finding for finding (`tests/it/check_c122.rs` compares them).
//! Positions are UTF-16 offsets, as in the JavaScript.

use super::finding::{Finding, Severity};
use super::jsre::{has_reason, rx, units};
use super::mask::mask_units;
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
static PREFILTER: LazyLock<Regex> = LazyLock::new(|| rx(r"format!|\.replace\(|assert_(?:eq|ne)!"));
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
}

impl Rule {
    fn letter(self) -> char {
        match self {
            Rule::A => 'A',
            Rule::B => 'B',
            Rule::C => 'C',
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
    if SKIP_PATH.is_match(file) {
        return Vec::new();
    }
    let t: Vec<u16> = text.encode_utf16().collect();
    let m: Vec<u16> = mask_units(&t);
    let masked = String::from_utf16_lossy(&m);
    let from = if TEST_FILE.is_match(file) {
        0
    } else {
        match CFG_TEST_MOD.find(&masked) {
            Some(x) => units(&masked[..x.start()]),
            None => return Vec::new(),
        }
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let line_at = |i: usize| t[..i.min(t.len())].iter().filter(|&&c| c == b'\n' as u16).count() + 1;
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
        let start = units(&masked[..c.start()]);
        if start < from {
            continue;
        }
        let open = units(&masked[..c.end()]) - 1;
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
    for c in FOLD.find_iter(text) {
        let start = units(&text[..c.start()]);
        if start < from || slice(&m, start, start + 9) != ".replace(" {
            continue;
        }
        push(start, Rule::B, "folds path separators by hand with `.replace`");
    }
    for c in ASSERT_CALL.find_iter(&masked) {
        let start = units(&masked[..c.start()]);
        if start < from {
            continue;
        }
        let open = units(&masked[..c.end()]) - 1;
        let Some(end) = close_of(&m, open) else { continue };
        let args = slice(&m, open, end);
        if PATH_TEXT.iter().any(|p| args.contains(p)) && !args.contains("file_name()") {
            push(start, Rule::C, "compares path text, not paths");
        }
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
        Rule::C => {
            "Compare `Path`s, or use `paddock_testkit::assert_same_path!`; keep text only for what a program printed (`path::text`)."
        }
    }
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
                let bare = if s.waived == Some(false) {
                    " (the waiver comment has no reason)"
                } else {
                    ""
                };
                let severity = if s.rule == Rule::C { Severity::Warn } else { SEVERITY_AB };
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
    fn non_ascii_text_before_a_site_does_not_shift_it() {
        assert_eq!(sites("let a = \"é😀\"; // é\nlet e = format!(\"{}/x\", p.display());"), ["A2"]);
    }
}
