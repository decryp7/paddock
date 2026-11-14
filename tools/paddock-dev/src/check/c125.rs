//! C125: no hard-coded machine disk paths (rule 40; ER-21 class "stale machine paths after a disk move"). A `/Volumes/<name>`
//! literal names one disk of one machine: when the disk is renamed or replaced the literal goes stale and whatever reads it quietly
//! stops working (the tart link, the app plists, and the worktree and docker guards, which defaulted to a disk that no longer existed;
//! fix/guard-paths-derived, a5907d18c, 2026-11-14).
//!
//! Flagged: every `/Volumes/<name>` literal (the name starts with a letter or digit; a letter, digit, `_`, `.` or `~` right before
//! it makes it part of another path) in every text file of the tree, one finding per literal. Not read: the documents that record
//! history (the `history` path patterns of `conformance/c125-allow.json`, each with a reason; one that names no file is a warning),
//! the checkout's own `.git` file and owner note, and the check's own files. Allowed: the per-site entries of the same file's
//! `allow` array (`path`, `site`, `reason`, `count`); stale and ambiguous entries are warnings. The allow list has the shape of
//! fix/allow-list-helper's `check::allow_list` and moves onto it once that lands. **Report-only (warnings) until its first green
//! run on the real tree is recorded here (ER-41).** Recorded: not yet.
//!
//! The JavaScript check `conformance/src/checks/C125-machine-paths.mjs` is the original; `tests/it/check_c125.rs` compares the two
//! finding for finding.

use super::finding::Finding;
use super::jsre::rx;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

/// The allow file both hosts read.
pub const ALLOW: &str = "conformance/c125-allow.json";
/// The check's own files: they name the pattern.
pub const IMPLEMENTATION: [&str; 5] = [
    "conformance/src/checks/C125-machine-paths.mjs",
    "conformance/test/c125-machine-paths.test.mjs",
    "tools/paddock-dev/src/check/c125.rs",
    "tools/paddock-dev/tests/it/check_c125.rs",
    ALLOW,
];
/// The checkout's own files, never tracked: a worktree's `.git` file and the worktree tool's owner note.
pub const CHECKOUT: [&str; 2] = [".git", ".paddock-owner.toml"];

static BINARY: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"(?i)\.(?:png|jpe?g|gif|ico|icns|webp|bmp|tiff?|pdf|zip|gz|tgz|xz|zst|7z|whl|wasm|dmg|pkg|msi|exe|dll|so|dylib|a|lib|rlib|o|obj|pdb|ttf|otf|woff2?|mp3|mp4|mov|wav|webm|parquet|duckdb|sqlite|db|evtx|dmp|bin|onnx|safetensors)$",
    )
});
static LITERAL: LazyLock<Regex> = LazyLock::new(|| rx(r"/Volumes/[A-Za-z0-9][A-Za-z0-9_.+@-]*(?:/[A-Za-z0-9_.+@-]*)*"));
const FIX: &str = "Derive the folder (git, `paddock.worktreeRoot`, an environment variable or a setting) and fail closed when it cannot; invented test data and documents that must name a disk get an entry in conformance/c125-allow.json with a reason.";

/// Every machine disk path in `text`: `(line, literal)`.
pub fn machine_paths(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    if !text.contains("/Volumes/") {
        return out;
    }
    let (mut line, mut from) = (1, 0);
    for m in LITERAL.find_iter(text) {
        if text[..m.start()]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '~'))
        {
            continue;
        }
        line += text[from..m.start()].matches('\n').count();
        from = m.start();
        out.push((line, m.as_str().to_string()));
    }
    out
}

/// A path pattern as a regular expression: `**/` is any number of folders, `*` one name, a trailing `/` the folder and all below it.
pub fn glob_regex(pattern: &str) -> Regex {
    let mut src = String::from("^");
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("**/") {
            src.push_str("(?:[^/]+/)*");
            rest = r;
            continue;
        }
        if c == '*' {
            src.push_str("[^/]*");
        } else {
            src.push_str(&regex::escape(&c.to_string()));
        }
        rest = &rest[c.len_utf8()..];
    }
    if pattern.ends_with('/') {
        src.push_str(".*");
    }
    src.push('$');
    rx(&src)
}

/// One history entry: a path pattern whose files are not read.
#[derive(Debug)]
pub struct History {
    path: String,
    re: Regex,
}

/// One allow entry: the file, a piece of the flagged literal, and how many literals of the file it covers.
#[derive(Debug)]
pub struct Entry {
    path: String,
    site: String,
    count: usize,
}

/// Reads the allow file: its history and allow entries, or why it cannot be read.
pub fn parse_allow(text: &str) -> Result<(Vec<History>, Vec<Entry>), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{ALLOW} is not JSON: {e}"))?;
    let (Some(history), Some(allow)) = (v["history"].as_array(), v["allow"].as_array()) else {
        return Err(format!("{ALLOW} needs a `history` and an `allow` array"));
    };
    let reason_ok = |e: &serde_json::Value| e["reason"].as_str().unwrap_or("").trim().chars().count() >= 8;
    let mut hist = Vec::new();
    for e in history {
        let path = e["path"].as_str().unwrap_or("");
        if path.is_empty() || !reason_ok(e) {
            return Err(format!(
                "{ALLOW}: every history entry needs a path and a reason of at least 8 characters"
            ));
        }
        hist.push(History {
            path: path.to_string(),
            re: glob_regex(path),
        });
    }
    let mut out = Vec::new();
    for e in allow {
        let (path, site) = (e["path"].as_str().unwrap_or(""), e["site"].as_str().unwrap_or("").trim());
        if path.is_empty() || site.is_empty() || !reason_ok(e) {
            return Err(format!(
                "{ALLOW}: every entry needs a path, a site (a piece of the flagged path) and a reason of at least 8 characters"
            ));
        }
        let count = match e.get("count") {
            None => 1,
            Some(c) => c
                .as_u64()
                .filter(|n| *n >= 1)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("{ALLOW}: `count` ({path}: {site}) is a whole number of at least 1"))?,
        };
        out.push(Entry {
            path: path.to_string(),
            site: site.to_string(),
            count,
        });
    }
    Ok((hist, out))
}

/// The warnings for the allow entries that matched another number of literals than their count.
fn allow_warnings(allow: &[Entry], hits: &[usize], looked: impl Fn(&str) -> bool) -> Vec<Finding> {
    let mut out = Vec::new();
    for (a, &hits) in allow.iter().zip(hits) {
        if hits > a.count {
            let fix = if a.count > 1 {
                format!("Lengthen its `site`, or raise its `count` to {hits} if every one is meant.")
            } else {
                "Lengthen its `site` so it names one line.".to_string()
            };
            out.push(Finding::warn(
                "C125",
                40,
                ALLOW,
                1,
                format!("ambiguous allow entry ({}: {}) matches {hits} sites", a.path, a.site),
                fix,
            ));
        } else if hits < a.count && looked(&a.path) {
            let (what, fix) = if hits == 0 {
                ("matches no site".to_string(), format!("Remove it from {ALLOW}."))
            } else {
                (
                    format!("matches only {hits} of its {} sites", a.count),
                    format!("Lower its `count` to {hits}."),
                )
            };
            out.push(Finding::warn(
                "C125",
                40,
                ALLOW,
                1,
                format!("stale allow entry ({}: {}) {what}", a.path, a.site),
                fix,
            ));
        }
    }
    out
}

/// The check.
pub struct C125;

impl Check for C125 {
    fn id(&self) -> &'static str {
        "C125"
    }

    fn rule(&self) -> u32 {
        40
    }

    fn title(&self) -> &'static str {
        "No hard-coded machine disk paths"
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
                    "{\"history\":[{\"path\":\"docs/adr/\",\"reason\":\"ADRs record history\"}],\"allow\":[{\"path\":\"src/ok.rs\",\"site\":\"/Volumes/Data\",\"reason\":\"invented test data\"}]}",
                ),
                ("hooks/guard.py", "MAIN = \"/Volumes/d8sst7/paddock\"\n"),
                ("src/ok.rs", "let v = \"/Volumes/Data\";\n"),
                ("docs/adr/0001.md", "built in /Volumes/d7sst7/x\n"),
            ],
            expect: vec![("hooks/guard.py", 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let lists = ctx
            .read(ALLOW)
            .map(|t| parse_allow(&t))
            .unwrap_or_else(|| Err(format!("{ALLOW} cannot be read")));
        let ((history, allow), why) = match lists {
            Ok(l) => (l, None),
            Err(e) => ((Vec::new(), Vec::new()), Some(e)),
        };
        let mut findings = Vec::new();
        let mut probes = 0;
        let mut completed = true;
        let mut hits = vec![0usize; allow.len()];
        let mut history_hits = vec![0usize; history.len()];
        let mut scanned = std::collections::HashSet::new();
        for f in ctx.files().iter() {
            if IMPLEMENTATION.contains(&f.as_str()) || CHECKOUT.contains(&f.as_str()) || BINARY.is_match(f) {
                continue;
            }
            if let Some(h) = history.iter().position(|e| e.re.is_match(f)) {
                history_hits[h] += 1;
                continue;
            }
            let Some(text) = ctx.read(f) else {
                completed = false;
                continue;
            };
            if text.contains('\0') {
                continue;
            }
            probes += 1;
            scanned.insert(f.to_string());
            for (line, literal) in machine_paths(&text) {
                let mut named = false;
                for (i, a) in allow.iter().enumerate() {
                    if a.path == *f && literal.contains(a.site.as_str()) {
                        hits[i] += 1;
                        named = true;
                    }
                }
                if named {
                    continue;
                }
                findings.push(Finding::warn(
                    "C125",
                    40,
                    f,
                    line,
                    format!("hard-codes the machine disk path `{literal}`: it goes stale when the disk is renamed or replaced"),
                    FIX,
                ));
            }
        }
        for (e, n) in history.iter().zip(&history_hits) {
            if *n == 0 {
                findings.push(Finding::warn(
                    "C125",
                    40,
                    ALLOW,
                    1,
                    format!("stale history entry ({}) names no file", e.path),
                    format!("Remove it from {ALLOW}."),
                ));
            }
        }
        findings.extend(allow_warnings(&allow, &hits, |p| scanned.contains(p) || ctx.read(p).is_none()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_placeholders_and_other_paths() {
        assert_eq!(
            machine_paths("a\nroots = [\"/Volumes/a/x\", \"/Volumes/B\"]\n"),
            [(2, "/Volumes/a/x".to_string()), (2, "/Volumes/B".to_string())]
        );
        assert_eq!(
            machine_paths("x=${X:-/Volumes/d8sst7/vm}\n"),
            [(1, "/Volumes/d8sst7/vm".to_string())]
        );
        assert!(machine_paths("/Volumes/<disk>/x /Volumes/... /System/Volumes/Data ~/Volumes/x\n").is_empty());
    }

    #[test]
    fn path_patterns() {
        assert!(glob_regex("apps/*/docs/adr/").is_match("apps/dyno/docs/adr/0001.md"));
        assert!(!glob_regex("apps/*/docs/adr/").is_match("apps/dyno/x/docs/adr/0001.md"));
        assert!(glob_regex("**/CHANGELOG.md").is_match("CHANGELOG.md"));
        assert!(glob_regex("**/CHANGELOG.md").is_match("apps/dyno/CHANGELOG.md"));
        assert!(!glob_regex("docs/lessons.md").is_match("docs/lessons.md.bak"));
        assert!(!glob_regex("docs/adr/").is_match("docs/adrs.md"));
    }

    #[test]
    fn the_allow_file_needs_both_lists_and_reasons() {
        assert!(parse_allow("{\"allow\": []}").unwrap_err().contains("history"));
        assert!(
            parse_allow("{\"history\": [], \"allow\": [{\"path\": \"a\", \"site\": \"x\", \"reason\": \"long enough\", \"count\": 0}]}")
                .is_err()
        );
        assert!(parse_allow("{\"history\": [{\"path\": \"docs/\"}], \"allow\": []}").is_err());
    }
}
