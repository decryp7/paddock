//! C174: a test fixture that builds a git repository with many `git` starts goes through `paddock_testkit::git_fixture` (rule 102, ER-20,
//! ER-22; user decision 74 of 2026-11-26). ER-21 class "a fixture builds its repository once per test": the `ci_runs` tests started 14
//! to 17 `git` processes in every test (`trial::world`, `green::train_g_shape`), about 45 ms each on the Windows runner, 710 s of one
//! train's Windows job (docs/benchmarks.md, "Git fixture repositories of the ci_runs tests"; perf/slow-git-tests). The one fix is the
//! shared module: the repository is built once per machine and recipe ([`paddock_testkit::git_fixture::template`]) and each test copies it.
//!
//! Read: every `.rs` file outside `target` and `node_modules`, with comments and the insides of literals masked
//! ([`super::lex::rust::mask`]). Only test code counts: a file under a `tests` folder or named `tests.rs`, `*_tests.rs` or
//! `testutil*.rs` ([`paddock_repo_graph::model::os::is_test_file`]), and the `#[cfg(test)]` items of any other file
//! ([`super::lex::rust::production_lines`]). In that code, per `fn`, a **git spawn** is a `Command::new("git")` (or any `X::new("git")`,
//! `TestCommand` included) or a call of a helper named `git`, `run_git`, `git_ok`, `git_in` or `git_out`. A `fn` with
//! [`MIN_SPAWNS`] or more spawns that does not use `git_fixture` is one finding at the `fn`: it builds its repository itself, per call.
//! A `fn` that names `git_fixture` is the template's builder (its spawns are paid once) and is not counted.
//!
//! The count is a heuristic by design (a spawn in a loop is one site; a helper under another name is not seen; a `;` inside a `fn`
//! signature, as in `[u8; 4]`, ends the read of that `fn`, which only ever costs a missed site).
//!
//! **Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree**, recorded here and in docs/gates.toml
//! (`[[report_only]]` C174, review by 2026-12-10). Lists only shrink (ER-46): no baseline file. Rust host only: born in Rust (user decision:
//! new checks are written in Rust).

use super::finding::Finding;
use super::jsre::rx;
use super::lex::rust::{mask, production_lines};
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_repo_graph::model::os::is_test_file;
use regex::Regex;
use std::sync::LazyLock;

/// The rule field: rule 85 (bulk, one git start per item), the rule this one refines; rule 102 (git fixtures) refines it.
const RULE: u32 = 85;

/// How many git starts in one `fn` make it a repository builder: six cost about 270 ms per test on the Windows runner.
pub const MIN_SPAWNS: usize = 6;

/// The shared module's own file: it names `git` nowhere, but it is the one place that may build the machinery.
const MODULE_FILE: &str = "crates/paddock-testkit/src/git_fixture.rs";

static FN: LazyLock<Regex> = LazyLock::new(|| rx(r"\bfn\s+([A-Za-z_]\w*)"));
static NEW_GIT: LazyLock<Regex> = LazyLock::new(|| rx(r#"\bnew\(\s*"git"\s*[,)]"#));
static HELPER: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|[^\w.])(?:run_git|git_ok|git_in|git_out|git)\s*\("));

const FIX: &str = "Build the repository once per machine with `paddock_testkit::git_fixture::template(name, include_str!(this file), |build, ready| ..)` and give each test `git_fixture::copy_tree(&template, &its_folder)`: no git start per test (rule 102).";

/// One repository builder: where its `fn` is, its name and how many git starts it makes.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line of the `fn`.
    pub line: usize,
    /// The function's name.
    pub name: String,
    /// The git starts it makes.
    pub spawns: usize,
}

/// A `fn` being read: its body ends when the braces return to the depth where it opened.
struct Frame {
    name: String,
    line: usize,
    /// The brace depth before its `{`.
    depth: usize,
    opened: bool,
    spawns: usize,
    fixture: bool,
}

/// The git spawns on one line (`raw` for the program name, `masked` so a comment or a string is no spawn).
fn spawns_on(raw: &str, masked: &str) -> usize {
    let helper = if masked.trim_start().starts_with("fn ") || masked.contains(" fn ") {
        0
    } else {
        HELPER.find_iter(masked).count()
    };
    let command = if masked.contains("new(") {
        NEW_GIT.find_iter(raw).count()
    } else {
        0
    };
    helper + command
}

/// The builders of one file's text. `test_file`: the whole file is test code.
pub fn scan(text: &str, test_file: bool) -> Vec<Site> {
    let masked = mask(text);
    let production = production_lines(text);
    let mut sites = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut depth = 0usize;
    for (i, ((raw, m), prod)) in text.split('\n').zip(masked.split('\n')).zip(&production).enumerate() {
        let test_line = test_file || (!m.trim().is_empty() && prod.trim().is_empty());
        if let Some(c) = FN.captures(m) {
            stack.push(Frame {
                name: c[1].to_string(),
                line: i + 1,
                depth,
                opened: false,
                spawns: 0,
                fixture: false,
            });
        }
        if test_line && let Some(top) = stack.last_mut() {
            top.spawns += spawns_on(raw, m);
        }
        if m.contains("git_fixture") {
            // A closure given to the template belongs to the `fn` that calls it, and so does every `fn` around it.
            stack.iter_mut().for_each(|f| f.fixture = true);
        }
        for ch in m.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    if let Some(top) = stack.last_mut().filter(|f| !f.opened) {
                        top.opened = true;
                    }
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    if stack.last().is_some_and(|f| f.opened && depth <= f.depth)
                        && let Some(f) = stack.pop()
                        && f.spawns >= MIN_SPAWNS
                        && !f.fixture
                    {
                        sites.push(Site {
                            line: f.line,
                            name: f.name,
                            spawns: f.spawns,
                        });
                    }
                }
                // A declaration without a body (a trait method) ends at its `;`.
                ';' if stack.last().is_some_and(|f| !f.opened && depth == f.depth) => {
                    stack.pop();
                }
                _ => {}
            }
        }
    }
    sites.sort_by_key(|s| s.line);
    sites
}

/// The files whose text can hold test code.
fn read(rel: &str) -> bool {
    rel.ends_with(".rs") && rel != MODULE_FILE && !rel.split('/').any(|s| s == "target" || s == "node_modules")
}

/// Whether the whole file is test code.
fn test_file(rel: &str) -> bool {
    is_test_file(rel) || rel.rsplit('/').next().is_some_and(|n| n.starts_with("testutil"))
}

/// The check.
pub struct C174;

impl Check for C174 {
    fn id(&self) -> &'static str {
        "C174"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a test fixture that builds a git repository with many git starts uses paddock-testkit's git_fixture"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        macro_rules! six {
            ($l:literal) => {
                concat!($l, $l, $l, $l, $l, $l)
            };
        }
        Control {
            files: vec![
                (
                    "crates/a/tests/it/repo.rs",
                    concat!("fn world() {\n", six!("    git(&d, &[\"x\"]);\n"), "}\n"),
                ),
                (
                    "tools/b/src/x/tests.rs",
                    concat!(
                        "fn world() {\n",
                        six!("    Command::new(\"git\").args([\"x\"]).output().unwrap();\n"),
                        "}\n"
                    ),
                ),
                (
                    "crates/c/src/lib.rs",
                    concat!("pub fn real() {\n", six!("    git(&d, &[\"x\"]);\n"), "}\n"),
                ),
                (
                    "crates/d/src/lib.rs",
                    concat!(
                        "pub fn real() {}\n#[cfg(test)]\nmod tests {\n    fn builder() {\n",
                        six!("        git(&d);\n"),
                        "    }\n}\n"
                    ),
                ),
                (
                    "crates/e/tests/it/tpl.rs",
                    concat!(
                        "fn shape() {\n    paddock_testkit::git_fixture::template(\"t\", \"r\", |b, _| {\n",
                        six!("        git(b);\n"),
                        "    });\n}\n"
                    ),
                ),
                (
                    "crates/f/tests/it/few.rs",
                    "fn world() {\n    git(a);\n    git(b);\n    git(c);\n}\n",
                ),
                (
                    "crates/g/tests/it/words.rs",
                    concat!("fn world() {\n", six!("    // git(a);\n"), six!("    let s = \"git(a)\";\n"), "}\n"),
                ),
            ],
            expect: vec![
                ("crates/a/tests/it/repo.rs", 1),
                ("crates/d/src/lib.rs", 4),
                ("tools/b/src/x/tests.rs", 1),
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
                findings.push(Finding::warn(
                    "C174",
                    RULE,
                    f,
                    s.line,
                    format!(
                        "`{}` starts git {} times to build a repository for a test (from {MIN_SPAWNS}): every test that calls it pays the starts",
                        s.name, s.spawns
                    ),
                    FIX,
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

    fn names(text: &str, test_file: bool) -> Vec<(usize, usize)> {
        scan(text, test_file).into_iter().map(|s| (s.line, s.spawns)).collect()
    }

    fn body(n: usize, line: &str) -> String {
        line.repeat(n)
    }

    #[test]
    fn a_test_fn_with_six_helper_calls_is_a_builder_at_its_fn_line() {
        let t = format!("\nfn world() {{\n{}}}\n", body(6, "    git(&d, &[\"x\"]);\n"));
        assert_eq!(names(&t, true), vec![(2, 6)]);
    }

    #[test]
    fn five_spawns_are_below_the_limit() {
        let t = format!("fn world() {{\n{}}}\n", body(MIN_SPAWNS - 1, "    git(a);\n"));
        assert!(names(&t, true).is_empty());
    }

    #[test]
    fn command_new_git_counts_but_another_program_does_not() {
        let t = format!("fn w() {{\n{}}}\n", body(6, "    Command::new(\"git\").output();\n"));
        assert_eq!(names(&t, true), vec![(1, 6)]);
        let t = format!("fn w() {{\n{}}}\n", body(6, "    Command::new(\"cargo\").output();\n"));
        assert!(names(&t, true).is_empty());
    }

    #[test]
    fn production_code_of_a_file_is_not_test_code_but_its_cfg_test_items_are() {
        let prod = format!("fn real() {{\n{}}}\n", body(6, "    git(a);\n"));
        assert!(names(&prod, false).is_empty());
        let both = format!(
            "{prod}#[cfg(test)]\nmod tests {{\n    fn b() {{\n{}    }}\n}}\n",
            body(6, "        git(a);\n")
        );
        assert_eq!(names(&both, false), vec![(11, 6)]);
    }

    #[test]
    fn a_fn_that_uses_the_shared_template_is_the_builder_and_is_not_counted() {
        let t = format!(
            "fn shape() {{\n    paddock_testkit::git_fixture::template(\"n\", \"r\", |b, _| {{\n{}    }});\n}}\n",
            body(14, "        git(b);\n")
        );
        assert!(names(&t, true).is_empty());
    }

    #[test]
    fn comments_strings_definitions_and_methods_are_no_spawn() {
        let t = format!(
            "fn git(a: &str) {{}}\nfn w() {{\n{}{}{}}}\n",
            body(6, "    // git(a);\n"),
            body(6, "    let s = \"git(a)\";\n"),
            body(6, "    repo.git(a);\n")
        );
        assert!(names(&t, true).is_empty(), "{:?}", names(&t, true));
    }

    #[test]
    fn a_trait_declaration_without_a_body_does_not_swallow_the_next_fn() {
        let t = format!("trait T {{\n    fn a(&self);\n}}\nfn w() {{\n{}}}\n", body(6, "    git(a);\n"));
        assert_eq!(names(&t, true), vec![(4, 6)]);
    }

    #[test]
    fn two_fns_are_counted_apart() {
        let t = format!(
            "fn a() {{\n{}}}\nfn b() {{\n{}}}\n",
            body(3, "    git(x);\n"),
            body(3, "    git(x);\n")
        );
        assert!(names(&t, true).is_empty(), "three and three are not six");
    }

    #[test]
    fn the_shared_module_and_other_folders_are_not_read() {
        assert!(!read(MODULE_FILE));
        assert!(!read("tools/x/target/debug/a.rs"));
        assert!(read("tools/paddock-dev/src/ci_runs/trial.rs"));
        assert!(test_file("crates/a/tests/it/x.rs") && test_file("tools/p/src/ci_runs/testutil.rs") && !test_file("crates/a/src/lib.rs"));
    }
}
