//! The port of `scripts/merge-regression-check.mjs` (ADR 0089 outcomes added): files where a merge's result goes backwards
//! against its first parent. A file is flagged when the result differs from the first parent and either
//!
//! (a) the branch did not touch it (its blob equals the merge base's), so the merge itself changed it, or
//! (b) the result removes lines (and does not re-add them elsewhere in the file) that make up at least half of what one earlier
//!     commit added to the file (at least two lines), AND the branch's or the result's blob of the file equals a blob the file
//!     had earlier in the first parent's history: a stale tree brought an old version back. A file the branch deleted, renamed
//!     or rewrote into something new is no revert, so rule (b) skips it.
//!
//! Why: on 2026-11-05 a branch commit built on a stale tree reverted a memory fix, a redaction and test fixes with no conflict
//! (b48df1818). A finding is answered with `Reverted: <path>: <reason>` (or `Hunks-dropped:`) in the merge message.

use super::diff::{DIFF_FLAGS, FileDiff, Hunk, parse, trivial};
use super::git::{Git, short};
use super::{Ctx, Finding, Kind, trailers};
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

/// Every `(path, blob id)` the history of `first` gave `paths`: one `git log --raw` per hundred paths, not one per file.
fn history_blobs(git: &Git, first: &str, paths: &[&str]) -> Result<HashSet<(String, String)>, String> {
    let mut seen = HashSet::new();
    for chunk in paths.chunks(100) {
        let mut args = vec![
            "log",
            "--raw",
            "-z",
            "--no-abbrev",
            "--no-renames",
            "--no-merges",
            "--format=",
            first,
            "--",
        ];
        args.extend(chunk.iter().copied());
        let out = git.text(&args)?;
        // ":<mode> <mode> <old> <new> <status>" NUL path NUL
        let mut tokens = out.split('\0').map(str::trim).filter(|t| !t.is_empty());
        while let Some(meta) = tokens.next() {
            let Some(path) = tokens.next() else { break };
            let ids: Vec<&str> = meta.split(' ').collect();
            if ids.len() >= 5 {
                seen.insert((path.to_string(), ids[2].to_string()));
                seen.insert((path.to_string(), ids[3].to_string()));
            }
        }
    }
    Ok(seen)
}

/// Files where the result goes backwards against the first parent; `second` is the merged branch tip, `base` their merge base.
/// `to_result` is the diff from the first parent to the result.
pub fn check(cx: &Ctx, second: &str, base: &str, to_result: &[FileDiff]) -> Result<Vec<Finding>, String> {
    let (git, first, result, message, generated) = (cx.git, cx.parents[0].as_str(), cx.tree, cx.message, cx.generated);
    let touched: HashSet<String> = git.changed_paths(base, second)?.into_iter().collect();
    let diffs: HashMap<&str, &[Hunk]> = to_result.iter().map(|f| (f.old(), f.hunks.as_slice())).collect();
    let mut blobs: Option<(HashMap<String, String>, HashMap<String, String>)> = None;
    let mut flagged = Vec::new();
    // Files that look like reverts by their lines, with the blobs the stale-tree test needs: judged together afterwards.
    let mut pending: Vec<(String, HashMap<&str, usize>, Vec<String>)> = Vec::new();
    for file in git.changed_paths(first, result)? {
        if generated.is(&file)
            || trailers::path_reason(message, "Reverted", &file).is_some()
            || trailers::path_reason(message, "Hunks-dropped", &file).is_some()
        {
            continue;
        }
        if !touched.contains(&file) {
            flagged.push(Finding {
                kind: Kind::Backwards,
                subject: file,
                detail: "the branch never touched it, but the merge changed it against the first parent".into(),
            });
            continue;
        }
        let hunks = diffs.get(file.as_str()).copied().unwrap_or(&[]);
        let readded = counts(hunks.iter().flat_map(|h| h.added.iter()));
        let mut gone = counts(hunks.iter().flat_map(|h| h.removed.iter()).filter(|l| !trivial(l)));
        for (l, n) in &readded {
            if let Some(g) = gone.get_mut(l) {
                *g = g.saturating_sub(*n);
            }
        }
        // A revert needs at least MIN_LINES removed lines; most files stop here without another git call.
        if gone.values().sum::<usize>() < MIN_LINES {
            continue;
        }
        // The stale-tree test: the branch's or the result's version of the file is one the first parent's history had before.
        let (second_ids, result_ids) = match &blobs {
            Some(b) => b,
            None => blobs.insert((git.blob_ids(second)?, git.blob_ids(result)?)),
        };
        // A file the branch deleted has no branch blob; if the result lacks it too there is nothing to have come back.
        let mut oids: Vec<String> = second_ids.get(&file).cloned().into_iter().collect();
        oids.extend(result_ids.get(&file).cloned());
        if oids.is_empty() {
            continue;
        }
        pending.push((file, gone, oids));
    }
    let paths: Vec<&str> = pending.iter().map(|(f, _, _)| f.as_str()).collect();
    let seen = if paths.is_empty() {
        HashSet::new()
    } else {
        history_blobs(git, first, &paths)?
    };
    for (file, gone, oids) in pending {
        if !oids.iter().any(|o| seen.contains(&(file.clone(), o.clone()))) {
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
                        "removes {back} of the {} lines commit {} added, and its blob is one the file had before (a revert of it?)",
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
