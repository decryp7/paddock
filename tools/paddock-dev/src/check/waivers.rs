//! `paddock.waivers.toml`: `[[waiver]]` tables of string values (`check`, `path`, `reason`, `expires = YYYY-MM-DD`), read and
//! applied with the semantics of `conformance/src/util/waivers.mjs` (the oracle test compares them on a fixture of every
//! outcome). It is the whole TOML subset the file uses; anything else in it is reported rather than guessed at. A waiver past
//! its date is `W001` and waives nothing, a malformed one `W002`, one that waived nothing `W003` (a warning), a glob where
//! one exact path is needed `W004`.

use super::finding::{Finding, Severity, Waived};
use super::jsre::{DOT, rx, trim};
use regex::Regex;
use std::sync::LazyLock;

/// The file's name in the repository, for the findings about it.
pub const FILE: &str = "paddock.waivers.toml";

/// One `[[waiver]]` table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Waiver {
    /// The 1-based line of its `[[waiver]]` header.
    pub line: usize,
    /// The check it waives.
    pub check: Option<String>,
    /// The path it covers: exact, `*`, or a prefix ending in `*`.
    pub path: Option<String>,
    /// Why.
    pub reason: Option<String>,
    /// The last day it holds, `YYYY-MM-DD`.
    pub expires: Option<String>,
}

/// Checks whose waivers must name one exact path (no `*`): a glob would cover findings nobody has read.
const EXACT_PATH_CHECKS: [&str; 1] = ["C99"];

static TRAILING_COMMENT: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"\s+#{DOT}*$")));
static KEY_VALUE: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r#"^(\w+)\s*=\s*"((?:[^"\\]|\\{DOT})*)"$"#)));
static UNESCAPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\\(["\\])"#).unwrap_or_else(|e| panic!("{e}")));
static COMMIT_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i-u)^commit(?-u:\b)").unwrap_or_else(|e| panic!("{e}")));
static COMMIT_FIXED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^commit ([0-9a-f]{10})(?:[0-9a-f]{30})?$").unwrap_or_else(|e| panic!("{e}")));
static DATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$").unwrap_or_else(|e| panic!("{e}")));

/// Parses the waiver file. Returns the waivers and the problems (strings naming the line).
pub fn parse(text: &str) -> (Vec<Waiver>, Vec<String>) {
    let mut waivers: Vec<Waiver> = Vec::new();
    let mut problems = Vec::new();
    let mut open = false;
    for (i, raw) in text.split('\n').enumerate() {
        let stripped = TRAILING_COMMENT.replace(raw, "");
        let line = trim(&stripped);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[waiver]]" {
            waivers.push(Waiver {
                line: i + 1,
                ..Waiver::default()
            });
            open = true;
            continue;
        }
        let kv = KEY_VALUE.captures(line).filter(|_| open);
        let Some(kv) = kv else {
            problems.push(format!(
                "line {}: cannot read \"{}\" (expected [[waiver]] and key = \"value\")",
                i + 1,
                trim(raw)
            ));
            continue;
        };
        let value = UNESCAPE.replace_all(&kv[2], "$1").into_owned();
        if let Some(cur) = waivers.last_mut() {
            match &kv[1] {
                "check" => cur.check = Some(value),
                "path" => cur.path = Some(value),
                "reason" => cur.reason = Some(value),
                "expires" => cur.expires = Some(value),
                _ => {}
            }
        }
    }
    let kept = waivers.into_iter().filter_map(|w| commit_waiver_fixed(w, &mut problems)).collect();
    (kept, problems)
}

/// A C23 waiver for a commit is `path = "commit <sha>"`: exactly 10 hex digits or the full 40, in any case; the full sha is cut
/// to 10. Globs and other forms waive nothing (a problem is reported). Other C23 waivers keep their patterns.
fn commit_waiver_fixed(mut w: Waiver, problems: &mut Vec<String>) -> Option<Waiver> {
    let path = w.path.clone().unwrap_or_default();
    if w.check.as_deref() != Some("C23") || !COMMIT_PATH.is_match(&path) {
        return Some(w);
    }
    match COMMIT_FIXED.captures(&path.to_lowercase()) {
        Some(m) => {
            w.path = Some(format!("commit {}", &m[1]));
            Some(w)
        }
        None => {
            problems.push(format!(
                "line {}: a commit waiver needs path = \"commit <10 or 40 hex digits>\" (no pattern), not \"{path}\"; it waives nothing",
                w.line
            ));
            None
        }
    }
}

/// `path` against a waiver or filter pattern: exact, or a prefix ending in `*`.
pub fn path_matches(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    match pattern.strip_suffix('*') {
        Some(prefix) => path.starts_with(prefix),
        None => pattern == path,
    }
}

/// What a run tells the engine about itself.
pub struct Applying<'a> {
    /// The run's findings.
    pub findings: Vec<Finding>,
    /// The parsed waivers and the problems of the file.
    pub waivers: &'a [Waiver],
    /// Problems the parser reported.
    pub problems: &'a [String],
    /// Today, `YYYY-MM-DD` (UTC).
    pub today: &'a str,
    /// The ids of the checks that ran, for `W003`.
    pub checks_run: &'a [String],
    /// A waiver the run could not have used (a scoped run): no `W003` for it.
    pub skip_unused: &'a dyn Fn(&Waiver) -> bool,
}

/// Splits findings into kept and waived, and adds the findings about bad waivers. A finding with `unwaivable` is never waived.
pub fn apply(a: Applying<'_>) -> (Vec<Finding>, Vec<Waived>) {
    let mut extra: Vec<Finding> = a
        .problems
        .iter()
        .map(|p| Finding::error("W002", 5, FILE, 1, format!("waiver file: {p}"), "Fix the line."))
        .collect();
    let mut live: Vec<&Waiver> = Vec::new();
    for w in a.waivers {
        let check = w.check.clone().unwrap_or_default();
        let path = w.path.clone().unwrap_or_default();
        let complete = !check.is_empty()
            && !path.is_empty()
            && w.reason.as_deref().is_some_and(|r| !trim(r).is_empty())
            && w.expires.as_deref().is_some_and(|e| DATE.is_match(e));
        if !complete {
            extra.push(Finding::waiver_problem(
                "W002",
                Severity::Error,
                FILE,
                w.line,
                "a waiver needs check, path, reason and expires (YYYY-MM-DD)",
                "Fill in all four keys, or delete the waiver.",
            ));
        } else if EXACT_PATH_CHECKS.contains(&check.as_str()) && path.contains('*') {
            extra.push(Finding::waiver_problem(
                "W004",
                Severity::Error,
                FILE,
                w.line,
                format!("waiver for {check} {path} uses a glob; a {check} waiver names one exact path"),
                "Name the exact path of each finding (for C99: Cargo.lock#<key> as the finding prints it).",
            ));
        } else if w.expires.as_deref().unwrap_or_default() < a.today {
            extra.push(Finding::waiver_problem(
                "W001",
                Severity::Error,
                FILE,
                w.line,
                format!("waiver for {check} {path} expired on {}", w.expires.as_deref().unwrap_or_default()),
                "Fix the finding, or renew the waiver with a new date and a reason.",
            ));
        } else {
            live.push(w);
        }
    }
    let mut kept = Vec::new();
    let mut waived = Vec::new();
    let mut used = vec![false; live.len()];
    for f in a.findings {
        let hit = if f.unwaivable {
            None
        } else {
            live.iter()
                .position(|x| x.check.as_deref() == Some(f.check.as_str()) && path_matches(x.path.as_deref().unwrap_or_default(), &f.file))
        };
        match hit {
            Some(i) => {
                used[i] = true;
                waived.push(Waived {
                    finding: f,
                    reason: live[i].reason.clone().unwrap_or_default(),
                    expires: live[i].expires.clone().unwrap_or_default(),
                });
            }
            None => kept.push(f),
        }
    }
    for (i, w) in live.iter().enumerate() {
        let check = w.check.clone().unwrap_or_default();
        if used[i] || !a.checks_run.contains(&check) || (a.skip_unused)(w) {
            continue;
        }
        extra.push(Finding::waiver_problem(
            "W003",
            Severity::Warn,
            FILE,
            w.line,
            format!("waiver for {check} {} waives nothing", w.path.as_deref().unwrap_or_default()),
            "The finding is gone: delete the waiver.",
        ));
    }
    kept.extend(extra);
    (kept, waived)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(check: &str, path: &str, expires: &str) -> Waiver {
        Waiver {
            line: 3,
            check: Some(check.into()),
            path: Some(path.into()),
            reason: Some("r".into()),
            expires: Some(expires.into()),
        }
    }

    fn run(findings: Vec<Finding>, waivers: &[Waiver], today: &str) -> (Vec<Finding>, Vec<Waived>) {
        apply(Applying {
            findings,
            waivers,
            problems: &[],
            today,
            checks_run: &["C39".to_string()],
            skip_unused: &|_| false,
        })
    }

    #[test]
    fn parses_comments_escapes_and_problems() {
        let (ws, problems) =
            parse("# c\n[[waiver]]\ncheck = \"C39\" # trailing\npath = \"a\\\\b\\\"c\"\nreason = \"x # y\"\nbad line\nkey = \"v\"\n");
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].path.as_deref(), Some("a\\b\"c"));
        assert_eq!(ws[0].reason, None, "a ` #` inside a value cuts it, as the JavaScript reader does");
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("line 5:"));
    }

    #[test]
    fn a_key_before_any_table_is_a_problem() {
        let (ws, problems) = parse("check = \"C39\"\n");
        assert!(ws.is_empty());
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn commit_waivers_are_cut_to_ten_digits_or_dropped() {
        let (ws, problems) = parse(
            "[[waiver]]\ncheck = \"C23\"\npath = \"commit ABCDEF0123456789abcdef0123456789abcdef01\"\n[[waiver]]\ncheck = \"C23\"\npath = \"commit abc*\"\n[[waiver]]\ncheck = \"C23\"\npath = \"docs/x.md\"\n",
        );
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].path.as_deref(), Some("commit abcdef0123"));
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn path_patterns() {
        assert!(path_matches("*", "x"));
        assert!(path_matches("apps/*", "apps/a/b.rs"));
        assert!(path_matches("a.rs", "a.rs"));
        assert!(!path_matches("a.rs", "a.rs2"));
    }

    #[test]
    fn expired_and_incomplete_waivers_waive_nothing_and_unused_ones_warn() {
        let f = Finding::error("C39", 40, "crates/a.rs", 1, "m", "f");
        let (kept, waived) = run(vec![f.clone()], &[w("C39", "crates/*", "2000-01-01")], "2026-11-10");
        assert!(waived.is_empty());
        assert_eq!(kept.iter().map(|k| k.check.as_str()).collect::<Vec<_>>(), ["C39", "W001"]);
        let (kept, waived) = run(vec![f.clone()], &[w("C39", "crates/*", "2026-11-10")], "2026-11-10");
        assert_eq!(waived.len(), 1, "the last day still holds");
        assert!(kept.is_empty());
        let (kept, _) = run(vec![], &[w("C39", "crates/*", "2099-01-01")], "2026-11-10");
        assert_eq!((kept[0].check.as_str(), kept[0].severity), ("W003", Severity::Warn));
        let mut bad = w("C39", "x", "2099-01-01");
        bad.reason = Some("  ".into());
        assert_eq!(run(vec![], &[bad], "2026-11-10").0[0].check, "W002");
        assert_eq!(
            run(vec![], &[w("C99", "Cargo.lock#*", "2099-01-01")], "2026-11-10").0[0].check,
            "W004"
        );
    }

    #[test]
    fn an_unwaivable_finding_is_never_waived() {
        let f = Finding::error("C39", 40, "a.rs", 1, "check broken", "f").unwaivable();
        let (kept, waived) = run(vec![f], &[w("C39", "*", "2099-01-01")], "2026-11-10");
        assert!(waived.is_empty());
        assert_eq!(kept[0].check, "C39");
    }

    #[test]
    fn a_scoped_run_does_not_report_a_waiver_it_could_not_use() {
        let (kept, _) = apply(Applying {
            findings: vec![],
            waivers: &[w("C39", "crates/zzz/*", "2099-01-01")],
            problems: &[],
            today: "2026-11-10",
            checks_run: &["C39".to_string()],
            skip_unused: &|_| true,
        });
        assert!(kept.is_empty());
    }
}
