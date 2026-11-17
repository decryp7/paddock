//! A member should be built on the train's base: the newest commit of the train's base line that the train has merged so far.
//! **Report-only** ([`ENFORCED`] is false, ER-41, like `gates::ENFORCED`): a member that lacks it gets a `::warning::` and the
//! exit code is unchanged, until the check has run green on a real train and the change that records it flips the switch.
//!
//! The base line is declared by the train's own history, not by a second list (ER-42): a train stacked on another carries
//! re-merge commits `chore(train): merge train/<previous> (<sha>) into train N` (also `Merge train/<previous> (<sha>) into
//! train/<n>`, any case). The newest one on the train's own first-parent line, the commits not on `origin/main` (`--first-parent
//! <head> ^origin/main`: an older train's re-merge that landed on main is not this train's), names the line and, as its second
//! parent, the commit of it merged so far. A train with none is cut from main (the queue's `base` is main's commit,
//! docs/plans/merge-queue.md): the base is then the merge base of the train and `origin/main`. Other members merged ahead of
//! this one are not required: members are merged one after another and never contain each other.
//!
//! Not judged, and said so: nothing declares a base (no re-merge commit and no `origin/main`), or the train and main share no
//! commit ([`Base::Unknown`], ER-33); a target that is main or `origin/main` by name, or not ahead of `origin/main`
//! ([`Base::NotATrain`]).

use super::git::{Git, short};

/// Whether a member without the base fails the check. False until a green run on a real train is recorded (ER-41).
pub const ENFORCED: bool = false;

/// The newest commits of the first-parent line that are read for a re-merge commit.
const DEPTH: &str = "2000";

/// What the train says its base is.
#[derive(Debug, PartialEq, Eq)]
pub enum Base {
    /// The commit every member should contain, and the line it belongs to (`train/<name>` or `main`).
    Commit { sha: String, line: String },
    /// Nothing declares a base, or it cannot be measured: why.
    Unknown(String),
    /// The target is main, or not ahead of main: a merge into main itself, not a train.
    NotATrain,
}

/// Whether a name is main by name (the local branch or the remote one, with or without `refs/...`).
fn is_main_name(name: &str) -> bool {
    matches!(name.trim(), "main" | "origin/main" | "refs/heads/main" | "refs/remotes/origin/main")
}

/// The earlier train a re-merge subject names (`train/<name>`), `None` when the subject is no re-merge of a train. Any case. After
/// `merge ` comes the name, in one of three spellings: `train/<name>` (the integrator's), `branch 'train/<name>'` (git's default
/// message) or `remote-tracking branch 'origin/train/<name>'` (the remote prefix is dropped); a trailing `(sha)` is not part of it.
fn remerged_line(subject: &str) -> Option<String> {
    let lower = subject.to_ascii_lowercase();
    let mut at = lower.find("merge ")? + "merge ".len();
    let mut remote = false;
    for noise in ["remote-tracking branch ", "branch "] {
        if lower[at..].starts_with(noise) {
            at += noise.len();
            remote = noise.starts_with("remote");
            break;
        }
    }
    let token = subject[at..].split_whitespace().next()?.trim_matches('\'');
    let lower_token = token.to_ascii_lowercase();
    if lower_token.starts_with("train/") {
        return Some(token.to_string());
    }
    // `origin/train/<name>`, only in the remote-tracking spelling: one remote name, then the train.
    let (_, rest) = token.split_once('/').filter(|_| remote)?;
    rest.to_ascii_lowercase().starts_with("train/").then(|| rest.to_string())
}

/// `Some(full sha)` of `name`, `None` when this clone has no such commit.
fn resolve(git: &Git, name: &str) -> Result<Option<String>, String> {
    let out = git.bytes_or_none(&["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")])?;
    let id = String::from_utf8_lossy(&out).trim().to_string();
    Ok((!id.is_empty()).then_some(id))
}

/// The base of the train whose head is `head` and whose name is `target_name`.
pub fn base_of(git: &Git, head: &str, target_name: &str) -> Result<Base, String> {
    if is_main_name(target_name) {
        return Ok(Base::NotATrain);
    }
    let main = resolve(git, "origin/main")?;
    // The walk stops at main: `origin/main`, and the local `main` too (in the commit form the target has no name to say it is main,
    // and a local main ahead of origin carries re-merges of its own).
    let local_main = resolve(git, "main")?;
    let mut args = vec![
        "log".to_string(),
        "--first-parent".into(),
        "-n".into(),
        DEPTH.into(),
        "--format=%H%x00%P%x00%s".into(),
        head.into(),
    ];
    args.extend(main.iter().chain(local_main.iter()).map(|m| format!("^{m}")));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    for rec in git.text(&args)?.lines() {
        let mut f = rec.splitn(3, '\0');
        let (Some(_), Some(parents), Some(subject)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let parents: Vec<&str> = parents.split_whitespace().collect();
        if parents.len() == 2
            && let Some(line) = remerged_line(subject)
        {
            return Ok(Base::Commit {
                sha: parents[1].to_string(),
                line,
            });
        }
    }
    let Some(main) = main else {
        return Ok(Base::Unknown(
            "the train has no re-merge commit of an earlier train and this clone has no origin/main to measure it from".into(),
        ));
    };
    if git.is_ancestor(head, &main)? {
        return Ok(Base::NotATrain);
    }
    // Exit 1 of `merge-base` (no common commit) is an answer, not a failure.
    let sha = String::from_utf8_lossy(&git.bytes_or_none(&["merge-base", head, &main])?)
        .trim()
        .to_string();
    if sha.is_empty() {
        return Ok(Base::Unknown("the train and origin/main share no commit".into()));
    }
    Ok(Base::Commit { sha, line: "main".into() })
}

/// The text of the problem: a warning with the report-only note, or (`enforced`) an error.
fn render(sha: &str, line: &str, train: &str, member: &str, enforced: bool) -> String {
    let level = if enforced { "error" } else { "warning" };
    format!(
        "::{level}::merge check: {member} is built on an old base: it does not contain {}, the newest commit of {line} that {train} has merged. Next step: run `./dev merge sync-member {train}` before you merge the train into it (it lists what to re-apply), merge the train, push, then check it again.{}",
        short(sha),
        if enforced {
            ""
        } else {
            " (report-only: the exit code is unchanged)"
        }
    )
}

/// The warning (or, when [`ENFORCED`], the error) for a member that lacks the train's base; `None` when it has it.
pub fn problem(git: &Git, head: &str, member_commit: &str, train: &str, member: &str) -> Result<Option<String>, String> {
    match base_of(git, head, train)? {
        Base::Commit { sha, line } if !git.is_ancestor(&sha, member_commit)? => Ok(Some(render(&sha, &line, train, member, ENFORCED))),
        _ => Ok(None),
    }
}

/// The notice for a train whose base nothing declares (the check did not judge), `None` otherwise.
pub fn unknown(git: &Git, head: &str, train: &str) -> Result<Option<String>, String> {
    Ok(match base_of(git, head, train)? {
        Base::Unknown(why) => Some(format!(
            "::notice::stale-base check did not run (the base is not declared): {why}. Next step: `git fetch origin` so origin/main exists; no member is judged for an old base until it does."
        )),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remerge_subject_names_its_train_in_any_case_and_every_spelling() {
        for (s, want) in [
            ("Merge branch 'train/2026-11-15-13' into train/2026-11-17-14", "train/2026-11-15-13"),
            ("Merge remote-tracking branch 'origin/train/x' into train/y", "train/x"),
            (
                "chore(train): merge train/2026-11-15-13 (bc9b9dc22) into train 14",
                "train/2026-11-15-13",
            ),
            ("Merge train/x (abc1234) into train/y", "train/x"),
            ("CHORE(TRAIN): MERGE TRAIN/Old (abc1234) INTO TRAIN 3", "TRAIN/Old"),
        ] {
            assert_eq!(remerged_line(s).as_deref(), Some(want), "{s}");
        }
        for s in [
            "chore(train): merge feat/x (abc1234) into train 14",
            "feat: train/x merge",
            "merge main into train",
            "Merge branch 'feat/train/x' into train/y",
            "Merge remote-tracking branch 'origin/feat/x' into train/y",
            "Merge branch 'origin/train/x' into train/y",
        ] {
            assert_eq!(remerged_line(s), None, "{s}");
        }
    }

    #[test]
    fn report_only_is_a_warning_with_the_note_and_enforced_is_an_error_without_it() {
        let w = render("0123456789abcdef", "main", "train", "m", false);
        assert!(
            w.starts_with("::warning::") && w.contains("report-only") && w.contains("012345678"),
            "{w}"
        );
        let e = render("0123456789abcdef", "main", "train", "m", true);
        assert!(e.starts_with("::error::") && !e.contains("report-only"), "{e}");
    }

    #[test]
    fn main_is_main_by_name_only_for_its_spellings() {
        assert!(is_main_name("main") && is_main_name("origin/main") && is_main_name("refs/heads/main"));
        assert!(!is_main_name("train/main") && !is_main_name("mainline") && !is_main_name("train"));
    }
}
