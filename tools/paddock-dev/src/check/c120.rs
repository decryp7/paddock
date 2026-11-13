//! C120: nothing guesses cargo's build-folder layout (ER-21). "Two folders up from `current_exe()` or `OUT_DIR`" is the profile folder
//! in one cargo layout and a wrong folder in the other; it broke three times (Pitcrew's supervision tests, CI run 8027; Debrief's
//! plugins tests, run 8028; paddock-build). The one reader of both layouts is `paddock_env::cargo_layout`. In Rust and shell under
//! `crates/`, `tools/`, `apps/` and `scripts/`, outside that module, this flags `current_exe()` or `OUT_DIR` followed by two or more
//! parent or `..` hops, and a literal `deps` folder segment. Report-only (warnings) until its first green run (ER-41).
//!
//! The port of `conformance/src/checks/C120-layout-guess.mjs`, finding for finding (the oracle test compares them). Lines that start with a
//! comment are not read; a justified site carries `// conformance: layout-guess — <reason>` on the line or the line above.

use super::finding::Finding;
use super::inline_waiver::{InlineWaiver, recognise};
use super::jsre::{WS, line_at, rx, trim};
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The files that read both layouts, and this file (its docs and tests name the patterns).
const IMPLEMENTATION: [&str; 3] = [
    "crates/paddock-env/src/cargo_layout.rs",
    "crates/paddock-testkit/src/cargo_layout.rs",
    "tools/paddock-dev/src/check/c120.rs",
];

static SCOPE: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:crates|tools|apps|scripts)/"));
static VENDORED: LazyLock<Regex> = LazyLock::new(|| rx(r"^apps/[^/]+/vendor/"));
static PREFILTER: LazyLock<Regex> = LazyLock::new(|| rx(r"current_exe|OUT_DIR|deps"));

const START: &str = r"(?:current_exe\s*\(|\bOUT_DIR\b)";

/// A `deps` path segment: in the middle (`/deps/`) or at the end of a path (`/deps"`, `/deps` at the end of a line).
const DEPS: &str = r#"[/\\]deps(?:[/\\]|["'`)\r\n]|$)"#;

/// The Rust patterns, in the order of the JavaScript's `RUST_PATTERNS` (the order breaks ties at one position).
static RUST_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (
            rx(&format!(r"{START}[^;]*?\.parent\s*\([^;]*?\.parent\s*\(")),
            "reaches the profile folder by two or more `.parent()` hops from `current_exe()` or `OUT_DIR`",
        ),
        (
            rx(&format!(
                r"{START}[^;]*?\.ancestors\s*\(\)[^;]*?\.nth\s*\(\s*(?:[2-9]|[1-9][0-9]+)\s*\)"
            )),
            "reaches the profile folder by `.ancestors().nth(n)` with n of 2 or more from `current_exe()` or `OUT_DIR`",
        ),
        (
            rx(&format!(r#"{START}[^;]*?(?:"\.\.[/\\]+\.\."|"\.\."[^;]*?"\.\.")"#)),
            "reaches the profile folder by two or more `..` hops from `current_exe()` or `OUT_DIR`",
        ),
        (
            rx(&format!(r#"{DEPS}|\.(?:join|push)\s*\(\s*"deps"\s*\)"#)),
            "names the `deps` folder of a build folder",
        ),
    ]
});
static SHELL_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (
            rx(r"\bOUT_DIR\b[^\n]*?(?:\.\.[/\\]+\.\.|dirname[^\n]*dirname)|dirname[^\n]*dirname[^\n]*\bOUT_DIR\b"),
            "reaches the profile folder by two or more hops from `OUT_DIR`",
        ),
        (rx(DEPS), "names the `deps` folder of a build folder"),
    ]
});
/// The token (up to the last white space or quote) before a hit on its line; a URL or a `node_modules/` path is not a layout.
static TOKEN_TAIL: LazyLock<Regex> = LazyLock::new(|| rx(&format!("[^{WS}\"'`]*$")));
static NOT_A_LAYOUT: LazyLock<Regex> = LazyLock::new(|| rx(r"https?://|node_modules[/\\]"));
static TRAILING_SLASHES: LazyLock<Regex> = LazyLock::new(|| rx(r"\s//[^\n]*$"));
static TRAILING_HASH: LazyLock<Regex> = LazyLock::new(|| rx(r"\s#[^\n]*$"));

/// One layout guess in a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guess {
    /// The 1-based line.
    pub line: usize,
    /// What it does, in a sentence.
    pub what: &'static str,
    /// The waiver comment: the site's, else the file's (`file-layout-guess`), else a bare site comment.
    pub waived: InlineWaiver,
}

/// `text` with the lines that start with `prefix` (after white space) emptied and a trailing comment (white space, then `prefix`) cut
/// off; the line count stays.
pub fn without_comment_lines(text: &str, prefix: &str) -> String {
    let trailing = if prefix == "#" { &TRAILING_HASH } else { &TRAILING_SLASHES };
    text.split('\n')
        .map(|l| {
            if trim(l).starts_with(prefix) {
                String::new()
            } else {
                trailing.replace(l, "").into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The layout guesses in one source; `shell` picks the pattern set.
pub fn layout_guesses(text: &str, shell: bool) -> Vec<Guess> {
    let code = without_comment_lines(text, if shell { "#" } else { "//" });
    let lines: Vec<&str> = text.split('\n').collect();
    let patterns = if shell { &SHELL_PATTERNS } else { &RUST_PATTERNS };
    let mut hits: Vec<(usize, usize, &'static str)> = Vec::new();
    for (order, (re, what)) in patterns.iter().enumerate() {
        for m in re.find_iter(&code) {
            hits.push((m.start(), order, what));
        }
    }
    hits.sort_by_key(|h| (h.0, h.1));
    let file_waiver = recognise(text, "file-layout-guess");
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for (index, _, what) in hits {
        let line_start = code[..index].rfind('\n').map_or(0, |p| p + 1);
        if TOKEN_TAIL
            .find(&code[line_start..index])
            .is_some_and(|t| NOT_A_LAYOUT.is_match(t.as_str()))
        {
            continue;
        }
        let line = line_at(&code, index);
        if !seen.insert(line) {
            continue;
        }
        let near = format!(
            "{}\n{}",
            lines.get(line - 1).copied().unwrap_or(""),
            if line >= 2 {
                lines.get(line - 2).copied().unwrap_or("")
            } else {
                ""
            }
        );
        let waived = match (recognise(&near, "layout-guess"), &file_waiver) {
            (site @ InlineWaiver::Reason(_), _) => site,
            (_, file @ InlineWaiver::Reason(_)) => file.clone(),
            (site, _) => site,
        };
        out.push(Guess { line, what, waived });
    }
    out
}

/// The check.
pub struct C120;

impl Check for C120 {
    fn id(&self) -> &'static str {
        "C120"
    }

    fn rule(&self) -> u32 {
        40
    }

    fn mode(&self) -> super::Mode {
        super::Mode::ReportOnly
    }

    fn title(&self) -> &'static str {
        "No guessing of the cargo build-folder layout"
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    "crates/c/src/bad.rs",
                    "fn a() {\n    let d = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_path_buf();\n}\n",
                ),
                (
                    "crates/c/src/good.rs",
                    "fn a() {\n    let d = paddock_env::cargo_layout::profile_dir_of_exe(&std::env::current_exe().unwrap());\n}\n// current_exe().parent().parent()\n",
                ),
                (
                    "crates/paddock-env/src/cargo_layout.rs",
                    "fn a() {\n    let d = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap();\n}\n",
                ),
            ],
            expect: vec![("crates/c/src/bad.rs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let fix = "Use `paddock_env::cargo_layout` (`paddock_testkit::cargo_layout` in tests), which reads both cargo layouts and returns `None` for any other. A justified case carries `// conformance: layout-guess — <reason>`.";
        let mut findings = Vec::new();
        let mut probes = 0;
        let mut completed = true;
        for f in ctx.files().iter() {
            let shell = f.ends_with(".sh");
            if (!shell && !f.ends_with(".rs")) || !SCOPE.is_match(f) || VENDORED.is_match(f) || IMPLEMENTATION.contains(&f.as_str()) {
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
            for g in layout_guesses(&text, shell) {
                let site = Finding::warn(
                    "C120",
                    40,
                    f,
                    g.line,
                    format!("{}: right in one cargo layout, a wrong folder in the other", g.what),
                    fix,
                );
                findings.push(g.waived.apply(site, || {
                    Finding::warn(
                        "C120",
                        40,
                        f,
                        g.line,
                        "the layout-guess waiver gives no reason",
                        "Write `// conformance: layout-guess — <why this cannot be the other cargo layout>`.",
                    )
                }));
            }
        }
        let has_src = ctx.tree_files().iter().any(|f| f.ends_with(".rs") || f.ends_with(".sh"));
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: has_src,
                detail: "the tree has a .rs or .sh file".into(),
            },
        }
    }
}
