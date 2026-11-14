//! One allow list per check, one site per entry (ER-21; PHPStan's `reportUnmatchedIgnoredErrors` and the `count` of its baseline).
//! An entry names a file (`path`), a piece of the flagged line (`site`), a `reason` of at least 8 characters and, when the same line
//! appears more than once in the file, how many sites it covers (`count`, default 1). A whole file or folder is never allowed. After
//! a run, an entry that matched fewer sites than its count is stale and one that matched more is ambiguous: both are warnings, so the
//! list only ever names the sites that still exist. Used by C121 and C124. The port of `conformance/src/util/allow-list.mjs` (the
//! parity tests of C121 and C124 compare the two).

use super::finding::Finding;

/// One entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The file.
    pub path: String,
    /// A piece of the flagged line.
    pub site: String,
    /// Why the site may stay.
    pub reason: String,
    /// How many sites of the file the entry covers.
    pub count: usize,
}

/// Reads the allow list `file` (its JSON `text`): its entries, or why it cannot be read.
pub fn parse(text: &str, file: &str) -> Result<Vec<Entry>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{file} is not JSON: {e}"))?;
    let list = v["allow"].as_array().ok_or_else(|| format!("{file} has no `allow` array"))?;
    let mut out = Vec::new();
    for e in list {
        let (path, site, reason) = (
            e["path"].as_str().unwrap_or(""),
            e["site"].as_str().unwrap_or("").trim(),
            e["reason"].as_str().unwrap_or("").trim(),
        );
        if path.is_empty() || site.is_empty() || reason.chars().count() < 8 {
            return Err(format!(
                "{file}: every entry needs a path, a site (a piece of the flagged line) and a reason of at least 8 characters"
            ));
        }
        let count = match e.get("count") {
            None => 1,
            Some(c) => c
                .as_u64()
                .filter(|n| *n >= 1)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| format!("{file}: `count` ({path}: {site}) is a whole number of at least 1"))?,
        };
        out.push(Entry {
            path: path.to_string(),
            site: site.to_string(),
            reason: reason.to_string(),
            count,
        });
    }
    Ok(out)
}

/// The allow list of one run: [`AllowList::allows`] answers per flagged site and counts, [`AllowList::warnings`] names the stale and
/// ambiguous entries.
pub struct AllowList {
    file: String,
    entries: Vec<Entry>,
    hits: Vec<usize>,
}

impl AllowList {
    /// The list `file` with its `entries`.
    pub fn new(file: &str, entries: Vec<Entry>) -> AllowList {
        let hits = vec![0; entries.len()];
        AllowList {
            file: file.to_string(),
            entries,
            hits,
        }
    }

    /// Whether an entry names the site at `path` whose line is `text`; every entry that does counts the site.
    pub fn allows(&mut self, path: &str, text: &str) -> bool {
        let mut named = false;
        for (i, a) in self.entries.iter().enumerate() {
            if a.path == path && text.contains(a.site.as_str()) {
                self.hits[i] += 1;
                named = true;
            }
        }
        named
    }

    /// The warnings of check `id` (rule `rule`) for the entries that matched another number of sites than their count. `looked(path)`
    /// is true when the run read that file or it does not exist: an entry for a file the run never looked at is neither stale nor
    /// matched.
    pub fn warnings(&self, id: &str, rule: u32, looked: impl Fn(&str) -> bool) -> Vec<Finding> {
        let mut out = Vec::new();
        for (a, &hits) in self.entries.iter().zip(&self.hits) {
            if hits > a.count {
                let fix = if a.count > 1 {
                    format!("Lengthen its `site`, or raise its `count` to {hits} if every one is meant.")
                } else {
                    "Lengthen its `site` so it names one line.".to_string()
                };
                out.push(Finding::warn(
                    id,
                    rule,
                    &self.file,
                    1,
                    format!("ambiguous allow entry ({}: {}) matches {hits} sites", a.path, a.site),
                    fix,
                ));
            } else if hits < a.count && looked(&a.path) {
                let (what, fix) = if hits == 0 {
                    ("matches no site".to_string(), format!("Remove it from {}.", self.file))
                } else {
                    (
                        format!("matches only {hits} of its {} sites", a.count),
                        format!("Lower its `count` to {hits}."),
                    )
                };
                out.push(Finding::warn(
                    id,
                    rule,
                    &self.file,
                    1,
                    format!("stale allow entry ({}: {}) {what}", a.path, a.site),
                    fix,
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "conformance/cX-allow.json";

    fn list(entries: &str) -> AllowList {
        AllowList::new(FILE, parse(&format!("{{\"allow\": [{entries}]}}"), FILE).unwrap())
    }

    fn entry(extra: &str) -> String {
        format!("{{\"path\": \"a/b.rs\", \"site\": \"needle\", \"reason\": \"a reason long enough\"{extra}}}")
    }

    fn messages(l: &AllowList, looked: bool) -> Vec<String> {
        l.warnings("CX", 21, |_| looked).into_iter().map(|f| f.message).collect()
    }

    #[test]
    fn an_entry_needs_a_path_a_site_and_a_reason_and_count_is_at_least_1() {
        assert_eq!(list(&entry("")).entries[0].count, 1);
        assert_eq!(list(&entry(", \"count\": 3")).entries[0].count, 3);
        for bad in [
            "{\"path\": \"a\", \"reason\": \"a reason long enough\"}",
            "{\"site\": \"a\", \"reason\": \"a reason long enough\"}",
            "{\"path\": \"a\", \"site\": \"b\", \"reason\": \"short\"}",
        ] {
            assert!(
                parse(&format!("{{\"allow\": [{bad}]}}"), FILE)
                    .unwrap_err()
                    .contains("path, a site")
            );
        }
        for count in ["0", "-1", "1.5", "\"2\""] {
            assert!(
                parse(&format!("{{\"allow\": [{}]}}", entry(&format!(", \"count\": {count}"))), FILE)
                    .unwrap_err()
                    .contains("count")
            );
        }
        assert!(parse("{}", FILE).unwrap_err().contains("no `allow` array"));
        assert!(parse("nope", FILE).unwrap_err().contains("not JSON"));
    }

    #[test]
    fn a_site_is_allowed_only_in_its_own_file_and_only_when_the_line_holds_the_site_text() {
        let mut l = list(&entry(""));
        assert!(l.allows("a/b.rs", "let x = needle;"));
        assert!(!l.allows("a/c.rs", "let x = needle;"));
        assert!(!l.allows("a/b.rs", "let x = other;"));
        assert!(messages(&l, true).is_empty());
    }

    #[test]
    fn no_match_is_stale_more_than_the_count_is_ambiguous_fewer_is_stale() {
        assert_eq!(
            messages(&list(&entry("")), true),
            ["stale allow entry (a/b.rs: needle) matches no site"]
        );
        let mut two = list(&entry(""));
        two.allows("a/b.rs", "needle 1");
        two.allows("a/b.rs", "needle 2");
        assert_eq!(messages(&two, true), ["ambiguous allow entry (a/b.rs: needle) matches 2 sites"]);
        let mut counted = list(&entry(", \"count\": 2"));
        counted.allows("a/b.rs", "needle 1");
        counted.allows("a/b.rs", "needle 2");
        assert!(messages(&counted, true).is_empty());
        let mut short = list(&entry(", \"count\": 3"));
        short.allows("a/b.rs", "needle 1");
        assert_eq!(
            messages(&short, true),
            ["stale allow entry (a/b.rs: needle) matches only 1 of its 3 sites"]
        );
    }

    #[test]
    fn an_entry_for_a_file_the_run_did_not_look_at_is_not_stale() {
        assert!(messages(&list(&entry("")), false).is_empty());
        let mut l = list(&entry(""));
        l.allows("a/b.rs", "needle");
        l.allows("a/b.rs", "needle");
        assert_eq!(messages(&l, false).len(), 1);
    }
}
