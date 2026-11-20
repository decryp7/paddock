//! `train land`: fast-forwards `main` to the train head and verifies it (the port of the integrator's `land-*.sh` tails).
//!
//! Only a fast-forward: the push names the train head as the new value of `main` with no `+`, no force and no rebase, and is
//! refused before it is made when `origin/main` is not an ancestor of the head. The result is verified with the remote's own
//! answer (`ls-remote`), never with the push's exit code alone. Git is behind [`Git`], so the tests never touch a repository.

use super::sequence::{RELEASE_WORKFLOWS, Stop, app_of};
use crate::ci_runs::landable::Landable;
use crate::ci_runs::verdict::Verdict;
use std::collections::BTreeSet;
use std::io::Write;

/// The git calls `land` needs, on the integrator's own worktree.
pub trait Git {
    /// The commit `HEAD` of the worktree is on.
    fn head(&mut self) -> Result<String, String>;
    /// What the remote says `refs/heads/<name>` is (`ls-remote`); `None` when the remote has no such branch.
    fn remote_branch(&mut self, name: &str) -> Result<Option<String>, String>;
    /// Fetches the remote's branches (so `main`'s head is a local object).
    fn fetch(&mut self) -> Result<(), String>;
    /// Whether `ancestor` is reachable from `of`.
    fn is_ancestor(&mut self, ancestor: &str, of: &str) -> Result<bool, String>;
    /// Pushes `sha` as the new value of the remote branch `name`, never forced. The repository's own pre-push gate runs.
    fn push_ff(&mut self, sha: &str, name: &str) -> Result<(), String>;
}

/// What `land` asks CI about the head.
pub trait Ci {
    /// The shared reader's verdict ([`crate::ci_runs::verdict::verdict`]) of `workflow` on `sha`.
    fn verdict(&mut self, workflow: &str, sha: &str) -> Verdict;
    /// The train-only answer for a full run that is red by failures ER-12 lets a train land on (`ci_runs::landable`), or why not.
    fn landable(&mut self, workflow: &str, sha: &str) -> Result<Landable, String>;
    /// The projects the plan of `from..to` reaches (`*` for everything).
    fn projects(&mut self, from: &str, to: &str) -> Result<BTreeSet<String>, String>;
    /// The release runs of `workflow` on this train's ref that succeeded, newest first: run id and the commit it tested.
    fn release_runs(&mut self, workflow: &str, branch: &str) -> Result<Vec<(u64, String)>, String>;
}

/// How the landing ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Landed {
    /// `main` was moved to the head.
    FastForwarded {
        /// What `main` was.
        from: String,
        /// The head it is now.
        to: String,
        /// What the landing rested on: one line per full run and per accepted row.
        notes: Vec<String>,
    },
    /// `main` already was the head (a landing that was made before; nothing was pushed).
    AlreadyOnMain(String),
}

/// A train branch: `train/` and then letters, digits, `.`, `_`, `-`.
pub fn check_branch(branch: &str) -> Result<(), String> {
    let ok = branch
        .strip_prefix("train/")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')));
    if ok {
        Ok(())
    } else {
        Err(format!("`{branch}` is not a train branch (train/<name>)"))
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

/// Green for `wf` on the head by the shared reader, or a full run that is red only by what ER-12 lets a train land on
/// (`Landable`); every accepted row is logged and noted.
fn require_full(ci: &mut dyn Ci, wf: &str, head: &str, log: &mut dyn Write, notes: &mut Vec<String>) -> Result<(), Stop> {
    match ci.verdict(wf, head) {
        Verdict::Green { run } => {
            let line = format!("{wf}: full green run {run} on {} (ci verdict)", short(head));
            let _ = writeln!(log, "train land: {line}");
            notes.push(line);
            Ok(())
        }
        Verdict::Red { run: Some(_), reason } => match ci.landable(wf, head) {
            Ok(full) => {
                let _ = writeln!(log, "train land: {wf}: landed on a {}", full.summary());
                for l in &full.lines {
                    let _ = writeln!(log, "train land: {wf}:   {l}");
                }
                notes.push(format!("{wf}: landed on a {}", full.summary()));
                notes.extend(full.lines.iter().map(|l| format!("{wf}:   {l}")));
                Ok(())
            }
            Err(why) => Err(Stop::new(
                format!(
                    "{wf} has no full green run on {} and its red run is not landable: {reason}; {why}",
                    short(head)
                ),
                format!(
                    "run the release sequence (`./dev train release`) until `./dev ci verdict --workflow {wf} --sha {head}` is green, or until the only failures are tests ER-12 lets you accept (--accept-flaky), then land"
                ),
            )),
        },
        Verdict::Red { run: None, reason } => Err(Stop::new(
            format!("{wf} has no full run of {}: {reason}", short(head)),
            format!(
                "run the release sequence (`./dev train release`) until `./dev ci verdict --workflow {wf} --sha {head}` is green, then land"
            ),
        )),
        Verdict::NotMeasured(why) => Err(Stop::new(
            format!("{wf} on {} could not be measured: {why}", short(head)),
            "fix what it names (GITEA_TOKEN, the network); not measured is never a pass, nothing was pushed",
        )),
    }
}

/// Rule 54: a release workflow's app that the plan from main reaches must have a green release run on this train: one on the head,
/// or on an earlier commit of the train that is an ancestor of the head with the plan from there to the head not reaching the app.
fn require_release(ci: &mut dyn Ci, git: &mut dyn Git, branch: &str, wf: &str, head: &str, log: &mut dyn Write) -> Result<(), Stop> {
    let app = app_of(wf);
    let runs = ci.release_runs(wf, branch).map_err(|e| {
        Stop::new(
            format!("the {wf} runs of {branch} could not be read: {e}"),
            "check GITEA_TOKEN and the network, then land again",
        )
    })?;
    for (run, sha) in runs {
        let covered = if sha == head {
            true
        } else {
            git.is_ancestor(&sha, head).unwrap_or(false)
                && ci
                    .projects(&sha, head)
                    .is_ok_and(|p| !p.contains("*") && !p.iter().any(|x| x.eq_ignore_ascii_case(&app)))
        };
        if covered {
            let _ = writeln!(log, "train land: {wf}: green release run {run} covers {} (rule 54)", short(head));
            return Ok(());
        }
    }
    Err(Stop::new(
        format!(
            "the plan reaches {app} but no green {wf} run on {branch} covers {} (rule 54)",
            short(head)
        ),
        format!("run the release sequence (`./dev train release`) so {wf} builds the installers, then land"),
    ))
}

/// Fast-forwards `main` to the head of `branch` and verifies it.
pub fn land(branch: &str, git: &mut dyn Git, ci: &mut dyn Ci, log: &mut dyn Write) -> Result<Landed, Stop> {
    check_branch(branch).map_err(|e| Stop::new(e, "name the train branch, for example train/2026-11-19-16"))?;
    let git_err = |what: &str| {
        let what = what.to_string();
        move |e: String| Stop::new(format!("{what}: {e}"), "fix what git printed, then run `./dev train land` again")
    };
    let head = git.head().map_err(git_err("the worktree's HEAD could not be read"))?;
    let remote = git
        .remote_branch(branch)
        .map_err(git_err("the remote could not be asked"))?
        .ok_or_else(|| {
            Stop::new(
                format!("the remote has no {branch}"),
                "push the train branch through the gate, run its CI, then land it",
            )
        })?;
    if remote != head {
        return Err(Stop::new(
            format!("this worktree is on {} but origin/{branch} is {}", short(&head), short(&remote)),
            "run this in the worktree that holds the train, with the pushed head checked out",
        ));
    }
    git.fetch().map_err(git_err("the fetch failed"))?;
    let main = git
        .remote_branch("main")
        .map_err(git_err("the remote could not be asked for main"))?
        .ok_or_else(|| Stop::new("the remote has no main", "check the remote"))?;
    if main == head {
        let _ = writeln!(log, "train land: main is already {}; nothing to push", short(&head));
        return Ok(Landed::AlreadyOnMain(head));
    }
    let ff = git
        .is_ancestor(&main, &head)
        .map_err(git_err("the ancestry of main could not be read"))?;
    if !ff {
        return Err(Stop::new(
            format!(
                "origin/main ({}) is not an ancestor of {}: no fast-forward",
                short(&main),
                short(&head)
            ),
            "merge origin/main into the train, push it through the gate and run the CI sequence again; never force or rebase",
        ));
    }
    // ER-10: nothing lands without the full run on this exact head, green by the one shared reader, or red only by what ER-12 lets
    // a train land on (a sealed `Landable`, never a weaker `Verdict`); a missing, partial, red or not measured one pushes nothing.
    let mut notes = Vec::new();
    require_full(ci, "ci.yml", &head, log, &mut notes)?;
    let projects = ci.projects(&main, &head).map_err(|e| {
        Stop::new(
            format!("the plan of {}..{} could not be read: {e}", short(&main), short(&head)),
            "fix the planner error above, then run `./dev train land` again",
        )
    })?;
    let reaches = |app: &str| projects.contains("*") || projects.iter().any(|p| p.eq_ignore_ascii_case(app));
    if reaches("debrief") {
        require_full(ci, "debrief.yml", &head, log, &mut notes)?;
    }
    for (wf, _) in RELEASE_WORKFLOWS {
        if reaches(&app_of(wf)) {
            require_release(ci, git, branch, wf, &head, log)?;
            notes.push(format!("{wf}: a green release run covers the head (rule 54)"));
        }
    }
    let _ = writeln!(log, "train land: fast-forwarding main {} -> {}", short(&main), short(&head));
    git.push_ff(&head, "main").map_err(git_err("the push of main was refused"))?;
    let now = git
        .remote_branch("main")
        .map_err(git_err("main could not be verified"))?
        .unwrap_or_default();
    if now != head {
        return Err(Stop::new(
            format!(
                "after the push the remote's main is {} and not {}",
                if now.is_empty() { "missing" } else { short(&now) },
                short(&head)
            ),
            "read the push output above and `git ls-remote origin refs/heads/main`; do not push again until you know why",
        ));
    }
    let _ = writeln!(log, "train land: LANDED main={} (verified with ls-remote)", short(&now));
    Ok(Landed::FastForwarded {
        from: main,
        to: head,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OLD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// A remote and a worktree in a table. `push_moves` is whether a push really moves the remote's main.
    struct Fake {
        head: String,
        remote: BTreeMap<String, String>,
        ancestor: bool,
        push_moves: bool,
        pushed: Vec<(String, String)>,
        fetched: usize,
        push_error: Option<String>,
    }

    fn fake() -> Fake {
        Fake {
            head: HEAD.into(),
            remote: BTreeMap::from([("train/x".to_string(), HEAD.to_string()), ("main".to_string(), OLD.to_string())]),
            ancestor: true,
            push_moves: true,
            pushed: Vec::new(),
            fetched: 0,
            push_error: None,
        }
    }

    impl Git for Fake {
        fn head(&mut self) -> Result<String, String> {
            Ok(self.head.clone())
        }
        fn remote_branch(&mut self, name: &str) -> Result<Option<String>, String> {
            Ok(self.remote.get(name).cloned())
        }
        fn fetch(&mut self) -> Result<(), String> {
            self.fetched += 1;
            Ok(())
        }
        fn is_ancestor(&mut self, _: &str, _: &str) -> Result<bool, String> {
            Ok(self.ancestor)
        }
        fn push_ff(&mut self, sha: &str, name: &str) -> Result<(), String> {
            self.pushed.push((sha.into(), name.into()));
            if let Some(e) = &self.push_error {
                return Err(e.clone());
            }
            if self.push_moves {
                self.remote.insert(name.into(), sha.into());
            }
            Ok(())
        }
    }

    struct FakeCi {
        verdicts: BTreeMap<String, Verdict>,
        landables: BTreeMap<String, Result<Landable, String>>,
        projects: BTreeSet<String>,
        releases: BTreeMap<String, Vec<(u64, String)>>,
        later_projects: BTreeSet<String>,
        asked: Vec<String>,
    }
    fn green() -> FakeCi {
        FakeCi {
            verdicts: BTreeMap::from([
                ("ci.yml".to_string(), Verdict::Green { run: 1 }),
                ("debrief.yml".to_string(), Verdict::Green { run: 2 }),
            ]),
            landables: BTreeMap::new(),
            projects: BTreeSet::new(),
            releases: BTreeMap::new(),
            later_projects: BTreeSet::new(),
            asked: Vec::new(),
        }
    }
    fn red(run: Option<u64>) -> Verdict {
        Verdict::Red { run, reason: "red".into() }
    }
    fn full() -> Landable {
        Landable::for_test(5, 1, &["accepted FLAKY (e2e): a test"])
    }
    impl Ci for FakeCi {
        fn verdict(&mut self, workflow: &str, _: &str) -> Verdict {
            self.asked.push(workflow.into());
            self.verdicts
                .get(workflow)
                .cloned()
                .unwrap_or(Verdict::NotMeasured("no verdict in the table".into()))
        }
        fn landable(&mut self, workflow: &str, _: &str) -> Result<Landable, String> {
            self.landables
                .get(workflow)
                .cloned()
                .unwrap_or(Err("no landable in the table".into()))
        }
        fn projects(&mut self, from: &str, _: &str) -> Result<BTreeSet<String>, String> {
            Ok(if from == OLD {
                self.projects.clone()
            } else {
                self.later_projects.clone()
            })
        }
        fn release_runs(&mut self, workflow: &str, _: &str) -> Result<Vec<(u64, String)>, String> {
            Ok(self.releases.get(workflow).cloned().unwrap_or_default())
        }
    }

    fn run(f: &mut Fake) -> Result<Landed, Stop> {
        land("train/x", f, &mut green(), &mut Vec::new())
    }

    /// The positive control: a train that is ahead of main, pushed and checked out, with a green full run, lands, and the remote says so.
    #[test]
    fn a_fast_forward_lands_and_is_verified() {
        let mut f = fake();
        let got = run(&mut f).unwrap();
        let Landed::FastForwarded { from, to, notes } = got else {
            panic!("{got:?}")
        };
        assert_eq!((from.as_str(), to.as_str()), (OLD, HEAD));
        assert!(notes[0].contains("full green run 1"), "{notes:?}");
        assert_eq!(f.pushed, vec![(HEAD.to_string(), "main".to_string())]);
        assert_eq!(f.fetched, 1);
    }

    /// A verdict that is not Green (and not landable) for the exact head pushes nothing.
    #[test]
    fn a_missing_partial_red_or_not_measured_verdict_pushes_nothing() {
        for v in [
            None,
            Some(red(None)),
            Some(red(Some(5))),
            Some(Verdict::NotMeasured("no token".into())),
        ] {
            let mut f = fake();
            let mut ci = green();
            match v.clone() {
                Some(v) => ci.verdicts.insert("ci.yml".into(), v),
                None => ci.verdicts.remove("ci.yml"),
            };
            let stop = land("train/x", &mut f, &mut ci, &mut Vec::new()).unwrap_err();
            assert!(stop.cause.contains("ci.yml"), "{stop:?}");
            assert!(f.pushed.is_empty(), "{v:?}");
        }
    }

    /// ER-12: a red full run lands only when the train-only `Landable` says so; its accepted rows are logged and noted.
    #[test]
    fn a_red_full_run_lands_only_when_it_is_landable_and_says_what_was_accepted() {
        let mut ci = green();
        ci.verdicts.insert("ci.yml".into(), red(Some(5)));
        ci.landables.insert("ci.yml".into(), Ok(full()));
        let (mut f, mut log) = (fake(), Vec::new());
        let got = land("train/x", &mut f, &mut ci, &mut log).unwrap();
        let Landed::FastForwarded { notes, .. } = got else { panic!() };
        assert!(
            notes.iter().any(|n| n.contains("landed on a full run 5 with 1 accepted FLAKY")),
            "{notes:?}"
        );
        assert!(String::from_utf8(log).unwrap().contains("accepted FLAKY (e2e): a test"));
        assert_eq!(f.pushed.len(), 1);
        // not landable: refused with both reasons, nothing pushed
        let mut ci = green();
        ci.verdicts.insert("ci.yml".into(), red(Some(5)));
        ci.landables.insert("ci.yml".into(), Err("a NEW failure".into()));
        let mut f = fake();
        let stop = land("train/x", &mut f, &mut ci, &mut Vec::new()).unwrap_err();
        assert!(
            stop.cause.contains("not landable") && stop.cause.contains("a NEW failure"),
            "{stop:?}"
        );
        assert!(f.pushed.is_empty());
    }

    #[test]
    fn debrief_must_be_green_too_when_the_plan_reaches_it_and_only_then() {
        let mut ci = green();
        ci.projects = BTreeSet::from(["debrief".to_string()]);
        ci.verdicts.insert("debrief.yml".into(), red(Some(3)));
        let mut f = fake();
        assert!(
            land("train/x", &mut f, &mut ci, &mut Vec::new())
                .unwrap_err()
                .cause
                .contains("debrief.yml")
        );
        assert!(f.pushed.is_empty());
        let mut ci = green();
        ci.verdicts.insert("debrief.yml".into(), red(Some(3)));
        let mut f = fake();
        land("train/x", &mut f, &mut ci, &mut Vec::new()).unwrap();
        assert_eq!(ci.asked, vec!["ci.yml".to_string()], "not reached: not even asked");
        let mut ci = green();
        ci.projects = BTreeSet::from(["debrief".to_string()]);
        ci.releases.insert("dyno-release.yml".into(), vec![]);
        let mut f = fake();
        land("train/x", &mut f, &mut ci, &mut Vec::new()).unwrap();
        assert_eq!(f.pushed.len(), 1);
    }

    /// Rule 54: an app whose release path the plan reaches needs a green release run on the head, or on an earlier train commit with
    /// the app not reached since; a missing, red-only or unrelated one pushes nothing.
    #[test]
    fn a_reached_apps_release_run_must_cover_the_head() {
        let reached = || {
            let mut ci = green();
            ci.projects = BTreeSet::from(["dyno".to_string()]);
            ci
        };
        // none
        let mut f = fake();
        let stop = land("train/x", &mut f, &mut reached(), &mut Vec::new()).unwrap_err();
        assert!(
            stop.cause.contains("dyno") && stop.cause.contains("dyno-release.yml") && stop.cause.contains("rule 54"),
            "{stop:?}"
        );
        assert!(f.pushed.is_empty());
        // on the head
        let mut ci = reached();
        ci.releases.insert("dyno-release.yml".into(), vec![(9, HEAD.into())]);
        let mut f = fake();
        land("train/x", &mut f, &mut ci, &mut Vec::new()).unwrap();
        // on an earlier commit, app not reached since
        let mut ci = reached();
        ci.releases.insert("dyno-release.yml".into(), vec![(8, "cccccccc".into())]);
        let mut f = fake();
        land("train/x", &mut f, &mut ci, &mut Vec::new()).unwrap();
        // on an earlier commit, app reached since, or not an ancestor
        let mut ci = reached();
        ci.releases.insert("dyno-release.yml".into(), vec![(8, "cccccccc".into())]);
        ci.later_projects = BTreeSet::from(["dyno".to_string()]);
        let mut f = fake();
        assert!(land("train/x", &mut f, &mut ci, &mut Vec::new()).is_err());
        let mut ci = reached();
        ci.releases.insert("dyno-release.yml".into(), vec![(8, "cccccccc".into())]);
        let mut f = fake();
        f.ancestor = false;
        // (main..head is also "not an ancestor" for the fast-forward check, so this one stops there; both push nothing)
        assert!(land("train/x", &mut f, &mut ci, &mut Vec::new()).is_err());
        assert!(f.pushed.is_empty());
        // everything reached: every release workflow is needed
        let mut ci = green();
        ci.projects = BTreeSet::from(["*".to_string()]);
        let mut f = fake();
        assert!(land("train/x", &mut f, &mut ci, &mut Vec::new()).is_err());
    }

    #[test]
    fn a_push_the_remote_did_not_take_is_not_a_landing() {
        let mut f = fake();
        f.push_moves = false;
        let stop = run(&mut f).unwrap_err();
        assert!(
            stop.cause
                .contains("after the push the remote's main is bbbbbbbbb and not aaaaaaaaa"),
            "{stop:?}"
        );
    }

    #[test]
    fn a_main_that_moved_on_is_no_fast_forward_and_nothing_is_pushed() {
        let mut f = fake();
        f.ancestor = false;
        let stop = run(&mut f).unwrap_err();
        assert!(
            stop.cause.contains("not an ancestor") && stop.next.contains("never force or rebase"),
            "{stop:?}"
        );
        assert!(f.pushed.is_empty());
    }

    #[test]
    fn a_worktree_that_is_not_on_the_pushed_head_pushes_nothing() {
        let mut f = fake();
        f.head = OLD.into();
        assert!(
            run(&mut f)
                .unwrap_err()
                .cause
                .contains("this worktree is on bbbbbbbbb but origin/train/x is aaaaaaaaa")
        );
        assert!(f.pushed.is_empty());
        let mut f = fake();
        f.remote.remove("train/x");
        assert!(run(&mut f).unwrap_err().cause.contains("the remote has no train/x"));
        assert!(f.pushed.is_empty());
    }

    #[test]
    fn a_main_that_already_is_the_head_pushes_nothing() {
        let mut f = fake();
        f.remote.insert("main".into(), HEAD.into());
        assert_eq!(run(&mut f).unwrap(), Landed::AlreadyOnMain(HEAD.into()));
        assert!(f.pushed.is_empty());
    }

    #[test]
    fn a_refused_push_stops_with_the_cause() {
        let mut f = fake();
        f.push_error = Some("pre-push: conformance failed".into());
        let stop = run(&mut f).unwrap_err();
        assert!(
            stop.cause.contains("the push of main was refused: pre-push: conformance failed"),
            "{stop:?}"
        );
    }

    #[test]
    fn only_train_branches_land() {
        for bad in ["main", "feat/x", "train/", "train/a b", "train/x;rm", "", "refs/heads/train/x"] {
            let mut f = fake();
            assert!(land(bad, &mut f, &mut green(), &mut Vec::new()).is_err(), "{bad}");
            assert!(f.pushed.is_empty());
        }
    }
}
