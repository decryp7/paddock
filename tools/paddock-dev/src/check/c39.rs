//! C39: tests never free a port they probe (rule 40, ADR 0023). A port read from a listener that is then dropped is free
//! again: the OS may hand it to another process before the test connects, and the test then talks to that server. Flagged, in
//! any Rust file:
//!
//! - a chain that reads the port off a temporary listener: `TcpListener::bind(..).unwrap().local_addr()` (also `?`,
//!   `.expect(..)`, `.unwrap_or_else(..)`, `.map_err(..)`, `.context(..)`, `.with_context(..)`, `.await`; also under a `use ..
//!   as` alias, and a socket2 `Socket::new(..)` that is bound and dropped);
//! - a block (a `{ .. }` expression or a fn body) that binds `let l = ..bind(..)` and ends with `l.local_addr()..`, using `l`
//!   for nothing else: the listener drops when the block ends;
//! - `drop(l)` of a listener whose `local_addr()` was read before.
//!
//! The fix: `paddock_testkit::net::HeldPort` keeps the port taken. A justified case carries
//! `// conformance: freed-port — <reason>` on the line or the line above.
//!
//! The port of `conformance/src/checks/C39-freed-port.mjs`, finding for finding (the oracle test compares them).

use super::finding::Finding;
use super::jsre::{DOT, line_at, rx};
use super::mask::mask_rust;
use super::record::Precondition;
use super::{Check, CheckOutput, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The shape of a freed port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `bind(..).unwrap().local_addr()`.
    Chain,
    /// A block that returns the port of a listener it drops.
    Block,
    /// `drop(l)` after the port was read.
    Drop,
}

/// Whether a site carries the waiver comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waived {
    /// No comment.
    No,
    /// The comment without a reason.
    NoReason,
    /// The comment with a reason.
    Reason,
}

/// One freed port in a Rust source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreedPort {
    /// The 1-based line.
    pub line: usize,
    /// Its shape.
    pub kind: Kind,
    /// The waiver comment.
    pub waived: Waived,
}

static WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"conformance:\s*freed-port\s*[—–-]+\s*(\S{DOT}{{2,}})")));
static BARE_WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*freed-port"));
static ALIAS: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:TcpListener|TcpSocket)\s+as\s+(\w+)"));
static TAIL: LazyLock<Regex> =
    LazyLock::new(|| rx(r"^\s*(?:\?|\.\s*await\b|\.\s*unwrap\s*\(\s*\)|\.\s*(?:expect|unwrap_or_else|map_err|context|with_context)\s*\()"));
static LOCAL_ADDR_NEXT: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*\.\s*local_addr\s*\("));
static LET_NAME: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*let\s+(?:mut\s+)?(\w+)\s*(?::[^=]+)?="));
static SETUP_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*\.\s*(?:bind|set_\w+)\s*\("));
static BIND_PRE: LazyLock<Regex> = LazyLock::new(|| rx(r"::bind\s*\("));

fn bind_pattern(clean: &str) -> Regex {
    let mut names = vec!["TcpListener".to_string(), "TcpSocket".to_string()];
    names.extend(ALIAS.captures_iter(clean).map(|c| c[1].to_string()));
    rx(&format!(r"\b(?:(?:{})::bind|Socket::new)\s*\(", names.join("|")))
}

/// Byte index of the bracket closing the one at `open` in masked text.
fn closing(text: &str, open: usize) -> Option<usize> {
    let b = text.as_bytes();
    let o = b[open];
    let c = match o {
        b'(' => b')',
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };
    let mut depth = 0usize;
    for (i, &x) in b.iter().enumerate().skip(open) {
        if x == o {
            depth += 1;
        } else if x == c {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Byte index of the `{` of the innermost block that encloses `at`.
fn enclosing_block(text: &str, at: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut depth = 0usize;
    for i in (0..at).rev() {
        if b[i] == b'}' {
            depth += 1;
        } else if b[i] == b'{' {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// The end of a call's unwrap tail after `i`: `.unwrap()`, `.expect(..)`, `?`, `.await`, in any order.
fn skip_tail(text: &str, mut i: usize) -> usize {
    loop {
        let Some(m) = TAIL.find(&text[i..]) else { return i };
        i += m.end();
        let s = m.as_str();
        if s.ends_with('(') && !s.ends_with("()") {
            let Some(end) = closing(text, i - 1) else { return i };
            i = end + 1;
        }
    }
}

fn word_re(name: &str, rest: &str) -> Regex {
    rx(&format!(r"\b{}{rest}", regex::escape(name)))
}

/// The freed ports in a Rust source.
pub fn freed_ports(text: &str) -> Vec<FreedPort> {
    let clean = mask_rust(text);
    let raw: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut push = |i: usize, kind: Kind| {
        let line = line_at(&clean, i);
        let here = format!(
            "{}\n{}",
            raw.get(line - 1).copied().unwrap_or(""),
            if line >= 2 { raw.get(line - 2).copied().unwrap_or("") } else { "" }
        );
        let waived = if WAIVER.is_match(&here) {
            Waived::Reason
        } else if BARE_WAIVER.is_match(&here) {
            Waived::NoReason
        } else {
            Waived::No
        };
        out.push(FreedPort { line, kind, waived });
    };
    for m in bind_pattern(&clean).find_iter(&clean) {
        let open = m.end() - 1;
        let Some(close) = closing(&clean, open) else { continue };
        let after = skip_tail(&clean, close + 1);
        if LOCAL_ADDR_NEXT.is_match(&clean[after..]) {
            push(m.start(), Kind::Chain);
            continue;
        }
        // `let NAME = ...bind(..)` as a statement.
        let before = &clean[..m.start()];
        let stmt_start = [before.rfind(';'), before.rfind('{'), before.rfind('}')]
            .into_iter()
            .flatten()
            .max()
            .map_or(0, |p| p + 1);
        let Some(let_m) = LET_NAME.captures(&clean[stmt_start..m.start()]) else {
            continue;
        };
        let name = let_m[1].to_string();
        let Some(semi) = clean[after..].find(';').map(|p| p + after) else {
            continue;
        };
        let Some(block) = enclosing_block(&clean, m.start()) else {
            continue;
        };
        let Some(end) = closing(&clean, block) else { continue };
        let body = if semi < end { &clean[semi + 1..end] } else { "" };
        // Setting a socket up (socket2: `s.bind(..)`, `s.set_*(..)`) is not a use that keeps it.
        let name_re = word_re(&name, r"\b");
        let uses = name_re.find_iter(body).filter(|u| !SETUP_CALL.is_match(&body[u.end()..])).count();
        let port_reads = word_re(&name, r"\s*\.\s*local_addr\s*\(").find_iter(body).count();
        if let Some(drop_at) = word_re("drop", &format!(r"\s*\(\s*{}\s*\)", regex::escape(&name))).find(body) {
            // The port (or a value made from it) is used after the drop.
            let before_drop = &body[..drop_at.start()];
            let later = &body[drop_at.start()..];
            let derived_re = rx(&format!(
                r"\blet\s+(?:mut\s+)?(\w+)[^;=]*=[^;]*\b{}\s*\.\s*local_addr\s*\(",
                regex::escape(&name)
            ));
            let derived: Vec<String> = derived_re.captures_iter(before_drop).map(|c| c[1].to_string()).collect();
            let used_after = derived.iter().any(|d| {
                let redefine = rx(&format!(r"\blet\s+(?:mut\s+)?{}\b", regex::escape(d)));
                let upto = redefine.find(later).map_or(later, |r| &later[..r.start()]);
                rx(&format!(r"\b{}\b", regex::escape(d))).is_match(upto)
            });
            if used_after {
                push(semi + 1 + drop_at.start(), Kind::Drop);
            }
            continue;
        }
        let tail = &body[body.rfind(';').map_or(0, |p| p + 1)..];
        let tail_read = word_re(&name, r"\s*\.\s*local_addr\s*\(").is_match(tail);
        if tail_read && uses == port_reads {
            push(m.start(), Kind::Block);
        }
    }
    out
}

/// A snippet with a freed port and one without: the check's positive control (ER-33).
fn control() -> bool {
    let bad = "fn a() {\n    let p = TcpListener::bind(\"127.0.0.1:0\").unwrap().local_addr().unwrap().port();\n}\n";
    let good = "fn a() {\n    let held = HeldPort::reset();\n    get(held.port());\n}\n";
    freed_ports(bad).iter().map(|p| p.kind).collect::<Vec<_>>() == [Kind::Chain] && freed_ports(good).is_empty()
}

/// The check.
pub struct C39;

impl Check for C39 {
    fn id(&self) -> &'static str {
        "C39"
    }

    fn rule(&self) -> u32 {
        40
    }

    fn title(&self) -> &'static str {
        "Tests never free a port they probe"
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let fix = "Keep the port taken: `paddock_testkit::net::HeldPort::hang_up()` / `::reset()`; for \"refused\", inject a connector or test the error mapping (ADR 0023). A justified case carries `// conformance: freed-port — <reason>`.";
        let mut findings = Vec::new();
        let mut probes = 0;
        let mut completed = true;
        for f in ctx.files().iter().filter(|f| f.ends_with(".rs")) {
            let Some(text) = ctx.read(f) else {
                completed = false;
                continue;
            };
            if text.is_empty() {
                continue;
            }
            probes += 1;
            if !BIND_PRE.is_match(&text) {
                continue;
            }
            for p in freed_ports(&text) {
                let what = match p.kind {
                    Kind::Chain => "reads the port of a listener it drops at once",
                    Kind::Block => "returns the port of a listener the block drops",
                    Kind::Drop => "drops a listener whose port it read",
                };
                match p.waived {
                    Waived::Reason => {}
                    Waived::NoReason => findings.push(Finding::error(
                        "C39",
                        40,
                        f,
                        p.line,
                        "the freed-port waiver gives no reason",
                        "Write `// conformance: freed-port — <why the port may be freed>`.",
                    )),
                    Waived::No => findings.push(Finding::error(
                        "C39",
                        40,
                        f,
                        p.line,
                        format!("{what}: another process may be given the port before the test uses it"),
                        fix,
                    )),
                }
            }
        }
        CheckOutput {
            findings,
            probes,
            completed,
            control: control(),
            precondition: Precondition {
                met: !ctx.tree_files().is_empty(),
                detail: "the tree lists at least one file".into(),
            },
        }
    }
}
