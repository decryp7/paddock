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

pub mod accept;
pub mod hold;
pub mod land;
pub mod real;
pub mod sequence;
pub mod verdict;

use crate::wait::{MAX_LIMIT, fmt_duration, parse_duration};
use clap::{Args, Subcommand};
use land::Git as _;
use paddock_cli::{CliError, Exit, Report};
use serde_json::json;
use std::collections::BTreeMap;

const SERVER: &str = "https://dev.decryptology.net";
/// The `train` commands.
#[derive(Subcommand)]
pub enum TrainCommand {
    /// A train's release sequence: a failed-only fix round (judged: only accepted FLAKY tests may remain), debrief.yml after a
    /// ci.yml round, then the release workflows; stops at the first red. Runs under the train-CI hold. It does not land.
    Release(ReleaseArgs),
    /// Fast-forward `main` to the train head (never a force or a rebase) and verify it with `ls-remote`.
    Land(LandArgs),
    /// After a train landed: the bounded, never-blocking disk clean of the landing step (`train land` runs it itself).
    Landed(crate::disk::landing::LandedArgs),
    /// Whether this process holds the train-CI reservation (the proof `train release` makes): exit 0 when it does. Run it as
    /// `scripts/train-ci-hold.sh <label> -- ./dev train hold-check`.
    HoldCheck,
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
    /// A flaky test the full run may be red by, as `apps/<app>/app/<spec>:<line>[:<col>]` (repeat): the same ER-12 check as
    /// `train release`. Without any, only a green full run (by `ci verdict`) lands.
    #[arg(long = "accept-flaky", value_name = "LOCATION")]
    accept_flaky: Vec<String>,
}

/// Runs a `train` command.
pub fn command(c: TrainCommand) -> Result<Report, CliError> {
    match c {
        TrainCommand::Release(a) => release(&a),
        TrainCommand::Land(a) => land_cmd(&a),
        TrainCommand::Landed(a) => run_landed(&a),
        TrainCommand::HoldCheck => hold_check(),
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
    let local = git.head().map_err(CliError::failure)?;
    let remote = git.remote_branch(&a.branch).map_err(CliError::failure)?;
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
    // ER-12, enforced: an accepted flaky test is one this train did not touch (nor the source it drives).
    let mut changed = Vec::new();
    if !plan.accepted.is_empty() {
        git.fetch().map_err(CliError::failure)?;
        changed = git.changed("origin/main...HEAD").map_err(CliError::failure)?;
        refuse_touched(&plan.accepted, &changed, &root, "train release")?;
    }
    // The hold: run only while holding the train-CI reservation, proven; else run again under it (a hard deadline around the child).
    let mut hold = hold::ShellHold {
        repo: root.clone(),
        args: std::env::args().skip(1).collect(),
        max: plan.max,
    };
    let reexecuted = std::env::var_os(hold::REEXEC_VAR).is_some();
    match hold::enter(&mut hold, reexecuted, &mut std::io::stderr()) {
        hold::Enter::Run => {}
        // The child printed its own report or its one `::error::`; its exit code is ours.
        hold::Enter::Exit(code) => std::process::exit(code),
        hold::Enter::Refuse(line) => return Err(CliError::new(Exit::Findings, line)),
    }
    let mut ports = real::RealPorts {
        root,
        branch: a.branch.clone(),
        sha: plan.sha.clone(),
        server: a.server.clone(),
        repo: a.repo.clone(),
        token,
        clock: crate::wait::SystemClock::new(),
        accepted: plan.accepted.clone(),
        changed,
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
                    sequence::Source::Verdict(r) => format!("{wf}: full green run {r} by `ci verdict`"),
                    sequence::Source::Landable(r) => {
                        format!("{wf}: full run {r}, red only by accepted flaky tests and failures already on main")
                    }
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

/// ER-12: refuses an `--accept-flaky` the train touches (or whose feature cannot be resolved: fail closed).
fn refuse_touched(accepted: &[verdict::Accepted], changed: &[String], root: &std::path::Path, who: &str) -> Result<(), CliError> {
    let tree = |a: &verdict::Accepted| real::tree_of(root, a);
    match accept::touched(accepted, changed, &tree) {
        Ok(t) if t.is_empty() => Ok(()),
        Ok(t) => Err(CliError::new(
            Exit::Findings,
            format!(
                "::error::{who}: --accept-flaky is refused, the train touches what it accepts: {}. Next step: fix the test in its owner's branch (a flaky test the change touches blocks the landing, ER-12), or drop that --accept-flaky.",
                t.join("; ")
            ),
        )),
        Err(why) => Err(CliError::new(
            Exit::Findings,
            format!("::error::{who}: --accept-flaky is refused: {why}."),
        )),
    }
}

/// `train hold-check`: the proof of the hold, as a command (exit 0 when this process holds the train-CI reservation).
fn hold_check() -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(None)?;
    match hold::proof(&root, &hold::HoldEnv::ambient()) {
        Ok(()) => Ok(Report::new(json!({"held": true}), "This process holds the train-CI reservation.")),
        Err(why) => Err(CliError::new(
            Exit::Findings,
            format!(
                "::error::train hold-check: the train-CI hold is not proven ({why}). Next step: run it as `scripts/train-ci-hold.sh <label> -- ./dev train hold-check`."
            ),
        )),
    }
}

/// The disk clean after a landing (`disk::landing`): it never blocks, an error or the limit is a warning.
fn run_landed(a: &crate::disk::landing::LandedArgs) -> Result<Report, CliError> {
    crate::disk::landing::landed(a, &|k| std::env::var(k).ok(), &mut |l| eprintln!("{l}"), &|| {
        crate::worktree::procs::process_folders().ok()
    })
}

/// The longest the landing clean may take beyond its own `--max`: a hung phase (a process listing, a git read) must not hold a
/// landing that is already verified. It is a hard deadline around the whole clean, not a tuned limit.
const CLEAN_GRACE: std::time::Duration = std::time::Duration::from_secs(60);

fn land_cmd(a: &LandArgs) -> Result<Report, CliError> {
    land::check_branch(&a.branch).map_err(|e| CliError::usage(e).with_hint("Run `./dev train land --help`."))?;
    let accepted = a
        .accept_flaky
        .iter()
        .map(|l| verdict::Accepted::parse(l))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CliError::usage(e).with_hint("Run `./dev train land --help`."))?;
    let token = crate::ci_runs::token()
        .map_err(|e| e.with_hint("The landing asks the shared verdict reader, which needs a token that can read Actions."))?;
    let root = crate::gates::command::repo_root(None)?;
    let mut git = real::GitCli { root: &root };
    let mut changed = Vec::new();
    if !accepted.is_empty() {
        git.fetch().map_err(CliError::failure)?;
        changed = git.changed("origin/main...HEAD").map_err(CliError::failure)?;
        refuse_touched(&accepted, &changed, &root, "train land")?;
    }
    let mut ci = real::RealCi {
        root: root.clone(),
        server: SERVER.to_string(),
        repo: "decryp7/paddock".to_string(),
        token,
        accepted,
        changed,
    };
    let landed = land::land(&a.branch, &mut git, &mut ci, &mut std::io::stderr())
        .map_err(|stop| CliError::new(Exit::Findings, stop.annotation().replace("train release", "train land")))?;
    // The landing is verified; the disk clean that follows can only warn (it never blocks or fails the landing).
    let clean_text = landing_clean(&a.branch);
    Ok(match landed {
        land::Landed::FastForwarded { from, to, notes } => Report::new(
            json!({"landed": true, "from": from, "to": to, "notes": notes}),
            format!(
                "Landed: main is now {} (was {}); verified with ls-remote.\n{}\n{clean_text}",
                &to[..9],
                &from[..9],
                notes.join("\n")
            ),
        ),
        land::Landed::AlreadyOnMain(sha) => Report::new(
            json!({"landed": false, "already": true, "main": sha}),
            format!("main already is {}; nothing was pushed.\n{clean_text}", &sha[..9]),
        ),
    })
}

/// The clean of `train landed`, in a thread with a hard deadline (its own `--max` plus [`CLEAN_GRACE`]); the text for the report.
fn landing_clean(train: &str) -> String {
    let args = crate::disk::landing::LandedArgs {
        train: train.to_string(),
        common: Default::default(),
        max: crate::disk::landing::DEFAULT_MAX.to_string(),
    };
    let own = parse_duration(&args.max).unwrap_or(std::time::Duration::from_secs(600));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // silent: the receiver is gone once the deadline passed, and the landing no longer waits for this
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = tx.send(run_landed(&args).map(|r| r.text).map_err(|e| e.message));
    });
    match rx.recv_timeout(own + CLEAN_GRACE) {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => format!("::warning::train land: the disk clean could not run: {e}"),
        Err(_) => format!(
            "::warning::train land: the disk clean did not finish within {} (its limit plus a grace); the landing stands. Next step: run `./dev train landed {train}` later.",
            fmt_duration(own + CLEAN_GRACE)
        ),
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
            "accept=apps/debrief/app/e2e/a11y.e2e.ts:6:7",
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
