//! The port of `scripts/merge-regression-check.mjs` (ADR 0089 outcomes added): files where a merge's result goes backwards
//! against its first parent. A file is flagged when the result differs from the first parent and either
//!
//! (a) the branch did not touch it (its blob equals the merge base's), so the merge itself changed it, or
//! (b) the result removes lines (and does not re-add them elsewhere in the file) that make up at least half of what one earlier
//!     commit added to the file (at least two lines): a revert of that commit, as when a branch commit is built on a stale tree.
//!
//! Why: on 2026-11-05 a branch commit built on a stale tree reverted a memory fix, a redaction and test fixes with no conflict
//! (b48df1818).

use super::diff::{DIFF_FLAGS, FileDiff, Hunk, parse, trivial};
use super::git::{Git, short};
use super::{Finding, Kind};
use std::collections::{HashMap, HashSet};

/// Commits touching a file that are searched for the commit being reverted.
const DEPTH: usize = 50;
const MIN_LINES: usize = 2;
const MIN_SHARE: f64 = 0.5;

fn counts<'a>(lines: impl Iterator<Item = &'a String>) -> HashMap<&'a str, usize> {
    let mut m = HashMap::new();
    for l in lines {
        *m.entry(l.as_str()).or_insert(0) += 1;
    }
    m
}

/// Files where `result` (a tree) goes backwards against `first`; `second` is the merged branch tip, `base` their merge base.
/// `to_result` is the diff from `first` to `result`.
pub fn check(git: &Git, first: &str, second: &str, result: &str, base: &str, to_result: &[FileDiff]) -> Result<Vec<Finding>, String> {
    let touched: HashSet<String> = git.changed_paths(base, second)?.into_iter().collect();
    let diffs: HashMap<&str, &[Hunk]> = to_result.iter().map(|f| (f.path.as_str(), f.hunks.as_slice())).collect();
    let mut flagged = Vec::new();
    for file in git.changed_paths(first, result)? {
        if !touched.contains(&file) {
            flagged.push(Finding {
                kind: Kind::Backwards,
                subject: file,
                detail: "the branch never touched it, but the merge changed it against the first parent".into(),
            });
            continue;
        }
        let hunks = diffs.get(file.as_str()).copied().unwrap_or(&[]);
        let added: Vec<String> = hunks.iter().flat_map(|h| h.added.iter().cloned()).collect();
        let removed: Vec<String> = hunks.iter().flat_map(|h| h.removed.iter().cloned()).collect();
        let readded = counts(added.iter());
        let mut gone = counts(removed.iter().filter(|l| !trivial(l)));
        for (l, n) in &readded {
            if let Some(g) = gone.get_mut(l) {
                *g = g.saturating_sub(*n);
            }
        }
        // A revert needs at least MIN_LINES removed lines; most files stop here without a `git log`.
        if gone.values().sum::<usize>() < MIN_LINES {
            continue;
        }
        let depth = DEPTH.to_string();
        let mut args = vec!["log", "--no-merges", "-n", depth.as_str(), "--format=%x01%h", "-p"];
        args.extend(DIFF_FLAGS);
        args.extend([first, "--", file.as_str()]);
        let log = git.text(&args)?;
        for chunk in log.split('\x01').filter(|c| !c.is_empty()) {
            let sha = chunk.lines().next().unwrap_or("").trim();
            let mine: Vec<String> = parse(chunk)?
                .iter()
                .flat_map(|f| f.hunks.iter())
                .flat_map(|h| h.added.iter())
                .filter(|l| !trivial(l))
                .cloned()
                .collect();
            let back = mine.iter().filter(|l| gone.get(l.as_str()).copied().unwrap_or(0) > 0).count();
            if back >= MIN_LINES && back as f64 >= MIN_SHARE * mine.len() as f64 {
                flagged.push(Finding {
                    kind: Kind::Backwards,
                    subject: file.clone(),
                    detail: format!(
                        "removes {back} of the {} lines commit {} added (a revert of it?)",
                        mine.len(),
                        short(sha)
                    ),
                });
                break;
            }
        }
    }
    Ok(flagged)
}
