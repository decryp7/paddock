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
//! may read them: its path (a folder, ending in `/`, or a file) goes in `conformance/c121-allow.json` with a reason.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here (ER-41).** Recorded: not yet. First count (2026-11-13): 10 findings, all the parity fixtures of C121 itself, now allowed with a reason.
//!
//! Rust first (the rule's own host); `conformance/src/checks/C121-declared-data.mjs` is the port and the `c121` tests of `tests/it/check.rs`
//! compares them finding for finding.

use super::finding::Finding;
use super::jsre::rx;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The allow list both hosts read.
pub const ALLOW: &str = "conformance/c121-allow.json";

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

/// `(line, what)` of every line of `text` that reads a workflow's text with a text tool or, in a test, a regular expression.
pub fn flagged(kind: Kind, text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        let t = line.trim();
        if t.starts_with('#') || t.starts_with("//") || !WORKFLOW.is_match(line) {
            continue;
        }
        if let Some(m) = TOOL.find(line) {
            out.push((i + 1, format!("`{}`", m.as_str())));
        } else if kind == Kind::Test && REGEX_USE.is_match(line) {
            out.push((i + 1, "a regular expression".to_string()));
        }
    }
    out
}

/// One entry of the allow list.
#[derive(Debug, PartialEq, Eq)]
pub struct Allowed {
    /// A file, or a folder (ending in `/`).
    pub path: String,
    /// Why it may read workflows.
    pub reason: String,
}

/// Reads the allow list; an unreadable one, or an entry without a reason, is why the check cannot look.
pub fn parse_allow(text: &str) -> Result<Vec<Allowed>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{ALLOW} is not JSON: {e}"))?;
    let list = v["allow"].as_array().ok_or_else(|| format!("{ALLOW} has no `allow` array"))?;
    let mut out = Vec::new();
    for e in list {
        let (path, reason) = (e["path"].as_str().unwrap_or(""), e["reason"].as_str().unwrap_or("").trim());
        if path.is_empty() || reason.chars().count() < 8 {
            return Err(format!("{ALLOW}: every entry needs a path and a reason of at least 8 characters"));
        }
        out.push(Allowed {
            path: path.to_string(),
            reason: reason.to_string(),
        });
    }
    Ok(out)
}

fn allowed(allow: &[Allowed], path: &str) -> bool {
    allow.iter().any(|a| {
        if a.path.ends_with('/') {
            path.starts_with(&a.path)
        } else {
            path == a.path
        }
    })
}

const FIX: &str = "Read the data through the one accessor that owns it (`./dev ci conformance-args`, `cargo metadata`, the register's parser, a constant the owner exports; rule 79). A test of a check of workflows may read them: list its path with a reason in conformance/c121-allow.json.";

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

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ALLOW,
                    "{\"allow\":[{\"path\":\"conformance/test/\",\"reason\":\"tests of checks of workflows\"}]}",
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
        let allow = ctx
            .read(ALLOW)
            .map(|t| parse_allow(&t))
            .unwrap_or_else(|| Err(format!("{ALLOW} cannot be read")));
        let (allow, why) = match allow {
            Ok(a) => (a, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let mut findings = Vec::new();
        let mut probes = 0;
        let mut completed = true;
        for f in ctx.files().iter() {
            let Some(kind) = kind_of(f) else { continue };
            if allowed(&allow, f) {
                continue;
            }
            let Some(text) = ctx.read(f) else {
                completed = false;
                continue;
            };
            probes += 1;
            for (line, what) in flagged(kind, &text) {
                findings.push(Finding::warn(
                    "C121",
                    79,
                    f,
                    line,
                    format!("reads a workflow file's text with {what}: the data it needs has one accessor"),
                    FIX,
                ));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: why.is_none(),
                detail: why.unwrap_or_else(|| format!("{ALLOW} is read")),
            },
        }
    }
}
