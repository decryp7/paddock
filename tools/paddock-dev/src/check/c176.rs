//! C176: a test builds its paths in a temp folder it owns, never by joining onto the temp folder everybody shares (rule 97, ER-51,
//! ER-45). ER-21 class: `member_trial::pushed` put its bare origin at `<temp parent>/origin-<pid>.git`, a name every test of a process
//! shares and nothing removed, so a reused pid or a threaded run re-initialised an old origin and the push failed with "fetch first";
//! `msg-final.txt` and `chain-final.txt` were fixed names in the same parent. The one fix: `tempfile::tempdir()` or
//! `paddock_testkit::private_tempdir` (or a fixture helper that owns one, `Repo::owned_file`) and a join onto that folder's path.
//!
//! Read: every `.rs` file outside `target` and `node_modules`, with comments and the insides of literals masked
//! ([`super::lex::rust::mask`]). Only test code counts: a file under a `tests` folder or named `tests.rs`, `*_tests.rs` or `testutil*.rs`
//! ([`paddock_repo_graph::model::os::is_test_file`]), and the `#[cfg(test)]` items of any other file
//! ([`super::lex::rust::production_lines`]). A finding is, in that code, a **join onto the shared temp parent**:
//! - `temp_dir().join(..)`, `ambient().temp_dir.join(..)` and `.temp_dir().join(..)` (an `as_path()` between is the same);
//! - a name bound to the temp folder (`let tmp = std::env::temp_dir();`) and joined later in the file (`tmp.join(..)`);
//! - `.path().join("..").join(..)` and `.path().parent().unwrap().join(..)`: the parent of a testkit temp folder is the shared one
//!   (a parent only read or passed on, as in `GIT_CEILING_DIRECTORIES`, writes nothing and is not a finding).
//!
//! A site that is not a shared name (a test that builds a traversal path on purpose, never writing it) is waived in place with
//! `// conformance: shared-temp-parent — <why>` on its line or the line above; the report lists it under `waived` with the reason.
//!
//! The reading is lexical by design (a path that reaches the temp folder through a helper or a field is not seen, and a
//! `.path().parent()` of a file path is a false positive): a report-only check whose count is the starting list.
//!
//! **Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree**, recorded here and in docs/gates.toml
//! (`[[report_only]]` C176, review by 2026-12-11). Lists only shrink (ER-46): no baseline file. Rust host only: born in Rust.

use super::finding::Finding;
use super::inline_waiver::{CommentView, Kind};
use super::jsre::rx;
use super::lex::rust::{mask, production_lines};
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_repo_graph::model::os::is_test_file;
use regex::Regex;
use std::sync::LazyLock;

/// The rule field: rule 97 (ER-51), the rule this one refines: a unit of work writes only under a folder it owns.
const RULE: u32 = 97;

/// A join straight onto the temp folder, possibly over lines.
static DIRECT: LazyLock<Regex> = LazyLock::new(|| rx(r"temp_dir(?:\(\))?\s*(?:\.\s*as_path\(\)\s*)?\.\s*join\s*\("));
/// A name built in the parent of a testkit temp folder: `.path().join("..").join(..)` or `.path().parent().unwrap().join(..)`.
static PARENT: LazyLock<Regex> = LazyLock::new(|| {
    rx(r#"\.path\(\)\s*\.\s*(?:join\s*\(\s*"\.\."\s*\)|parent\s*\(\)\s*(?:\.\s*(?:unwrap|expect)\s*\([^)]*\)\s*)?)\s*\.\s*join\s*\("#)
});
/// A name bound to the temp folder itself.
static BOUND: LazyLock<Regex> = LazyLock::new(|| rx(r"\blet\s+(?:mut\s+)?([A-Za-z_]\w*)\s*(?::[^=;]+)?=\s*([^;]*temp_dir[^;]*);"));

/// One join onto the shared temp parent: its 1-based line and what was joined onto.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// How the shared parent is reached.
    pub via: String,
}

/// The sites of one file's text. `test_file`: the whole file is test code.
pub fn scan(text: &str, test_file: bool) -> Vec<Site> {
    if !text.contains("temp_dir") && !text.contains(".path()") {
        return Vec::new();
    }
    let masked = mask(text);
    let raw_lines: Vec<&str> = text.split('\n').collect();
    let masked_lines: Vec<&str> = masked.split('\n').collect();
    let production = production_lines(text);
    // A line is test code when the whole file is, or when the production reading emptied a line the masked text has.
    let is_test = |line: usize| {
        test_file
            || (production.get(line).is_some_and(|p| p.trim().is_empty()) && masked_lines.get(line).is_some_and(|m| !m.trim().is_empty()))
    };
    // The 0-based line of a byte offset and whether the masked text kept the first character there (a comment or literal blanks it).
    let starts: Vec<usize> = raw_lines
        .iter()
        .scan(0usize, |at, l| {
            let s = *at;
            *at += l.len() + 1;
            Some(s)
        })
        .collect();
    let locate = |offset: usize| -> Option<usize> {
        let line = starts.partition_point(|s| *s <= offset) - 1;
        let col = text[starts[line]..offset].chars().count();
        let code = masked_lines
            .get(line)
            .and_then(|m| m.chars().nth(col))
            .is_some_and(|c| !c.is_whitespace());
        (code && is_test(line)).then_some(line + 1)
    };
    let mut sites: Vec<Site> = Vec::new();
    for m in DIRECT.find_iter(text) {
        if let Some(line) = locate(m.start()) {
            sites.push(Site {
                line,
                via: "temp_dir().join(..)".into(),
            });
        }
    }
    for m in PARENT.find_iter(text) {
        if let Some(line) = locate(m.start()) {
            sites.push(Site {
                line,
                via: "the parent of a temp folder's path".into(),
            });
        }
    }
    for b in BOUND.captures_iter(text) {
        let init = &b[2];
        if init.contains(".join") || init.contains("tempdir") || init.contains("tempfile") {
            continue;
        }
        let Some(whole) = b.get(0) else { continue };
        let name = regex::escape(&b[1]);
        let joined = rx(&format!(r"\b{name}\s*\.\s*join\s*\("));
        for j in joined.find_iter(&text[whole.end()..]) {
            if let Some(line) = locate(whole.end() + j.start()) {
                sites.push(Site {
                    line,
                    via: format!("`{}`, bound to the temp folder, joined", &b[1]),
                });
            }
        }
    }
    sites.sort_by(|a, b| (a.line, &a.via).cmp(&(b.line, &b.via)));
    sites.dedup_by(|a, b| a.line == b.line);
    sites
}

/// Whether the whole file is test code.
fn test_file(rel: &str) -> bool {
    is_test_file(rel) || rel.rsplit('/').next().is_some_and(|n| n.starts_with("testutil"))
}

const FIX: &str = "Make the test's own folder (`tempfile::tempdir()` or `paddock_testkit::private_tempdir`, kept for the whole use) and join onto its path; for a file beside a fixture repository use a helper that owns the folder, as `Repo::owned_file` does (rule 97, ER-51).";

/// The check.
pub struct C176;

impl Check for C176 {
    fn id(&self) -> &'static str {
        "C176"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a test builds its paths in a temp folder it owns, never by joining onto the shared temp parent"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn origin(&self) -> super::Origin {
        super::Origin::BornInRust
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                // Red: the old origin fixture, a bound temp folder, the parent of a testkit folder.
                (
                    "tools/paddock-dev/tests/it/shared_name.rs",
                    "fn f() {\n    let bare = std::env::temp_dir().join(format!(\"origin-{}.git\", std::process::id()));\n}\n",
                ),
                (
                    "crates/a/tests/it/bound.rs",
                    "fn f() {\n    let tmp = std::env::temp_dir();\n    let p = tmp.join(\"x\");\n}\n",
                ),
                (
                    "crates/a/tests/it/parent.rs",
                    "fn f(r: &Repo) {\n    let f = r.path().join(\"..\").join(\"msg.txt\");\n}\n",
                ),
                // Green: a folder the test owns, a comment, production code, a cfg(test) item that owns its folder.
                (
                    "crates/a/tests/it/owned.rs",
                    "fn f() {\n    let d = tempfile::tempdir().unwrap();\n    let p = d.path().join(\"x\");\n}\n",
                ),
                (
                    "crates/a/tests/it/commented.rs",
                    "// std::env::temp_dir().join(\"x\") is the shared parent\nfn f() {}\n",
                ),
                (
                    "crates/a/tests/it/waived.rs",
                    "fn f(r: &Repo) {\n    // conformance: shared-temp-parent — a traversal path built to be refused, never written\n    let m = r.path().join(\"..\").join(\"x\");\n}\n",
                ),
                (
                    "crates/a/src/lib.rs",
                    "pub fn scratch() -> std::path::PathBuf {\n    std::env::temp_dir().join(\"x\")\n}\n",
                ),
                (
                    "crates/a/src/with_tests.rs",
                    "pub fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        let d = tempfile::tempdir().unwrap();\n        let _ = d.path().join(\"x\");\n    }\n}\n",
                ),
            ],
            expect: vec![
                ("crates/a/tests/it/bound.rs", 3),
                ("crates/a/tests/it/parent.rs", 2),
                ("tools/paddock-dev/tests/it/shared_name.rs", 2),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| f.ends_with(".rs")) {
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            let sites = scan(&text, test_file(f));
            let view = (!sites.is_empty()).then(|| CommentView::of(&text, Kind::Rust));
            for s in sites {
                let site = Finding::warn(
                    "C176",
                    RULE,
                    f,
                    s.line,
                    format!(
                        "a path is joined onto the shared temp folder ({}): every test and every run shares that parent",
                        s.via
                    ),
                    FIX,
                );
                let Some(view) = &view else { continue };
                findings.push(view.waiver(s.line, "shared-temp-parent").apply(site, || {
                    Finding::warn(
                        "C176",
                        RULE,
                        f,
                        s.line,
                        "the shared-temp-parent waiver gives no reason",
                        "Write `// conformance: shared-temp-parent — <why this path is never a name tests share>`.",
                    )
                }));
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
    fn a_join_onto_the_temp_folder_is_a_site_in_every_spelling() {
        let t = "fn a() {\n    let _ = std::env::temp_dir().join(\"x\");\n    let _ = ambient().temp_dir.join(\"y\");\n    let _ = a\n        .temp_dir()\n        .as_path()\n        .join(\"z\");\n}\n";
        assert_eq!(lines(t, true), vec![2, 3, 5]);
    }

    #[test]
    fn a_bound_temp_folder_is_a_site_at_each_join_and_a_tempdir_binding_is_none() {
        let t = "fn a() {\n    let tmp = std::env::temp_dir();\n    let _ = tmp.join(\"x\");\n    let _ = tmp.join(\"y\");\n    let own = tempfile::tempdir().unwrap();\n    let _ = own.path().join(\"z\");\n}\n";
        assert_eq!(lines(t, true), vec![3, 4]);
    }

    #[test]
    fn the_parent_of_a_temp_folder_path_is_a_site() {
        let t = "fn a(r: &Repo) {\n    let _ = r.path().join(\"..\").join(\"m\");\n    let _ = r.path().parent().unwrap().join(\"n\");\n    let _ = r.path().parent();\n    let _ = f(&r.path().join(\"..\"));\n    let _ = r.path().join(\"sub\");\n}\n";
        assert_eq!(lines(t, true), vec![2, 3]);
    }

    #[test]
    fn a_comment_and_a_literal_are_no_site() {
        let t = "// temp_dir().join(\"x\")\nfn a() {\n    let s = \"temp_dir().join(x)\";\n}\n";
        assert!(lines(t, true).is_empty());
    }

    #[test]
    fn only_test_code_counts() {
        let t = "fn a() { let _ = std::env::temp_dir().join(\"x\"); }\n#[cfg(test)]\nmod tests {\n    fn t() {\n        let _ = std::env::temp_dir().join(\"y\");\n    }\n}\n";
        assert_eq!(lines(t, false), vec![5]);
        assert!(lines("fn a() { let _ = std::env::temp_dir().join(\"x\"); }\n", false).is_empty());
    }

    #[test]
    fn the_test_file_names_are_the_shared_ones() {
        assert!(test_file("crates/a/tests/it/x.rs") && test_file("crates/a/src/tests.rs") && test_file("crates/a/src/testutil_x.rs"));
        assert!(!test_file("crates/a/src/lib.rs"));
    }
}
