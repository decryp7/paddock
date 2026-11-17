//! A member that is built on a merge the target later reverted (report-only, ER-41). Train 12 merged and reverted
//! feat/test-env-builder six times: five branches that had been based on it could not join train 13 (32-file conflicts), and an
//! earlier reverted import broke another member. The check reads what the target did since the member's history parted from it:
//! a commit of `member..target` whose message says `This reverts commit <sha>` (the line `git revert` writes, also for
//! `git revert -m` of a merge) names a commit; when that commit is a merge the member contains, the member is built on work the
//! target took out. A revert that is itself reverted (a re-land by revert-of-revert) is not live and says nothing, however deep
//! the chain. It never fails the check: one `::warning::` line per reverted merge, with the next step.

use super::git::{Git, short};
use super::{Merge, generator_step::Step};
use paddock_cli::Exit;
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// The commit a message reverts, from git's own line `This reverts commit <hex>`.
fn reverted_by(message: &str) -> Vec<String> {
    const KEY: &str = "This reverts commit ";
    message
        .lines()
        .filter_map(|l| l.strip_prefix(KEY))
        .map(|rest| rest.chars().take_while(char::is_ascii_hexdigit).collect::<String>())
        .filter(|h| h.len() == 40 || h.len() == 64)
        .collect()
}

/// The commits whose revert is still in force: `(reverted, reverter)` for every commit that has a revert in `commits` which no
/// later revert undoes. `commits` is `(sha, message)`. A reverter that is itself reverted (by a live reverter) does not count;
/// the walk is by memo and a cycle (impossible in a real history) counts as not live.
fn live_reverts(commits: &[(String, String)]) -> Vec<(String, String)> {
    let mut reverters: HashMap<String, Vec<String>> = HashMap::new();
    for (sha, message) in commits {
        for target in reverted_by(message) {
            reverters.entry(target).or_default().push(sha.clone());
        }
    }
    fn reverted(sha: &str, reverters: &HashMap<String, Vec<String>>, memo: &mut HashMap<String, bool>, open: &mut HashSet<String>) -> bool {
        if let Some(v) = memo.get(sha) {
            return *v;
        }
        if !open.insert(sha.to_string()) {
            return false;
        }
        // A commit's revert is in force unless a live revert of that revert undoes it: so a commit is "in effect reverted" when
        // some reverter of it is itself not in effect reverted.
        let undone = reverters
            .get(sha)
            .is_some_and(|rs| rs.iter().any(|r| !reverted(r, reverters, memo, open)));
        open.remove(sha);
        memo.insert(sha.to_string(), undone);
        undone
    }
    let mut memo = HashMap::new();
    let mut out = Vec::new();
    let mut targets: Vec<&String> = reverters.keys().collect();
    targets.sort();
    for t in targets {
        if reverted(t, &reverters, &mut memo, &mut HashSet::new())
            && let Some(r) = reverters[t]
                .iter()
                .find(|r| !reverted(r, &reverters, &mut memo, &mut HashSet::new()))
        {
            out.push((t.clone(), r.clone()));
        }
    }
    out
}

/// The branch a merge commit's subject names: `Merge branch 'x'` or `... merge x into train N`; else the subject itself.
fn branch_of(subject: &str) -> String {
    if let Some(rest) = subject.split("Merge branch '").nth(1)
        && let Some((name, _)) = rest.split_once('\'')
    {
        return name.to_string();
    }
    if let Some(rest) = subject.split(": merge ").nth(1) {
        return rest.split(" into ").next().unwrap_or(rest).trim().to_string();
    }
    subject.to_string()
}

/// One merge the member contains and the target reverted.
struct Reverted {
    merge: String,
    revert: String,
    branch: String,
    revert_subject: String,
}

fn find(git: &Git, target: &str, member: &str) -> Result<Vec<Reverted>, String> {
    let log = git.text(&["log", "--format=%H%x00%B%x01", &format!("{member}..{target}")])?;
    let commits: Vec<(String, String)> = log
        .split('\u{1}')
        .filter_map(|rec| {
            let (sha, body) = rec.trim_start().split_once('\0')?;
            Some((sha.to_string(), body.to_string()))
        })
        .collect();
    let mut out = Vec::new();
    for (reverted, reverter) in live_reverts(&commits) {
        // A commit this clone does not have cannot be in the member's history.
        let Ok(line) = git.text(&["rev-list", "--parents", "-n", "1", &reverted]) else {
            continue;
        };
        if line.split_whitespace().count() < 3 {
            continue; // not a merge
        }
        // Zero commits reachable from the merge and not from the member: the member contains it.
        let outside = git.text(&["rev-list", "--count", &reverted, &format!("^{member}")])?;
        if outside.trim() != "0" {
            continue;
        }
        let subject = |sha: &str| git.text(&["show", "-s", "--format=%s", sha]).map(|s| s.trim().to_string());
        out.push(Reverted {
            branch: branch_of(&subject(&reverted)?),
            revert_subject: subject(&reverter)?,
            merge: reverted,
            revert: reverter,
        });
    }
    Ok(out)
}

/// The step: one warning per reverted merge, a warning that it could not look, or nothing (`None`) when the member is clean.
pub fn run(git: &Git, merge: &Merge) -> Option<Step> {
    let [target, member] = merge.parents.as_slice() else { return None };
    of_refs(git, &merge.label, target, member)
}

/// The same for a member and a target that have no merge result (they conflict: the case that matters most, since a member built
/// on a reverted merge is what conflicts with the revert).
pub fn of_refs(git: &Git, label: &str, target: &str, member: &str) -> Option<Step> {
    match find(git, target, member) {
        Ok(found) if found.is_empty() => None,
        Ok(found) => Some(Step {
            text: found
                .iter()
                .map(|r| {
                    format!(
                        "::warning::{}: its history contains the merge of {} ({}), which {} reverted in {} ({}). Next step: re-base on the train head, or land the reverted branch first.",
                        label,
                        r.branch,
                        short(&r.merge),
                        short(target),
                        short(&r.revert),
                        r.revert_subject
                    )
                })
                .collect(),
            data: json!({"outcome": "warnings", "reverted": found.iter().map(|r| json!({"branch": r.branch, "merge": r.merge, "revert": r.revert})).collect::<Vec<_>>()}),
            exit: Exit::Ok,
        }),
        Err(why) => Some(Step {
            text: vec![format!(
                "::warning::reverted-base check NOT MEASURED: {why}. Next step: run `./dev merge check` again; this check is report-only."
            )],
            data: json!({"outcome": "not measured", "why": why}),
            exit: Exit::Ok,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "a111111111111111111111111111111111111111";

    fn revert_of(sha: &str) -> String {
        format!("Revert \"x\"\n\nThis reverts commit {sha}, reversing\nchanges made to {A}.\n")
    }

    #[test]
    fn the_trailer_is_read_not_the_subject() {
        assert_eq!(reverted_by(&revert_of(A)), vec![A.to_string()]);
        assert!(reverted_by("Revert \"x\"\n\nbecause").is_empty());
        assert!(reverted_by("This reverts commit zz").is_empty());
    }

    #[test]
    fn a_revert_is_live_until_it_is_reverted_and_the_chain_may_be_deep() {
        let c = |s: &str, m: String| (s.to_string(), m);
        let r1 = "c333333333333333333333333333333333333333";
        let r2 = "d444444444444444444444444444444444444444";
        let one = vec![c(r1, revert_of(A))];
        assert_eq!(live_reverts(&one), vec![(A.to_string(), r1.to_string())]);
        let two = vec![c(r1, revert_of(A)), c(r2, revert_of(r1))];
        assert!(
            live_reverts(&two).iter().all(|(t, _)| t != A),
            "a revert-of-revert re-lands the work"
        );
        let r3 = "e555555555555555555555555555555555555555";
        let three = vec![c(r1, revert_of(A)), c(r2, revert_of(r1)), c(r3, revert_of(r2))];
        assert_eq!(
            live_reverts(&three),
            vec![(A.to_string(), r1.to_string()), (r2.to_string(), r3.to_string())]
        );
    }

    #[test]
    fn the_branch_is_named_from_the_merge_subject() {
        assert_eq!(branch_of("Merge branch 'feat/x' into main"), "feat/x");
        assert_eq!(branch_of("chore(train): merge feat/x into train 12"), "feat/x");
        assert_eq!(branch_of("something else"), "something else");
    }
}
