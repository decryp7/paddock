//! C130: Rust test code does not plant a shell script as a fixture; it installs the testkit's stand-in (`paddock_testkit::StandIn`).
//! A fixture written as `#!/bin/sh` (or bash, zsh, through `env`) cannot run on Windows, so its test is `#[cfg(unix)]` or carries a
//! windows-twin waiver, and a hand-written script rarely prints what the real program prints (an npm stand-in that printed nothing hid a
//! `--json` bug). ER-21 class "a fixture program that exists only on Unix": train 12's planner control never ran on Windows
//! (42619e558), run 8138 lost grep and tr, WP-7 put windows-twin waivers on three unix-only test files. Share first (rule 42): one
//! stand-in binary, written once.
//!
//! Flagged, one finding per line, in Rust test code only (a file of a `tests/` folder, `tests.rs`, `*_test.rs`, or a file from its first
//! `#[cfg(test)]` / `#[cfg(all(test, ..))]` / `#[cfg(any(.., test))]` line on, which also covers a test-only item in the middle of a
//! file): a line that holds `#!/bin/sh`, `#!/bin/bash`, `#!/usr/bin/env bash` and the like. Comment lines (`//`) are not read.
//!
//! Not flagged, said out loud: a script built without a shebang (`sh -c`), a shebang another way (a byte string split over lines), and
//! shell files themselves (C129 flags those; this reads the Rust text C129 names as its known gap, so the two never report the same
//! line). A fixture that has to be a script (a hook or a wrapper script under test) carries `// conformance: stand-in — <why>` on its
//! line or the line above, and is counted: the allow list must name it (site `conformance: stand-in`, a count per file), so a new waiver is
//! growth of the list, which the shrink-only rule ([`super::allow_list`], against the merge base) puts to the user. Today's sites are the allow list `conformance/c130-allow.json` (path, site = the shebang text, reason,
//! count), read by the shared [`super::allow_list`]: a stale entry is a warning (remove it or lower its count), a site beyond an
//! entry's count is reported at its own line, an unreadable list makes the check broken. Convert a site to `StandIn` and delete its
//! entry. The check's own files are not read ([`IMPLEMENTATION`]).
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here (ER-41).** Recorded: pending (2026-11-19): the train integrator records the run id of the first green run on the real tree here, in c130.rs and in docs/gates.toml, and only then is `mode` switched to blocking.
//!
//! Rust host; `conformance/src/checks/C130-stand-in-fixtures.mjs` is the port and `tests/it/check_c130.rs` compares them.

use super::allow_list::{self, Admit, Entry, Spec};
use super::finding::Finding;
use super::jsre::{has_reason, rx};
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The allow list both hosts read.
pub const ALLOW: &str = "conformance/c130-allow.json";

/// The check's own files: they name the pattern, their invented lines are test data.
pub const IMPLEMENTATION: [&str; 5] = [
    "conformance/src/checks/C130-stand-in-fixtures.mjs",
    "conformance/test/c130-stand-in-fixtures.test.mjs",
    "tools/paddock-dev/src/check/c130.rs",
    "tools/paddock-dev/tests/it/check_c130.rs",
    ALLOW,
];

static SHEBANG: LazyLock<Regex> = LazyLock::new(|| rx(r"#!/(?:usr/)?bin/(?:env[ \t]+)?(?:ba|z)?sh\b"));
/// The marker up to its dashes; a reason must follow ([`has_reason`], JavaScript's `\S.{2,}`).
static WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*stand-in\s*([—–-]+)"));
static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)tests?/|(?:^|/)tests\.rs$|_tests?\.rs$"));
static NOT_READ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:target|vendor|fixtures|node_modules)/"));
static WRITES_PROGRAM: LazyLock<Regex> = LazyLock::new(|| rx(r"\bwrite_program\("));
static CFG_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"^[ \t]*#\[cfg\((?:test|(?:all|any)\([^\]]*\btest\b[^\]]*)\)\]"));

const WAIVER_FIX: &str = "Waivers are counted, so none is free: list it in conformance/c130-allow.json (path, site `conformance: stand-in`, a reason and the file's count), which the user approves, or convert the fixture to `StandIn`.";
const FIX: &str = "Install the testkit's stand-in instead: `paddock_testkit::StandIn::new(dir, \"name\").stdout(..).exit(..).records_args().install()?` (it runs on Windows and records its calls). A fixture that has to be a script carries `// conformance: stand-in — <why>`; an existing site is listed in conformance/c130-allow.json until it is converted.";

/// Whether `path` is a Rust file this check reads.
pub fn readable(path: &str) -> bool {
    path.ends_with(".rs") && !NOT_READ.is_match(path) && !IMPLEMENTATION.contains(&path)
}

/// The `site` an allow entry uses to name a waived fixture.
pub const WAIVED_SITE: &str = "conformance: stand-in";

/// `(line, the line's text, waived)` of every shebang line in the test code of `text` (the Rust file `path`); `waived` when a comment
/// with a reason is on it or above.
pub fn sites(path: &str, text: &str) -> Vec<(usize, String, bool)> {
    if !readable(path) {
        return Vec::new();
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let from = if TEST_PATH.is_match(path) || WRITES_PROGRAM.is_match(text) {
        0
    } else {
        match lines.iter().position(|l| CFG_TEST.is_match(l)) {
            Some(i) => i,
            None => return Vec::new(),
        }
    };
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(from) {
        if line.trim().starts_with("//") || !SHEBANG.is_match(line) {
            continue;
        }
        let above = i.checked_sub(1).and_then(|p| lines.get(p)).is_some_and(|l| has_reason(l, &WAIVER));
        out.push((i + 1, (*line).to_string(), has_reason(line, &WAIVER) || above));
    }
    out
}

/// Reads the allow list: its entries, or why it cannot be read (the shared parser, [`super::allow_list`]).
pub fn parse_allow(text: &str) -> Result<Vec<Entry>, String> {
    allow_list::parse(text, ALLOW)
}

static SPEC: Spec = Spec {
    id: "C130",
    rule: 42,
    file: ALLOW,
    unseen: "a shell fixture",
    restore: "Restore conformance/c130-allow.json: {\"allow\": [{\"path\": \"...\", \"site\": \"#!/bin/sh\", \"reason\": \"...\", \"count\": 1}]}.",
};

/// The check.
pub struct C130;

impl Check for C130 {
    fn id(&self) -> &'static str {
        "C130"
    }

    fn rule(&self) -> u32 {
        42
    }

    fn title(&self) -> &'static str {
        "Rust tests install the testkit stand-in, not a #!/bin/sh fixture"
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
                    "{\"allow\":[{\"path\":\"tools/t/tests/it/ok.rs\",\"site\":\"#!/bin/sh\",\"reason\":\"baseline: planted before StandIn\"}]}",
                ),
                ("tools/t/tests/it/ok.rs", "fn a() {\n    write(\"#!/bin/sh\\nexit 1\\n\");\n}\n"),
                ("tools/t/tests/it/bad.rs", "fn b() {}\nfn c() {\n    write(\"#!/usr/bin/env bash\\n\");\n}\n"),
                ("tools/t/tests/it/comment.rs", "// the old test wrote #!/bin/sh\nfn d() {}\n"),
                ("tools/t/src/lib.rs", "const SHIM: &str = \"#!/bin/sh\\n\";\n"),
            ],
            expect: vec![("tools/t/tests/it/bad.rs", 3)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut allow = match allow_list::read(ctx, &SPEC, allow_list::entries_only(ALLOW)) {
            Ok((allow, ())) => allow,
            Err(broken) => return *broken,
        };
        let mut findings = Vec::new();
        for f in ctx.files().iter() {
            if !readable(f) {
                continue;
            }
            let text = match allow.read(ctx, f) {
                Ok(text) => text,
                Err(unread) => {
                    findings.push(*unread);
                    continue;
                }
            };
            for (line, at, waived) in sites(f, &text) {
                let said = if waived {
                    allow.admit(f, |site| site == WAIVED_SITE)
                } else {
                    allow.admit(f, |site| at.contains(site))
                };
                if waived {
                    if said != Admit::Allowed {
                        let what = if said == Admit::Beyond { "a stand-in waiver beyond the count of its allow entry" } else { "a stand-in waiver the allow list does not name" };
                        findings.push(Finding::warn("C130", 42, f, line, what.to_string(), WAIVER_FIX));
                    }
                    continue;
                }
                let finding = match said {
                    Admit::Allowed => continue,
                    Admit::Beyond => Finding::warn(
                        "C130",
                        42,
                        f,
                        line,
                        "writes another shell fixture beyond the count of its allow entry".to_string(),
                        format!(
                            "Install the testkit's stand-in instead (`paddock_testkit::StandIn`), or raise the entry's `count` in {ALLOW} only for a site that has to stay a script."
                        ),
                    ),
                    Admit::Unlisted => Finding::warn(
                        "C130",
                        42,
                        f,
                        line,
                        "a test plants a shell script as a fixture program: it cannot run on Windows and does not say what the real program prints".to_string(),
                        FIX,
                    ),
                };
                findings.push(finding);
            }
        }
        allow.output(ctx, findings)
    }
}
