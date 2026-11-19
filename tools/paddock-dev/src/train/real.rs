// ci-runs-reader: train release: finds the run it dispatched and waits for its end; a red round is judged by ci triage, never by its conclusion alone
//! The real ports of `train release` and `train land`: Gitea through the shared client, the disk check, the triage and `git`.
//! Nothing here decides anything; the tests of the logic use tables (`sequence`, `land`), never Gitea or a repository (ER-14).

use super::find;
use super::land::Git;
use super::sequence::{Ports, RunEnd};
use crate::ci_runs::client::{Client, RunInfo};
use crate::ci_runs::{dispatch, time, triage};
use crate::disk::check::{CheckArgs, run as disk_check};
use crate::disk::config::CommonArgs;
use crate::wait::{CiRunProbe, Clock, CurlHttp, Outcome, SystemClock, fmt_duration, wait_for};
use paddock_proc::{Limits, Outcome as ProcOutcome, command, run_bounded};
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
}

impl RealPorts {
    fn newest_runs(&self) -> Result<Vec<RunInfo>, String> {
        let mut http = CurlHttp;
        let mut client = Client::new(&mut http, &self.server, &self.repo, &self.token);
        Ok(client.runs_page(1)?.runs)
    }
}

impl Ports for RealPorts {
    fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    fn disk_check(&mut self) -> Result<(), String> {
        let args = CheckArgs {
            paths: vec![self.root.clone()],
            common: CommonArgs::default(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = disk_check(&args, &|k| std::env::var(k).ok(), &mut out, &mut err);
        let (out, err) = (String::from_utf8_lossy(&out), String::from_utf8_lossy(&err));
        // The check's own lines go to our log; a warning is not a refusal.
        eprint!("{out}{err}");
        if code == 0 {
            return Ok(());
        }
        let cause = err.lines().find(|l| l.contains("::error::")).unwrap_or("the disk check refused");
        Err(cause.trim().trim_start_matches("::error::").trim().to_string())
    }

    fn dispatch(&mut self, workflow: &str, inputs: &[(String, String)], wait: Duration) -> Result<u64, String> {
        let kv: Vec<String> = inputs.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let wait = wait.max(Duration::from_secs(1));
        let mut req = dispatch::parse_request(workflow, &self.branch, &kv, &fmt_duration(wait), None)?;
        match dispatch::train_waiters(&self.root) {
            Ok(n) => req.train_waiters = n,
            Err(e) => eprintln!("::warning::train release: {e}; a train run waits here, not in its first job."),
        }
        let before = find::newest(&self.newest_runs()?, workflow, &self.branch, &self.sha);
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
        find::find_new_run(
            &mut || self.newest_runs(),
            &mut |d| sleeper.sleep(d),
            (workflow, &self.branch, &self.sha),
            before,
            find::TRIES,
            find::PAUSE,
        )
    }

    fn wait_run(&mut self, run: u64, max: Duration) -> RunEnd {
        let mut http = CurlHttp;
        let mut probe = CiRunProbe::new(&mut http, &self.server, &self.repo, run, self.token.clone());
        let mut clock = SystemClock::new();
        match wait_for(&mut probe, &mut clock, max, POLL, &mut std::io::stderr()) {
            Outcome::Done { .. } => RunEnd::Green,
            Outcome::Failed { .. } => RunEnd::Red,
            Outcome::TimedOut { last, max } => RunEnd::Unfinished(format!("{last} after {} (the bound)", fmt_duration(max))),
            Outcome::Broken { error, .. } => RunEnd::Unfinished(format!("the run could not be read: {error}")),
        }
    }

    fn run_info(&mut self, run: u64) -> Result<Option<RunInfo>, String> {
        let mut http = CurlHttp;
        Client::new(&mut http, &self.server, &self.repo, &self.token).run(run)
    }

    fn triage(&mut self, run: u64) -> Result<Value, String> {
        let mut http = CurlHttp;
        let mut client = Client::new(&mut http, &self.server, &self.repo, &self.token);
        let req = triage::Request {
            run,
            against: None,
            app: None,
            days: 14,
            history: true,
            repo: self.repo.clone(),
            now_secs: time::now_secs(),
        };
        let report = triage::collect(&mut client, &req, &mut |l| eprintln!("{l}"))?;
        let mut doc = report.json();
        // `ci triage --json` adds `ok` when the triage worked; here it worked, or `collect` returned the error above.
        doc["ok"] = Value::Bool(true);
        Ok(doc)
    }
}

/// An upper bound for one push, the repository's pre-push gate included. Not a measured value: the gate's own steps are bounded
/// by themselves, and this only keeps a hung push from holding the train for ever (ER-32).
const PUSH_LIMIT: Duration = Duration::from_secs(60 * 60);
const QUICK_LIMIT: Duration = Duration::from_secs(300);

/// `git` on the integrator's worktree. It never runs with `GIT_DIR`, `GIT_WORK_TREE` or `GIT_INDEX_FILE` set: those point git at
/// another repository.
pub struct GitCli<'a> {
    /// The worktree.
    pub root: &'a Path,
}

impl GitCli<'_> {
    fn base(&self, args: &[&str]) -> std::process::Command {
        let mut cmd = command("git");
        cmd.arg("-C").arg(self.root).args(args);
        for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
            cmd.env_remove(var);
        }
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
