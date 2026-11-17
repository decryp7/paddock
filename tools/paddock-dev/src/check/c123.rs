//! C123: a Rust test starts `./dev`, `cargo` or `node` only through `paddock_testkit::TestCommand` (ADR 0100, ER-14, ER-21). A child
//! started with a plain `Command` inherits the machine's whole environment, so every new environment lookup in the program under test
//! quietly reaches real machine state (the pre-push tests ran the real heavy steps; `dev()` found the real engineering-rules `heavy.sh`
//! through ENGINEERING_RULES and took machine-wide slots, fix/precommit-heavy-slot, 2ff5f5072). Judged per Rust test file (a path
//! under `tests/`, a `tests.rs` or `*_test.rs` file, or the part of any file after a `#[cfg(..)]` naming `test`, never `not(test)`):
//! (1) a constructor (`Command::new`, `paddock_proc::command`, `quiet_command`, `<crate>::process::command`) whose first argument is a
//! built binary (`env!("CARGO_BIN_EXE_*")`, or a name bound to one: a `const`, `static`, `let` or a function returning one, in the
//! file or `pub` in another test file of the crate), cargo or rustc (also `env!`/`env::var` of `CARGO` and `RUSTC`), `"rustup"`,
//! `"node"`, `"npm"`, `"npx"`, `"dev"` / `"./dev"` or a path joined to `"dev"` (each also with `.exe` or `.cmd`), and in tools/paddock-dev
//! also `"sh"` / `"bash"` (the wrapper script `./dev` and the CI scripts run under one); (2) any other use of the paddock-dev binary or
//! of cargo's path (`env!("CARGO_BIN_EXE_paddock-dev")`, `env!("CARGO")`, `env::var("CARGO")`) in a function whose code (comments
//! left out) does not name `TestCommand`; (3) the machine's `PATH` read in a function that builds a `TestCommand` (use `path_first`).
//! Known false negatives (ADR 0100): a program held in a variable, an alias of `Command`, a block comment naming `TestCommand`, and
//! spawns in a non-test helper module.
//! A justified case carries `conformance: test-env — <why>` on the line or above. JavaScript tests are not judged. Report-only for now
//! (ER-41).
//!
//! The port of `conformance/src/checks/C123-test-process-builder.mjs`, finding for finding (`tests/it/check_c123.rs` compares them).

use super::finding::Finding;
use super::jsre::rx;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

const ARG: &str = r#"env!\("CARGO_BIN_EXE_[0-9A-Za-z_-]+"\)|env!\("(?:CARGO|RUSTC)"\)|"(?:cargo|rustc|rustup|node|npm|npx|dev|\./dev)(?:\.exe|\.cmd)?"|(?:std::)?env::var(?:_os)?\("(?:CARGO|RUSTC)"\)|[0-9A-Za-z_:.&()]*\.join\("(?:\./)?dev"\)"#;
/// A shell, which starts the wrapper script `./dev` and the CI scripts in the tests of paddock-dev.
const SHELL: &str = r#""(?:sh|bash)(?:\.exe)?""#;
fn ctor(arg: &str) -> Regex {
    rx(&format!("(?:\\bCommand::new|\\b(?:\\w+::)*(?:quiet_)?command)\\(\\s*({arg})"))
}
static BOUND: LazyLock<Regex> =
    LazyLock::new(|| rx("(?:const|static|let(?:\\s+mut)?)\\s+([A-Za-z_][A-Za-z0-9_]*)\\s*(?::[^=;]*)?=\\s*[^;]*?env!\\(\"CARGO_BIN_EXE_"));
static CTOR: LazyLock<Regex> = LazyLock::new(|| ctor(ARG));
/// In the tests of paddock-dev a shell starts the wrapper script `./dev` and the CI scripts, so a shell is one of the programs judged.
static CTOR_SHELL: LazyLock<Regex> = LazyLock::new(|| ctor(&format!("{ARG}|{SHELL}")));
static PATH_VAR: LazyLock<Regex> = LazyLock::new(|| rx("\\b(?:env::)?var(?:_os)?\\(\"PATH\"\\)"));
static DEV_BIN: LazyLock<Regex> =
    LazyLock::new(|| rx("env!\\(\"CARGO_BIN_EXE_paddock-dev\"\\)|env!\\(\"CARGO\"\\)|\\b(?:env::)?var(?:_os)?\\(\"CARGO\"\\)"));
static FN_LINE: LazyLock<Regex> = LazyLock::new(|| rx("^(\\s*)(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?(?:unsafe\\s+)?fn\\s+\\w"));
static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| rx("(?:^|/)tests?/|(?:^|/)tests?\\.rs$|_tests?\\.rs$"));
static WAIVER: LazyLock<Regex> = LazyLock::new(|| rx("conformance:\\s*test-env[ \\t]*[—–-]+[ \\t]*(\\S.{2,})"));
static BARE_WAIVER: LazyLock<Regex> = LazyLock::new(|| rx("conformance:\\s*test-env"));
static CFG_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"#\[cfg\([^\]]*\btest\b[^\]]*\)\]"));
static NOT_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"\bnot\(\s*test\b"));
/// A function's name and whether its signature returns something (`fn child_program() -> PathBuf`).
static FN_NAME: LazyLock<Regex> = LazyLock::new(|| rx(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)[^{;]*->"));
static PUB_BEFORE: LazyLock<Regex> = LazyLock::new(|| rx(r"\bpub(?:\([^)]*\))?\s+$"));
static PUB_FN: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*pub\b"));

/// Whether a line opens a test part: a `#[cfg(..)]` naming `test`, not `not(test)`.
pub fn is_cfg_test(line: &str) -> bool {
    CFG_TEST.find(line).is_some_and(|m| !NOT_TEST.is_match(m.as_str()))
}

/// The crate a Rust file belongs to: its path up to the `tests/` or `src/` folder (the whole path when it has neither).
pub fn crate_of(path: &str) -> &str {
    ["/tests/", "/src/"].iter().filter_map(|d| path.find(d)).min().map_or(path, |i| &path[..i])
}

/// `text` without its line comments (`//` outside a string to the end of the line), so a word in a comment does not count.
pub fn code(text: &str) -> String {
    text.split('\n')
        .map(|l| {
            let b = l.as_bytes();
            let (mut k, mut in_str) = (0, false);
            while k < b.len() {
                if b[k] == b'\\' && in_str {
                    k += 1;
                } else if b[k] == b'"' {
                    in_str = !in_str;
                } else if !in_str && b[k] == b'/' && b.get(k + 1) == Some(&b'/') {
                    return &l[..k];
                }
                k += 1;
            }
            l
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A name a file binds to a built binary (see [`bound_names`]).
#[derive(Debug, PartialEq, Eq)]
pub struct Bound {
    /// The name.
    pub name: String,
    /// `pub`: the other test files of the same crate use it too.
    pub exported: bool,
}

/// The names a file binds to a built binary: a `const`, `static` or `let` set from `env!("CARGO_BIN_EXE_..")`, and a function that
/// returns one (`fn child_program() -> PathBuf { ..env!(..) }`).
pub fn bound_names(text: &str) -> Vec<Bound> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut out: Vec<Bound> = BOUND
        .captures_iter(text)
        .filter_map(|m| {
            let at = m.get(0)?.start();
            let mut from = at.saturating_sub(16);
            while !text.is_char_boundary(from) {
                from -= 1;
            }
            Some(Bound {
                name: m[1].to_string(),
                exported: PUB_BEFORE.is_match(&text[from..at]),
            })
        })
        .collect();
    for (i, l) in lines.iter().enumerate() {
        if !FN_LINE.is_match(l) {
            continue;
        }
        let sig = format!("{l} {}", lines.get(i + 1).copied().unwrap_or(""));
        if let Some(m) = FN_NAME.captures(&sig)
            && code(&function_text(&lines, i)).contains("env!(\"CARGO_BIN_EXE_")
        {
            out.push(Bound {
                name: m[1].to_string(),
                exported: PUB_FN.is_match(l),
            });
        }
    }
    out
}

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

/// The spawn sites of one Rust file, in line order. `exported`: the names the other files of its crate export bound to a built
/// binary ([`bound_names`]).
pub fn spawn_sites(text: &str, path: &str, exported: &[String]) -> Vec<Site> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut starts = Vec::new();
    let mut off = 0;
    for l in text.split('\n') {
        starts.push(off);
        off += l.len() + 1;
    }
    let cfg = lines.iter().position(|l| is_cfg_test(l));
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
    let mut bound: Vec<String> = Vec::new();
    for b in bound_names(text).into_iter().map(|b| b.name).chain(exported.iter().cloned()) {
        if !bound.contains(&b) {
            bound.push(b);
        }
    }
    let shell = path.starts_with("tools/paddock-dev/");
    let dynamic;
    let judged: &Regex = if !bound.is_empty() {
        let base = if shell { format!("{ARG}|{SHELL}") } else { ARG.to_string() };
        dynamic = ctor(&format!("{base}|&?(?:{})\\b", bound.join("|")));
        &dynamic
    } else if shell {
        &CTOR_SHELL
    } else {
        &CTOR
    };
    for m in judged.captures_iter(text) {
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
        if code(&function_text(&lines, i)).contains("TestCommand") {
            continue;
        }
        out.push(Site {
            line: i + 1,
            what: m.as_str().to_string(),
            waived: waiver(i),
        });
    }
    for m in PATH_VAR.find_iter(text) {
        let i = line_of(&starts, m.start());
        if comment(i) || !in_test(i) || seen.contains(&i) {
            continue;
        }
        if !code(&function_text(&lines, i)).contains("TestCommand") {
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

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
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
        // A `pub` name bound to a built binary counts in every test file of its crate (the folder above `tests/` or `src/`).
        let mut exported: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        for f in files.iter().filter(|f| f.ends_with(".rs")) {
            let Some(text) = ctx.read(f) else { continue };
            if !TEST_PATH.is_match(f) && !text.split('\n').any(is_cfg_test) {
                continue;
            }
            let names = exported.entry(crate_of(f).to_string()).or_default();
            names.extend(bound_names(&text).into_iter().filter(|b| b.exported).map(|b| b.name));
        }
        let (mut findings, mut probes, mut completed) = (Vec::new(), 0, true);
        let mut any = false;
        for f in files.iter().filter(|f| f.ends_with(".rs")) {
            let Some(text) = ctx.read(f) else {
                if TEST_PATH.is_match(f) {
                    completed = false;
                }
                continue;
            };
            if !TEST_PATH.is_match(f) && !text.split('\n').any(is_cfg_test) {
                continue;
            }
            any = true;
            probes += 1;
            for s in spawn_sites(&text, f, exported.get(crate_of(f)).map_or(&[][..], Vec::as_slice)) {
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
        spawn_sites(text, path, &[]).iter().map(|s| s.line).collect()
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
    fn any_built_binary_a_bound_name_and_a_spliced_machine_path_are_sites() {
        let t = "crates/a/tests/it/x.rs";
        assert_eq!(
            lines("fn t() {\n    Command::new(env!(\"CARGO_BIN_EXE_heavy\")).output();\n}\n", t),
            vec![2]
        );
        let bound =
            "pub const BIN: &str = env!(\"CARGO_BIN_EXE_heavy\");\nfn t() {\n    let mut c = command(BIN);\n    Command::new(&BIN);\n}\n";
        assert_eq!(lines(bound, t), vec![3, 4]);
        let handed = "fn t() {\n    let s = SpawnSpec::new(env!(\"CARGO_BIN_EXE_child\"), \"x\");\n}\n";
        assert!(lines(handed, t).is_empty());
        let spliced = "fn t() {\n    let p = std::env::var(\"PATH\").unwrap();\n    TestCommand::new(\"x\").env(\"PATH\", p);\n}\n";
        assert_eq!(lines(spliced, t), vec![2]);
        assert!(lines("fn t() {\n    let p = std::env::var(\"PATH\").unwrap();\n    check(p);\n}\n", t).is_empty());
    }

    // Review of a8fca0ebd (P2 3): the gaps, each a probe that failed before the fix.
    #[test]
    fn a_function_returning_a_built_binary_binds_its_name() {
        let t = "fn child_program() -> PathBuf {\n    PathBuf::from(env!(\"CARGO_BIN_EXE_x\"))\n}\nfn t() {\n    Command::new(child_program());\n}\n";
        assert_eq!(lines(t, "crates/a/tests/it/x.rs"), vec![5]);
        let common = "pub fn child_program() -> PathBuf {\n    PathBuf::from(env!(\"CARGO_BIN_EXE_x\"))\n}\n";
        assert_eq!(
            bound_names(common),
            vec![Bound {
                name: "child_program".into(),
                exported: true
            }]
        );
        let user = "fn t() {\n    paddock_proc::command(child_program());\n}\n";
        assert_eq!(spawn_sites(user, "a/tests/it/e.rs", &["child_program".into()]).len(), 1);
        assert_eq!(crate_of("a/tests/it/common/mod.rs"), "a");
    }

    #[test]
    fn rustc_rustup_npm_npx_exe_spellings_and_a_joined_dev_are_judged() {
        let t = "fn t() {\n    Command::new(\"rustc\");\n    Command::new(\"rustup\");\n    Command::new(\"npm\");\n    Command::new(\"npx\");\n    Command::new(\"cargo.exe\");\n    Command::new(env!(\"RUSTC\"));\n    Command::new(std::env::var(\"RUSTC\").unwrap());\n    Command::new(root.join(\"dev\"));\n}\n";
        assert_eq!(lines(t, "crates/a/tests/it/x.rs"), vec![2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn the_builder_named_only_in_a_comment_does_not_count() {
        let t = "fn t() {\n    // TestCommand is not used here\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\"); // nor TestCommand\n    run(bin);\n}\n";
        assert_eq!(lines(t, "crates/a/tests/it/x.rs"), vec![3]);
        assert_eq!(code("a(\"//x\"); // TestCommand"), "a(\"//x\"); ");
    }

    #[test]
    fn any_cfg_naming_test_opens_the_test_part_and_not_test_does_not() {
        let t = "fn run() {\n    Command::new(\"cargo\");\n}\n#[cfg(all(test, unix))]\nmod tests {\n    fn t() {\n        Command::new(\"cargo\");\n    }\n}\n";
        assert_eq!(lines(t, "crates/a/src/lib.rs"), vec![7]);
        assert!(lines("#[cfg(not(test))]\nfn run() {\n    Command::new(\"cargo\");\n}\n", "crates/a/src/lib.rs").is_empty());
    }

    #[test]
    fn a_waiver_needs_a_reason() {
        let w = "fn t() {\n    // conformance: test-env — a probe of the shell\n    Command::new(\"cargo\");\n}\n";
        assert_eq!(spawn_sites(w, "a/tests/x.rs", &[])[0].waived.as_deref(), Some("a probe of the shell"));
        let bare = "fn t() {\n    // conformance: test-env\n    Command::new(\"cargo\");\n}\n";
        assert_eq!(spawn_sites(bare, "a/tests/x.rs", &[])[0].waived.as_deref(), Some(""));
    }
}
