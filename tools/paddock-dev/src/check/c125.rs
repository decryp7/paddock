//! C125: no hard-coded machine disk paths (rule 40; ER-21 class "stale machine paths after a disk move"). A `/Volumes/<name>`
//! literal names one disk of one machine: when the disk is renamed or replaced the literal goes stale and whatever reads it quietly
//! stops working (the tart link, the app plists, and the worktree and docker guards, which defaulted to a disk that no longer existed;
//! fix/guard-paths-derived, a5907d18c, 2026-11-14).
//!
//! Flagged: every `/Volumes/<name>` literal, also with its slashes escaped (`\/Volumes\/<name>`, a regular expression) and reported
//! unescaped (the name starts with a letter or digit; a letter, digit, `_`, `.` or `~` right before it makes it part of another
//! path) in every text file of the tree, one finding per literal. Not read as findings: Markdown documents that record history (the
//! `history` path patterns of `conformance/c125-allow.json`, each with a reason; only `.md` files, and an entry that skipped no
//! literal is a warning; dated plans are read), the checkout's own `.git` file and owner note, and the check's own files. Allowed:
//! the per-site entries of the same file's `allow` array, read by the shared [`super::allow_list`]; a site names a path by whole
//! components, and a literal beyond its entries' count is reported at its own line. A file that cannot be read is not measured (an
//! error at that file); an allow file that cannot be read makes the check broken (one finding, nothing else). **Report-only
//! (warnings) until its first green run on the real tree is recorded here (ER-41).** Recorded: not yet.
//!
//! The JavaScript check `conformance/src/checks/C125-machine-paths.mjs` is the original; `tests/it/check_c125.rs` compares the two
//! finding for finding.

use super::allow_list::{self, Admit, AllowList};
use super::finding::Finding;
use super::jsre::rx;
use super::outcome::Outcome;
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
/// A `/` may be escaped (`\/`, a regular expression literal); the finding names the path unescaped.
static LITERAL: LazyLock<Regex> = LazyLock::new(|| rx(r"\\?/Volumes\\?/[A-Za-z0-9][A-Za-z0-9_.+@-]*(?:\\?/[A-Za-z0-9_.+@-]*)*"));
const BROKEN_FIX: &str = "Restore conformance/c125-allow.json: {\"history\": [{\"path\": \"...\", \"reason\": \"...\"}], \"allow\": [{\"path\": \"...\", \"site\": \"...\", \"reason\": \"...\", \"count\": 1}]}.";
const UNREAD_FIX: &str = "Make the file readable (permissions) and run the check again.";
const FIX: &str = "Derive the folder (git, `paddock.worktreeRoot`, an environment variable or a setting) and fail closed when it cannot; invented test data and documents that must name a disk get an entry in conformance/c125-allow.json with a reason.";

/// Every machine disk path in `text`: `(line, literal)`.
pub fn machine_paths(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    if !text.contains("Volumes") {
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
        out.push((line, m.as_str().replace("\\/", "/")));
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

/// One history entry: a path pattern whose Markdown files' literals are skipped.
#[derive(Debug)]
pub struct History {
    path: String,
    re: Regex,
}

/// Whether the allow `site` names `literal` by whole path components (`/Volumes/Data` names `/Volumes/Data/x`, not
/// `/Volumes/Database`).
pub fn names_path(site: &str, literal: &str) -> bool {
    literal == site
        || literal
            .strip_prefix(site)
            .is_some_and(|rest| site.ends_with('/') || rest.starts_with('/'))
}

/// Reads the allow file: its history entries and the shared allow list's entries, or why it cannot be read.
pub fn parse_allow(text: &str) -> Result<(Vec<History>, Vec<allow_list::Entry>), String> {
    let allow = allow_list::parse(text, ALLOW)?;
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{ALLOW} is not JSON: {e}"))?;
    let Some(history) = v["history"].as_array() else {
        return Err(format!("{ALLOW} has no `history` array"));
    };
    let mut hist = Vec::new();
    for e in history {
        let path = e["path"].as_str().unwrap_or("");
        if path.is_empty() || e["reason"].as_str().unwrap_or("").trim().chars().count() < 8 {
            return Err(format!(
                "{ALLOW}: every history entry needs a path and a reason of at least 8 characters"
            ));
        }
        hist.push(History {
            path: path.to_string(),
            re: glob_regex(path),
        });
    }
    Ok((hist, allow))
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
        let (history, entries) = match lists {
            Ok(l) => l,
            // As the JavaScript host: one check-broken finding and nothing else (the allow file was the one thing looked at).
            Err(why) => {
                return CheckOutput {
                    findings: vec![
                        Finding::warn("C125", 40, ALLOW, 1, format!("the check cannot look: {why}"), BROKEN_FIX)
                            .unwaivable()
                            .with_outcome(Outcome::CheckBroken),
                    ],
                    probes: 1,
                    completed: true,
                    precondition: Precondition {
                        met: true,
                        detail: format!("{ALLOW} is read"),
                    },
                };
            }
        };
        let mut allow = AllowList::new(ALLOW, entries);
        let mut findings = Vec::new();
        let mut probes = 0;
        let mut history_hits = vec![0usize; history.len()];
        let mut scanned = std::collections::HashSet::new();
        for f in ctx.files().iter() {
            if IMPLEMENTATION.contains(&f.as_str()) || CHECKOUT.contains(&f.as_str()) || BINARY.is_match(f) {
                continue;
            }
            let h = if f.ends_with(".md") {
                history.iter().position(|e| e.re.is_match(f))
            } else {
                None
            };
            // A file that cannot be read is a not-measured finding at that file, as in the JavaScript host.
            let Some(text) = ctx.read(f) else {
                findings.push(
                    Finding::error(
                        "C125",
                        40,
                        f,
                        1,
                        format!("not measured: {f} could not be read, so a disk path in it cannot be seen"),
                        UNREAD_FIX,
                    )
                    .unwaivable()
                    .with_outcome(Outcome::NotMeasured),
                );
                continue;
            };
            if text.contains('\0') {
                continue;
            }
            probes += 1;
            scanned.insert(f.to_string());
            for (line, literal) in machine_paths(&text) {
                if let Some(h) = h {
                    history_hits[h] += 1;
                    continue;
                }
                let finding = match allow.admit(f, |site| names_path(site, &literal)) {
                    Admit::Allowed => continue,
                    Admit::Beyond => Finding::warn(
                        "C125",
                        40,
                        f,
                        line,
                        format!("hard-codes the machine disk path `{literal}` beyond the count of its allow entry"),
                        format!("Derive it, or raise the entry's `count` in {ALLOW} if it is invented data too."),
                    ),
                    Admit::Unlisted => Finding::warn(
                        "C125",
                        40,
                        f,
                        line,
                        format!("hard-codes the machine disk path `{literal}`: it goes stale when the disk is renamed or replaced"),
                        FIX,
                    ),
                };
                findings.push(finding);
            }
        }
        for (e, n) in history.iter().zip(&history_hits) {
            if *n == 0 {
                findings.push(Finding::warn(
                    "C125",
                    40,
                    ALLOW,
                    1,
                    format!("stale history entry ({}) skipped no disk path", e.path),
                    format!("Remove it from {ALLOW}."),
                ));
            }
        }
        findings.extend(allow.warnings("C125", 40, |p| scanned.contains(p) || ctx.read(p).is_none()));
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{ALLOW} is read"),
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
    fn an_escaped_literal_is_a_machine_path_reported_unescaped() {
        assert_eq!(
            machine_paths("assert.match(e, /only drives \\/Volumes\\/d8sst7\\/vm-data\\/x/);\n"),
            [(1, "/Volumes/d8sst7/vm-data/x/".to_string())]
        );
        assert_eq!(machine_paths(r#"re = r"\/Volumes\/Data""#), [(1, "/Volumes/Data".to_string())]);
        assert!(machine_paths(r"firmlink \/System\/Volumes\/Data and \/Volumes\/<disk>").is_empty());
    }

    #[test]
    fn path_patterns() {
        assert!(glob_regex("apps/*/docs/adr/").is_match("apps/dyno/docs/adr/0001.md"));
        assert!(!glob_regex("apps/*/docs/adr/").is_match("apps/dyno/x/docs/adr/0001.md"));
        assert!(glob_regex("**/CHANGELOG.md").is_match("CHANGELOG.md"));
        assert!(glob_regex("**/CHANGELOG.md").is_match("apps/dyno/CHANGELOG.md"));
        assert!(!glob_regex("notes/kept.md").is_match("notes/kept.md.bak"));
        assert!(!glob_regex("docs/adr/").is_match("docs/adrs.md"));
    }

    #[test]
    fn the_allow_file_needs_both_lists_and_reasons() {
        assert!(parse_allow("{\"allow\": []}").unwrap_err().contains("history"));
        assert!(names_path("/Volumes/Data", "/Volumes/Data/x") && names_path("/Volumes/Data", "/Volumes/Data"));
        assert!(!names_path("/Volumes/Data", "/Volumes/Database") && !names_path("/Volumes/M", "/Volumes/Media"));
        assert!(names_path("/Volumes/Data/", "/Volumes/Data/x"));
        assert!(
            parse_allow("{\"history\": [], \"allow\": [{\"path\": \"a\", \"site\": \"x\", \"reason\": \"long enough\", \"count\": 0}]}")
                .is_err()
        );
        assert!(parse_allow("{\"history\": [{\"path\": \"docs/\"}], \"allow\": []}").is_err());
    }
}
