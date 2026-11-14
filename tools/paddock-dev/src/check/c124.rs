//! C124: a gate's result is built through the shared outcome, never by hand (ADR 0089 amendment 2026-11-13, WP-T6; ER-21: the class
//! "a gate gives its verdict from an indirect signal" recurred at rows 2 to 7 of the amendment's table).
//!
//! Flagged, one site at a time, in the gate and check tools' Rust (`tools/paddock-dev/src/`, `crates/paddock-bench/src/`) outside
//! test code (a `tests/` folder, `tests.rs`, `*_test.rs`, and everything after a file's first `#[cfg(test)]`):
//!
//! - a local enum with a `NotMeasured`, `Clean`, `Pass`, `Current` or `Green` variant: a second result type;
//! - a JSON object literal with a `"status"`, `"verdict"` or `"outcome"` key: a result a consumer would read without evidence;
//! - `fn fails(&self) -> bool` or `fn passed(&self) -> bool`: the bool conversion the shared type forbids;
//! - a call of `Evidence::ci_run(` or `Evidence::adopt(` outside the modules that own them;
//! - `measure(.., || Ok(Vec::new()))` (also `vec![]`, `Vec::default()`, `Default::default()`): a measurement that always finds
//!   nothing reads as a pass.
//!
//! In JavaScript, outside `conformance/src/outcome.mjs`: an object literal `status: 'clean'` (a pass made by hand).
//!
//! Pre-existing sites are listed one by one in `conformance/c124-allow.json`: a path, a `site` (a piece of the flagged line) and a
//! reason (shrink only). An entry that matches no site is stale and one that matches several is ambiguous: both are warnings. **Report-only (warnings)
//! until its first green run on the real tree is recorded here (ER-41).** Recorded: not yet.
//!
//! The JavaScript check `conformance/src/checks/C124-gate-outcomes.mjs` is the original; the `c124` tests of `tests/it/check.rs`
//! compare the two finding for finding.

use super::finding::Finding;
use super::jsre::rx;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

/// The allow list both hosts read.
pub const ALLOW: &str = "conformance/c124-allow.json";

static RUST: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:tools/paddock-dev/src|crates/paddock-bench/src)/.*\.rs$"));
static OWNER: LazyLock<Regex> = LazyLock::new(|| rx(r"^crates/paddock-cli/"));
static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:tests?|__tests__)/|(?:^|/)tests\.rs$|_test\.rs$"));
static VARIANT: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(NotMeasured|Clean|Pass|Current|Green)\b"));
static ENUM: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+\w+"));
static JSON_KEY: LazyLock<Regex> = LazyLock::new(|| rx(r#""(?:status|verdict|outcome)"\s*:"#));
static BOOL_FN: LazyLock<Regex> = LazyLock::new(|| rx(r"\bfn\s+(?:fails|passed)\s*\(\s*&self\s*\)\s*->\s*bool\b"));
static CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:Evidence|Proof)::(?:ci_run|adopt)\s*\("));
static EMPTY_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"\|\|\s*(?:\{\s*)?Ok\(\s*(?:Vec::new\(\)|vec!\s*\[\s*\]|Vec::default\(\)|Default::default\(\))\s*\)\s*;?\s*\}?\s*,?\s*$")
});
static MEASURE_CALL: LazyLock<Regex> = LazyLock::new(|| rx(r"\bmeasure\s*\("));
static JSON_USE: LazyLock<Regex> = LazyLock::new(|| rx(r"json!|serde_json"));
static CFG_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*#\[cfg\(test\)\]"));
static JS_FILE: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:conformance/(?:src|bin)|scripts|tools)/.*\.mjs$"));
static JS_PASS: LazyLock<Regex> = LazyLock::new(|| rx(r#"\bstatus\s*:\s*['"]clean['"]"#));
static JS_TEST: LazyLock<Regex> = LazyLock::new(|| rx(r"\.test\.mjs$"));

const CALL_OWNERS: [&str; 2] = ["tools/paddock-dev/src/ci_runs/verdict.rs", "tools/paddock-dev/src/pre_push/known.rs"];
const FIX: &str = "Build the result with `paddock_cli::measure` (or `Evidence::adopt`) and print it with `Outcome::json()`; read another gate's result with `Outcome::from_wire`, never its exit code (ADR 0089 amendment 2026-11-13).";

/// One flagged site: its line, the text the allow list's `site` is looked for in, and what is wrong with it.
pub type Site = (usize, String, String);

/// Every site in the Rust text `text` of `path`.
pub fn flagged_rust(path: &str, text: &str) -> Vec<Site> {
    if OWNER.is_match(path) || TEST_PATH.is_match(path) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let end = lines.iter().position(|l| CFG_TEST.is_match(l)).unwrap_or(lines.len());
    let json_used = JSON_USE.is_match(text);
    let mut depth: i64 = -1; // brace depth inside an enum, -1 outside
    let mut header = String::new(); // the enum's own line, so the `NotMeasured,` of two enums are two sites
    for (i, line) in lines[..end].iter().enumerate() {
        if line.trim().starts_with("//") {
            continue;
        }
        if depth < 0 && ENUM.is_match(line) && line.contains('{') {
            depth = 0;
            header = line.trim().to_string();
        }
        if depth >= 0 {
            if depth == 1
                && let Some(c) = VARIANT.captures(line)
            {
                out.push((
                    i + 1,
                    format!("{header} {}", line.trim()),
                    format!("an enum variant `{}` (a second result type)", &c[1]),
                ));
            }
            for ch in line.chars() {
                depth += match ch {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                };
            }
            if depth <= 0 {
                depth = -1;
            }
        }
        if json_used && JSON_KEY.is_match(line) {
            out.push((
                i + 1,
                line.to_string(),
                "a JSON result object with a status, verdict or outcome key".to_string(),
            ));
        }
        if BOOL_FN.is_match(line) {
            out.push((i + 1, line.to_string(), "a `fails`/`passed` bool on a result type".to_string()));
        }
        if CALL.is_match(line) && !CALL_OWNERS.contains(&path) {
            out.push((
                i + 1,
                line.to_string(),
                "a call of `Evidence::ci_run`/`adopt` outside its owner".to_string(),
            ));
        }
    }
    // Comment lines are blanked (line numbers stay), then each `measure(` call is cut at its balanced closing parenthesis.
    let code = lines[..end]
        .iter()
        .map(|l| if l.trim().starts_with("//") { "" } else { *l })
        .collect::<Vec<_>>()
        .join("\n");
    for m in MEASURE_CALL.find_iter(&code) {
        let (mut depth, mut k) = (1, m.end());
        for (off, ch) in code[m.end()..].char_indices() {
            depth += match ch {
                '(' => 1,
                ')' => -1,
                _ => 0,
            };
            k = m.end() + off;
            if depth == 0 {
                break;
            }
        }
        if depth != 0 || !EMPTY_TAIL.is_match(&code[m.end()..k]) {
            continue;
        }
        let line = code[..m.start()].matches('\n').count() + 1;
        out.push((
            line,
            lines[line - 1].to_string(),
            "a `measure` whose closure always returns an empty list (an empty measurement reads as a pass)".to_string(),
        ));
    }
    out
}

/// Every hand-made pass in the JavaScript text of `path`.
pub fn flagged_js(path: &str, text: &str) -> Vec<Site> {
    if path == "conformance/src/outcome.mjs"
        || path == "conformance/src/checks/C124-gate-outcomes.mjs"
        || TEST_PATH.is_match(path)
        || JS_TEST.is_match(path)
    {
        return Vec::new();
    }
    text.split('\n')
        .enumerate()
        .filter(|(_, l)| !l.trim().starts_with("//") && JS_PASS.is_match(l))
        .map(|(i, l)| (i + 1, l.to_string(), "an object literal `status: 'clean'` (a pass made by hand)".to_string()))
        .collect()
}

/// One allow-list entry: the file, a piece of the flagged line, and (read, never used) why.
pub struct Entry {
    path: String,
    site: String,
}

/// Reads the allow list: its entries, or why it cannot be read.
pub fn parse_allow(text: &str) -> Result<Vec<Entry>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{ALLOW} is not JSON: {e}"))?;
    let list = v["allow"].as_array().ok_or_else(|| format!("{ALLOW} has no `allow` array"))?;
    let mut out = Vec::new();
    for e in list {
        let (path, site, reason) = (
            e["path"].as_str().unwrap_or(""),
            e["site"].as_str().unwrap_or("").trim(),
            e["reason"].as_str().unwrap_or("").trim(),
        );
        if path.is_empty() || site.is_empty() || reason.chars().count() < 8 {
            return Err(format!(
                "{ALLOW}: every entry needs a path, a site (a piece of the flagged line) and a reason of at least 8 characters"
            ));
        }
        out.push(Entry {
            path: path.to_string(),
            site: site.to_string(),
        });
    }
    Ok(out)
}

/// The indexes of the entries that name this site: the file and a piece of the flagged line.
fn matching(allow: &[Entry], path: &str, text: &str) -> Vec<usize> {
    allow
        .iter()
        .enumerate()
        .filter(|(_, a)| a.path == path && text.contains(a.site.as_str()))
        .map(|(i, _)| i)
        .collect()
}

/// The check.
pub struct C124;

impl Check for C124 {
    fn id(&self) -> &'static str {
        "C124"
    }

    fn rule(&self) -> u32 {
        21
    }

    fn title(&self) -> &'static str {
        "A gate's result is built through the shared outcome, never by hand"
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
                    ALLOW,
                    "{\"allow\":[{\"path\":\"tools/paddock-dev/src/ok.rs\",\"site\":\"NotMeasured\",\"reason\":\"the seed, shrink only\"}]}",
                ),
                ("tools/paddock-dev/src/x/mod.rs", "pub enum V {\n    NotMeasured,\n}\n"),
                ("tools/paddock-dev/src/ok.rs", "pub enum V {\n    NotMeasured,\n}\n"),
                ("tools/paddock-dev/src/y.rs", "// json!({\"status\": 1})\nfn f() {}\n"),
            ],
            expect: vec![("tools/paddock-dev/src/x/mod.rs", 2)],
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
        let mut hits = vec![0usize; allow.len()];
        let mut scanned = std::collections::HashSet::new();
        for f in ctx.files().iter() {
            let rust = RUST.is_match(f);
            if !rust && !JS_FILE.is_match(f) {
                continue;
            }
            let Some(text) = ctx.read(f) else {
                completed = false;
                continue;
            };
            probes += 1;
            scanned.insert(f.to_string());
            let sites = if rust { flagged_rust(f, &text) } else { flagged_js(f, &text) };
            for (line, at, what) in sites {
                let named = matching(&allow, f, &at);
                for n in &named {
                    hits[*n] += 1;
                }
                if !named.is_empty() {
                    continue;
                }
                findings.push(Finding::warn("C124", 21, f, line, format!("builds a gate result by hand: {what}"), FIX));
            }
        }
        for (n, a) in allow.iter().enumerate() {
            if hits[n] == 0 && (scanned.contains(&a.path) || ctx.read(&a.path).is_none()) {
                findings.push(Finding::warn(
                    "C124",
                    21,
                    ALLOW,
                    1,
                    format!("stale allow entry ({}: {}) matches no site", a.path, a.site),
                    "Remove it from conformance/c124-allow.json.",
                ));
            } else if hits[n] > 1 {
                findings.push(Finding::warn(
                    "C124",
                    21,
                    ALLOW,
                    1,
                    format!("ambiguous allow entry ({}: {}) matches {} sites", a.path, a.site, hits[n]),
                    "Lengthen its `site` so it names one line.",
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
