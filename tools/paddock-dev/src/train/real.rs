//! The real ports of `train release` and `train land`: Gitea through the shared client, the disk check, the triage and `git`.
//! Nothing here decides anything; the tests of the logic use tables (`sequence`, `land`), never Gitea or a repository (ER-14).

use super::accept::{FlakyCheck, Tree};
use super::land::Git;
use super::sequence::{Ports, Reach, RunEnd};
use super::verdict::Accepted;
use crate::ci_runs::dispatch;
use crate::ci_runs::landable::Landable;
use crate::ci_runs::train_runs::{self as runs, RunFacts};
use crate::ci_runs::verdict::Verdict;
use crate::wait::{CiRunProbe, Clock, CurlHttp, Outcome, SystemClock, fmt_duration, wait_for};
use paddock_proc::{Limits, Outcome as ProcOutcome, run_bounded};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often a running CI run is looked at.
const POLL: Duration = Duration::from_secs(30);

/// Gitea and the repository for the sequence.
pub struct RealPorts {
    /// The worktree of the train.
    pub root: PathBuf,
    /// The train branch every run is dispatched on.
    pub branch: String,
    /// Its head.
    pub sha: String,
    /// The Gitea server.
    pub server: String,
    /// The repository on it.
    pub repo: String,
    /// The API token (a header only).
    pub token: String,
    /// Started with the sequence.
    pub clock: SystemClock,
    /// What `--accept-flaky` named.
    pub accepted: Vec<Accepted>,
    /// The files the train changes against main.
    pub changed: Vec<String>,
}

impl Ports for RealPorts {
    fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    fn disk_check(&mut self) -> Result<(), String> {
        // The check `ci dispatch` makes first (the runner volumes, refused under 50 GiB): this sequence dispatches in-process, so it
        // asks the same function, once per dispatch, and adds no check of its own.
        crate::disk::check::before_dispatch(&|k| std::env::var(k).ok(), &mut std::io::stderr())
    }

    fn dispatch(&mut self, workflow: &str, inputs: &[(String, String)], wait: Duration) -> Result<u64, String> {
        let kv: Vec<String> = inputs.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let wait = wait.max(Duration::from_secs(1));
        let mut req = dispatch::parse_request(workflow, &self.branch, &kv, &fmt_duration(wait), None)?;
        match dispatch::train_waiters(&self.root) {
            Ok(n) => req.train_waiters = n,
            Err(e) => eprintln!("::warning::train release: {e}; a train run waits here, not in its first job."),
        }
        let before = runs::newest(
            &runs::newest_runs(&self.server, &self.repo, &self.token)?,
            workflow,
            &self.branch,
            &self.sha,
        );
        let read = |name: &str| std::fs::read_to_string(self.root.join(".gitea/workflows").join(name)).ok();
        let mut http = CurlHttp;
        let mut clock = dispatch::system_clock();
        let target = dispatch::Target {
            server: &self.server,
            repo: &self.repo,
            token: &self.token,
        };
        match dispatch::run(&mut http, &target, &req, &read, &mut clock, &mut std::io::stderr())? {
            dispatch::Dispatched::Yes { .. } => {}
            dispatch::Dispatched::StillBusy { last } => return Err(format!("the runner stayed busy ({last})")),
        }
        let mut sleeper = SystemClock::new();
        runs::find_new_run(
            &mut || runs::newest_runs(&self.server, &self.repo, &self.token),
            &mut |d| sleeper.sleep(d),
            (workflow, &self.branch, &self.sha),
            before,
            runs::TRIES,
            runs::PAUSE,
        )
    }

    fn wait_run(&mut self, run: u64, max: Duration) -> RunEnd {
        let mut http = CurlHttp;
        let mut probe = CiRunProbe::new(&mut http, &self.server, &self.repo, run, self.token.clone());
        let mut clock = SystemClock::new();
        match wait_for(&mut probe, &mut clock, max, POLL, &mut std::io::stderr()) {
            // The probe's own pass/fail wording is not read: it only says the run is over; the result is read from the run.
            Outcome::Done { .. } | Outcome::Failed { .. } => RunEnd::Finished,
            Outcome::TimedOut { last, max } => RunEnd::Unfinished(format!("{last} after {} (the bound)", fmt_duration(max))),
            Outcome::Broken { error, .. } => RunEnd::Unfinished(format!("the run could not be read: {error}")),
        }
    }

    fn run_facts(&mut self, run: u64) -> Result<Option<RunFacts>, String> {
        runs::facts(&self.server, &self.repo, &self.token, run)
    }

    fn ci_verdict(&mut self, workflow: &str, sha: &str) -> Verdict {
        runs::full_verdict(&self.server, &self.repo, &self.token, workflow, sha)
    }

    fn landable(&mut self, workflow: &str, sha: &str) -> Result<Landable, String> {
        landable_of(
            &self.root,
            (&self.server, &self.repo, &self.token),
            workflow,
            sha,
            &self.accepted,
            &self.changed,
        )
    }

    fn triage(&mut self, run: u64) -> Result<Value, String> {
        runs::triage_doc(&self.server, &self.repo, &self.token, run)
    }

    fn is_ancestor(&mut self, ancestor: &str, of: &str) -> Result<bool, String> {
        Git::is_ancestor(&mut GitCli { root: &self.root }, ancestor, of)
    }

    fn reach(&mut self, app: &str, from_sha: &str) -> Result<Reach, String> {
        reach_of(&self.root, app, from_sha, &self.sha)
    }
}

/// A ceiling (ER-12: not a tuned limit, nothing waits for it to pass) for one push with the repository's pre-push gate: 60 minutes.
/// Measured 2026-11-19: the pre-push gate of the push that carried this very change took 342.75 s (push log of that push, recorded
/// in the lessons file, "The train integrator's sequence scripts are a command now"), so the ceiling is about ten times the
/// measurement. Its only job is to keep a hung push from holding the train's runners for ever (ER-32); the gate's own steps have
/// their own bounds.
const PUSH_LIMIT: Duration = Duration::from_secs(60 * 60);
const QUICK_LIMIT: Duration = Duration::from_secs(300);

/// `git` on the integrator's worktree, started through `git_env::git()`: never with `GIT_DIR`, `GIT_WORK_TREE` or `GIT_INDEX_FILE`
/// set (those point git at another repository).
pub struct GitCli<'a> {
    /// The worktree.
    pub root: &'a Path,
}

impl GitCli<'_> {
    fn base(&self, args: &[&str]) -> std::process::Command {
        let mut cmd = crate::git_env::git();
        cmd.arg("-C").arg(self.root).args(args);
        cmd
    }

    fn run(&self, args: &[&str], limit: Duration) -> Result<String, String> {
        let out = run_bounded(self.base(args), Limits::new(limit).max_output(1 << 20))
            .map_err(|e| format!("git {} could not run: {e}", args[0]))?;
        match out.outcome {
            ProcOutcome::Exited(st) if st.success() => {
                let err = out.stderr_text();
                if !err.trim().is_empty() {
                    eprintln!("{}", err.trim_end());
                }
                Ok(out.stdout_text())
            }
            ProcOutcome::TimedOut => Err(format!("git {} did not finish within {}", args[0], fmt_duration(limit))),
            _ => {
                let err = out.stderr_text();
                let tail: Vec<&str> = err.lines().rev().take(12).collect();
                Err(format!(
                    "git {} failed: {}",
                    args[0],
                    tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
                ))
            }
        }
    }
}

impl GitCli<'_> {
    /// The files `git diff --name-only <range>` lists.
    pub fn changed(&mut self, range: &str) -> Result<Vec<String>, String> {
        Ok(self
            .run(&["diff", "--name-only", range], QUICK_LIMIT)?
            .lines()
            .map(str::to_string)
            .collect())
    }
}

/// The tracked source and the declaration of an app's package at the worktree's head (what [`super::accept`] resolves features from).
pub fn tree_of(root: &Path, a: &Accepted) -> Result<Tree, String> {
    let pkg = a.package();
    let tracked = GitCli { root }
        .run(&["ls-files", "--", &format!("{pkg}/src")], QUICK_LIMIT)?
        .lines()
        .map(str::to_string)
        .collect();
    let decl_path = root.join(&pkg).join(super::accept::DECLARATION);
    let declaration = match std::fs::read_to_string(&decl_path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{} cannot be read: {e}", decl_path.display())),
    };
    Ok(Tree { tracked, declaration })
}

/// The `Landable` of a full run (sealed in `ci_runs::landable`), as the text the sequence and the landing use.
pub fn landable_of(
    root: &Path,
    (server, repo, token): (&str, &str, &str),
    workflow: &str,
    sha: &str,
    accepted: &[Accepted],
    changed: &[String],
) -> Result<Landable, String> {
    let tree = |a: &Accepted| tree_of(root, a);
    let check = FlakyCheck {
        accepted,
        changed,
        tree: &tree,
    };
    let text = GitCli { root }.run(&["show", &format!("{sha}:.gitea/workflows/{workflow}")], QUICK_LIMIT)?;
    crate::ci_runs::landable::landable((server, repo, token), workflow, sha, &text, &|app, t| check.check(app, t))
}

/// What `train land` asks CI about the head.
pub struct RealCi {
    /// The worktree.
    pub root: PathBuf,
    /// The Gitea server.
    pub server: String,
    /// The repository.
    pub repo: String,
    /// The API token.
    pub token: String,
    /// What `--accept-flaky` named.
    pub accepted: Vec<Accepted>,
    /// The files the train changes against main.
    pub changed: Vec<String>,
}

impl super::land::Ci for RealCi {
    fn verdict(&mut self, workflow: &str, sha: &str) -> Verdict {
        runs::full_verdict(&self.server, &self.repo, &self.token, workflow, sha)
    }

    fn landable(&mut self, workflow: &str, sha: &str) -> Result<Landable, String> {
        landable_of(
            &self.root,
            (&self.server, &self.repo, &self.token),
            workflow,
            sha,
            &self.accepted,
            &self.changed,
        )
    }

    fn projects(&mut self, from: &str, to: &str) -> Result<std::collections::BTreeSet<String>, String> {
        let plan = crate::queue::real::run_planner(&self.root, from, to)?;
        Ok(crate::queue::real::projects_of_plan(&plan))
    }

    fn release_runs(&mut self, workflow: &str, branch: &str) -> Result<Vec<(u64, String)>, String> {
        runs::release_runs(&self.server, &self.repo, &self.token, workflow, branch)
    }
}

/// What the planner says `from..to` reaches, for `app`.
pub fn reach_of(root: &Path, app: &str, from: &str, to: &str) -> Result<Reach, String> {
    let plan = crate::queue::real::run_planner(root, from, to)?;
    let projects = crate::queue::real::projects_of_plan(&plan);
    let reached = projects.contains("*") || projects.iter().any(|p| p.eq_ignore_ascii_case(app));
    Ok(Reach {
        reached,
        diff: if projects.is_empty() {
            "nothing".into()
        } else {
            projects.into_iter().collect::<Vec<_>>().join(", ")
        },
    })
}

impl Git for GitCli<'_> {
    fn head(&mut self) -> Result<String, String> {
        Ok(self.run(&["rev-parse", "HEAD"], QUICK_LIMIT)?.trim().to_string())
    }

    fn remote_branch(&mut self, name: &str) -> Result<Option<String>, String> {
        let refname = format!("refs/heads/{name}");
        let text = self.run(&["ls-remote", "origin", &refname], QUICK_LIMIT)?;
        Ok(text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .find(|(_, r)| *r == refname)
            .map(|(sha, _)| sha.to_string()))
    }

    fn fetch(&mut self) -> Result<(), String> {
        self.run(&["fetch", "--quiet", "origin"], QUICK_LIMIT).map(|_| ())
    }

    fn is_ancestor(&mut self, ancestor: &str, of: &str) -> Result<bool, String> {
        let out = run_bounded(self.base(&["merge-base", "--is-ancestor", ancestor, of]), Limits::new(QUICK_LIMIT))
            .map_err(|e| format!("git merge-base could not run: {e}"))?;
        match out.outcome {
            ProcOutcome::Exited(st) if st.code() == Some(0) => Ok(true),
            ProcOutcome::Exited(st) if st.code() == Some(1) => Ok(false),
            _ => Err(format!("git merge-base failed: {}", out.stderr_text().trim())),
        }
    }

    fn push_ff(&mut self, sha: &str, name: &str) -> Result<(), String> {
        // `<sha>:refs/heads/<name>` without `+`: the server refuses anything but a fast-forward.
        let spec = format!("{sha}:refs/heads/{name}");
        self.run(&["push", "origin", &spec], PUSH_LIMIT).map(|_| ())
    }
}
