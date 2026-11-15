//! One allow list per check, one site per entry (ER-21; PHPStan's `reportUnmatchedIgnoredErrors` and the `count` of its baseline).
//! An entry names a file (`path`), a piece of the flagged line (`site`), a `reason` of at least 8 characters and, when the same line
//! appears more than once in the file, how many sites it covers (`count`, default 1). A whole file or folder is never allowed. An
//! entry allows at most `count` sites: a site beyond it is reported at its own file and line, as an unlisted one is. After a run, an
//! entry that matched fewer sites than its count is stale, a warning, so the list only ever names the sites that still exist.
//!
//! The reader also owns the error paths, so every check that uses it has the same ones on both hosts (ER-21 class "the two hosts
//! diverge on an error path": C125 in 865498235, then C121 and C124): an allow file that is missing, not JSON, without its list or
//! with a malformed entry is one unwaivable check-broken finding and nothing else ([`read`]), and a file the check cannot read is a
//! not-measured error at that file ([`AllowList::read`]). Used by C121, C124 and C125, each naming its file in
//! [`super::Check::allow_list`]. The port of `conformance/src/util/allow-list.mjs`; `tests/it/check_allow_list.rs` compares the error
//! paths of every user on both hosts.

use super::finding::Finding;
use super::outcome::Outcome;
use super::record::Precondition;
use super::{CheckOutput, Ctx};
use std::collections::HashSet;
use std::rc::Rc;

const UNREAD_FIX: &str = "Make the file readable (permissions) and run the check again.";

/// The check that reads an allow list, as the reader needs it.
#[derive(Debug)]
pub struct Spec {
    /// The check's id.
    pub id: &'static str,
    /// The rule it enforces.
    pub rule: u32,
    /// The allow file.
    pub file: &'static str,
    /// What a file the check cannot read hides (`a disk path`), for the not-measured message.
    pub unseen: &'static str,
    /// The fix of the check-broken finding: how the allow file looks.
    pub restore: &'static str,
}

/// The allow list of check `spec` read through `ctx`, with what `parse` reads beside its entries; or, when the file is missing, not
/// JSON, without its list or with a malformed entry, the check's whole output: one unwaivable check-broken warning at the allow
/// file (the file was the one thing looked at).
pub fn read<T>(
    ctx: &Ctx,
    spec: &'static Spec,
    parse: impl FnOnce(&str) -> Result<(Vec<Entry>, T), String>,
) -> Result<(AllowList, T), Box<CheckOutput>> {
    let parsed = ctx
        .read(spec.file)
        .map(|t| parse(&t))
        .unwrap_or_else(|| Err(format!("{} cannot be read", spec.file)));
    match parsed {
        Ok((entries, data)) => Ok((AllowList::new(spec, entries), data)),
        Err(why) => Err(Box::new(CheckOutput {
            findings: vec![
                Finding::warn(
                    spec.id,
                    spec.rule,
                    spec.file,
                    1,
                    format!("the check cannot look: {why}"),
                    spec.restore,
                )
                .unwaivable()
                .with_outcome(Outcome::CheckBroken),
            ],
            probes: 1,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{} is read", spec.file),
            },
        })),
    }
}

/// The shared parser as [`read`] takes it, for a check with no other list in its allow file.
pub fn entries_only(file: &str) -> impl FnOnce(&str) -> Result<(Vec<Entry>, ()), String> + '_ {
    move |text| parse(text, file).map(|e| (e, ()))
}

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
            // `1.0` is the number 1 to JavaScript's JSON.parse, which the other host uses: a whole float counts as its integer.
            Some(c) => c
                .as_u64()
                .or_else(|| {
                    c.as_f64()
                        .filter(|f| f.fract() == 0.0 && (1.0..9_007_199_254_740_992.0).contains(f))
                        .map(whole)
                })
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

/// A whole, positive float below 2^53 as its integer (the caller checked both).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "whole, positive and below 2^53"
)]
fn whole(f: f64) -> u64 {
    f as u64
}

/// What [`AllowList::admit`] says about one site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    /// An entry with room left names it; the site counts on it.
    Allowed,
    /// Entries name it, but every one is at its count: the caller reports it at its own file and line.
    Beyond,
    /// No entry names it.
    Unlisted,
}

/// The allow list of one run: [`AllowList::read`] reads a file the check looks at, [`AllowList::admit`]
/// answers per flagged site and counts, [`AllowList::warnings`] names the stale entries.
pub struct AllowList {
    spec: &'static Spec,
    entries: Vec<Entry>,
    hits: Vec<usize>,
    scanned: HashSet<String>,
    unread: HashSet<String>,
}

impl AllowList {
    /// The list of check `spec` with its `entries`.
    pub fn new(spec: &'static Spec, entries: Vec<Entry>) -> AllowList {
        let hits = vec![0; entries.len()];
        AllowList {
            spec,
            entries,
            hits,
            scanned: HashSet::new(),
            unread: HashSet::new(),
        }
    }

    /// The text of `path`, a file the check looks at, or, when it cannot be read, a not-measured error at that file, which the check
    /// reports in its place (the file is then neither scanned nor a reason to call an entry stale).
    pub fn read(&mut self, ctx: &Ctx, path: &str) -> Result<Rc<String>, Box<Finding>> {
        if let Some(text) = ctx.read(path) {
            self.scanned.insert(path.to_string());
            return Ok(text);
        }
        self.unread.insert(path.to_string());
        let Spec { id, rule, unseen, .. } = *self.spec;
        Err(Box::new(
            Finding::error(
                id,
                rule,
                path,
                1,
                format!("not measured: {path} could not be read, so {unseen} in it cannot be seen"),
                UNREAD_FIX,
            )
            .unwaivable()
            .with_outcome(Outcome::NotMeasured),
        ))
    }

    /// The check's output: its `findings`, then the warnings of the entries; every file the run read is a probe.
    pub fn output(self, ctx: &Ctx, mut findings: Vec<Finding>) -> CheckOutput {
        findings.extend(self.warnings(ctx));
        CheckOutput {
            findings,
            probes: self.scanned.len(),
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{} is read", self.spec.file),
            },
        }
    }

    /// What the list says about one flagged site at `path`: `matches(site)` says whether an entry's `site` names it (C121 and C124:
    /// the flagged line holds the site's text; C125: the literal starts with the site's whole path components). The site counts on the
    /// first matching entry with room left; when every matching entry is at its count it is [`Admit::Beyond`], and the caller reports
    /// it at its own file and line.
    pub fn admit(&mut self, path: &str, matches: impl Fn(&str) -> bool) -> Admit {
        let named: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].path == path && matches(&self.entries[i].site))
            .collect();
        if named.is_empty() {
            return Admit::Unlisted;
        }
        match named.into_iter().find(|&i| self.hits[i] < self.entries[i].count) {
            Some(i) => {
                self.hits[i] += 1;
                Admit::Allowed
            }
            None => Admit::Beyond,
        }
    }

    /// The warnings for the entries that matched fewer sites than their count ([`AllowList::admit`] never counts more). An entry is stale only for a file the
    /// run read, or one that does not exist: an entry for a file the run never looked at, or could not read, is neither stale nor
    /// matched.
    pub fn warnings(&self, ctx: &Ctx) -> Vec<Finding> {
        let Spec { id, rule, file, .. } = *self.spec;
        let looked = |p: &str| self.scanned.contains(p) || (!self.unread.contains(p) && ctx.read(p).is_none());
        let mut out = Vec::new();
        for (a, &hits) in self.entries.iter().zip(&self.hits) {
            if hits < a.count && looked(&a.path) {
                let (what, fix) = if hits == 0 {
                    ("matches no site".to_string(), format!("Remove it from {file}."))
                } else {
                    (
                        format!("matches only {hits} of its {} sites", a.count),
                        format!("Lower its `count` to {hits}."),
                    )
                };
                out.push(Finding::warn(
                    id,
                    rule,
                    file,
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
    use crate::check::config::Config;
    use crate::check::tree::Tree;

    const FILE: &str = "conformance/cX-allow.json";
    static SPEC: Spec = Spec {
        id: "CX",
        rule: 21,
        file: FILE,
        unseen: "a site",
        restore: "Restore it.",
    };

    fn list(entries: &str) -> AllowList {
        AllowList::new(&SPEC, parse(&format!("{{\"allow\": [{entries}]}}"), FILE).unwrap())
    }

    fn entry(extra: &str) -> String {
        format!("{{\"path\": \"a/b.rs\", \"site\": \"needle\", \"reason\": \"a reason long enough\"{extra}}}")
    }

    /// Runs `f` with a run's view of a tree of `files`.
    fn with_ctx<R>(files: &[(&str, &str)], f: impl FnOnce(&Ctx) -> R) -> R {
        let tree = Tree::in_memory(files);
        let config = Config::bare("fixture");
        let ctx = Ctx::new(&tree, tree.files(), None, &config, None, "2026-11-15".into());
        f(&ctx)
    }

    /// The warnings' messages; `looked`: the run read `a/b.rs` (else it never looked at it).
    fn messages(mut l: AllowList, looked: bool) -> Vec<String> {
        with_ctx(&[("a/b.rs", "needle")], |ctx| {
            if looked {
                l.read(ctx, "a/b.rs").unwrap();
            }
            l.warnings(ctx).into_iter().map(|f| f.message).collect()
        })
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
        // A whole float is its integer, as JSON.parse reads it in the other host.
        assert_eq!(list(&entry(", \"count\": 1.0")).entries[0].count, 1);
        assert_eq!(list(&entry(", \"count\": 3.0")).entries[0].count, 3);
        for count in ["0", "-1", "1.5", "0.0", "\"2\""] {
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
    fn a_site_is_allowed_only_in_its_own_file_and_only_when_the_entry_names_it() {
        let mut l = list(&entry(""));
        let holds = |line: &'static str| move |site: &str| line.contains(site);
        assert_eq!(l.admit("a/c.rs", holds("let x = needle;")), Admit::Unlisted);
        assert_eq!(l.admit("a/b.rs", holds("let x = other;")), Admit::Unlisted);
        assert_eq!(l.admit("a/b.rs", holds("let x = needle;")), Admit::Allowed);
        assert!(messages(l, true).is_empty());
    }

    #[test]
    fn admit_counts_up_to_the_count_and_calls_the_rest_beyond_fewer_is_stale() {
        let mut l = list(&entry(", \"count\": 2"));
        let exact = |site: &str| site == "needle";
        assert_eq!(l.admit("a/b.rs", exact), Admit::Allowed);
        assert_eq!(l.admit("a/b.rs", exact), Admit::Allowed);
        assert_eq!(l.admit("a/b.rs", exact), Admit::Beyond);
        assert!(
            messages(l, true).is_empty(),
            "a site beyond the count is the caller's finding at the site, not a warning on the entry"
        );
        assert_eq!(
            messages(list(&entry("")), true),
            ["stale allow entry (a/b.rs: needle) matches no site"]
        );
        let mut short = list(&entry(", \"count\": 3"));
        short.admit("a/b.rs", exact);
        assert_eq!(
            messages(short, true),
            ["stale allow entry (a/b.rs: needle) matches only 1 of its 3 sites"]
        );
    }

    #[test]
    fn an_entry_for_a_file_the_run_did_not_look_at_is_not_stale() {
        assert!(messages(list(&entry("")), false).is_empty());
    }

    #[test]
    fn the_reader_owns_the_broken_file_and_the_unreadable_file() {
        let allow = format!("{{\"allow\": [{}, {}]}}", entry(""), entry("").replace("a/b.rs", "a/gone.rs"));
        with_ctx(&[(FILE, allow.as_str()), ("a/b.rs", "x")], |ctx| {
            let Ok((mut l, ())) = read(ctx, &SPEC, entries_only(FILE)) else {
                panic!("the allow file is read")
            };
            assert_eq!(l.read(ctx, "a/b.rs").unwrap().as_str(), "x");
            let out = l.output(ctx, Vec::new());
            assert_eq!(out.probes, 1);
            let said: Vec<String> = out.findings.into_iter().map(|f| f.message).collect();
            assert_eq!(
                said,
                [
                    "stale allow entry (a/b.rs: needle) matches no site",
                    "stale allow entry (a/gone.rs: needle) matches no site"
                ]
            );
        });
        for text in [None, Some("nope"), Some("{}"), Some("{\"allow\": [{\"path\": \"a\"}]}")] {
            let files: Vec<(&str, &str)> = text.map(|t| (FILE, t)).into_iter().collect();
            with_ctx(&files, |ctx| {
                let out = *read(ctx, &SPEC, entries_only(FILE)).err().unwrap();
                assert_eq!(out.findings.len(), 1, "{text:?}");
                let f = &out.findings[0];
                assert_eq!((f.file.as_str(), f.unwaivable, f.outcome), (FILE, true, Some(Outcome::CheckBroken)));
                assert!(f.message.starts_with("the check cannot look: "), "{}", f.message);
            });
        }
        with_ctx(&[], |ctx| {
            let mut l = list(&entry(""));
            let f = l.read(ctx, "a/b.rs").unwrap_err();
            assert_eq!(
                (f.file.as_str(), f.unwaivable, f.outcome),
                ("a/b.rs", true, Some(Outcome::NotMeasured))
            );
            assert_eq!(f.message, "not measured: a/b.rs could not be read, so a site in it cannot be seen");
            assert!(
                l.warnings(ctx).is_empty(),
                "an entry for a file that could not be read is not stale: its sites are unknown"
            );
        });
    }
}
