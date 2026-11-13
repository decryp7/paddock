//! C123: a Rust test starts `./dev`, `cargo` or `node` only through `paddock_testkit::TestCommand` (ADR 0100, ER-14, ER-21). A child
//! started with a plain `Command` inherits the machine's whole environment, so every new environment lookup in the program under test
//! quietly reaches real machine state (the pre-push tests ran the real heavy steps; `dev()` found the real engineering-rules `heavy.sh`
//! through ENGINEERING_RULES and took machine-wide slots, fix/precommit-heavy-slot, 2ff5f5072). Judged per Rust test file (a path
//! under `tests/`, a `tests.rs` or `*_test.rs` file, or the part of any file after `#[cfg(test)]`): (1) a constructor (`Command::new`,
//! `paddock_proc::command`, `quiet_command`, `<crate>::process::command`) whose first argument is the paddock-dev binary, cargo, `"node"`
//! or `"dev"` / `"./dev"`, and in tools/paddock-dev also `"sh"` / `"bash"` (the wrapper script `./dev` and the CI scripts run under one);
//! (2) any other use of the paddock-dev binary or of cargo's path (`env!("CARGO_BIN_EXE_paddock-dev")`, `env!("CARGO")`,
//! `env::var("CARGO")`) in a function whose text does not name `TestCommand`.
//! A justified case carries `conformance: test-env — <why>` on the line or above. JavaScript tests are not judged. Report-only for now
//! (ER-41).
//!
//! The port of `conformance/src/checks/C123-test-process-builder.mjs`, finding for finding (`tests/it/check_c123.rs` compares them).

use super::finding::Finding;
use super::jsre::rx;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

const ARG: &str =
    "env!\\(\"CARGO_BIN_EXE_paddock-dev\"\\)|env!\\(\"CARGO\"\\)|\"(?:cargo|node|dev|\\./dev)\"|(?:std::)?env::var(?:_os)?\\(\"CARGO\"\\)";
fn ctor(arg: &str) -> Regex {
    rx(&format!("(?:\\bCommand::new|\\b(?:\\w+::)*(?:quiet_)?command)\\(\\s*({arg})"))
}
static CTOR: LazyLock<Regex> = LazyLock::new(|| ctor(ARG));
/// In the tests of paddock-dev a shell starts the wrapper script `./dev` and the CI scripts, so a shell is one of the programs judged.
static CTOR_SHELL: LazyLock<Regex> = LazyLock::new(|| ctor(&format!("{ARG}|\"(?:sh|bash)\"")));
static DEV_BIN: LazyLock<Regex> =
    LazyLock::new(|| rx("env!\\(\"CARGO_BIN_EXE_paddock-dev\"\\)|env!\\(\"CARGO\"\\)|\\b(?:env::)?var(?:_os)?\\(\"CARGO\"\\)"));
static FN_LINE: LazyLock<Regex> = LazyLock::new(|| rx("^(\\s*)(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?(?:unsafe\\s+)?fn\\s+\\w"));
static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| rx("(?:^|/)tests?/|(?:^|/)tests?\\.rs$|_tests?\\.rs$"));
static WAIVER: LazyLock<Regex> = LazyLock::new(|| rx("conformance:\\s*test-env[ \\t]*[—–-]+[ \\t]*(\\S.{2,})"));
static BARE_WAIVER: LazyLock<Regex> = LazyLock::new(|| rx("conformance:\\s*test-env"));
static CFG_TEST: LazyLock<Regex> = LazyLock::new(|| rx("#\\[cfg\\(test\\)\\]"));

const FIX: &str = "Start the child with `paddock_testkit::TestCommand::new(..)` (an empty environment plus the listed one, a private HOME and TMPDIR); add what the test needs with `.env(\"NAME\", value)`. A justified case carries `// conformance: test-env — <why>`.";

/// One spawn site: the 1-based line, what was started and the waiver (`None`, `Some("")` for a bare comment, `Some(reason)`).
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// The matched argument.
    pub what: String,
    /// The waiver.
    pub waived: Option<String>,
}

/// The line index (0-based) of byte offset `idx`.
fn line_of(starts: &[usize], idx: usize) -> usize {
    starts.partition_point(|s| *s <= idx).saturating_sub(1)
}

/// The text of the function around line `i` (from its `fn` line to its closing brace), or the line itself outside a function.
fn function_text(lines: &[&str], i: usize) -> String {
    for s in (0..=i).rev() {
        let Some(m) = FN_LINE.captures(lines[s]) else { continue };
        let close = format!("{}}}", &m[1]);
        let mut e = s + 1;
        while e < lines.len() && lines[e] != close {
            e += 1;
        }
        return if e >= i {
            lines[s..=e.min(lines.len() - 1)].join("\n")
        } else {
            lines[i].to_string()
        };
    }
    lines[i].to_string()
}

/// The spawn sites of one Rust file, in line order.
pub fn spawn_sites(text: &str, path: &str) -> Vec<Site> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut starts = Vec::new();
    let mut off = 0;
    for l in text.split('\n') {
        starts.push(off);
        off += l.len() + 1;
    }
    let cfg = lines.iter().position(|l| l.trim() == "#[cfg(test)]");
    let whole = TEST_PATH.is_match(path);
    let in_test = |i: usize| whole || cfg.is_some_and(|c| i >= c);
    let waiver = |i: usize| -> Option<String> {
        let near = format!("{}\n{}", if i > 0 { lines[i - 1] } else { "" }, lines[i]);
        if let Some(w) = WAIVER.captures(&near) {
            Some(w[1].to_string())
        } else if BARE_WAIVER.is_match(&near) {
            Some(String::new())
        } else {
            None
        }
    };
    let comment = |i: usize| lines[i].trim_start().starts_with("//");
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let ctor = if path.starts_with("tools/paddock-dev/") {
        &*CTOR_SHELL
    } else {
        &*CTOR
    };
    for m in ctor.captures_iter(text) {
        let whole_match = m.get(0).map_or(0, |g| g.start());
        let i = line_of(&starts, whole_match);
        if comment(i) || !in_test(i) {
            continue;
        }
        seen.insert(i);
        seen.insert(line_of(&starts, m.get(1).map_or(0, |g| g.start()))); // the argument may sit on the next line
        out.push(Site {
            line: i + 1,
            what: m[1].to_string(),
            waived: waiver(i),
        });
    }
    for m in DEV_BIN.find_iter(text) {
        let i = line_of(&starts, m.start());
        if comment(i) || !in_test(i) || seen.contains(&i) {
            continue;
        }
        if function_text(&lines, i).contains("TestCommand") {
            continue;
        }
        out.push(Site {
            line: i + 1,
            what: m.as_str().to_string(),
            waived: waiver(i),
        });
    }
    out.sort_by_key(|s| s.line);
    out
}

/// The check.
pub struct C123;

impl Check for C123 {
    fn id(&self) -> &'static str {
        "C123"
    }

    fn rule(&self) -> u32 {
        3
    }

    fn title(&self) -> &'static str {
        "a Rust test starts ./dev, cargo or node only through paddock_testkit::TestCommand"
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    "crates/x/tests/bad.rs",
                    "#[test]\nfn t() {\n    let _ = std::process::Command::new(env!(\"CARGO_BIN_EXE_paddock-dev\")).output();\n}\n",
                ),
                (
                    "crates/x/tests/good.rs",
                    "#[test]\nfn t() {\n    let _ = TestCommand::new(env!(\"CARGO_BIN_EXE_paddock-dev\")).output();\n}\n",
                ),
                (
                    "crates/x/tests/waived.rs",
                    "#[test]\nfn t() {\n    // conformance: test-env — the child is the program under test itself\n    let _ = std::process::Command::new(\"cargo\").output();\n}\n",
                ),
            ],
            expect: vec![("crates/x/tests/bad.rs", 3)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let files = ctx.files();
        let (mut findings, mut probes, mut completed) = (Vec::new(), 0, true);
        let mut any = false;
        for f in files.iter().filter(|f| f.ends_with(".rs")) {
            let Some(text) = ctx.read(f) else {
                if TEST_PATH.is_match(f) {
                    completed = false;
                }
                continue;
            };
            if !TEST_PATH.is_match(f) && !CFG_TEST.is_match(&text) {
                continue;
            }
            any = true;
            probes += 1;
            for s in spawn_sites(&text, f) {
                if s.waived.as_deref().is_some_and(|w| !w.is_empty()) {
                    continue;
                }
                let bare = if s.waived.is_some() {
                    " (the waiver comment has no reason)"
                } else {
                    ""
                };
                findings.push(Finding::warn(
                    "C123",
                    3,
                    f,
                    s.line,
                    format!(
                        "a test starts `{}` without paddock_testkit::TestCommand{bare} (ER-14: the child inherits the machine's environment)",
                        s.what
                    ),
                    FIX,
                ));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: any,
                detail: "the tree has a Rust test file".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str, path: &str) -> Vec<usize> {
        spawn_sites(text, path).iter().map(|s| s.line).collect()
    }

    #[test]
    fn a_plain_command_for_dev_cargo_or_node_in_a_test_file_is_found_and_the_builder_is_not() {
        let bad = "fn t() {\n    Command::new(\"node\").output();\n    paddock_proc::command(\n        env!(\"CARGO\"),\n    );\n}\n";
        assert_eq!(lines(bad, "crates/a/tests/it/x.rs"), vec![2, 3]);
        let ok = "fn t() {\n    TestCommand::new(\"node\").output();\n    Command::new(\"git\").output();\n}\n";
        assert!(lines(ok, "crates/a/tests/it/x.rs").is_empty());
    }

    #[test]
    fn production_code_is_not_judged_until_its_test_module() {
        let text = "fn run() {\n    Command::new(\"cargo\");\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        Command::new(\"cargo\");\n    }\n}\n";
        assert_eq!(lines(text, "crates/a/src/lib.rs"), vec![7]);
    }

    #[test]
    fn the_dev_binary_handed_on_is_a_site_unless_its_function_uses_the_builder() {
        let handed = "fn t() {\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\");\n    run(bin);\n}\n";
        assert_eq!(lines(handed, "tools/x/tests/it/a.rs"), vec![2]);
        let built = "fn t() {\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\");\n    TestCommand::new(\"sh\").arg(bin);\n}\n";
        assert!(lines(built, "tools/x/tests/it/a.rs").is_empty());
    }

    #[test]
    fn a_waiver_needs_a_reason() {
        let w = "fn t() {\n    // conformance: test-env — a probe of the shell\n    Command::new(\"cargo\");\n}\n";
        assert_eq!(spawn_sites(w, "a/tests/x.rs")[0].waived.as_deref(), Some("a probe of the shell"));
        let bare = "fn t() {\n    // conformance: test-env\n    Command::new(\"cargo\");\n}\n";
        assert_eq!(spawn_sites(bare, "a/tests/x.rs")[0].waived.as_deref(), Some(""));
    }
}
