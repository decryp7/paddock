//! The mechanical lint of a project wiki (the LLM-wiki pattern): cheap and exact, so it runs on every wiki change. The
//! contradiction check against rules and ADRs is a bounded reading pass by a reviewer, not this.
//!
//! Errors: a relative link that does not resolve (file or heading anchor); a page index.md does not list; an orphan; a page
//! without a real, non-future `verified:` date or without `sources:`; a bullet naming no source; a malformed log line; copied
//! rule text (a run of [`COPY_WORDS`] or more words that a rules source also contains); a steward-log citation by clock time.
//! Warnings: a page not verified within the age limit. Pages named `*-template.md` are templates, not pages: skipped.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::date::{self, Day};
use crate::text::{self, COPY_WORDS, Value};

/// The pages that are not topic pages.
const SPECIAL: [&str; 3] = ["README.md", "index.md", "log.md"];
/// Pages not verified within this many days are flagged.
pub const MAX_AGE_DAYS: i64 = 30;
/// Most Markdown files read from a wiki folder.
pub const MAX_FILES: usize = 5_000;
/// Largest file read, in bytes.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// A rules source for the copy check.
#[derive(Debug, Clone)]
pub struct RuleSource {
    /// What messages call it (the path or `ref:path` given).
    pub name: String,
    /// Its text.
    pub text: String,
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The file, as a path relative to the current folder.
    pub file: String,
    /// 1-based line, 0 when it concerns the whole file.
    pub line: usize,
    /// What is wrong and how to fix it.
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "{}:{}: {}", self.file, self.line, self.message)
        } else {
            write!(f, "{}: {}", self.file, self.message)
        }
    }
}

/// The result of a lint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Findings that fail the lint.
    pub errors: Vec<Finding>,
    /// Findings that fail it only with `--strict`.
    pub warnings: Vec<Finding>,
    /// Topic pages checked.
    pub pages: usize,
    /// Markdown files read (pages, README, index, log).
    pub files: usize,
}

/// What stops a lint from running at all.
#[derive(Debug)]
pub enum LintError {
    /// The wiki folder or a page could not be read.
    Io(String),
    /// A limit was exceeded (too many files, a file too large).
    Limit(String),
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintError::Io(m) | LintError::Limit(m) => f.write_str(m),
        }
    }
}

/// What to lint.
pub struct Request<'a> {
    /// The wiki folder.
    pub wiki_dir: &'a Path,
    /// The current folder, which finding paths are relative to.
    pub cwd: &'a Path,
    /// Rules sources for the copy check (may be empty).
    pub rules: &'a [RuleSource],
    /// Today.
    pub today: Day,
    /// The staleness limit in days.
    pub max_age: i64,
    /// Names of logs whose entries must be cited by line number, not by clock time (a line that contains the name and a
    /// clock time like 14:33 is an error).
    pub cite_by_line: &'a [String],
}

/// Lexically normalises a path (`.` and `..` removed), like Node's `path.resolve` after joining.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `to` relative to `from` (both absolute and normalised); empty when equal.
pub fn relative(from: &Path, to: &Path) -> String {
    let a: Vec<_> = from.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = a[common..].iter().map(|_| "..".to_string()).collect();
    parts.extend(b[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    parts.join("/")
}

fn read_text(path: &Path) -> Result<String, LintError> {
    let meta = std::fs::metadata(path).map_err(|e| LintError::Io(format!("cannot read {}: {e}", path.display())))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(LintError::Limit(format!(
            "{} is larger than {} bytes",
            path.display(),
            MAX_FILE_BYTES
        )));
    }
    let bytes = std::fs::read(path).map_err(|e| LintError::Io(format!("cannot read {}: {e}", path.display())))?;
    // Windows line ends read as Unix ones, so a CRLF checkout lints like an LF one.
    Ok(String::from_utf8_lossy(&bytes).replace("\r\n", "\n"))
}

/// A real calendar date in front matter, as a day number.
fn verified_day(meta: &HashMap<String, Value>) -> Option<(String, Day)> {
    let s = meta.get("verified")?.as_text();
    let day = date::parse_date(&s)?;
    Some((s, day))
}

/// Lints a wiki folder.
pub fn lint(req: &Request<'_>) -> Result<Outcome, LintError> {
    let root = normalize(&if req.wiki_dir.is_absolute() {
        req.wiki_dir.to_path_buf()
    } else {
        req.cwd.join(req.wiki_dir)
    });
    let mut names: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(&root).map_err(|e| LintError::Io(format!("cannot read the wiki folder {}: {e}", root.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| LintError::Io(format!("cannot list {}: {e}", root.display())))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.ends_with(".md") && !name.ends_with("-template.md") && entry.path().is_file() {
            names.push(name);
        }
    }
    names.sort();
    if names.len() > MAX_FILES {
        return Err(LintError::Limit(format!(
            "{} has more than {MAX_FILES} Markdown files",
            root.display()
        )));
    }
    let mut texts: HashMap<&str, String> = HashMap::new();
    for n in &names {
        texts.insert(n, read_text(&root.join(n))?);
    }
    let pages: Vec<&str> = names.iter().map(String::as_str).filter(|n| !SPECIAL.contains(n)).collect();

    let mut out = Outcome {
        pages: pages.len(),
        files: names.len(),
        ..Outcome::default()
    };
    let at = |f: &str, line: usize, message: String| -> Finding {
        let rel = relative(req.cwd, &root.join(f));
        Finding {
            file: if rel.is_empty() { f.to_string() } else { rel },
            line,
            message,
        }
    };

    // Links resolve; inbound links per page (index and log do not count for the orphan check).
    let mut inbound: HashMap<&str, HashSet<&str>> = pages.iter().map(|p| (*p, HashSet::new())).collect();
    let mut indexed: HashSet<&str> = HashSet::new();
    let mut heading_cache: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for f in names.iter().map(String::as_str) {
        let page = text::parse_page(&texts[f]);
        let shift = texts[f][..page.body_start].matches('\n').count();
        for link in text::links_of(page.body) {
            if link.target.is_empty() {
                continue;
            }
            let path = normalize(&root.join(&link.target));
            let line = link.line + shift;
            if !path.exists() {
                out.errors.push(at(f, line, format!("broken link: {} does not exist", link.target)));
                continue;
            }
            if !link.anchor.is_empty() && path.to_string_lossy().ends_with(".md") {
                let heads = heading_cache.entry(path.clone()).or_insert_with(|| match read_text(&path) {
                    Ok(t) => text::headings_of(&t),
                    Err(_) => Vec::new(),
                });
                if !heads.contains(&link.anchor) {
                    out.errors
                        .push(at(f, line, format!("broken link: {} has no heading #{}", link.target, link.anchor)));
                }
            }
            let hit = path
                .strip_prefix(&root)
                .ok()
                .and_then(|p| p.to_str())
                .and_then(|h| pages.iter().find(|p| **p == h))
                .copied();
            if let Some(hit) = hit {
                if f == "index.md" {
                    indexed.insert(hit);
                }
                if hit != f && f != "index.md" && f != "log.md" {
                    inbound.entry(hit).or_default().insert(f);
                }
            }
        }
    }
    if !texts.contains_key("index.md") {
        out.errors.push(at("index.md", 0, "missing index.md".into()));
    }
    if !texts.contains_key("log.md") {
        out.errors.push(at("log.md", 0, "missing log.md".into()));
    }

    for p in &pages {
        if !indexed.contains(p) {
            out.errors.push(at(p, 0, "not listed in index.md".into()));
        }
        if inbound.get(p).is_none_or(HashSet::is_empty) {
            out.errors.push(at(p, 0, "orphan: no other page links to it".into()));
        }
        let page = text::parse_page(&texts[p]);
        match verified_day(&page.meta) {
            None => out
                .errors
                .push(at(p, 1, "front matter needs `verified: YYYY-MM-DD` (a real date)".into())),
            Some((s, day)) => {
                let age = req.today - day;
                if age < 0 {
                    out.errors.push(at(p, 1, format!("verified {s} is in the future")));
                } else if age > req.max_age {
                    out.warnings
                        .push(at(p, 1, format!("stale: verified {s}, {age} days ago (limit {})", req.max_age)));
                }
            }
        }
        if !matches!(page.meta.get("sources"), Some(Value::List(l)) if !l.is_empty()) {
            out.errors.push(at(p, 1, "front matter needs a non-empty `sources:` list".into()));
        }
        // A claim without a source is not written: every bullet carries a link or an explicit `src:`.
        let shift = texts[p][..page.body_start].matches('\n').count();
        for line in text::bullets_without_source(page.body) {
            out.errors
                .push(at(p, line + shift, "bullet names no source (add a link or `src: ...`)".into()));
        }
    }

    // The log: one dated line per change, in the fixed shape.
    if let Some(log) = texts.get("log.md") {
        for (i, raw) in log.split('\n').enumerate() {
            if raw.starts_with("## ") && !is_log_line(raw) {
                out.errors.push(at(
                    "log.md",
                    i + 1,
                    "log line must be `## [YYYY-MM-DD] ingest|lint|fix | subject`".into(),
                ));
            }
        }
    }

    // A named log's clock times are labels someone typed, some duplicated or mis-dated: cite its line number instead.
    for name in req.cite_by_line.iter().filter(|n| !n.is_empty()) {
        for f in &names {
            for (i, raw) in texts[f.as_str()].split('\n').enumerate() {
                if raw.contains(name.as_str()) && text::has_clock_time(raw) {
                    out.errors
                        .push(at(f, i + 1, format!("cites {name} by clock time; cite `{name}:<line>` instead")));
                }
            }
        }
    }

    // No rule text: the wiki links to rules and never restates them (checked against every rules source given).
    // Branches often carry the same rules text: one window set per distinct text.
    let mut distinct: HashMap<&str, usize> = HashMap::new();
    let mut window_sets: Vec<HashSet<String>> = Vec::new();
    let sets: Vec<(&RuleSource, usize)> = req
        .rules
        .iter()
        .map(|r| {
            let idx = *distinct.entry(r.text.as_str()).or_insert_with(|| {
                window_sets.push(text::windows(&r.text, COPY_WORDS).into_iter().collect());
                window_sets.len() - 1
            });
            (r, idx)
        })
        .collect();
    for f in &names {
        let mine = text::windows(text::parse_page(&texts[f.as_str()]).body, COPY_WORDS);
        for (src, idx) in &sets {
            let set = &window_sets[*idx];
            if let Some(hit) = mine.iter().find(|w| set.contains(*w)) {
                out.errors.push(at(
                    f,
                    0,
                    format!("copies rule text ({COPY_WORDS}+ words shared with {}): \"{hit}...\"; link the rule instead", src.name),
                ));
            }
        }
    }
    Ok(out)
}

/// `## [YYYY-MM-DD] ingest|lint|fix | subject` with a real date and a non-empty subject.
pub fn is_log_line(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix("## [") else { return false };
    let Some((day, rest)) = rest.split_at_checked(10) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix("] ") else { return false };
    // A real calendar day (the Node lint accepted any digits, so 2026-13-45 passed).
    if date::parse_date(day).is_none() {
        return false;
    }
    let Some(rest) = ["ingest", "lint", "fix"].iter().find_map(|k| rest.strip_prefix(k)) else {
        return false;
    };
    rest.strip_prefix(" | ").is_some_and(|s| {
        s.chars()
            .next()
            .is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines() {
        assert!(is_log_line("## [2026-10-31] ingest | one | seed"));
        assert!(!is_log_line("## 2026-10-31 something"));
        assert!(!is_log_line("## [2026-10-31] edit | one | x"));
        assert!(!is_log_line("## [2026-10-31] fix | "));
        assert!(!is_log_line("## [2026-10-3é] fix | x"));
        assert!(!is_log_line("## [2026-13-45] fix | x"));
        assert!(!is_log_line("## [2026-02-29] fix | x"));
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("/a/b"), Path::new("/a/b/docs/x.md")), "docs/x.md");
        assert_eq!(relative(Path::new("/a/b"), Path::new("/a/c/x.md")), "../c/x.md");
        assert_eq!(relative(Path::new("/a"), Path::new("/a")), "");
        assert_eq!(normalize(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    }
}
