//! C159: a file that ends processes by a bare pid has no file-wide `allow` of `clippy::disallowed_methods` (ADR 0033, rule 53; ER-21
//! class "a blanket lint allow hides the very calls the lint bans": Pitcrew's supervision test harness had
//! `#![allow(.., clippy::disallowed_methods, ..)]` over a `sysinfo` `Process::kill` sweep, and killed other jobs' processes in run 8674).
//!
//! `lint/clippy.toml` bans `libc::kill`, `libc::killpg`, `nix::sys::signal::kill`, `nix::sys::signal::killpg`, `sysinfo::Process::kill`
//! and `sysinfo::Process::kill_with`. Clippy sees them by type, so a blanket allow hides every one of them in its scope, and C53's
//! dated waiver could cover the blanket. A justified site carries its own `#[expect(clippy::disallowed_methods, reason = "...")]`
//! (it fails when the site goes away) and nothing wider.
//!
//! Read: every `*.rs` outside `node_modules`, `target`, fixtures and vendored code, and this check's own files. A file **calls a banned
//! kill** when its code (comments and string contents masked) has `libc::kill(`, `libc::killpg(`, `signal::kill(`, `killpg(`,
//! `TerminateProcess(` or `TerminateJobObject(`, runs `taskkill`, `Stop-Process`, `/bin/kill` or `kill`/`pkill` (C53's reading), or
//! uses `sysinfo` to find a process (`.process(`, `.processes(`) and calls `.kill()` or `.kill_with(` in the same file. Such a file is
//! flagged once for every attribute that allows the lint beyond one site: an inner `#![allow(..)]`, `#![expect(..)]` or
//! `#![cfg_attr(.., allow(..))]` naming `clippy::disallowed_methods`, or an outer `#[allow(..)]` / `#[cfg_attr(.., allow(..))]` naming it.
//! An outer `#[expect(.., reason = ..)]` is the allowed form. The finding is unwaivable (a `paddock.waivers.toml` entry does not
//! cover it): fix the file.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here and in docs/gates.toml (ER-41).** Recorded:
//! not yet (2026-11-23).

use super::c53;
use super::finding::Finding;
use super::jsre::rx;
use super::mask::mask_rust;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

/// The check's own files: they name the patterns.
pub const IMPLEMENTATION: [&str; 2] = ["tools/paddock-dev/src/check/c159.rs", "tools/paddock-dev/tests/it/check_c159.rs"];

static NOT_READ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:node_modules|target|fixtures|vendor)/"));
static TYPED_KILL: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:libc::|signal::)?kill(?:pg)?\s*\(\s*[A-Za-z_]"));
static SYSINFO: LazyLock<Regex> = LazyLock::new(|| rx(r"\bsysinfo\b"));
static FINDS_PROCESS: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(?:process|processes|processes_by_name|processes_by_exact_name)\s*\("));
static METHOD_KILL: LazyLock<Regex> = LazyLock::new(|| rx(r"\.kill(?:_with)?\s*\("));
/// An attribute that allows (or expects) the lint: from `#` to the closing `]`, over lines.
static ATTR: LazyLock<Regex> = LazyLock::new(|| rx(r"#!?\[[^\]]*\bclippy::disallowed_methods\b[^\]]*\]"));

const FIX: &str = "Take the blanket off: put `#[expect(clippy::disallowed_methods, reason = \"...\")]` on each site that really needs the call, and end other processes through paddock_proc with a ProcessId (ADR 0033).";

/// Whether `path` is a file this check reads.
pub fn readable(path: &str) -> bool {
    path.ends_with(".rs") && !NOT_READ.is_match(path) && !IMPLEMENTATION.contains(&path)
}

/// Why `text` (a Rust source) calls a banned kill, in a few words, or `None`.
pub fn calls_a_banned_kill(text: &str) -> Option<String> {
    if let Some(k) = c53::pid_kills(text)
        .into_iter()
        .find(|k| !k.what.starts_with("an open") && !k.what.starts_with("allows"))
    {
        return Some(k.what);
    }
    let masked = mask_rust(text);
    if let Some(m) = TYPED_KILL.find(&masked) {
        return Some(format!(
            "calls `{}`, which signals a bare pid",
            m.as_str().split('(').next().unwrap_or("").trim()
        ));
    }
    if SYSINFO.is_match(&masked) && FINDS_PROCESS.is_match(&masked) && METHOD_KILL.is_match(&masked) {
        return Some("finds a process with `sysinfo` and calls `.kill()` on it".to_string());
    }
    None
}

/// `(line, the attribute's first line)` of every attribute in `text` that allows the lint beyond one site: any inner attribute naming
/// it, and an outer one unless it is `expect`.
pub fn wide_allows(text: &str) -> Vec<(usize, String)> {
    let masked = mask_rust(text);
    let mut out = Vec::new();
    for m in ATTR.find_iter(&masked) {
        let attr = m.as_str();
        // The allowed form is an outer `expect` (also inside `cfg_attr`): it fails when the site goes away and covers one item.
        let one_line = attr.split_whitespace().collect::<Vec<_>>().join(" ");
        if !one_line.starts_with("#!") && one_line.contains("expect(") && !one_line.contains("allow(") {
            continue;
        }
        let line = masked[..m.start()].bytes().filter(|b| *b == b'\n').count() + 1;
        out.push((line, one_line));
    }
    out
}

/// The check.
pub struct C159;

impl Check for C159 {
    fn id(&self) -> &'static str {
        "C159"
    }

    fn rule(&self) -> u32 {
        53
    }

    fn title(&self) -> &'static str {
        "a file that ends processes by a bare pid has no file-wide allow of clippy::disallowed_methods"
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
                    "apps/a/tests/sweep.rs",
                    "#![allow(clippy::unwrap_used, clippy::disallowed_methods)]\nuse sysinfo::System;\nfn k(s: &System, p: Pid) {\n    s.process(p).unwrap().kill();\n}\n",
                ),
                (
                    "apps/a/tests/sig.rs",
                    "fn k(p: i32) {\n    #[allow(clippy::disallowed_methods)]\n    unsafe { libc::kill(p, 9) };\n}\n",
                ),
                (
                    "apps/a/tests/site.rs",
                    "fn k(p: i32) {\n    #[expect(clippy::disallowed_methods, reason = \"a stand-in's own pid\")]\n    unsafe { libc::kill(p, 9) };\n}\n",
                ),
                (
                    "apps/a/tests/no_kill.rs",
                    "#![allow(clippy::disallowed_methods)]\nfn f() { std::process::Command::new(\"ls\"); }\n",
                ),
                (
                    "apps/a/tests/comment.rs",
                    "// #![allow(clippy::disallowed_methods)]\n// libc::kill(p, 9);\n",
                ),
                (
                    "apps/a/vendor/d/src/w.rs",
                    "#![allow(clippy::disallowed_methods)]\nfn a(p: i32) { libc::kill(p, 9); }\n",
                ),
            ],
            expect: vec![("apps/a/tests/sig.rs", 2), ("apps/a/tests/sweep.rs", 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| readable(f)) {
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            if !text.contains("disallowed_methods") {
                continue;
            }
            let wide = wide_allows(&text);
            if wide.is_empty() {
                continue;
            }
            let Some(why) = calls_a_banned_kill(&text) else { continue };
            for (line, attr) in wide {
                findings.push(
                    Finding::warn(
                        "C159",
                        53,
                        f,
                        line,
                        format!("`{attr}` allows `clippy::disallowed_methods` beyond one site in a file that {why}: Clippy cannot see a bare-pid kill here"),
                        FIX,
                    )
                    .unwaivable(),
                );
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

    fn flagged(text: &str) -> bool {
        !wide_allows(text).is_empty() && calls_a_banned_kill(text).is_some()
    }

    #[test]
    fn a_blanket_inner_allow_in_a_file_that_kills_a_sysinfo_process_is_flagged() {
        let t = "#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods, missing_docs)]\nuse sysinfo::System;\nfn k(s: &System) { s.process(p).unwrap().kill(); }\n";
        assert!(flagged(t));
        assert_eq!(wide_allows(t)[0].0, 1);
    }

    #[test]
    fn an_outer_allow_and_a_cfg_attr_allow_are_flagged_too_but_an_outer_expect_is_the_allowed_form() {
        let kill = "fn k(p: i32) { unsafe { libc::kill(p, 9) }; }\n";
        assert!(flagged(&format!("#[allow(clippy::disallowed_methods)]\n{kill}")));
        assert!(flagged(&format!("#![cfg_attr(test, allow(clippy::disallowed_methods))]\n{kill}")));
        assert!(flagged(&format!("#![expect(clippy::disallowed_methods, reason = \"x\")]\n{kill}")));
        assert!(!flagged(&format!("#[expect(clippy::disallowed_methods, reason = \"x\")]\n{kill}")));
        assert!(!flagged(&format!(
            "#[cfg_attr(\n    unix,\n    expect(clippy::disallowed_methods, reason = \"x\")\n)]\n{kill}"
        )));
        assert!(flagged(&format!(
            "#[allow(\n    clippy::unwrap_used,\n    clippy::disallowed_methods,\n)]\n{kill}"
        )));
    }

    #[test]
    fn a_file_that_ends_nothing_by_pid_may_allow_the_lint() {
        assert!(!flagged(
            "#![allow(clippy::disallowed_methods)]\nfn f() { Command::new(\"ls\"); }\n"
        ));
        // `.kill()` on a std child is not a bare-pid kill: sysinfo is not looking up a process.
        assert!(!flagged(
            "#![allow(clippy::disallowed_methods)]\nfn f(c: &mut Child) { c.kill().ok(); }\n"
        ));
    }

    #[test]
    fn win32_and_program_kills_count_as_calls_and_a_comment_or_string_does_not() {
        assert!(flagged(
            "#![allow(clippy::disallowed_methods)]\nfn a(h: HANDLE) { unsafe { TerminateProcess(h, 1) }; }\n"
        ));
        assert!(flagged(
            "#![allow(clippy::disallowed_methods)]\nfn a() { Command::new(\"taskkill\"); }\n"
        ));
        assert!(!flagged(
            "#![allow(clippy::disallowed_methods)]\n// libc::kill(p, 9);\nconst S: &str = \"libc::kill(p, 9)\";\n"
        ));
        assert!(wide_allows("// #![allow(clippy::disallowed_methods)]\n").is_empty());
    }

    #[test]
    fn this_check_and_vendored_or_fixture_files_are_not_read() {
        assert!(readable("apps/pitcrew/crates/pitcrew-service/tests/supervision.rs"));
        assert!(!readable("tools/paddock-dev/src/check/c159.rs"));
        assert!(!readable("apps/dyno/vendor/tray-icon/src/lib.rs"));
        assert!(!readable("tools/paddock-dev/tests/fixtures/check/cases/a.rs"));
    }
}
