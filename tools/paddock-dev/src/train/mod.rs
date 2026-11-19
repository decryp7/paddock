//! `dev train`: the train integrator's release sequence and landing (the port of the scratch scripts `seq-release.sh`,
//! `seq-full.sh`, `triage-verdict.py` and `land-*.sh` of train 14).
//!
//! * `train release <train-branch> --since-run <id|passed:id> --workflow ci.yml|debrief.yml --max <duration>`: under the
//!   train-CI hold (`scripts/train-ci-hold.sh`, the one reservation member runs wait on), a failed-only fix round judged by
//!   [`verdict`], then `debrief.yml` after a `ci.yml` round, then the release workflows; it stops at the first run that is not green.
//! * `train land <train-branch>`: fast-forwards `main` to the train head and verifies it with `ls-remote`. Nothing else.
//!
//! The logic is pure behind ports ([`sequence`], [`land`]); [`real`] holds the Gitea and git side. ADR 0098 and the train checklist
//! (docs/train-checklist.md) say where in a train's life each runs.

pub mod find;
pub mod land;
pub mod real;
pub mod sequence;
pub mod verdict;

use crate::wait::{MAX_LIMIT, fmt_duration, parse_duration};
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Exit, Report};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

const SERVER: &str = "https://dev.decryptology.net";
/// The exit code of the hold when another train's sequence holds it (`scripts/vm-lock.sh`: EX_TEMPFAIL).
const HOLD_BUSY: i32 = 75;

/// The `train` commands.
#[derive(Subcommand)]
pub enum TrainCommand {
    /// A train's release sequence: a failed-only fix round (judged: only accepted FLAKY tests may remain), debrief.yml after a
    /// ci.yml round, then the release workflows; stops at the first red. Runs under the train-CI hold. It does not land.
    Release(ReleaseArgs),
    /// Fast-forward `main` to the train head (never a force or a rebase) and verify it with `ls-remote`.
    Land(LandArgs),
}

/// Arguments of `train release`.
#[derive(Args, Clone)]
pub struct ReleaseArgs {
    /// The train branch, for example `train/2026-11-19-16`. Its head must be pushed and checked out in this worktree.
    branch: String,
    /// The run the fix round repeats (`since_run`: its failed jobs only), or `passed:<run>` for a round that already ran: it is
    /// not dispatched again, but judged now.
    #[arg(long, value_name = "RUN|passed:RUN")]
    since_run: String,
    /// The workflow of the fix round.
    #[arg(long, value_name = "ci.yml|debrief.yml")]
    workflow: String,
    /// A flaky test that may remain in a red fix round, as `file:line:col` or `file:line` (repeat). Only FLAKY tests (passed on a
    /// retry in the run) can be accepted, never a new or failed one; its fix item is recorded first (ER-12).
    #[arg(long = "accept-flaky", value_name = "LOCATION")]
    accept_flaky: Vec<String>,
    /// Use this green run of an unchanged tree instead of dispatching a release workflow, as `<workflow>=<run>` (repeat).
    #[arg(long = "reuse", value_name = "WORKFLOW=RUN")]
    reuse: Vec<String>,
    /// The longest the whole sequence takes, for example 14h. Required: every wait ends.
    #[arg(long, value_name = "DURATION")]
    max: String,
    /// Internal: the hold is already taken (this process runs under `scripts/train-ci-hold.sh`).
    #[arg(long, hide = true)]
    already_held: bool,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `train land`.
#[derive(Args)]
pub struct LandArgs {
    /// The train branch, for example `train/2026-11-19-16`; its pushed head must be checked out in this worktree.
    branch: String,
}

/// Runs a `train` command.
pub fn command(c: TrainCommand) -> Result<Report, CliError> {
    match c {
        TrainCommand::Release(a) => release(&a),
        TrainCommand::Land(a) => land_cmd(&a),
    }
}

fn parse_reuse(items: &[String]) -> Result<BTreeMap<String, u64>, String> {
    let mut out = BTreeMap::new();
    for item in items {
        let (wf, run) = item
            .split_once('=')
            .and_then(|(w, r)| Some((w, r.parse::<u64>().ok()?)))
            .filter(|(w, _)| !w.is_empty())
            .ok_or_else(|| format!("--reuse {item:?} is not <workflow>=<run id>"))?;
        if out.insert(wf.to_string(), run).is_some() {
            return Err(format!("--reuse {wf} is given twice"));
        }
    }
    Ok(out)
}

/// Everything `release` can refuse before it touches Gitea, as a plan (the head is filled in later).
fn plan_of(a: &ReleaseArgs) -> Result<sequence::Plan, String> {
    land::check_branch(&a.branch)?;
    let max = parse_duration(&a.max)?;
    if max.is_zero() || max > MAX_LIMIT {
        return Err(format!("--max {} is out of range: use 1s to 24h.", fmt_duration(max)));
    }
    let accepted = a
        .accept_flaky
        .iter()
        .map(|l| verdict::Accepted::parse(l))
        .collect::<Result<Vec<_>, _>>()?;
    let plan = sequence::Plan {
        branch: a.branch.clone(),
        sha: String::new(),
        workflow: sequence::FixWorkflow::parse(&a.workflow)?,
        round: sequence::Round::parse(&a.since_run)?,
        accepted,
        reuse: parse_reuse(&a.reuse)?,
        max,
    };
    plan.check()?;
    Ok(plan)
}

fn release(a: &ReleaseArgs) -> Result<Report, CliError> {
    let usage = |e: String| CliError::usage(e).with_hint("Run `./dev train release --help`.");
    let mut plan = plan_of(a).map_err(usage)?;
    let token = crate::ci_runs::token().map_err(|e| e.with_hint("The sequence needs a token that can write Actions."))?;
    let root = crate::gates::command::repo_root(None)?;
    let mut git = real::GitCli { root: &root };
    let local = land::Git::head(&mut git).map_err(CliError::failure)?;
    let remote = land::Git::remote_branch(&mut git, &a.branch).map_err(CliError::failure)?;
    if remote.as_deref() != Some(local.as_str()) {
        return Err(CliError::new(
            Exit::Findings,
            format!(
                "::error::train release: this worktree is on {} but origin/{} is {}. Next step: run it in the worktree that holds the train, after the push through the gate.",
                &local[..local.len().min(9)],
                a.branch,
                remote.as_deref().map_or("missing".to_string(), |r| r[..r.len().min(9)].to_string())
            ),
        ));
    }
    plan.sha = local;
    if !a.already_held {
        return held(&root);
    }
    let mut ports = real::RealPorts {
        root,
        branch: a.branch.clone(),
        sha: plan.sha.clone(),
        server: a.server.clone(),
        repo: a.repo.clone(),
        token,
        clock: crate::wait::SystemClock::new(),
    };
    let mut log = std::io::stderr();
    match sequence::release(&plan, &mut ports, &mut log) {
        Ok(done) => {
            let lines: Vec<String> = done
                .steps
                .iter()
                .map(|(wf, s)| match s {
                    sequence::Source::Ran(r) => format!("{wf}: run {r} green"),
                    sequence::Source::Judged(r) => format!("{wf}: run {r} judged: only accepted flaky tests failed"),
                    sequence::Source::Reused(r) => format!("{wf}: reused green run {r}"),
                })
                .collect();
            let steps: Vec<_> = done
                .steps
                .iter()
                .map(|(w, s)| json!({"workflow": w, "source": format!("{s:?}")}))
                .collect();
            Ok(Report::new(
                json!({"green": true, "branch": a.branch, "sha": plan.sha, "steps": steps}),
                format!(
                    "Release sequence green on {} ({}).\n{}\nNext step: ./dev train land {}",
                    &plan.sha[..9],
                    a.branch,
                    lines.join("\n"),
                    a.branch
                ),
            ))
        }
        Err(stop) => Err(CliError::new(Exit::Findings, stop.annotation())),
    }
}

/// Runs this same command again under the train-CI hold and ends with its exit code: the reservation is the lock
/// `scripts/train-ci-hold.sh` takes (a detached holder, freed when the command ends), which has no in-process library, so it
/// wraps the child that does the work. The child bounds itself with `--max`.
fn held(root: &Path) -> Result<Report, CliError> {
    let exe = crate::tool_cache::self_exe().map_err(|e| CliError::failure(format!("this program cannot be found: {e}")))?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let started = std::time::Instant::now();
    eprintln!("train release: taking the train-CI hold (scripts/train-ci-hold.sh)");
    let status = paddock_proc::command("bash")
        .arg(root.join("scripts/train-ci-hold.sh"))
        .arg("train-release")
        .arg("--")
        .arg(exe)
        .args(&args)
        .arg("--already-held")
        .status()
        .map_err(|e| CliError::failure(format!("the hold could not be started: {e}")))?;
    let code = status.code().unwrap_or(1);
    eprintln!(
        "train release: the hold ended after {} (exit {code})",
        fmt_duration(started.elapsed())
    );
    if code == HOLD_BUSY {
        eprintln!(
            "::error::train release: another train's CI sequence holds the runners. Next step: `scripts/train-ci-hold.sh --status` names it; wait for it to end, then run this again."
        );
    }
    // The child printed its own report or its one `::error::`; its exit code is ours.
    std::process::exit(code);
}

fn land_cmd(a: &LandArgs) -> Result<Report, CliError> {
    land::check_branch(&a.branch).map_err(|e| CliError::usage(e).with_hint("Run `./dev train land --help`."))?;
    let root = crate::gates::command::repo_root(None)?;
    let mut git = real::GitCli { root: &root };
    match land::land(&a.branch, &mut git, &mut std::io::stderr()) {
        Ok(land::Landed::FastForwarded { from, to }) => Ok(Report::new(
            json!({"landed": true, "from": from, "to": to}),
            format!("Landed: main is now {} (was {}); verified with ls-remote.", &to[..9], &from[..9]),
        )),
        Ok(land::Landed::AlreadyOnMain(sha)) => Ok(Report::new(
            json!({"landed": false, "already": true, "main": sha}),
            format!("main already is {}; nothing was pushed.", &sha[..9]),
        )),
        Err(stop) => Err(CliError::new(
            Exit::Findings,
            stop.annotation().replace("train release", "train land"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> ReleaseArgs {
        let mut a = ReleaseArgs {
            branch: "train/2026-11-19-16".into(),
            since_run: "8300".into(),
            workflow: "ci.yml".into(),
            accept_flaky: vec![],
            reuse: vec![],
            max: "14h".into(),
            already_held: false,
            repo: "decryp7/paddock".into(),
            server: "https://gitea.invalid".into(),
        };
        for e in extra {
            match e.split_once('=') {
                Some(("reuse", v)) => a.reuse.push(v.to_string()),
                Some(("accept", v)) => a.accept_flaky.push(v.to_string()),
                Some(("max", v)) => a.max = v.to_string(),
                Some(("branch", v)) => a.branch = v.to_string(),
                Some(("workflow", v)) => a.workflow = v.to_string(),
                Some(("since", v)) => a.since_run = v.to_string(),
                _ => panic!("{e}"),
            }
        }
        a
    }

    #[test]
    fn good_arguments_make_a_plan_and_every_bad_one_is_refused_before_anything_runs() {
        let plan = plan_of(&args(&[
            "accept=e2e/a11y.e2e.ts:6:7",
            "reuse=dyno-release.yml=8302",
            "since=passed:8307",
        ]))
        .unwrap();
        assert_eq!(plan.reuse["dyno-release.yml"], 8302);
        assert_eq!(plan.accepted.len(), 1);
        for bad in [
            "branch=main",
            "max=0s",
            "max=25h",
            "max=soon",
            "workflow=dyno.yml",
            "since=x",
            "accept=nonsense",
            "reuse=dyno.yml=1",
            "reuse=dyno-release.yml",
            "reuse=dyno-release.yml=x",
        ] {
            assert!(plan_of(&args(&[bad])).is_err(), "{bad}");
        }
        assert!(plan_of(&args(&["reuse=dyno-release.yml=1", "reuse=dyno-release.yml=2"])).is_err());
    }
}
