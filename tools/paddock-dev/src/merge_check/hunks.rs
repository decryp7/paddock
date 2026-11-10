//! Hunks a parent added since the merge base that are missing from the result. A merge may drop a hunk only through a conflict
//! resolution, and then its message says so: `Hunks-dropped: <path>: <reason>` (or a folder ending in `/`). Without that
//! reason the hunk was lost silently (087570da4: a merge kept one side's files and dropped the other side's verdict, hook and
//! test).
//!
//! A hunk counts when it adds at least two non-trivial lines; it is missing when at least half of them (and at least two) are
//! among the lines the result removes from the parent's file and does not add back elsewhere in it, each line counted once.
//! A file that every other parent lacks under its name in the base (deleted on the other side; for a file this parent
//! renamed, the name it had before) is skipped: that is a modify/delete conflict, resolved on
//! purpose. A file git merged cleanly, and the result is exactly its merge, is skipped. Generated files ([`super::generated`]) are skipped: they are regenerated, not merged.

use super::diff::{DIFF_FLAGS, FileDiff, parse, trivial};
use super::git::short;
use super::{Ctx, Finding, Kind, trailers};
use std::collections::{HashMap, HashSet};

const MIN_LINES: usize = 2;

/// The lines `diff` (parent to result) removes from a file and does not add back in it, with their counts.
fn gone(file: &FileDiff) -> HashMap<&str, usize> {
    let mut m: HashMap<&str, usize> = HashMap::new();
    for l in file.hunks.iter().flat_map(|h| h.removed.iter()) {
        *m.entry(l.as_str()).or_insert(0) += 1;
    }
    for l in file.hunks.iter().flat_map(|h| h.added.iter()) {
        if let Some(n) = m.get_mut(l.as_str()) {
            *n = n.saturating_sub(1);
        }
    }
    m
}

/// The hunks of `parents[index]` (relative to `base`) that `result` lacks. `to_result` is the diff from that parent to the
/// result.
pub fn check(cx: &Ctx, index: usize, base: &str, to_result: &[FileDiff]) -> Result<Vec<Finding>, String> {
    let (git, parents, message, generated) = (cx.git, cx.parents, cx.message, cx.generated);
    let parent = parents[index].as_str();
    let mut args = vec!["diff"];
    args.extend(DIFF_FLAGS);
    args.extend([base, parent]);
    let added_by_parent = parse(&git.text(&args)?)?;
    let mut others: Option<Vec<HashSet<String>>> = None;
    let by_path: HashMap<&str, &FileDiff> = to_result.iter().map(|f| (f.old(), f)).collect();
    let mut out = Vec::new();
    for file in added_by_parent.iter().filter(|f| !f.deleted && !generated.is(&f.path)) {
        let Some(change) = by_path.get(file.path.as_str()) else { continue };
        // Git merged it cleanly and the result is that merge: nothing this parent added can be missing from it.
        if cx.merged_cleanly(&file.path) {
            continue;
        }
        let hunks: Vec<Vec<&String>> = file
            .hunks
            .iter()
            .map(|h| h.added.iter().filter(|l| !trivial(l)).collect::<Vec<_>>())
            .filter(|h| h.len() >= MIN_LINES)
            .collect();
        if hunks.is_empty() {
            continue;
        }
        let mut left = gone(change);
        let (mut lost, mut worst) = (0usize, 0usize);
        for h in &hunks {
            let missing = h
                .iter()
                .filter(|l| match left.get_mut(l.as_str()) {
                    Some(n) if *n > 0 => {
                        *n -= 1;
                        true
                    }
                    _ => false,
                })
                .count();
            if missing >= MIN_LINES && missing * 2 >= h.len() {
                lost += 1;
                worst = worst.max(missing);
            }
        }
        if lost == 0 || trailers::path_reason(message, "Hunks-dropped", &file.path).is_some() {
            continue;
        }
        if others.is_none() {
            let mut sets = Vec::new();
            for p in parents.iter().enumerate().filter(|(j, _)| *j != index).map(|(_, p)| p) {
                sets.push(
                    git.paths(&["ls-tree", "-r", "--name-only", "-z", p])?
                        .into_iter()
                        .collect::<HashSet<_>>(),
                );
            }
            others = Some(sets);
        }
        if others
            .as_ref()
            .is_some_and(|sets| !sets.is_empty() && sets.iter().all(|s| !s.contains(file.old())))
        {
            continue;
        }
        let absent = if change.deleted { " (the file is not in the result)" } else { "" };
        out.push(Finding {
            kind: Kind::DroppedHunk,
            subject: file.path.clone(),
            detail: format!(
                "{lost} of the {} hunk(s) parent {} ({}) added since the merge base are missing from the result{absent}, up to {worst} line(s) each, and the message has no `Hunks-dropped: {}: <reason>`",
                hunks.len(),
                index + 1,
                short(parent),
                file.path
            ),
        });
    }
    Ok(out)
}
