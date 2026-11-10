//! Files the merge took whole from one parent although the other parent changed them too. A file is flagged when its blob in
//! the result equals parent P's, and every other parent changed it since the merge base (its blob differs from the base's and
//! from the result's): the other side's changes cannot be in the result. This is the cheap, precise signal for an "ours" or
//! "theirs" resolution of a file both sides edited (087570da4, the Dyno docs of 9e18230c5). The answer is
//! `Hunks-dropped: <path>: <reason>`. Generated files are skipped, and so are rendered images (`.png`, `.svg`, ...): they
//! follow their generator, which is flagged itself.

use super::generated::Generated;
use super::git::short;
use super::{Finding, Kind, trailers};
use std::collections::HashMap;

fn image(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".svg", ".jpg", ".jpeg", ".gif", ".webp", ".ico"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// The shared rule of ADR 0097's approval walk too: a merge may only carry a change one side made since the base. For one path,
/// `result` is acceptable when it differs from parent `mine`, or when every other parent still has the base's version (only
/// this side changed it). It is not when `result == mine`, this side changed the file (`mine != base`), and every other parent
/// has the file in a version that is neither the base's nor `mine`'s: the merge itself changed the file by dropping the other
/// side's change. A parent that lacks the file is a deletion, a conflict resolved on purpose, not judged here. Versions are
/// blob ids.
pub fn merge_changed_it(base: Option<&str>, mine: &str, others: &[Option<&str>], result: Option<&str>) -> bool {
    result == Some(mine) && base != Some(mine) && !others.is_empty() && others.iter().all(|o| o.is_some() && *o != base && *o != Some(mine))
}

/// The blob ids of one revision: path to blob.
pub type Blobs = HashMap<String, String>;

/// Files `result` took whole from `parents[index]` against the other parents. `blobs[j]` is parent `j`'s listing, `base` the
/// merge base's, `result` the result's.
pub fn check(
    parents: &[String],
    index: usize,
    blobs: &[Blobs],
    base: &Blobs,
    result: &Blobs,
    message: &str,
    generated: &Generated,
) -> Vec<Finding> {
    let mine = &blobs[index];
    let mut out = Vec::new();
    let mut paths: Vec<&String> = mine.keys().collect();
    paths.sort();
    for path in paths {
        let oid = &mine[path];
        let others: Vec<Option<&str>> = blobs
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != index)
            .map(|(_, b)| b.get(path).map(String::as_str))
            .collect();
        if !merge_changed_it(
            base.get(path).map(String::as_str),
            oid,
            &others,
            result.get(path).map(String::as_str),
        ) || generated.is(path)
            || image(path)
            || trailers::path_reason(message, "Hunks-dropped", path).is_some()
        {
            continue;
        }
        if trailers::path_reason(message, "Reverted", path).is_some() {
            continue;
        }
        out.push(Finding {
            kind: Kind::WholeFile,
            subject: path.clone(),
            detail: format!(
                "the result is parent {}'s ({}) version of the file, and the other parent changed it since the merge base too, so its changes are not in the result; the message has no `Hunks-dropped: {path}: <reason>`",
                index + 1,
                short(&parents[index])
            ),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_merge_may_only_carry_what_one_side_changed() {
        // Only this side changed it: fine. Both changed it and the result is this side's whole: the merge changed it.
        assert!(!merge_changed_it(Some("b"), "p", &[Some("b")], Some("p")));
        assert!(merge_changed_it(Some("b"), "p", &[Some("o")], Some("p")));
        // The result is neither parent's (a real merge), or this side did not change it, or the other side deleted it.
        assert!(!merge_changed_it(Some("b"), "p", &[Some("o")], Some("m")));
        assert!(!merge_changed_it(Some("b"), "b", &[Some("o")], Some("b")));
        assert!(!merge_changed_it(Some("b"), "p", &[None], Some("p")));
        // A file added on both sides with different content has no base version.
        assert!(merge_changed_it(None, "p", &[Some("o")], Some("p")));
        // Octopus: every other parent must have changed it.
        assert!(!merge_changed_it(Some("b"), "p", &[Some("o"), Some("b")], Some("p")));
        assert!(merge_changed_it(Some("b"), "p", &[Some("o"), Some("q")], Some("p")));
    }
}
