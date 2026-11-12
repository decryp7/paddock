//! C53: nothing is ended by a bare pid (ADR 0033, rule 53). The OS hands a freed pid on (Windows at once), so a pid kept from a
//! snapshot, a file or a parent link can name another program by the time it is signalled. Every kill goes through
//! `paddock_proc` with a `ProcessId` or through a handle or job object this process owns. The main check is Clippy's
//! `disallowed-methods`; this one covers what Clippy cannot see, in any Rust file outside the one implementation
//! (`crates/paddock-proc/src/sys.rs`) and vendored third-party code:
//!
//! - programs that kill by pid, named in string literals: `taskkill`, `Stop-Process`, `/bin/kill`, a `kill` or `pkill` command;
//! - Win32 calls: `TerminateProcess(`, `TerminateJobObject(`;
//! - a blanket `#![allow(clippy::disallowed_methods)]` (also inside `#![cfg_attr(.., allow(..))]);
//! - open follow-ups: a comment or `#[expect]` reason naming `ADR 0033 WP-<n>`; each needs a dated waiver in
//!   `paddock.waivers.toml`.
//!
//! A justified site carries `// conformance: pid-kill — <reason>` on the line or the line above.
//!
//! The port of `conformance/src/checks/C53-pid-kills.mjs`, finding for finding (the oracle test compares them). Positions are
//! UTF-16 offsets, as in the JavaScript, so a hit orders the same way next to non-ASCII text.

use super::finding::Finding;
use super::jsre::{has_reason, line_at, rx, units};
use super::mask::mask_units;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The one place that may signal a pid: it does so right after the identity check.
const IMPLEMENTATION: [&str; 1] = ["crates/paddock-proc/src/sys.rs"];

/// The marker up to its dashes; a reason must follow ([`has_reason`]).
static WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*pid-kill\s*([—–-]+)"));
static BARE_WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*pid-kill"));
static VENDORED: LazyLock<Regex> = LazyLock::new(|| rx(r"^apps/[^/]+/vendor/"));
static PREFILTER: LazyLock<Regex> =
    LazyLock::new(|| rx(r"kill|Stop-Process|Terminate(?:Process|JobObject)|ADR 0033 WP-|disallowed_methods")); // conformance: pid-kill — a pattern that finds these words in source text; nothing here ends a process
static BLANKET: LazyLock<Regex> = LazyLock::new(|| rx(r"#!\[(?:cfg_attr\([^\]]*?)?allow\([^\]]*\bclippy::disallowed_methods\b"));
static FOLLOW_UP: LazyLock<Regex> = LazyLock::new(|| rx(r"ADR 0033 (WP-\d+)"));
static WIN32: LazyLock<[(Regex, &str); 2]> = LazyLock::new(|| {
    [
        (rx(r"\bTerminateProcess\s*\("), "ends a process with `TerminateProcess`"),
        (rx(r"\bTerminateJobObject\s*\("), "ends a job with `TerminateJobObject`"),
    ]
});
/// Programs that kill by pid, matched on the unmasked text, since their names are string literals. The flag: the quote to check
/// is the match's start (false) or its last `"` (true).
static PROGRAMS: LazyLock<[(Regex, bool); 4]> = LazyLock::new(|| {
    [
        (rx(r#""(taskkill)(?:\.exe)?""#), false),
        // conformance: pid-kill — a pattern that finds `Stop-Process` in source text; this file ends no process
        (rx(r#""[^"\n]*\b(Stop-Process)\b[^"\n]*""#), false),
        (rx(r#""(/(?:usr/)?bin/kill)""#), false),
        (rx(r#"\b(?:command|Command::new)\s*\(\s*"(kill|pkill)""#), true),
    ]
});

/// One pid kill in a Rust source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PidKill {
    /// The 1-based line.
    pub line: usize,
    /// What it does, in a sentence.
    pub what: String,
    /// The waiver comment: `Some(true)` with a reason, `Some(false)` without one, `None` for no comment (a follow-up marker never has one).
    pub waived: Option<bool>,
}

struct Hit {
    index: usize,
    line: usize,
    what: String,
    follow_up: bool,
}

/// The pid kills in one Rust source.
pub fn pid_kills(text: &str) -> Vec<PidKill> {
    let mask_u = mask_units(&text.encode_utf16().collect::<Vec<u16>>());
    let masked = String::from_utf16_lossy(&mask_u);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut hits: Vec<Hit> = Vec::new();
    for (re, what) in WIN32.iter() {
        for m in re.find_iter(&masked) {
            hits.push(Hit {
                index: units(&masked[..m.start()]),
                line: line_at(&masked, m.start()),
                what: (*what).into(),
                follow_up: false,
            });
        }
    }
    for (re, last_quote) in PROGRAMS.iter() {
        for m in re.captures_iter(text) {
            let whole = m.get(0).map_or(0..0, |w| w.range());
            // Only a literal in code: in a comment or inside another string the mask has blanked the quote.
            let q = if *last_quote {
                whole.start + text[whole.clone()].rfind('"').unwrap_or(0)
            } else {
                whole.start
            };
            if mask_u.get(units(&text[..q])) == Some(&(b'"' as u16)) {
                hits.push(Hit {
                    index: units(&text[..whole.start]),
                    line: line_at(text, whole.start),
                    what: format!("runs `{}`, which ends processes by pid", &m[1]),
                    follow_up: false,
                });
            }
        }
    }
    for m in BLANKET.find_iter(&masked) {
        hits.push(Hit {
            index: units(&masked[..m.start()]),
            line: line_at(&masked, m.start()),
            what: "allows `clippy::disallowed_methods` for a whole crate or module, which hides bare-pid kills from Clippy".into(),
            follow_up: false,
        });
    }
    for m in FOLLOW_UP.captures_iter(text) {
        let start = m.get(0).map_or(0, |w| w.start());
        hits.push(Hit {
            index: units(&text[..start]),
            line: line_at(text, start),
            what: format!("an open ADR 0033 follow-up ({}): a known bare-pid kill or parent-pid walk", &m[1]),
            follow_up: true,
        });
    }
    hits.sort_by_key(|h| h.index);
    hits.into_iter()
        .map(|h| {
            let near = format!(
                "{}\n{}",
                lines.get(h.line - 1).copied().unwrap_or(""),
                if h.line >= 2 {
                    lines.get(h.line - 2).copied().unwrap_or("")
                } else {
                    ""
                }
            );
            // A follow-up marker is waived only in paddock.waivers.toml (dated), never inline.
            let waived = if h.follow_up {
                None
            } else if has_reason(&near, &WAIVER) {
                Some(true)
            } else if BARE_WAIVER.is_match(&near) {
                Some(false)
            } else {
                None
            };
            PidKill {
                line: h.line,
                what: h.what,
                waived,
            }
        })
        .collect()
}

/// The check.
pub struct C53;

impl Check for C53 {
    fn id(&self) -> &'static str {
        "C53"
    }

    fn rule(&self) -> u32 {
        53
    }

    fn title(&self) -> &'static str {
        "Nothing is ended by a bare pid"
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    "crates/c/src/bad.rs",
                    "fn a(h: HANDLE) {\n    unsafe { TerminateProcess(h, 1) };\n}\n",
                ),
                (
                    "crates/c/src/good.rs",
                    "fn a(c: &mut Child) {\n    let _ = c.kill();\n}\n// \"taskkill\"\n",
                ),
                (
                    "crates/paddock-proc/src/sys.rs",
                    "fn a(h: HANDLE) {\n    unsafe { TerminateProcess(h, 1) };\n}\n",
                ),
                (
                    "apps/x/vendor/d/src/w.rs",
                    "fn a(h: HANDLE) {\n    unsafe { TerminateProcess(h, 1) };\n}\n",
                ),
            ],
            expect: vec![("crates/c/src/bad.rs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let fix = "End it through `paddock_proc` with a `ProcessId` (`kill_tree`, `end`, `terminate`; `Supervised` for a child), or through a handle or job object this process owns (ADR 0033). A justified case carries `// conformance: pid-kill — <reason>`.";
        let mut findings = Vec::new();
        let mut probes = 0;
        let mut completed = true;
        for f in ctx.files().iter() {
            if !f.ends_with(".rs") || IMPLEMENTATION.contains(&f.as_str()) || VENDORED.is_match(f) {
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
            for k in pid_kills(&text) {
                match k.waived {
                    Some(true) => {}
                    Some(false) => findings.push(Finding::error(
                        "C53",
                        53,
                        f,
                        k.line,
                        "the pid-kill waiver gives no reason",
                        "Write `// conformance: pid-kill — <why this pid cannot have been handed on>`.",
                    )),
                    None if k.what.starts_with("an open") || k.what.starts_with("allows") => findings.push(Finding::error(
                        "C53",
                        53,
                        f,
                        k.line,
                        k.what,
                        "Fix the site through paddock_proc (ADR 0033), or give it a dated waiver in paddock.waivers.toml.",
                    )),
                    None => findings.push(Finding::error(
                        "C53",
                        53,
                        f,
                        k.line,
                        format!("{}: the pid may belong to another process by now", k.what),
                        fix,
                    )),
                }
            }
        }
        findings.extend(super::c53_liveness::liveness_findings(ctx, ctx.root()));
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: ctx.tree_files().iter().any(|f| f.ends_with(".rs")),
                detail: "the tree has a .rs file".into(),
            },
        }
    }
}
