//! C121: a test or a registry entry reads another part of the repository's declared data through its accessor, never by grepping or
//! slicing the file's text (rule 79, ADR 0098 WP-2; the instance that started it: bddd2947c, a registry entry that ran `sed` over
//! ci.yml for a line that train 7 had moved, while `ci_run::contract::conformance_args` already read it).
//!
//! Flagged, one line at a time:
//!
//! - in a registry (`docs/*.toml`): a line that names a workflow (`.gitea/workflows/`) and runs a text tool (`grep`, `egrep`, `fgrep`,
//!   `sed`, `awk`, `rg`, `perl`);
//! - in a test file (a `tests/` or `test/` folder, `*.test.*`, `tests.rs`, `*_test.rs`, `.test.sh`; Rust, JavaScript, TypeScript or
//!   shell): such a line, or a line that names a workflow and applies a regular expression to it (`.match(`, `.matchAll(`,
//!   `new RegExp(`, `Regex::new(`).
//!
//! Partly checked, said out loud: slicing another file's text by anchors and hard-coded paths into another crate (`11c365fc4`,
//! `0308201b4`) have no cheap low-noise lint; the rule, review and the train's test run cover them. A test of a check of workflows
//! may read them: each such site goes in `conformance/c121-allow.json` with its path, a `site` (a piece of the flagged line), a reason
//! and, for a line the file repeats, a `count`; the shared [`super::allow_list`] (also C124's) warns on a stale entry, reports a site
//! beyond an entry's count at its own line and never allows a whole file or folder.
//!
//! Not read: the check's own files ([`IMPLEMENTATION`], as C120's): their invented lines are test data, not reads.
//!
//! The shared reader owns the error paths: an allow file that cannot be read makes the check broken (one finding, nothing else),
//! and a file the check cannot read is not measured (an error finding at that file).
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here (ER-41).** Recorded: not yet. First count (2026-11-13): 10 findings, all the parity fixtures of C121 itself; since 2026-11-15 its own files are not read ([`IMPLEMENTATION`]), so they need no entry.
//!
//! Rust first (the rule's own host); `conformance/src/checks/C121-declared-data.mjs` is the port and `tests/it/check_c121.rs` compares
//! them finding for finding.

use super::allow_list::{self, Admit, Entry, Spec};
use super::finding::Finding;
use super::jsre::rx;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The allow list both hosts read.
pub const ALLOW: &str = "conformance/c121-allow.json";

/// The check's own files, as C120's: its sources and tests name the patterns (their invented lines are test data), and its allow list.
pub const IMPLEMENTATION: [&str; 5] = [
    "conformance/src/checks/C121-declared-data.mjs",
    "conformance/test/c121-declared-data.test.mjs",
    "tools/paddock-dev/src/check/c121.rs",
    "tools/paddock-dev/tests/it/check_c121.rs",
    ALLOW,
];

static TOOL: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:grep|egrep|fgrep|sed|awk|rg|perl)\b"));
static WORKFLOW: LazyLock<Regex> = LazyLock::new(|| rx(r#"\.gitea/workflows/|(?:^|[\s"'`/])workflows/[\w.-]+\.ya?ml"#));
static REGEX_USE: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(?:match|matchAll)\(|new RegExp\(|Regex::new\("));
static REGISTRY: LazyLock<Regex> = LazyLock::new(|| rx(r"^docs/[^/]+\.toml$"));
static TEST_PATH: LazyLock<Regex> =
    LazyLock::new(|| rx(r"(?:^|/)(?:tests?|__tests__)/|\.(?:test|spec)\.[cm]?[jt]s$|(?:^|/)tests\.rs$|_test\.rs$|\.test\.sh$"));
static TEST_EXT: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(?:rs|mjs|js|cjs|ts|sh)$"));
static NOT_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:fixtures|node_modules)/"));

/// How a file is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A registry of the repository (`docs/*.toml`).
    Registry,
    /// A test file.
    Test,
}

/// The kind of `path`, or `None` when the check does not read it.
pub fn kind_of(path: &str) -> Option<Kind> {
    if REGISTRY.is_match(path) {
        Some(Kind::Registry)
    } else if TEST_PATH.is_match(path) && TEST_EXT.is_match(path) && !NOT_TEST.is_match(path) {
        Some(Kind::Test)
    } else {
        None
    }
}

/// `(line, what, the line's text)` of every line of `text` that reads a workflow's text with a text tool or, in a test, a regular
/// expression.
pub fn flagged(kind: Kind, text: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        let t = line.trim();
        if t.starts_with('#') || t.starts_with("//") || !WORKFLOW.is_match(line) {
            continue;
        }
        if let Some(m) = TOOL.find(line) {
            out.push((i + 1, format!("`{}`", m.as_str()), line.to_string()));
        } else if kind == Kind::Test && REGEX_USE.is_match(line) {
            out.push((i + 1, "a regular expression".to_string(), line.to_string()));
        }
    }
    out
}

/// Reads the allow list: its entries, or why it cannot be read (the shared parser, [`super::allow_list`]).
pub fn parse_allow(text: &str) -> Result<Vec<Entry>, String> {
    allow_list::parse(text, ALLOW)
}

static SPEC: Spec = Spec {
    id: "C121",
    rule: 79,
    file: ALLOW,
    unseen: "a read of a workflow's text",
    restore: "Restore conformance/c121-allow.json: {\"allow\": [{\"path\": \"...\", \"site\": \"...\", \"reason\": \"...\"}]}.",
};

const FIX: &str = "Read the data through the one accessor that owns it (`./dev ci conformance-args`, `cargo metadata`, the register's parser, a constant the owner exports; rule 79). A test of a check of workflows may read them: list the site (its path, a piece of the line and a reason) in conformance/c121-allow.json.";

/// The check.
pub struct C121;

impl Check for C121 {
    fn id(&self) -> &'static str {
        "C121"
    }

    fn rule(&self) -> u32 {
        79
    }

    fn title(&self) -> &'static str {
        "Declared data is read through its accessor, not by searching a workflow's text"
    }

    fn scope(&self) -> super::CheckScope {
        super::CheckScope::Root
    }

    fn mode(&self) -> super::Mode {
        super::Mode::ReportOnly
    }

    fn allow_list(&self) -> Option<&'static allow_list::Spec> {
        Some(&SPEC)
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ALLOW,
                    "{\"allow\":[{\"path\":\"conformance/test/c.test.mjs\",\"site\":\"text.match(/x/)\",\"reason\":\"tests of checks of workflows\"}]}",
                ),
                (
                    "docs/x.toml",
                    "[[g]]\ncheck = [\"sh\", \"-c\", \"sed -n p .gitea/workflows/ci.yml\"]\n",
                ),
                (
                    "docs/y.toml",
                    "# grep .gitea/workflows/ci.yml in a comment\n[[g]]\ncheck = [\"./dev\", \"ci\", \"conformance-args\"]\n",
                ),
                (
                    "tools/t/tests/it/a.rs",
                    "fn a() {\n    let _ = Regex::new(\"x\"); // reads .gitea/workflows/ci.yml\n}\n",
                ),
                (
                    "tools/t/tests/it/b.rs",
                    "fn b() {\n    let ci = std::fs::read_to_string(\".gitea/workflows/ci.yml\");\n}\n",
                ),
                (
                    "conformance/test/c.test.mjs",
                    "const t = text.match(/x/); // .gitea/workflows/ci.yml\n",
                ),
                (
                    "tools/t/tests/fixtures/f.rs",
                    "Command::new(\"grep\"); // .gitea/workflows/ci.yml\n",
                ),
            ],
            expect: vec![("docs/x.toml", 2), ("tools/t/tests/it/a.rs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut allow = match allow_list::read(ctx, &SPEC, allow_list::entries_only(ALLOW)) {
            Ok((allow, ())) => allow,
            Err(broken) => return *broken,
        };
        let mut findings = Vec::new();
        for f in ctx.files().iter() {
            if IMPLEMENTATION.contains(&f.as_str()) {
                continue;
            }
            let Some(kind) = kind_of(f) else { continue };
            let text = match allow.read(ctx, f) {
                Ok(text) => text,
                Err(unread) => {
                    findings.push(*unread);
                    continue;
                }
            };
            for (line, what, at) in flagged(kind, &text) {
                let finding = match allow.admit(f, |site| at.contains(site)) {
                    Admit::Allowed => continue,
                    Admit::Beyond => Finding::warn(
                        "C121",
                        79,
                        f,
                        line,
                        format!("reads a workflow file's text with {what} beyond the count of its allow entry"),
                        format!("Read it through its accessor, or raise the entry's `count` in {ALLOW} if this site is meant too."),
                    ),
                    Admit::Unlisted => Finding::warn(
                        "C121",
                        79,
                        f,
                        line,
                        format!("reads a workflow file's text with {what}: the data it needs has one accessor"),
                        FIX,
                    ),
                };
                findings.push(finding);
            }
        }
        allow.output(ctx, findings)
    }
}
