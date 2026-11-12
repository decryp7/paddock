//! C53, liveness half: asking whether a RAW pid is alive has the same reuse problem as ending it (ADR 0033, rule 53), in tests as
//! much as in product code. The port of `pidLiveness` and `livenessFindings` in `conformance/src/checks/C53-pid-kills.mjs`, finding
//! for finding (the oracle tests compare them). Ask a `paddock_proc::ProcessId` instead. Existing sites sit in
//! `conformance/baselines/C53.json` (file and hash of the normalised line; it may only shrink); a new justified one carries
//! `// conformance: pid-liveness — <reason>` on the line or the line above.

use super::Ctx;
use super::finding::Finding;
use super::jsre::{has_reason, is_js_space, line_at, rx, units};
use super::mask::mask_units;
use paddock_proc::{Limits, command, run_bounded};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

/// The baseline of the known sites.
pub const LIVENESS_BASELINE: &str = "conformance/baselines/C53.json";
const NAMES: [&str; 3] = ["is_alive", "may_be_alive", "presence"];
const IMPLEMENTATION: [&str; 1] = ["crates/paddock-proc/src/sys.rs"];

static WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*pid-liveness\s*([—–-]+)"));
static BARE_WAIVER: LazyLock<Regex> = LazyLock::new(|| rx(r"conformance:\s*pid-liveness"));
static VENDORED: LazyLock<Regex> = LazyLock::new(|| rx(r"^apps/[^/]+/vendor/"));
static PREFILTER: LazyLock<Regex> = LazyLock::new(|| rx(r"is_alive|may_be_alive|presence|kill|OpenProcess|Pid::from")); // conformance: pid-kill — a pattern that finds these words in source text; nothing here ends a process
static USE: LazyLock<Regex> = LazyLock::new(|| rx(r"\buse\s+paddock_proc::(?:\{([^}]*)\}|(\*|\w+(?:\s+as\s+\w+)?))"));
static QUALIFIED: LazyLock<Regex> = LazyLock::new(|| rx(r"\bpaddock_proc::(is_alive|may_be_alive|presence)\s*\("));
static CRATE: LazyLock<Regex> = LazyLock::new(|| rx(r"\bcrate::(is_alive|may_be_alive|presence)\s*\("));
// conformance: pid-kill — a pattern that finds `kill(pid, 0)` in source text; this file ends no process
static KILL0: LazyLock<Regex> = LazyLock::new(|| rx(r"\bkill\s*\([^;\n]*?,\s*(?:0|None)\s*\)"));
static OPEN: LazyLock<Regex> = LazyLock::new(|| rx(r"\bOpenProcess\s*\("));
static PID_FROM: LazyLock<Regex> = LazyLock::new(|| rx(r"\bPid::from(?:_u32|_usize)?\s*\("));
static OWN_PID: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:std::)?process::id\(\)"));
static AS: LazyLock<Regex> = LazyLock::new(|| rx(r"\s+as\s+"));

/// One raw-pid liveness check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Liveness {
    /// The 1-based line.
    pub line: usize,
    /// What it does, in a sentence.
    pub what: String,
    /// The waiver: `Some(true)` with a reason, `Some(false)` without one, `None`.
    pub waived: Option<bool>,
    /// The text of the line.
    pub text: String,
}

struct Hit {
    index: usize,
    what: String,
}

/// The last `n` characters of `s`.
fn tail(s: &str, n: usize) -> &str {
    let start = s.char_indices().rev().nth(n.saturating_sub(1)).map_or(0, |(i, _)| i);
    &s[start..]
}

/// Whether `before` ends in `fn` and white space (a definition, not a call).
fn after_fn(before: &str) -> bool {
    let t = before.trim_end_matches(is_js_space);
    t.len() < before.len() && t.ends_with("fn") && !t[..t.len() - 2].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
}

/// The raw-pid liveness checks in one Rust source; `file` decides what counts (see the JavaScript).
pub fn pid_liveness(text: &str, file: &str) -> Vec<Liveness> {
    if IMPLEMENTATION.contains(&file) {
        return Vec::new();
    }
    let mask_u = mask_units(&text.encode_utf16().collect::<Vec<u16>>());
    let masked = String::from_utf16_lossy(&mask_u);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut hits: Vec<Hit> = Vec::new();
    let at = |start: usize| units(&masked[..start]);
    for m in QUALIFIED.captures_iter(&masked) {
        let w = m.get(0).map_or(0, |w| w.start());
        hits.push(Hit {
            index: at(w),
            what: format!("asks `paddock_proc::{}` about a raw pid", &m[1]),
        });
    }
    let in_proc = file.starts_with("crates/paddock-proc/");
    if in_proc {
        for m in CRATE.captures_iter(&masked) {
            let w = m.get(0).map_or(0, |w| w.start());
            hits.push(Hit {
                index: at(w),
                what: format!("asks `{}` about a raw pid", &m[1]),
            });
        }
    }
    // Names this file imports from paddock_proc: `use paddock_proc::{a, b as c}`, `use paddock_proc::a as c`, `use paddock_proc::*`.
    let mut imported: Vec<String> = Vec::new();
    let mut add_import = |n: String| {
        if !imported.contains(&n) {
            imported.push(n);
        }
    };
    for m in USE.captures_iter(&masked) {
        let list = m.get(1).or_else(|| m.get(2)).map_or("", |g| g.as_str());
        for item in list.split(',').map(|n| n.trim_matches(is_js_space)) {
            if item == "*" {
                NAMES.iter().for_each(|n| add_import((*n).to_string()));
            } else {
                let mut parts = AS.splitn(item, 2);
                let name = parts.next().unwrap_or("");
                let alias = parts.next();
                if NAMES.contains(&name) {
                    add_import(alias.unwrap_or(name).to_string());
                }
            }
        }
    }
    let mut names: Vec<String> = NAMES.iter().map(|n| (*n).to_string()).collect();
    for n in &imported {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    let bare = rx(&format!(
        r"({})\s*\(",
        names.iter().map(|n| regex::escape(n)).collect::<Vec<_>>().join("|")
    ));
    for m in bare.captures_iter(&masked) {
        let w = m.get(0).map_or(0, |w| w.start());
        let name = &m[1];
        let before = &masked[..w];
        if before.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':') {
            continue;
        }
        if (in_proc || imported.iter().any(|i| i == name)) && !after_fn(tail(before, 12)) {
            hits.push(Hit {
                index: at(w),
                what: format!("asks `{name}` about a raw pid"),
            });
        }
    }
    // Only `kill(pid, 0)` and nix's `kill(pid, None)` are liveness; a real signal is the kill check's.
    for m in KILL0.find_iter(&masked) {
        let two = tail(&masked[..m.start()], 2);
        let method = two.ends_with('.') || (two.chars().count() == 2 && two.starts_with('.') && two.ends_with(is_js_space));
        if !method {
            hits.push(Hit {
                index: at(m.start()),
                what: "checks a raw pid with `kill(pid, 0)`".into(),
            });
        }
    }
    for m in OPEN.find_iter(&masked) {
        hits.push(Hit {
            index: at(m.start()),
            what: "opens a process by its raw pid with `OpenProcess`".into(),
        });
    }
    for m in PID_FROM.find_iter(&masked) {
        if !OWN_PID.is_match(&masked[m.end()..]) {
            hits.push(Hit {
                index: at(m.start()),
                what: "looks a raw pid up in sysinfo".into(),
            });
        }
    }
    hits.sort_by_key(|h| h.index);
    hits.into_iter()
        .map(|h| {
            let line = line_at(&masked, masked_byte(&masked, h.index));
            let own = lines.get(line - 1).copied().unwrap_or("");
            let near = format!(
                "{own}\n{}",
                if line >= 2 {
                    lines.get(line - 2).copied().unwrap_or("")
                } else {
                    ""
                }
            );
            let waived = if has_reason(&near, &WAIVER) {
                Some(true)
            } else if BARE_WAIVER.is_match(&near) {
                Some(false)
            } else {
                None
            };
            Liveness {
                line,
                what: h.what,
                waived,
                text: own.to_string(),
            }
        })
        .collect()
}

/// The byte offset of UTF-16 offset `index` in `s`.
fn masked_byte(s: &str, index: usize) -> usize {
    let mut u = 0;
    for (b, c) in s.char_indices() {
        if u >= index {
            return b;
        }
        u += c.len_utf16();
    }
    s.len()
}

/// `lineHash`: the first 16 hex digits of the SHA-256 of the line with white space collapsed and trimmed.
pub fn line_hash(text: &str) -> String {
    let normal = text.split(is_js_space).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normal.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

struct Site {
    file: String,
    line: usize,
    what: String,
    hash: String,
}

struct Entry {
    file: String,
    line: Option<usize>,
    hash: String,
}

fn entries_of(raw: &str) -> Vec<Entry> {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    v.get("sites")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|e| Entry {
                    file: e.get("file").and_then(Value::as_str).unwrap_or("").to_string(),
                    line: e.get("line").and_then(Value::as_u64).map(|n| n as usize),
                    hash: e.get("hash").and_then(Value::as_str).unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

type Key = (String, String);

fn multiset<'a>(it: impl Iterator<Item = (&'a str, &'a str)>) -> HashMap<Key, usize> {
    let mut m = HashMap::new();
    for (f, h) in it {
        *m.entry((f.to_string(), h.to_string())).or_insert(0) += 1;
    }
    m
}

/// `git args` in `root`: standard output, or the error line the JavaScript reports (`Command failed: git ...`) and the standard error.
fn git(root: &Path, args: &[&str]) -> Result<String, (String, String)> {
    let mut cmd = command("git");
    cmd.arg("-C").arg(root).args(args);
    let failed = |e: String| (format!("Command failed: git {}", args.join(" ")), e);
    match run_bounded(cmd, Limits::new(Duration::from_secs(120)).max_output(1 << 26)) {
        Ok(out) if out.success() => Ok(out.stdout_text()),
        Ok(out) => Err(failed(out.stderr_text())),
        Err(e) => Err(failed(e.to_string())),
    }
}

/// The entries the baseline at the merge base lacks, or the reason it cannot be measured (`baselineGrowth`).
enum Growth {
    Added { base: String, added: usize },
    Reason(String),
}

fn growth(root: &Path, current: &[Entry]) -> Growth {
    let base_ref = std::env::var("PADDOCK_BASE_REF").unwrap_or_else(|_| "origin/main".into());
    let lookup = || -> Result<String, (String, String)> {
        let head = git(root, &["rev-parse", "HEAD"])?.trim_matches(is_js_space).to_string();
        let mut base = git(root, &["merge-base", "HEAD", &base_ref])?.trim_matches(is_js_space).to_string();
        if base == head {
            base = git(root, &["rev-parse", "HEAD^1"])?.trim_matches(is_js_space).to_string();
        }
        Ok(base)
    };
    let base = match lookup() {
        Ok(b) => b,
        Err((msg, _)) => {
            let first: String = msg.split('\n').next().unwrap_or("").chars().take(120).collect();
            return Growth::Reason(format!("no merge base ({first})"));
        }
    };
    let spec = format!("{base}:{LIVENESS_BASELINE}");
    let text = match git(root, &["show", &spec]) {
        Ok(t) => t,
        Err((_, stderr)) => {
            let missing = ["does not exist", "exists on disk, but not in", "not in"]
                .iter()
                .any(|p| stderr.contains(p));
            return if missing {
                Growth::Added { base, added: 0 }
            } else {
                Growth::Reason(format!("the baseline at {} cannot be read", &base[..base.len().min(9)]))
            };
        }
    };
    let mut left = multiset(entries_of(&text).iter().map(|e| (e.file.as_str(), e.hash.as_str())));
    let mut added = 0;
    for e in current {
        match left.get_mut(&(e.file.clone(), e.hash.clone())) {
            Some(n) if *n > 0 => *n -= 1,
            _ => added += 1,
        }
    }
    Growth::Added { base, added }
}

/// The liveness findings of the whole tree (appended after the kill findings, as the JavaScript does).
pub fn liveness_findings(ctx: &Ctx, root: &Path) -> Vec<Finding> {
    let fix = "Ask a `paddock_proc::ProcessId` (pid plus start time): `id.is_alive()` or `id.presence()`. A justified case carries `// conformance: pid-liveness — <reason>`.";
    let mut out = Vec::new();
    let mut sites: Vec<Site> = Vec::new();
    for f in ctx.files().iter() {
        if !f.ends_with(".rs") || VENDORED.is_match(f) {
            continue;
        }
        let Some(text) = ctx.read(f) else { continue };
        if text.is_empty() || !PREFILTER.is_match(&text) {
            continue;
        }
        for k in pid_liveness(&text, f) {
            match k.waived {
                Some(true) => {}
                Some(false) => out.push(Finding::error(
                    "C53",
                    53,
                    f,
                    k.line,
                    "the pid-liveness waiver gives no reason",
                    "Write `// conformance: pid-liveness — <why this pid cannot have been handed on>`.",
                )),
                None => sites.push(Site {
                    file: f.clone(),
                    line: k.line,
                    what: k.what,
                    hash: line_hash(&k.text),
                }),
            }
        }
    }
    let entries = ctx
        .read(LIVENESS_BASELINE)
        .filter(|r| !r.is_empty())
        .map(|r| entries_of(&r))
        .unwrap_or_default();
    let mut left = multiset(entries.iter().map(|e| (e.file.as_str(), e.hash.as_str())));
    for s in &sites {
        match left.get_mut(&(s.file.clone(), s.hash.clone())) {
            Some(n) if *n > 0 => *n -= 1,
            _ => out.push(Finding::error(
                "C53",
                53,
                &s.file,
                s.line,
                format!("{}: the pid may belong to another process by now", s.what),
                fix,
            )),
        }
    }
    // A `--files` run scans only some files (the JavaScript host has no scoped runs): it cannot tell which entries are stale or whether
    // the baseline grew, so it leaves both to the whole-tree run.
    let whole_run = ctx.files().len() == ctx.tree_files().len();
    for e in &entries {
        if whole_run && left.get(&(e.file.clone(), e.hash.clone())).copied().unwrap_or(0) > 0 {
            out.push(Finding::warn(
                "C53",
                53,
                &e.file,
                e.line.unwrap_or(1),
                format!(
                    "{LIVENESS_BASELINE} lists a liveness site that is gone (hash {}); the baseline may only shrink",
                    e.hash
                ),
                "Remove the entry.",
            ));
        }
    }
    if whole_run && !entries.is_empty() {
        match growth(root, &entries) {
            Growth::Reason(reason) => out.push(Finding::warn(
                "C53",
                53,
                LIVENESS_BASELINE,
                1,
                format!("the baseline's shrink-only rule is not enforced here: {reason}"),
                "Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.",
            )),
            Growth::Added { base, added } if added > 0 => out.push(Finding::error(
                "C53",
                53,
                LIVENESS_BASELINE,
                1,
                format!(
                    "the baseline grew: {added} entr{} not in the baseline at the merge base {}; it may only shrink",
                    if added == 1 { "y is" } else { "ies are" },
                    &base[..base.len().min(9)]
                ),
                "Remove the entries and fix the sites through a ProcessId. Growing the baseline needs the user's approval.",
            )),
            Growth::Added { .. } => {}
        }
    }
    out
}
