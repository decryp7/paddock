//! Tests a parent has that the result lacks. For every source file that differs between a parent and the result and whose
//! removed lines could hold a test (an attribute naming `test`, a `fn`, an `it(` / `test(` call), the test names of both
//! versions are counted; a name the parent has more often than the result is lost, unless the merge message says
//! `Tests-removed: <name>: <reason>`. Names are compared without their file or module, so a test that moved is not lost.

use super::diff::FileDiff;
use super::git::{Git, short};
use super::{Finding, Kind, testnames, trailers};
use regex::Regex;
use std::collections::BTreeMap;

fn tally(git: &Git, rev: &str, files: &[&str]) -> Result<BTreeMap<String, usize>, String> {
    let mut m = BTreeMap::new();
    for f in files {
        if let Some(text) = git.text_at(rev, f)? {
            for n in testnames::names(f, &text)? {
                *m.entry(n).or_insert(0) += 1;
            }
        }
    }
    Ok(m)
}

/// Whether a line the result removes could be part of a test: reading every changed file would be a process per file.
fn test_shaped(line: &str, js_call: &Regex, rust_fn: &Regex) -> bool {
    let t = line.trim();
    (t.starts_with("#[") && t.contains("test")) || rust_fn.is_match(t) || js_call.is_match(line)
}

/// The tests of `parent` (the `index`-th) that `result` lacks and the message does not explain. `to_result` is the diff from
/// the parent to the result.
pub fn check(git: &Git, parent: &str, index: usize, result: &str, to_result: &[FileDiff], message: &str) -> Result<Vec<Finding>, String> {
    let js_call = Regex::new(r"(?:^|[^.\w$])(?:it|test)(?:\.\w+)?\s*\(").map_err(|e| e.to_string())?;
    let rust_fn = Regex::new(r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s").map_err(|e| e.to_string())?;
    let files: Vec<&str> = to_result
        .iter()
        .filter(|f| testnames::is_source(&f.path))
        .filter(|f| {
            f.hunks
                .iter()
                .flat_map(|h| h.removed.iter())
                .any(|l| test_shaped(l, &js_call, &rust_fn))
        })
        .map(|f| f.path.as_str())
        .collect();
    let before = tally(git, parent, &files)?;
    let after = tally(git, result, &files)?;
    let mut out = Vec::new();
    for (name, n) in before {
        let now = after.get(&name).copied().unwrap_or(0);
        if now >= n || trailers::reason(message, "Tests-removed", &name).is_some() {
            continue;
        }
        out.push(Finding {
            kind: Kind::LostTest,
            subject: name.clone(),
            detail: format!(
                "parent {} ({}) has {n}, the result {now}, and the message has no `Tests-removed: {name}: <reason>`",
                index + 1,
                short(parent)
            ),
        });
    }
    Ok(out)
}
