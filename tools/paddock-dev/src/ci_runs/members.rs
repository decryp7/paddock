// ci-runs-reader: ci triage --members: which member of a hand-built train a red run blames
//! `ci triage <run> --members <train-base>` (ADR 0098 WP-1): the read-only blame the merge queue computes, for a train built by
//! hand. The members are the first-parent merges between the train's base and the commit the run tested; each member's reach is
//! the planner's answer for its own diff (`merge^1..merge`) and the files it changes. The rule is
//! [`crate::queue::blame::suspects`], the queue's own: a failure that is new (or flaky) goes to every member whose plan reaches
//! its project, or that changes a file the failing gate's log names; none is ever bisected or dropped. It never writes.

use crate::merge_check::git::Git;
use crate::queue::blame::{self, Reach};
use crate::queue::ports::{Blame, Failure};
use crate::queue::real::projects_of_plan;
use serde_json::{Value, json};

/// One member of the train.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The merge commit.
    pub merge: String,
    /// Its subject line.
    pub subject: String,
    /// What it reaches.
    pub reach: Reach,
}

/// The planner for a diff: `(base, head)` to its answer. The real one is [`crate::queue::real::run_planner`] in the checkout.
pub type Planner<'a> = &'a dyn Fn(&str, &str) -> Result<Value, String>;

/// The members of the train `base..head`, oldest first. `files` reads each member's changed files (only when a failure names
/// files). A merge with other than two parents is not a member (an octopus or a plain commit on the train, such as the queue's
/// marker commit); an unreadable one is an error: a blame that skipped a member would blame the wrong ones (ER-33).
pub fn read(git: &Git, base: &str, head: &str, plan: Planner, files: bool) -> Result<Vec<Member>, String> {
    let listed = git.text(&[
        "rev-list",
        "--first-parent",
        "--merges",
        "--reverse",
        "--parents",
        &format!("{base}..{head}"),
    ])?;
    let mut out = Vec::new();
    for line in listed.lines().filter(|l| !l.trim().is_empty()) {
        let ids: Vec<&str> = line.split_whitespace().collect();
        let [merge, first, _second] = ids.as_slice() else {
            return Err(format!(
                "{} is a merge with {} parents; only two-parent merges are members",
                ids[0],
                ids.len() - 1
            ));
        };
        let subject = git.text(&["show", "-s", "--format=%s", merge])?.trim().to_string();
        let projects = projects_of_plan(&plan(first, merge).map_err(|e| format!("the plan of {}: {e}", &merge[..merge.len().min(10)]))?);
        let changed = if files {
            git.text(&["diff", "--name-only", first, merge])?
                .lines()
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };
        out.push(Member {
            merge: merge.to_string(),
            subject,
            reach: Reach { projects, files: changed },
        });
    }
    Ok(out)
}

fn short(sha: &str) -> &str {
    sha.get(..9).unwrap_or(sha)
}

/// The blame as text and JSON: for every failure that is not already failing on main, the members that are suspects.
pub fn report(base: &str, head: &str, members: &[Member], failures: &[Failure]) -> (String, Value) {
    let blamed: Vec<&Failure> = failures.iter().filter(|f| matches!(f.blame, Blame::New | Blame::Flaky)).collect();
    let reaches: Vec<Reach> = members.iter().map(|m| m.reach.clone()).collect();
    let found = blame::suspects(&reaches, &blamed);
    let mut text = format!("members of the train {base}..{}: {} merge(s)\n", short(head), members.len());
    for (i, m) in members.iter().enumerate() {
        let reach = m.reach.projects.iter().cloned().collect::<Vec<_>>().join(", ");
        text.push_str(&format!("  {}. {} {}  reaches: {}\n", i + 1, short(&m.merge), m.subject, reach));
    }
    let named = |idx: &[usize]| -> String {
        idx.iter()
            .map(|i| format!("{}. {} {}", i + 1, short(&members[*i].merge), members[*i].subject))
            .collect::<Vec<_>>()
            .join("; ")
    };
    text.push_str(&format!(
        "blame ({} new or flaky failure(s); one that also fails on main is nobody's; every member that reaches a failure is named, nothing is bisected):\n",
        blamed.len()
    ));
    let mut rows = Vec::new();
    for f in &blamed {
        let by: Vec<usize> = found
            .precise
            .iter()
            .filter(|(_, hit)| hit.iter().any(|h| std::ptr::eq(*h, *f)))
            .map(|(i, _)| *i)
            .collect();
        let verdict = if !by.is_empty() {
            named(&by)
        } else if !found.wild.is_empty() {
            format!("only members whose plan runs everything could be the cause: {}", named(&found.wild))
        } else {
            "no member reaches it (check main, the runner and the base)".to_string()
        };
        text.push_str(&format!("  {} -> {verdict}\n", f.what));
        rows.push(json!({
            "failure": f.what,
            "members": by.iter().map(|i| members[*i].merge.clone()).collect::<Vec<_>>(),
            "plan_everything": if by.is_empty() { found.wild.iter().map(|i| members[*i].merge.clone()).collect::<Vec<_>>() } else { vec![] },
        }));
    }
    let data = json!({
        "base": base,
        "head": head,
        "members": members.iter().map(|m| json!({
            "merge": m.merge,
            "subject": m.subject,
            "projects": m.reach.projects,
        })).collect::<Vec<_>>(),
        "blame": rows,
    });
    (text, data)
}

/// Whether the run's failures need each member's changed files.
pub fn needs_files(failures: &[Failure]) -> bool {
    blame::needs_files(
        &failures
            .iter()
            .filter(|f| matches!(f.blame, Blame::New | Blame::Flaky))
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[allow(clippy::disallowed_methods, reason = "a test starts git in its own scratch repository")]
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A train of two merged members, `a` (touches crates/x) and `b` (touches tools/y), after a base commit.
    fn train() -> (tempfile::TempDir, String, String) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::write(p.join("base.txt"), "base\n").unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", "base"]);
        let base = git(p, &["rev-parse", "HEAD"]);
        for (name, file) in [("a", "crates_x.txt"), ("b", "tools_y.txt")] {
            git(p, &["checkout", "-q", "-b", name, "main"]);
            std::fs::write(p.join(file), name).unwrap();
            git(p, &["add", "-A"]);
            git(p, &["commit", "-q", "-m", &format!("feat: {name}")]);
            git(p, &["checkout", "-q", "main"]);
            git(p, &["merge", "-q", "--no-ff", "-m", &format!("Merge {name}"), name]);
        }
        let head = git(p, &["rev-parse", "HEAD"]);
        (d, base, head)
    }

    fn planner(first: &str, merge: &str) -> Result<Value, String> {
        // Which member it is follows from the diff the planner is asked about; the test names them by order of asking.
        let _ = (first, merge);
        Ok(json!({}))
    }

    #[test]
    fn the_members_are_the_first_parent_merges_oldest_first_with_their_own_diff_and_files() {
        let (d, base, head) = train();
        let asked = std::cell::RefCell::new(Vec::new());
        let plan = |f: &str, m: &str| {
            asked.borrow_mut().push((f.to_string(), m.to_string()));
            // The plan of the member that adds crates_x.txt reaches svc; the other reaches tool.
            let changed = git(d.path(), &["diff", "--name-only", f, m]);
            Ok(json!({"packages": {"rust": [if changed.contains("crates_x") { "svc" } else { "tool" }]}}))
        };
        let git_reader = Git::new(d.path());
        let members = read(&git_reader, &base, &head, &plan, true).unwrap();
        assert_eq!(
            members.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(),
            ["Merge a", "Merge b"]
        );
        assert_eq!(members[0].reach.projects, BTreeSet::from(["svc".to_string()]));
        assert_eq!(members[1].reach.files, ["tools_y.txt"]);
        // Each member is planned against the train before its own merge, never against the base.
        let first_of_second = git(d.path(), &["rev-parse", &format!("{}^1", members[1].merge)]);
        assert_eq!(asked.borrow()[1], (first_of_second, members[1].merge.clone()));
        // Without a failure that names files, no file list is read.
        assert!(
            read(&git_reader, &base, &head, &planner, false)
                .unwrap()
                .iter()
                .all(|m| m.reach.files.is_empty())
        );
    }

    #[test]
    fn a_member_whose_plan_cannot_be_read_stops_the_blame_instead_of_being_skipped() {
        let (d, base, head) = train();
        let e = read(&Git::new(d.path()), &base, &head, &|_, _| Err("no planner".into()), false).unwrap_err();
        assert!(e.contains("no planner") && e.contains("the plan of"), "{e}");
    }

    fn failure(what: &str, projects: &[&str], blame: Blame) -> Failure {
        Failure {
            what: what.into(),
            projects: projects.iter().map(|s| s.to_string()).collect(),
            files: Vec::new(),
            blame,
        }
    }

    fn member(n: &str, projects: &[&str]) -> Member {
        Member {
            merge: format!("{n}{n}{n}{n}{n}{n}{n}{n}{n}{n}"),
            subject: format!("Merge {n}"),
            reach: Reach {
                projects: projects.iter().map(|s| s.to_string()).collect(),
                files: Vec::new(),
            },
        }
    }

    #[test]
    fn the_report_names_the_members_a_new_failure_reaches_and_nobody_for_one_that_fails_on_main() {
        let members = [
            member("1", &["paddock-dev"]),
            member("2", &["paddock-dev", "pitwall"]),
            member("3", &["ui"]),
        ];
        let failures = [
            failure("paddock-dev safe_remove_lint::x", &["paddock-dev"], Blame::New),
            failure("ui e2e::y", &["ui"], Blame::OnMain),
            failure("pitwall z", &["nothing-reaches-this"], Blame::New),
        ];
        let (text, data) = report("origin/main", "deadbeefdeadbeef", &members, &failures);
        assert!(
            text.contains("paddock-dev safe_remove_lint::x -> 1. 111111111 Merge 1; 2. 222222222 Merge 2"),
            "{text}"
        );
        assert!(!text.contains("ui e2e::y"), "a failure that also fails on main is nobody's: {text}");
        assert!(text.contains("pitwall z -> no member reaches it"), "{text}");
        assert_eq!(data["blame"][0]["members"].as_array().unwrap().len(), 2);
        assert_eq!(data["members"].as_array().unwrap().len(), 3);
    }
}
