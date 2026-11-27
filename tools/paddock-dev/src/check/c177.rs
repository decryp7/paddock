//! C177: a test never reads a register or record file a tool writes, as the live file, to assert its exact content (rule 79, ER-42,
//! ER-46). ER-21 class: tests read a live, tool-written register with `include_str!` or `fs::read` and assert what is in it, so they
//! break whenever a tool appends to it. Two instances: `test_cache::tests::a_run_is_added_to_the_green_runs_once_and_the_file_still_parses`
//! broke when `shadow-run --write` added run 9180 to docs/test-cache.toml (commit 0e33f28f0 worked around it by clearing the list), and
//! the never-cached tests broke on any entry of `no_test_cache`. The fix is a fixture copy owned by the test, and for the shipped file a
//! test of its shape only, through its own accessor (`Config::read`, `register::load`: the accessor is not a finding).
//!
//! The registers are [`TOOL_WRITTEN`], declared once, here: each names the constant its writer uses, so a moved file moves the entry
//! (the JavaScript baselines have no Rust constant and are named by their folder). A site is, in test code (a file under a `tests`
//! folder, `tests.rs`, `*_tests.rs`, `testutil*.rs`, and the `#[cfg(test)]` items of any other file), a code literal that names one of
//! the registers with
//! - `include_str!` or `include_bytes!` on the same line (always the live file), or
//! - a read call (`read_to_string`, `fs::read`, `File::open`, `read_to_end`, `read_dir`) within three lines of it together with a
//!   live root (`repo_root`, `CARGO_MANIFEST_DIR`, `manifest_dir`, `env!`): a read under a temporary folder the test made is not live.
//!
//! The scan is a heuristic by design: a register reached only through the writer's constant (`repo_root().join(FILE)`) is not seen, and
//! a root that is not named as above is not known to be live; both only ever cost a missed site. The accessor read stays the way to read
//! the live file.
//!
//! Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree, recorded here and in docs/gates.toml
//! (`[[report_only]]` C177, review by 2026-12-11). No baseline file (ER-46): the report is the list. Rust host only: born in Rust.

use super::finding::Finding;
use super::jsre::rx;
use super::lex::rust::{mask, production_lines};
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_repo_graph::model::os::is_test_file;
use regex::Regex;
use std::sync::LazyLock;

/// The rule field: rule 79, tests read declared data through its accessor.
const RULE: u32 = 79;

/// How many lines before and after a register's name a read call and a live root may stand.
const WINDOW: usize = 3;

/// A register or record file a tool writes.
pub struct Register {
    /// The repository-relative file, or a folder or prefix (a name that contains it is the register).
    pub path: &'static str,
    /// The command that writes it.
    pub writer: &'static str,
}

/// The registers tools write, declared once. A path is the writer's own constant where there is one.
pub const TOOL_WRITTEN: [Register; 7] = [
    Register {
        path: crate::test_cache::config::FILE,
        writer: "`./dev ci test-cache shadow-run --write` (adds to [switch] green_runs)",
    },
    Register {
        path: crate::ci_runs::dispatch_guards::REGISTER,
        writer: "`./dev ci guard-record` (a guard's first green run)",
    },
    Register {
        path: crate::run_scope::REGISTER,
        writer: "`./dev ci scope-record` (a producer's first green scope run)",
    },
    Register {
        path: crate::escape::register::FILE,
        writer: "`./dev escape` (appends a nightly row)",
    },
    Register {
        path: crate::ci_runs::fix_items::LESSONS,
        writer: "`./dev ci fix-items` (appends a run's fix items)",
    },
    Register {
        path: crate::check_ids::RESERVED_DIR,
        writer: "`./dev check new-id --reserve` (one file per reserved id)",
    },
    Register {
        path: "conformance/baselines/",
        writer: "the `conformance/bin/paddock-*-baseline.mjs` tools (rewrite a shrunk baseline)",
    },
];

static READ: LazyLock<Regex> = LazyLock::new(|| rx(r"read_to_string|fs::read\b|File::open|read_to_end|read_dir"));
static LIVE: LazyLock<Regex> = LazyLock::new(|| rx(r"repo_root|CARGO_MANIFEST_DIR|manifest_dir|\benv!"));
static INCLUDE: LazyLock<Regex> = LazyLock::new(|| rx(r"include_(str|bytes)!"));

/// One live read of a register: its 1-based line, the register's path and how it is read.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line of the register's name.
    pub line: usize,
    /// The register's path.
    pub path: &'static str,
    /// Whether it is `include_str!` (else a read call on a live root).
    pub include: bool,
}

/// The live reads of the registers in one file's text. `test_file`: the whole file is test code.
pub fn scan(text: &str, test_file: bool) -> Vec<Site> {
    let named: Vec<&Register> = TOOL_WRITTEN.iter().filter(|r| text.contains(r.path)).collect();
    if named.is_empty() {
        return Vec::new();
    }
    let masked = mask(text);
    let production = production_lines(text);
    let raw: Vec<&str> = text.split('\n').collect();
    let code: Vec<&str> = masked.split('\n').collect();
    let is_test =
        |i: usize| test_file || (production.get(i).is_some_and(|p| p.is_empty()) && code.get(i).is_some_and(|m| !m.trim().is_empty()));
    let mut sites = Vec::new();
    for (i, line) in raw.iter().enumerate() {
        if !is_test(i) {
            continue;
        }
        for r in &named {
            let Some(at) = line.find(r.path) else { continue };
            // A name inside a literal the mask kept as code: its opening quote survives in the masked line, a comment's does not.
            let Some(open) = line[..at].rfind('"') else { continue };
            let open_char = line[..open].chars().count();
            if code[i].chars().nth(open_char) != Some('"') {
                continue;
            }
            if INCLUDE.is_match(code[i]) {
                sites.push(Site {
                    line: i + 1,
                    path: r.path,
                    include: true,
                });
                continue;
            }
            let from = i.saturating_sub(WINDOW);
            let to = (i + WINDOW + 1).min(code.len());
            let near = &code[from..to];
            if near.iter().any(|l| READ.is_match(l)) && near.iter().any(|l| LIVE.is_match(l)) {
                sites.push(Site {
                    line: i + 1,
                    path: r.path,
                    include: false,
                });
            }
        }
    }
    sites
}

/// The files whose text can hold test code.
fn read(rel: &str) -> bool {
    rel.ends_with(".rs") && !rel.split('/').any(|s| s == "target" || s == "node_modules")
}

/// Whether the whole file is test code.
fn test_file(rel: &str) -> bool {
    is_test_file(rel) || rel.rsplit('/').next().is_some_and(|n| n.starts_with("testutil"))
}

/// The check.
pub struct C177;

impl Check for C177 {
    fn id(&self) -> &'static str {
        "C177"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a test does not read a register a tool writes as the live file: a fixture copy, or its shape through the accessor"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                // Red: an include_str! of a register, and a read of one under the repository root.
                (
                    "tools/a/src/x/tests.rs",
                    "const CFG: &str = include_str!(\"../../../../docs/test-cache.toml\");\n",
                ),
                (
                    "tools/a/tests/it/y.rs",
                    "fn t() {\n    let p = repo_root().join(\"docs/escape-register.toml\");\n    let _ = std::fs::read_to_string(p);\n}\n",
                ),
                (
                    "crates/b/src/lib.rs",
                    "pub fn f() {}\n#[cfg(test)]\nmod tests {\n    const R: &str = include_str!(\"../../../conformance/baselines/C71.json\");\n}\n",
                ),
                // Green: a read under a temporary folder, the accessor, a comment, production code, an unrelated file.
                (
                    "tools/a/tests/it/temp.rs",
                    "fn t() {\n    let d = tempfile::tempdir().unwrap();\n    std::fs::write(d.path().join(\"docs/lessons.md\"), \"x\").unwrap();\n    let _ = std::fs::read_to_string(d.path().join(\"docs/lessons.md\"));\n}\n",
                ),
                (
                    "tools/a/tests/it/accessor.rs",
                    "fn t() {\n    // reads docs/test-cache.toml through its parser\n    let c = Config::read(&repo_root()).unwrap();\n}\n",
                ),
                (
                    "tools/a/src/writer.rs",
                    "pub fn load(root: &Path) -> String {\n    std::fs::read_to_string(root.join(\"docs/test-cache.toml\")).unwrap()\n}\nconst X: &str = include_str!(\"../docs/test-cache.toml\");\n",
                ),
                (
                    "tools/a/tests/it/other.rs",
                    "fn t() {\n    let _ = std::fs::read_to_string(repo_root().join(\"docs/generators.toml\"));\n}\n",
                ),
            ],
            expect: vec![
                ("crates/b/src/lib.rs", 4),
                ("tools/a/src/x/tests.rs", 1),
                ("tools/a/tests/it/y.rs", 2),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| read(f)) {
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            for s in scan(&text, test_file(f)) {
                let writer = TOOL_WRITTEN.iter().find(|r| r.path == s.path).map_or("", |r| r.writer);
                findings.push(Finding::warn(
                    "C177",
                    RULE,
                    f,
                    s.line,
                    format!(
                        "a test {} `{}`, a file {writer} writes: it breaks when the tool appends to it",
                        if s.include { "includes" } else { "reads" },
                        s.path
                    ),
                    "Test on a fixture copy the test owns (a literal or a file under its own temporary folder), and read the shipped file only through its accessor (`Config::read`, `register::load`) asserting its shape: it parses, its keys exist, its invariants hold, never what a tool wrote into it.",
                ));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: probes > 0 || ctx.is_in_memory(),
                detail: format!("{probes} Rust files read"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str, test_file: bool) -> Vec<usize> {
        scan(text, test_file).into_iter().map(|s| s.line).collect()
    }

    #[test]
    fn an_include_of_a_register_in_test_code_is_a_site_and_in_production_code_it_is_not() {
        let t = "const A: &str = include_str!(\"../../docs/escape-register.toml\");\n";
        assert_eq!(lines(t, true), vec![1]);
        assert!(lines(t, false).is_empty());
        let cfg = format!("pub fn f() {{}}\n#[cfg(test)]\nmod t {{\n    {t}}}\n");
        assert_eq!(lines(&cfg, false), vec![4]);
    }

    #[test]
    fn a_read_counts_only_with_a_live_root_within_the_window() {
        let live = "let p = repo_root().join(\"docs/lessons.md\");\nlet _ = std::fs::read_to_string(p);\n";
        assert_eq!(lines(live, true), vec![1]);
        let temp = "let p = dir.path().join(\"docs/lessons.md\");\nlet _ = std::fs::read_to_string(p);\n";
        assert!(lines(temp, true).is_empty());
        let far = format!(
            "let p = repo_root().join(\"docs/lessons.md\");\n{}let _ = std::fs::read_to_string(p);\n",
            "\n".repeat(WINDOW + 1)
        );
        assert!(lines(&far, true).is_empty(), "a read beyond the window is another statement");
    }

    #[test]
    fn a_name_in_a_comment_or_a_message_is_no_site() {
        let t = "// include_str!(\"docs/lessons.md\")\nfn t() {\n    let m = \"see docs/lessons.md\";\n}\n";
        assert!(lines(t, true).is_empty(), "{:?}", scan(t, true));
    }

    #[test]
    fn a_folder_register_matches_a_file_inside_it() {
        let t = "const A: &str = include_str!(\"../../conformance/baselines/C71.json\");\n";
        assert_eq!(scan(t, true)[0].path, "conformance/baselines/");
        let t = "let d = repo_root().join(\"conformance/reserved\");\nfor e in std::fs::read_dir(d) {}\n";
        assert_eq!(lines(t, true), vec![1]);
    }

    #[test]
    fn the_registers_are_the_writers_own_constants() {
        let paths: Vec<&str> = TOOL_WRITTEN.iter().map(|r| r.path).collect();
        assert!(paths.contains(&"docs/test-cache.toml") && paths.contains(&"docs/lessons.md") && paths.contains(&"conformance/reserved"));
        assert!(TOOL_WRITTEN.iter().all(|r| !r.path.is_empty() && !r.writer.is_empty()));
    }
}
