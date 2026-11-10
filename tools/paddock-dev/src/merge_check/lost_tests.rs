//! Tests a parent has that the result lacks, judged three-way. For every source file that differs between a parent and the
//! result and whose removed lines could hold a test (an attribute naming `test`, a `fn`, an `it(` / `test(` call), the test
//! names of both versions are counted (a renamed file is read at its old path in the parent and at its new one in the result,
//! and a test that moved to another file is found by name). Only files a side changed are looked at: those changed between the
//! base and this parent, and those changed between another parent and the result. A name the result has less often than a
//! three-way merge expects is lost unless
//!
//! - the other side deleted it on purpose: the count a three-way merge expects is the parent's plus what the other parents
//!   changed since the base (`before + other - base`, for each other parent), and the result has at least that many; or
//! - the message says `Tests-removed: <name>: <reason>` (or `<file>::<name>`).
//!
//! Names are compared without their file or module, so a test that moved is not lost.

use super::diff::FileDiff;
use super::git::{Git, short};
use super::{Ctx, Finding, Kind, testnames, trailers};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Test names with their counts, and the first file each was seen in.
#[derive(Default)]
struct Tally {
    counts: BTreeMap<String, usize>,
    file: BTreeMap<String, String>,
}

impl Tally {
    fn of(&self, name: &str) -> usize {
        self.counts.get(name).copied().unwrap_or(0)
    }
}

/// Counts the tests of `strict` files (an unreadable one is an error: the check cannot see its tests) and of `lenient` files,
/// which were only found by a text search (one that does not parse or is not UTF-8 is skipped: it is a fixture, not a test file).
fn tally(git: &Git, rev: &str, strict: &BTreeSet<String>, lenient: &BTreeSet<String>) -> Result<Tally, String> {
    let mut t = Tally::default();
    let mut add = |f: &String, names: Vec<String>| {
        for n in names {
            *t.counts.entry(n.clone()).or_insert(0) += 1;
            t.file.entry(n).or_insert_with(|| f.clone());
        }
    };
    for f in strict {
        if let Some(text) = git.text_at(rev, f)? {
            add(f, testnames::names(f, &text)?);
        }
    }
    for f in lenient.iter().filter(|f| !strict.contains(*f)) {
        if let Ok(Some(text)) = git.text_at(rev, f)
            && let Ok(names) = testnames::names(f, &text)
        {
            add(f, names);
        }
    }
    Ok(t)
}

/// The text patterns that find test `name` in source: a Rust test is `fn <name>`, a script test is its quoted name (in its
/// escaped form too, so `cargo's` is found as `cargo\'s`).
fn search_patterns(name: &str) -> Vec<String> {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return vec![
            format!("fn {name}"),
            format!("'{name}'"),
            format!("\"{name}\""),
            format!("`{name}`"),
        ];
    }
    let escaped = name.replace('\\', "\\\\").replace('\'', "\\'").replace('"', "\\\"");
    let mut v = vec![name.to_string()];
    if escaped != name {
        v.push(escaped);
    }
    v
}

/// Whether a line the result removes could be part of a test: reading every changed file would be a process per file.
fn test_shaped(line: &str, js_call: &Regex, rust_fn: &Regex) -> bool {
    let t = line.trim();
    (t.starts_with("#[") && t.contains("test")) || rust_fn.is_match(t) || js_call.is_match(line)
}

/// The source files of `rev` that mention one of `names` (a moved test is found where it went).
fn files_naming(git: &Git, rev: &str, names: &[&String]) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    let types = ["*.rs", "*.ts", "*.tsx", "*.js", "*.jsx", "*.mjs", "*.cjs"];
    for chunk in names.chunks(100) {
        let mut args: Vec<String> = vec!["grep".into(), "-l".into(), "-z".into(), "-F".into()];
        for n in chunk {
            for pattern in search_patterns(n) {
                args.push("-e".into());
                args.push(pattern);
            }
        }
        args.push(rev.into());
        args.push("--".into());
        args.extend(types.iter().map(|t| t.to_string()));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = git.bytes_or_none(&refs)?;
        let prefix = format!("{rev}:");
        for p in String::from_utf8_lossy(&out).split('\0').filter(|p| !p.is_empty()) {
            found.insert(p.strip_prefix(prefix.as_str()).unwrap_or(p).to_string());
        }
    }
    Ok(found)
}

/// Whether a file can hold a lost test: a side changed it (base to this parent, or another parent to the result). A file
/// neither side changed is the same everywhere, and a test in it cannot be lost by a merge.
fn changed_by_a_side(old_path: &str, base_to_parent: &HashSet<String>, others_to_result: &[&[FileDiff]]) -> bool {
    base_to_parent.contains(old_path)
        || others_to_result
            .iter()
            .any(|d| d.iter().any(|f| f.old() == old_path || f.path == old_path))
}

/// The tests of `parents[index]` that `result` lacks and neither the other side's deletion nor the message explains.
/// `to_result` is the diff from that parent to the result; `base` is the merge base of the parent with the first parent.
pub fn check(cx: &Ctx, index: usize, base: &str, to_result: &[FileDiff]) -> Result<Vec<Finding>, String> {
    let (git, parents, result, message, generated) = (cx.git, cx.parents, cx.tree, cx.message, cx.generated);
    let parent = parents[index].as_str();
    let js_call = Regex::new(r"(?:^|[^.\w$])(?:it|test)(?:\.\w+)?\s*\(").map_err(|e| e.to_string())?;
    let rust_fn = Regex::new(r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s").map_err(|e| e.to_string())?;
    // The parent's files whose removed lines could hold a test, plus where a renamed one went.
    let base_to_parent: HashSet<String> = git.changed_paths(base, parent)?.into_iter().collect();
    let others_to_result: Vec<&[FileDiff]> = cx
        .diffs
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != index)
        .map(|(_, d)| d.as_slice())
        .collect();
    let mut files = BTreeSet::new();
    for f in to_result.iter().filter(|f| {
        testnames::is_source(f.old()) && !generated.is(f.old()) && changed_by_a_side(f.old(), &base_to_parent, &others_to_result)
    }) {
        if f.hunks
            .iter()
            .flat_map(|h| h.removed.iter())
            .any(|l| test_shaped(l, &js_call, &rust_fn))
        {
            files.insert(f.old().to_string());
            files.insert(f.path.clone());
        }
    }
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let lost = |before: &Tally, after: &Tally| -> Vec<String> {
        before
            .counts
            .iter()
            .filter(|(n, c)| after.of(n) < **c)
            .map(|(n, _)| n.clone())
            .collect()
    };
    let mut found = BTreeSet::new();
    let (mut before, mut after) = (tally(git, parent, &files, &found)?, tally(git, result, &files, &found)?);
    let candidates = lost(&before, &after);
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    // A moved test is found where it went; both sides are counted over the same files.
    let named: Vec<&String> = candidates.iter().collect();
    found = files_naming(git, result, &named)?;
    if found.iter().any(|f| !files.contains(f)) {
        (before, after) = (tally(git, parent, &files, &found)?, tally(git, result, &files, &found)?);
    }
    let base_tally = tally(git, base, &files, &found)?;
    let mut others = Vec::new();
    for (j, p) in parents.iter().enumerate() {
        if j != index {
            others.push(tally(git, p, &files, &found)?);
        }
    }
    let mut out = Vec::new();
    for name in lost(&before, &after) {
        // What a three-way merge keeps: this parent's count, moved by whatever each other parent changed since the base.
        let base_count = base_tally.of(&name) as i64;
        let expected = before.of(&name) as i64 + others.iter().map(|t| t.of(&name) as i64 - base_count).sum::<i64>();
        let deleted_on_purpose = !others.is_empty() && (after.of(&name) as i64) >= expected.max(0);
        let file = before.file.get(&name).map(String::as_str).unwrap_or("");
        let qualified = format!("{file}::{name}");
        let reasoned =
            trailers::reason(message, "Tests-removed", &name).is_some() || trailers::reason(message, "Tests-removed", &qualified).is_some();
        if deleted_on_purpose || reasoned {
            continue;
        }
        out.push(Finding {
            kind: Kind::LostTest,
            subject: name.clone(),
            detail: format!(
                "parent {} ({}) has {} in {file}, the result {}, and the message has no `Tests-removed: {name}: <reason>` (or `{qualified}`)",
                index + 1,
                short(parent),
                before.of(&name),
                after.of(&name),
            ),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_searched_as_it_is_written_in_source() {
        assert_eq!(search_patterns("new_case")[0], "fn new_case");
        assert!(search_patterns("new_case").contains(&"'new_case'".to_string()));
        assert_eq!(
            search_patterns("keeps cargo's default"),
            vec!["keeps cargo's default", "keeps cargo\\'s default"]
        );
        assert_eq!(search_patterns("plain words"), vec!["plain words"]);
    }

    #[test]
    fn only_files_a_side_changed_can_lose_a_test() {
        let changed: HashSet<String> = ["a.rs".to_string()].into();
        let d = |p: &str| FileDiff {
            path: p.into(),
            old_path: None,
            deleted: false,
            hunks: Vec::new(),
        };
        let other = [d("b.rs")];
        assert!(changed_by_a_side("a.rs", &changed, &[&other]));
        assert!(changed_by_a_side("b.rs", &changed, &[&other]));
        assert!(!changed_by_a_side("c.rs", &changed, &[&other]));
    }
}
