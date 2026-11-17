//! A member that is built on a merge the target later reverted (report-only, ER-41). Train 12 merged and reverted
//! feat/test-env-builder six times: five branches that had been based on it could not join train 13 (32-file conflicts), and an
//! earlier reverted import broke another member. The check reads what the target did since the member's history parted from it
//! (`member..target`) and finds the commits that took a merge out:
//!
//! - `This reverts commit <sha>` (the line `git revert` writes, also for `git revert -m` of a merge; a full or abbreviated sha);
//! - a revert without that line, `Revert "<subject>"` (a hand-made pull-out): the commit whose subject is exactly `<subject>`
//!   (the newest one before it) is the reverted one; if none can be found the check says NOT MEASURED, never silence (ER-33).
//!
//! A revert that is itself reverted (a re-land by revert-of-revert, however deep) is not live, and neither is one that a later
//! merge of the same branch in `member..target` re-landed. A live reverted merge is a finding for the member when the member holds
//! the merge or any commit the merge brought in (a member forked from the reverted branch itself). A member forked before the
//! merge, or a reverted commit that is not a merge, says nothing. It never fails the check: one `::warning::` line per reverted
//! branch, with the next step.

use super::git::{Git, short};
use super::{Merge, generator_step::Step};
use paddock_cli::Exit;
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// What the member's check is about, for the wording of the next step.
pub struct Names<'a> {
    /// What to call the target (a branch name, or a short sha).
    pub target: &'a str,
    /// What to call the member.
    pub member: &'a str,
    /// The merge exists as a commit (the commit form) and is not about to be made.
    pub merged: bool,
}

/// The commit names a message reverts, from git's own line `This reverts commit <hex>` (7 to 64 hex digits, as written).
fn trailer_targets(message: &str) -> Vec<String> {
    const KEY: &str = "This reverts commit ";
    message
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix(KEY))
        .map(|rest| rest.chars().take_while(char::is_ascii_hexdigit).collect::<String>())
        .filter(|h| (7..=64).contains(&h.len()))
        .collect()
}

/// The subject a hand-made pull-out names: `Revert "<s>"` on the first line, for a message that has no trailer.
fn subject_target(message: &str) -> Option<String> {
    if !trailer_targets(message).is_empty() {
        return None;
    }
    let first = message.lines().next()?.trim();
    let s = first.strip_prefix("Revert \"")?.strip_suffix('"')?;
    (!s.is_empty()).then(|| s.to_string())
}

/// The commits whose revert is still in force: `(reverted, reverter)` from `edges`, `(reverter, reverted)` pairs of full shas.
/// A commit is reverted when some reverter of it is itself not reverted; so a revert-of-revert re-lands the work, however deep the
/// chain. The walk is by memo; a cycle (impossible in a real history) counts as not reverted.
fn live_reverts(edges: &[(String, String)]) -> Vec<(String, String)> {
    let mut reverters: HashMap<&str, Vec<&str>> = HashMap::new();
    for (reverter, reverted) in edges {
        reverters.entry(reverted).or_default().push(reverter);
    }
    fn reverted<'a>(
        sha: &'a str,
        rs: &HashMap<&'a str, Vec<&'a str>>,
        memo: &mut HashMap<&'a str, bool>,
        open: &mut HashSet<&'a str>,
    ) -> bool {
        if let Some(v) = memo.get(sha) {
            return *v;
        }
        if !open.insert(sha) {
            return false;
        }
        let undone = rs.get(sha).is_some_and(|v| v.iter().any(|r| !reverted(r, rs, memo, open)));
        open.remove(sha);
        memo.insert(sha, undone);
        undone
    }
    let mut memo = HashMap::new();
    let mut targets: Vec<&str> = reverters.keys().copied().collect();
    targets.sort_unstable();
    let mut out = Vec::new();
    for t in targets {
        if reverted(t, &reverters, &mut memo, &mut HashSet::new())
            && let Some(r) = reverters[t]
                .iter()
                .find(|r| !reverted(r, &reverters, &mut memo, &mut HashSet::new()))
        {
            out.push((t.to_string(), r.to_string()));
        }
    }
    out
}

/// The branch a merge commit's subject names: `Merge branch 'x'`, `... merge x into train N`, `Merge x (hex) into trial of y`;
/// a trailing `(hex)` is dropped; else the subject itself.
fn branch_of(subject: &str) -> String {
    let subject = strip_hex_suffix(subject.trim());
    if let Some(rest) = subject.split("Merge branch '").nth(1)
        && let Some((name, _)) = rest.split_once('\'')
    {
        return name.to_string();
    }
    let named = subject
        .split(": merge ")
        .nth(1)
        .or_else(|| subject.strip_prefix("Merge ").filter(|r| r.contains(" into ")));
    match named {
        Some(rest) => strip_hex_suffix(rest.split(" into ").next().unwrap_or(rest).trim()).to_string(),
        None => subject.to_string(),
    }
}

/// `name (abc1234)` becomes `name`.
fn strip_hex_suffix(s: &str) -> &str {
    if let Some(body) = s.strip_suffix(')')
        && let Some((head, hex)) = body.rsplit_once(" (")
        && hex.len() >= 7
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return head.trim_end();
    }
    s
}

/// One commit of `member..target`, newest first.
struct Commit {
    sha: String,
    is_merge: bool,
    message: String,
}

/// A reverted merge the member holds.
struct Reverted {
    merge: String,
    revert: String,
    branch: String,
    /// The member holds commits the merge brought in but not the merge itself (it was forked from the reverted branch).
    forked: bool,
}

/// What the look found: the merges, and the reverts it could not resolve.
struct Found {
    reverted: Vec<Reverted>,
    unresolved: Vec<String>,
}

/// The full commit id `name` stands for (a full or abbreviated sha), `None` when this clone has no such commit; any other git
/// failure is an error, never "absent" (ER-33).
fn resolve(git: &Git, name: &str) -> Result<Option<String>, String> {
    let out = git.bytes_or_none(&["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")])?;
    let id = String::from_utf8_lossy(&out).trim().to_string();
    Ok((!id.is_empty()).then_some(id))
}

/// The newest commit before `from` whose subject is exactly `subject` (and is not `from`). Newest is the right pick while a
/// subject repeats: a branch merged more than once gets the same merge subject each time, and a revert that names a subject
/// reverts the merge that was in force when it was made, which is the newest one before it. An older merge of the same branch
/// that the member holds is still found, because every live revert in `member..target` is judged on its own.
fn by_subject(git: &Git, from: &str, subject: &str) -> Result<Option<String>, String> {
    let log = git.text(&["log", "--format=%H%x00%s", "--fixed-strings", "--grep", subject, from])?;
    Ok(log
        .lines()
        .filter_map(|l| l.split_once('\0'))
        .find(|(sha, s)| *sha != from && s.trim() == subject)
        .map(|(sha, _)| sha.to_string()))
}

fn subject_of(git: &Git, sha: &str) -> Result<String, String> {
    git.text(&["show", "-s", "--format=%s", sha]).map(|s| s.trim().to_string())
}

/// Whether `member` holds `merge` or any commit it brought in (the commits reachable from it and not from its first parent).
fn holds(git: &Git, merge: &str, member: &str) -> Result<bool, String> {
    let first = format!("{merge}^1");
    let count = |not_member: bool| -> Result<usize, String> {
        let mut args = vec!["rev-list", "--count", merge, "--not", &first];
        if not_member {
            args.push(member);
        }
        git.text(&args)?
            .trim()
            .parse()
            .map_err(|e| format!("git rev-list --count printed no number: {e}"))
    };
    // The commits of the merge that the member lacks are fewer than the merge's own: the member holds some of them.
    Ok(count(true)? < count(false)?)
}

fn find(git: &Git, target: &str, member: &str) -> Result<Found, String> {
    if git.is_shallow()? {
        return Err("this clone is shallow, so the history that shows a revert may be missing (run `git fetch --unshallow`)".into());
    }
    let log = git.text(&["log", "--format=%H%x00%P%x00%B%x01", &format!("{member}..{target}")])?;
    let commits: Vec<Commit> = log
        .split('\u{1}')
        .filter_map(|rec| {
            let mut f = rec.trim_start().splitn(3, '\0');
            let (sha, parents, body) = (f.next()?, f.next()?, f.next()?);
            Some(Commit {
                sha: sha.to_string(),
                is_merge: parents.split_whitespace().count() > 1,
                message: body.to_string(),
            })
        })
        .collect();
    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    for c in &commits {
        for name in trailer_targets(&c.message) {
            match resolve(git, &name)? {
                Some(full) => edges.push((c.sha.clone(), full)),
                None => unresolved.push(format!("{} reverts {name}, which this clone does not have", short(&c.sha))),
            }
        }
        if let Some(s) = subject_target(&c.message) {
            match by_subject(git, &c.sha, &s)? {
                Some(full) => edges.push((c.sha.clone(), full)),
                None => unresolved.push(format!("{} reverts \"{s}\", and no commit has exactly that subject", short(&c.sha))),
            }
        }
    }
    let position: HashMap<&str, usize> = commits.iter().enumerate().map(|(i, c)| (c.sha.as_str(), i)).collect();
    let mut reverted = Vec::new();
    for (merge, revert) in live_reverts(&edges) {
        let line = git.text(&["rev-list", "--parents", "-n", "1", &merge])?;
        if line.split_whitespace().count() < 3 {
            continue; // not a merge: nothing was brought in
        }
        let branch = branch_of(&subject_of(git, &merge)?);
        // A later merge of the same branch (newer than the revert, in member..target) re-landed it.
        let relanded = position.get(revert.as_str()).is_some_and(|&r| {
            commits[..r]
                .iter()
                .any(|c| c.is_merge && c.sha != merge && branch_of(c.message.lines().next().unwrap_or("")) == branch)
        });
        if relanded || !holds(git, &merge, member)? {
            continue;
        }
        let forked = git.text(&["rev-list", "--count", &merge, &format!("^{member}")])?.trim() != "0";
        reverted.push(Reverted {
            merge,
            revert,
            branch,
            forked,
        });
    }
    Ok(Found { reverted, unresolved })
}

/// The step for a merge: warnings, a warning that it could not look, or nothing (`None`) when the member is clean.
pub fn run(git: &Git, merge: &Merge) -> Option<Step> {
    let [target, member] = merge.parents.as_slice() else { return None };
    let names = Names {
        target: &merge.target_name,
        member: &merge.member_name,
        merged: merge.commit.is_some(),
    };
    of_refs(git, &merge.label, target, member, &names)
}

/// The same for a member and a target that have no merge result (they conflict: the case that matters most, since a member built
/// on a reverted merge is what conflicts with the revert).
pub fn of_refs(git: &Git, label: &str, target: &str, member: &str, names: &Names) -> Option<Step> {
    let not_measured = |why: &str| Step {
        text: vec![format!(
            "::warning::reverted-base check NOT MEASURED: {why}. Next step: run `./dev merge check` again; this check is report-only."
        )],
        data: json!({"outcome": "not measured", "why": why}),
        exit: Exit::Ok,
    };
    let found = match find(git, target, member) {
        Ok(f) => f,
        Err(why) => return Some(not_measured(&why)),
    };
    let mut text = Vec::new();
    // One line per branch: its merges and the reverts that took them out.
    let mut branches: Vec<&str> = Vec::new();
    for r in &found.reverted {
        if !branches.contains(&r.branch.as_str()) {
            branches.push(&r.branch);
        }
    }
    for b in &branches {
        let of: Vec<&Reverted> = found.reverted.iter().filter(|r| r.branch == *b).collect();
        let list = |f: fn(&Reverted) -> &str| of.iter().map(|r| short(f(r))).collect::<Vec<_>>().join(", ");
        // The member is the reverted branch itself (its name, with or without a remote prefix): "land it first" makes no sense.
        let itself = names.member == *b || names.member.ends_with(&format!("/{b}"));
        let land = if itself { "" } else { ", or land the reverted branch first" };
        let next = if names.merged {
            format!(
                "the merged member may depend on {b}; build and test the merge, or re-base on {}{land}",
                names.target
            )
        } else {
            format!("re-base {} on {}{land}", names.member, names.target)
        };
        let what = if of.iter().all(|r| r.forked) {
            format!("is built on {b}, which {} reverted", names.target)
        } else {
            format!("contains the merge of {b}, which {} reverted", names.target)
        };
        text.push(format!(
            "::warning::{label}: its history {what} (merge {}; revert {}). Next step: {next}.",
            list(|r| &r.merge),
            list(|r| &r.revert),
        ));
    }
    if !found.unresolved.is_empty() {
        text.push(format!(
            "::warning::reverted-base check NOT MEASURED for: {}. Next step: name the reverted commit in the revert message (`git revert` does), or run `./dev merge check` again; this check is report-only.",
            found.unresolved.join("; ")
        ));
    }
    if text.is_empty() {
        return None;
    }
    Some(Step {
        text,
        data: json!({
            "outcome": if found.reverted.is_empty() { "not measured" } else { "warnings" },
            "reverted": found.reverted.iter().map(|r| json!({"branch": r.branch, "merge": r.merge, "revert": r.revert})).collect::<Vec<_>>(),
            "unresolved": found.unresolved,
        }),
        exit: Exit::Ok,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "a111111111111111111111111111111111111111";

    fn revert_of(sha: &str) -> String {
        format!("Revert \"x\"\n\nThis reverts commit {sha}, reversing\nchanges made to {A}.\n")
    }

    fn edge(reverter: &str, reverted: &str) -> (String, String) {
        (reverter.to_string(), reverted.to_string())
    }

    #[test]
    fn the_trailer_is_read_not_the_subject_and_short_shas_and_indents_count() {
        assert_eq!(trailer_targets(&revert_of(A)), vec![A.to_string()]);
        assert_eq!(
            trailer_targets("Revert \"x\"\n\n  This reverts commit abc1234, reversing\n"),
            vec!["abc1234".to_string()]
        );
        assert!(trailer_targets("Revert \"x\"\n\nbecause").is_empty());
        assert!(
            trailer_targets("This reverts commit abc12").is_empty(),
            "too short to name a commit"
        );
    }

    #[test]
    fn a_subject_pull_out_is_read_only_without_the_trailer() {
        assert_eq!(
            subject_target("Revert \"chore: merge x\"\n\nwhy").as_deref(),
            Some("chore: merge x")
        );
        assert_eq!(subject_target("Revert \"Revert \"y\"\""), Some("Revert \"y\"".to_string()));
        assert_eq!(subject_target(&revert_of(A)), None);
        assert_eq!(subject_target("fix: revert the thing"), None);
    }

    #[test]
    fn a_revert_is_live_until_it_is_reverted_and_the_chain_may_be_deep() {
        let (r1, r2, r3) = ("c333", "d444", "e555");
        assert_eq!(live_reverts(&[edge(r1, A)]), vec![edge(A, r1)]);
        assert!(
            live_reverts(&[edge(r1, A), edge(r2, r1)]).iter().all(|(t, _)| t != A),
            "a revert-of-revert re-lands the work"
        );
        assert_eq!(
            live_reverts(&[edge(r1, A), edge(r2, r1), edge(r3, r2)]),
            vec![edge(A, r1), edge(r2, r3)]
        );
    }

    #[test]
    fn the_branch_is_named_from_the_merge_subject() {
        assert_eq!(branch_of("Merge branch 'feat/x' into main"), "feat/x");
        assert_eq!(branch_of("chore(train): merge feat/x into train 12"), "feat/x");
        assert_eq!(branch_of("chore(train): merge feat/x (abc1234) into train 12"), "feat/x");
        assert_eq!(branch_of("Merge feat/x (abc1234def) into trial of main"), "feat/x");
        assert_eq!(branch_of("chore(train): merge feat/x (abc1234)"), "feat/x");
        assert_eq!(branch_of("something else"), "something else");
        assert_eq!(branch_of("fix: a (note)"), "fix: a (note)");
    }
}
