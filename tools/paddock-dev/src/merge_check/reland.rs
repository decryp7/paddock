//! The `Reverted:` lines of a re-land (`./dev merge check --write-reverted`). A member that re-lands work the train reverted
//! (subject `revert: reapply ...` or `Reapply "..."`, git's own `This reverts commit <revert>`) undoes the revert's lines, which the
//! backwards detector ([`super::backwards`]) reports file by file; the integrator then wrote one `Reverted: <path>: <reason>` per
//! file by hand (the Dyno screenshots merge needed 23). This writes them, and only for a finding it can tie to the re-land:
//!
//! - the member (`base..member`, no merges) has a commit `L` whose subject says it re-applies something, and
//! - `L` names a commit `S` of the target as the one it reverts (`This reverts commit S`, a full or abbreviated sha), or names a
//!   commit `X` it re-applies (`(cherry picked from commit X)`) that a commit `S` of the target reverts, and
//! - `S` is itself a revert (it has a `This reverts commit` line or a `Revert "..."` subject), and
//! - the finding's file is changed by both `L` and `S`,
//! - no other finding names that file (a whole-file take or a dropped hunk of it is not the re-land's), and
//! - the target has not changed the file since `S` (`git diff S <target> -- <path>` is empty): a later change on the target,
//!   a fix for example, is not part of what the re-land undoes, and a merge that lost it must not be excused here.
//!
//! Every other finding (a file the re-land does not touch, a member that only looks like one, a revert `S` of something else) is
//! left for a person; this never answers a finding it cannot tie. It reads and prints; it changes no merge.

use super::git::{Git, short};
use super::reverted_base::{resolve, trailer_targets};
use super::{Finding, Kind, Merge};
use std::collections::HashSet;

/// A commit of the member that re-lands.
struct Reland {
    sha: String,
    subject: String,
    /// The target's revert commits this re-land undoes, with the files each changed.
    undoes: Vec<(String, Vec<String>)>,
    files: HashSet<String>,
}

/// Whether a subject says the commit re-applies work (any case): `revert: reapply ...` (scope and `!` allowed), git's `Reapply "..."`, or the default
/// subject of `git revert` of a revert, `Revert "Revert ..."` (what `merge sync-member` tells the author to run).
pub fn is_reland_subject(subject: &str) -> bool {
    let s = subject.trim().to_ascii_lowercase();
    if s.starts_with("reapply \"") || s.starts_with("revert \"revert ") {
        return true;
    }
    // `revert: reapply ...`, with an optional `(scope)` and `!` (what conformance/src/commit-lint.mjs accepts).
    let Some(mut rest) = s.strip_prefix("revert") else { return false };
    if let Some(scoped) = rest.strip_prefix('(') {
        let Some((_, after)) = scoped.split_once(')') else { return false };
        rest = after;
    }
    rest.strip_prefix('!').unwrap_or(rest).starts_with(": reapply")
}

/// The commit names `(cherry picked from commit <hex>)` lines give.
fn cherry_picked(message: &str) -> Vec<String> {
    const KEY: &str = "(cherry picked from commit ";
    message
        .lines()
        .filter_map(|l| l.trim().strip_prefix(KEY))
        .map(|rest| rest.chars().take_while(char::is_ascii_hexdigit).collect::<String>())
        .filter(|h| (7..=64).contains(&h.len()))
        .collect()
}

/// Whether a message is a revert's: git's own line, or the `Revert "..."` subject.
fn is_revert(message: &str) -> bool {
    !trailer_targets(message).is_empty() || message.lines().next().is_some_and(|l| l.trim().starts_with("Revert \""))
}

fn files_of(git: &Git, sha: &str) -> Result<Vec<String>, String> {
    git.paths(&[
        "diff-tree",
        "--no-commit-id",
        "--name-only",
        "-r",
        "--no-renames",
        "-z",
        "--root",
        sha,
    ])
}

/// The target's revert commit `name` stands for, with its files: `None` when it is not a commit of `target` or not a revert.
fn target_revert(git: &Git, target: &str, name: &str) -> Result<Option<(String, Vec<String>)>, String> {
    let Some(full) = resolve(git, name)? else { return Ok(None) };
    let merge_base = git.text(&["merge-base", target, &full])?;
    if merge_base.trim() != full {
        return Ok(None);
    }
    let message = git.text(&["show", "-s", "--format=%B", &full])?;
    if !is_revert(&message) {
        return Ok(None);
    }
    let files = files_of(git, &full)?;
    Ok(Some((full, files)))
}

/// The reverts of the target that undo commit `x`: `git log --grep "This reverts commit <x>"` over the target.
fn reverts_of(git: &Git, target: &str, x: &str) -> Result<Vec<String>, String> {
    let out = git.text(&[
        "log",
        "--format=%H",
        "--fixed-strings",
        "--grep",
        &format!("This reverts commit {x}"),
        target,
    ])?;
    Ok(out.lines().map(str::to_string).collect())
}

fn relands(git: &Git, target: &str, member: &str, base: &str) -> Result<Vec<Reland>, String> {
    let log = git.text(&["log", "--no-merges", "--format=%H%x00%s%x00%B%x01", &format!("{base}..{member}")])?;
    let mut out = Vec::new();
    for rec in log.split('\u{1}').map(str::trim_start).filter(|r| !r.is_empty()) {
        let mut f = rec.splitn(3, '\0');
        let (Some(sha), Some(subject), Some(body)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        if !is_reland_subject(subject) {
            continue;
        }
        let mut undoes = Vec::new();
        for name in trailer_targets(body) {
            undoes.extend(target_revert(git, target, &name)?);
        }
        for x in cherry_picked(body) {
            for s in reverts_of(git, target, &x)? {
                undoes.extend(target_revert(git, target, &s)?);
            }
        }
        if undoes.is_empty() {
            continue;
        }
        out.push(Reland {
            sha: sha.to_string(),
            subject: subject.to_string(),
            undoes,
            files: files_of(git, sha)?.into_iter().collect(),
        });
    }
    Ok(out)
}

/// The `Reverted: <path>: <reason>` lines for the backwards findings the member's re-land explains, in the findings' order; the
/// findings it cannot tie are not mentioned (they stay findings).
pub fn lines(git: &Git, merge: &Merge, found: &[Finding]) -> Result<Vec<String>, String> {
    let wanted: Vec<&Finding> = found.iter().filter(|f| f.kind == Kind::Backwards).collect();
    let [target, member] = merge.parents.as_slice() else {
        return Ok(Vec::new());
    };
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let base = git.text(&["merge-base", target, member])?;
    let all = relands(git, target, member, base.trim())?;
    let mut out = Vec::new();
    for f in wanted {
        // Defence in depth: the guard below (the target's diff since the revert) covers this one whenever the member contains the
        // revert; this one also holds when a whole-file take or a dropped hunk is the finding that explains the loss.
        let other_kind = found
            .iter()
            .any(|g| matches!(g.kind, Kind::WholeFile | Kind::DroppedHunk) && g.subject == f.subject);
        if other_kind {
            continue;
        }
        let tie = all.iter().find_map(|l| {
            if !l.files.contains(&f.subject) {
                return None;
            }
            l.undoes.iter().find(|(_, files)| files.contains(&f.subject)).map(|(s, _)| (l, s))
        });
        if let Some((l, s)) = tie {
            // The target has changed the file since the revert: not only the re-land's undoing is in this finding.
            if !git
                .paths(&["diff", "--name-only", "-z", "--no-renames", s, target, "--", &f.subject])?
                .is_empty()
            {
                continue;
            }
            out.push(format!(
                "Reverted: {}: {} re-lands the work the revert {} took out ({})",
                f.subject,
                short(&l.sha),
                short(s),
                l.subject.replace('\n', " ")
            ));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reland_subject_is_reapply_in_either_form_and_nothing_else() {
        for yes in [
            "revert: reapply \"feat: x\"",
            "Reapply \"feat: x\"",
            "REAPPLY \"x\"",
            "  revert: Reapply x",
            "Revert \"Revert \"feat: x\"\"",
        ] {
            assert!(is_reland_subject(yes), "{yes}");
        }
        for no in [
            "Revert \"feat: x\"",
            "feat: reapply x",
            "fix: reapplying",
            "revert: x",
            "reapply: x",
        ] {
            assert!(!is_reland_subject(no), "{no}");
        }
    }

    /// The table `conformance/commit-lint.mjs` reads too (docs/known-copies.toml, mechanism `reland-rule`).
    const CASES: &str = include_str!("../../../../conformance/reland-cases.json");

    #[test]
    fn the_shared_case_table_gives_the_same_answers_as_the_javascript_rule() {
        let table: serde_json::Value = serde_json::from_str(CASES).unwrap();
        let subjects = table["subjects"].as_array().unwrap();
        let trailers = table["trailers"].as_array().unwrap();
        // The table is not empty and has both outcomes: a table read as nothing would pass every loop below.
        assert!(subjects.len() > 10 && trailers.len() > 5);
        assert!(subjects.iter().any(|c| c["reland"] == true) && subjects.iter().any(|c| c["reland"] == false));
        for c in subjects {
            let subject = c["subject"].as_str().unwrap();
            assert_eq!(is_reland_subject(subject), c["reland"].as_bool().unwrap(), "{subject}");
        }
        for c in trailers {
            let message = c["message"].as_str().unwrap();
            let want: Vec<&str> = c["targets"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
            assert_eq!(trailer_targets(message), want, "{message}");
        }
    }

    #[test]
    fn the_cherry_pick_line_names_its_commit_and_a_short_name_is_not_one() {
        let m = "x\n\n(cherry picked from commit 0123456789abcdef0123456789abcdef01234567)\n(cherry picked from commit abc12)\n";
        assert_eq!(cherry_picked(m), ["0123456789abcdef0123456789abcdef01234567"]);
    }
}
