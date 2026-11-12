//! One tick of the merge queue: read the ledger, take one step, stop. The state is the pull requests' labels and the train
//! branch (its marker commit), never a local file, so a tick can end anywhere and the next one carries on (idempotent).
//!
//! The step, by what the ledger shows:
//!
//! - a `queue/hold` label anywhere: nothing ([`Tick::Held`]);
//! - no `state/in-train` pull request: take up to [`MAX_BATCH`] `state/approved` ones, build the train, run the local gates,
//!   drop a conflict, a merge-check finding or a gate culprit, push the train branch, label the rest `state/in-train`;
//! - a train whose branch is stale or does not match the labels (main moved, a head moved, a drop was interrupted): rebuild it;
//! - a train with no run on its head: dispatch `ci.yml` (the affected plan against main; a rebuild after a drop reruns only
//!   what failed, `since_run`), unless the backup window or a busy runner says wait;
//! - a run going: wait;
//! - a failed run: triage it, drop every pull request whose affected projects hold a failure that is new (Q3: all suspects, no
//!   bisection) with a comment naming the failed tests, and rebuild the rest;
//! - a green run that is not a full one: dispatch the full run (ER-8); a full green run per `ci verdict`: fast-forward main and
//!   label the pull requests `state/landed`. A verdict that is not measured never lands.

use super::ports::{Blame, Built, Ci, CiRun, Failure, GateOutcome, Gates, Ledger, Marker, RunState, Sent, TRAIN_BRANCH, Train, label};
use crate::ci_runs::verdict::Verdict;
use crate::pr::gitea::Pull;
use std::collections::BTreeSet;

/// Pull requests per train (Q2).
pub const MAX_BATCH: usize = 6;
/// Runs of `ci.yml` on one train head before the queue stops asking for more (the affected run, the full run, one spare).
pub const MAX_RUNS_PER_HEAD: usize = 3;
/// The Windows backup window, local hours: no dispatch from 00:00 up to 04:00 (docs/ci-schedules.md).
pub const WINDOW: std::ops::Range<u32> = 0..4;

/// What a tick did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tick {
    /// Nothing to do, and why.
    Idle(String),
    /// A `queue/hold` label is on this pull request.
    Held(u64),
    /// The backup window: nothing is dispatched.
    Window,
    /// Another run holds a shared runner; nothing was dispatched.
    RunnerBusy(String),
    /// The train was built and pushed.
    Built {
        /// The train head.
        head: String,
        /// The pull requests in it.
        prs: Vec<u64>,
        /// Dropped on the way, with the reason.
        dropped: Vec<(u64, String)>,
    },
    /// A run was dispatched on the train head.
    Dispatched {
        /// The head.
        head: String,
        /// The inputs.
        inputs: Vec<(String, String)>,
    },
    /// A run is going.
    Waiting(u64),
    /// Suspects were dropped; `rebuilt` is false when none was left.
    Dropped {
        /// The suspects.
        dropped: Vec<u64>,
        /// The new train head, when pull requests remained.
        rebuilt: Option<String>,
    },
    /// main is the train head now.
    Landed {
        /// The new head of main.
        head: String,
        /// The pull requests landed.
        prs: Vec<u64>,
    },
    /// The queue cannot go on without a person (a failure that is not the train's, a gate that did not look, a verdict that was
    /// not measured). `measured` is false for those that must never read as a pass (ER-33).
    Stuck {
        /// Why, with the next step.
        why: String,
        /// Whether something could not be measured.
        measured: bool,
    },
}

impl Tick {
    /// One line for the log and the report.
    pub fn text(&self) -> String {
        let nums = |v: &[u64]| v.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ");
        match self {
            Tick::Idle(why) => format!("queue: idle: {why}"),
            Tick::Held(n) => format!("queue: on hold (queue/hold on #{n}); nothing done"),
            Tick::Window => "queue: the backup window (00:00-04:00 local): nothing dispatched".into(),
            Tick::RunnerBusy(who) => format!("queue: a shared runner is busy ({who}); nothing dispatched, the next tick looks again"),
            Tick::Built { head, prs, dropped } => format!(
                "queue: train {} built with {}{}",
                short(head),
                nums(prs),
                if dropped.is_empty() {
                    String::new()
                } else {
                    format!(
                        "; dropped {}",
                        dropped.iter().map(|(n, _)| format!("#{n}")).collect::<Vec<_>>().join(", ")
                    )
                }
            ),
            Tick::Dispatched { head, inputs } => format!(
                "queue: dispatched ci.yml on {} ({})",
                short(head),
                if inputs.is_empty() {
                    "full run".to_string()
                } else {
                    inputs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ")
                }
            ),
            Tick::Waiting(run) => format!("queue: run {run} is going; waiting"),
            Tick::Dropped { dropped, rebuilt } => format!(
                "queue: dropped {}{}",
                nums(dropped),
                rebuilt
                    .as_ref()
                    .map_or("; no pull request is left".into(), |h| format!("; train rebuilt as {}", short(h)))
            ),
            Tick::Landed { head, prs } => format!("queue: landed {} on main as {}", nums(prs), short(head)),
            Tick::Stuck { why, .. } => format!("::error::queue: {why}"),
        }
    }
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// What the tick knows about its surroundings.
pub struct Env {
    /// The local hour (0-23), for the backup window.
    pub hour: u32,
}

/// The four ports of a tick.
pub struct Ports<'a> {
    /// Gitea.
    pub ledger: &'a mut dyn Ledger,
    /// The working copy.
    pub train: &'a mut dyn Train,
    /// The local gates.
    pub gates: &'a mut dyn Gates,
    /// CI.
    pub ci: &'a mut dyn Ci,
}

fn has(p: &Pull, l: &str) -> bool {
    p.labels.iter().any(|x| x == l)
}

fn numbers(prs: &[&Pull]) -> Vec<u64> {
    let mut v: Vec<u64> = prs.iter().map(|p| p.number).collect();
    v.sort_unstable();
    v
}

/// One tick. An `Err` is a failure to read or write (the network, git); a state the queue cannot leave is [`Tick::Stuck`].
pub fn tick(p: &mut Ports, env: &Env) -> Result<Tick, String> {
    let pulls = p.ledger.open_pulls()?;
    if let Some(h) = pulls.iter().find(|x| has(x, label::HOLD)) {
        return Ok(Tick::Held(h.number));
    }
    let mut in_train: Vec<&Pull> = pulls.iter().filter(|x| x.base == "main" && has(x, label::IN_TRAIN)).collect();
    in_train.sort_by_key(|x| x.number);
    if in_train.is_empty() {
        let mut approved: Vec<&Pull> = pulls.iter().filter(|x| x.base == "main" && has(x, label::APPROVED)).collect();
        approved.sort_by_key(|x| x.number);
        approved.truncate(MAX_BATCH);
        if approved.is_empty() {
            return Ok(Tick::Idle("no pull request is approved or in a train".into()));
        }
        let mut branches = vec!["main".to_string(), TRAIN_BRANCH.to_string()];
        branches.extend(approved.iter().map(|x| x.branch.clone()));
        p.train.fetch(&branches)?;
        let old = p.train.train_head()?;
        let prs: Vec<Pull> = approved.into_iter().cloned().collect();
        return build_round(p, prs, 1, None, old);
    }
    let mut branches = vec!["main".to_string(), TRAIN_BRANCH.to_string()];
    branches.extend(in_train.iter().map(|x| x.branch.clone()));
    p.train.fetch(&branches)?;
    resume(p, env, &in_train)
}

fn resume(p: &mut Ports, env: &Env, in_train: &[&Pull]) -> Result<Tick, String> {
    let main = p.train.main_head()?;
    let want = numbers(in_train);
    let head = p.train.train_head()?;
    let marker = match &head {
        Some(h) => p.train.marker(h)?,
        None => None,
    };
    if let (Some(h), Some(m)) = (&head, &marker)
        && m.prs == want
        && p.train.contains(h, &main)?
    {
        // Pushed to main, then the tick ended before the labels: finish the landing.
        return finish_landing(p, h, in_train);
    }
    let valid = match (&head, &marker) {
        (Some(h), Some(m)) if m.prs == want && p.train.contains(&main, h)? => {
            let mut all = true;
            for pr in in_train {
                all &= p.train.contains(&pr.head, h)?;
            }
            all
        }
        _ => false,
    };
    if !valid {
        return rebuild_stale(p, in_train, head, marker, &main);
    }
    let (head, marker) = (
        head.unwrap_or_default(),
        marker.unwrap_or(Marker {
            prs: want,
            round: 1,
            since_run: None,
        }),
    );
    judge(p, env, in_train, &head, &marker, &main)
}

/// The train does not match the labels: rebuild from the labelled pull requests. A pull request whose head moved after it
/// entered the train goes back to review; when the stale train held more pull requests than are labelled now (a drop was
/// interrupted), the rebuild reruns only what that train's failed run failed.
fn rebuild_stale(p: &mut Ports, in_train: &[&Pull], old: Option<String>, marker: Option<Marker>, main: &str) -> Result<Tick, String> {
    let mut prs: Vec<Pull> = Vec::new();
    for pr in in_train {
        let moved = match (&old, &marker) {
            (Some(h), Some(m)) if m.prs.contains(&pr.number) => !p.train.contains(&pr.head, h)?,
            _ => false,
        };
        if moved {
            p.ledger.relabel(pr.number, &["+state/built"])?;
            p.ledger.comment(
                pr.number,
                "queue: this branch's head changed after it entered the train, so it goes back to review (state/built). Approve it again when it is ready.",
            )?;
        } else {
            prs.push((*pr).clone());
        }
    }
    let (mut round, mut since) = (1, None);
    if let (Some(h), Some(m)) = (&old, &marker)
        && prs.iter().all(|pr| m.prs.contains(&pr.number))
        && prs.len() < m.prs.len()
        && p.train.contains(main, h)?
    {
        round = m.round + 1;
        since = newest_failed(p.ci.runs(h)?);
    }
    if prs.is_empty() {
        return Ok(Tick::Idle("the train was stale and no pull request is left in it".into()));
    }
    build_round(p, prs, round, since, old)
}

fn newest_failed(runs: Vec<CiRun>) -> Option<u64> {
    runs.into_iter().find(|r| matches!(r.state, RunState::Failed(_))).map(|r| r.id)
}

/// Builds the train, runs the gates, drops what they name and pushes; at most one rebuild per pull request.
fn build_round(p: &mut Ports, mut prs: Vec<Pull>, round: u32, since: Option<u64>, old: Option<String>) -> Result<Tick, String> {
    let mut dropped_all: Vec<(u64, String)> = Vec::new();
    for _ in 0..=prs.len() + 1 {
        let main = p.train.main_head()?;
        let marker = Marker {
            prs: Vec::new(),
            round,
            since_run: since,
        };
        let Built { head, merged, dropped } = p.train.build(&main, &prs, &marker)?;
        for (n, why) in &dropped {
            drop_pr(p, *n, &format!("it could not join the train: {why}"))?;
        }
        dropped_all.extend(dropped.iter().cloned());
        prs.retain(|x| !dropped.iter().any(|(n, _)| *n == x.number));
        if merged.is_empty() {
            return Ok(Tick::Dropped {
                dropped: dropped_all.iter().map(|(n, _)| *n).collect(),
                rebuilt: None,
            });
        }
        match p.gates.run(&main, &head) {
            GateOutcome::Green => {
                p.train.push_train(&head, old.as_deref())?;
                for n in &merged {
                    p.ledger.relabel(*n, &["+state/in-train"])?;
                }
                return Ok(Tick::Built {
                    head,
                    prs: merged,
                    dropped: dropped_all,
                });
            }
            GateOutcome::NotMeasured(why) => {
                return Ok(Tick::Stuck {
                    why: format!(
                        "a local gate did not look ({why}); a train that was not measured is not a train that passed. Next step: fix the gate, the next tick builds again"
                    ),
                    measured: false,
                });
            }
            GateOutcome::Red(fails) => {
                let suspects = gate_suspects(p, &main, &prs, &merged, &fails)?;
                let text = fails
                    .iter()
                    .map(|f| format!("- {}: {}", f.name, f.detail))
                    .collect::<Vec<_>>()
                    .join("\n");
                for n in &suspects {
                    drop_pr(p, *n, &format!("a local gate failed on the train before any CI run:\n{text}"))?;
                    dropped_all.push((*n, text.clone()));
                }
                prs.retain(|x| !suspects.contains(&x.number));
                if prs.is_empty() {
                    return Ok(Tick::Dropped {
                        dropped: dropped_all.iter().map(|(n, _)| *n).collect(),
                        rebuilt: None,
                    });
                }
            }
        }
    }
    Ok(Tick::Stuck {
        why: "the train kept failing its local gates; a person must look".into(),
        measured: true,
    })
}

/// The merged pull requests a gate failure belongs to: those that change a file it names; all of them when it names none.
fn gate_suspects(p: &mut Ports, main: &str, prs: &[Pull], merged: &[u64], fails: &[super::ports::GateFailure]) -> Result<Vec<u64>, String> {
    let named: BTreeSet<&str> = fails.iter().flat_map(|f| f.files.iter().map(String::as_str)).collect();
    if named.is_empty() {
        return Ok(merged.to_vec());
    }
    let mut out = Vec::new();
    for pr in prs.iter().filter(|x| merged.contains(&x.number)) {
        if p.train.pr_files(main, &pr.head)?.iter().any(|f| named.contains(f.as_str())) {
            out.push(pr.number);
        }
    }
    Ok(if out.is_empty() { merged.to_vec() } else { out })
}

fn drop_pr(p: &mut Ports, number: u64, why: &str) -> Result<(), String> {
    p.ledger.relabel(number, &["+state/fixes"])?;
    p.ledger.comment(
        number,
        &format!("queue: dropped from the train, {why}\n\nFix it and approve the branch again (state/approved) to queue it."),
    )
}

fn dispatch(p: &mut Ports, env: &Env, head: &str, inputs: Vec<(String, String)>) -> Result<Tick, String> {
    if WINDOW.contains(&env.hour) {
        return Ok(Tick::Window);
    }
    match p.ci.dispatch(&inputs)? {
        Sent::Yes => Ok(Tick::Dispatched {
            head: head.to_string(),
            inputs,
        }),
        Sent::Busy(who) => Ok(Tick::RunnerBusy(who)),
    }
}

fn judge(p: &mut Ports, env: &Env, in_train: &[&Pull], head: &str, marker: &Marker, main: &str) -> Result<Tick, String> {
    let runs = p.ci.runs(head)?;
    let Some(newest) = runs.first() else {
        let mut inputs = vec![("base".to_string(), "origin/main".to_string())];
        if let Some(since) = marker.since_run {
            inputs.push(("since_run".to_string(), since.to_string()));
        }
        return dispatch(p, env, head, inputs);
    };
    match &newest.state {
        RunState::Running => Ok(Tick::Waiting(newest.id)),
        RunState::Success => match p.ci.verdict(head) {
            Verdict::Green { .. } => {
                p.train.land(head)?;
                finish_landing(p, head, in_train)
            }
            Verdict::NotMeasured(why) => Ok(Tick::Stuck {
                why: format!(
                    "the verdict on {} is not measured ({why}); nothing lands on a verdict that was not measured. Next step: fix what is named, the next tick asks again",
                    short(head)
                ),
                measured: false,
            }),
            Verdict::Red { reason, .. } => {
                if runs.len() >= MAX_RUNS_PER_HEAD {
                    return Ok(Tick::Stuck {
                        why: format!(
                            "{} runs on {} and still no full green run ({reason}). Next step: a person decides",
                            runs.len(),
                            short(head)
                        ),
                        measured: true,
                    });
                }
                dispatch(p, env, head, Vec::new())
            }
        },
        RunState::Failed(how) => {
            let id = newest.id;
            let how = how.clone();
            failed_run(p, in_train, head, marker, main, id, &how)
        }
    }
}

fn failed_run(p: &mut Ports, in_train: &[&Pull], head: &str, marker: &Marker, main: &str, run: u64, how: &str) -> Result<Tick, String> {
    if how == "cancelled" {
        return Ok(Tick::Stuck {
            why: format!(
                "run {run} on {} was cancelled; the queue does not guess. Next step: dispatch it again or cancel the train (remove state/in-train)",
                short(head)
            ),
            measured: true,
        });
    }
    let failures: Vec<Failure> = p.ci.failures(run).map_err(|e| format!("run {run} could not be triaged: {e}"))?;
    if failures.is_empty() || failures.iter().any(|f| f.blame == Blame::Unknown) {
        return Ok(Tick::Stuck {
            why: format!(
                "run {run} failed and its failures could not all be named ({} rows); the queue blames nobody it cannot prove. Next step: `./dev ci triage {run}`",
                failures.len()
            ),
            measured: false,
        });
    }
    let blamed: Vec<&Failure> = failures.iter().filter(|f| f.blame != Blame::OnMain).collect();
    if blamed.is_empty() {
        return Ok(Tick::Stuck {
            why: format!(
                "every failure of run {run} also fails on main; no pull request is to blame. Next step: fix main, then the train reruns"
            ),
            measured: true,
        });
    }
    let mut suspects: Vec<(&Pull, Vec<&Failure>)> = Vec::new();
    for pr in in_train {
        let reach = p.train.affected(main, &pr.head)?;
        let hit: Vec<&Failure> = blamed
            .iter()
            .copied()
            .filter(|f| f.projects.iter().any(|x| reach.contains(x) || reach.contains("*")))
            .collect();
        if !hit.is_empty() {
            suspects.push((pr, hit));
        }
    }
    if suspects.is_empty() {
        return Ok(Tick::Stuck {
            why: format!(
                "run {run} failed in {} but no pull request of the train reaches those projects. Next step: `./dev ci triage {run}`",
                names(&blamed)
            ),
            measured: true,
        });
    }
    for (pr, hit) in &suspects {
        let tests = hit.iter().map(|f| format!("- {}", f.what)).collect::<Vec<_>>().join("\n");
        let also = if suspects.len() > 1 {
            format!(
                "\n\n{} pull requests reach these projects; all of them are dropped and the rest retried (no bisection).",
                suspects.len()
            )
        } else {
            String::new()
        };
        drop_pr(
            p,
            pr.number,
            &format!(
                "run {run} on train {} failed in projects this branch reaches:\n{tests}{also}",
                short(head)
            ),
        )?;
    }
    let gone: Vec<u64> = suspects.iter().map(|(pr, _)| pr.number).collect();
    let rest: Vec<Pull> = in_train
        .iter()
        .filter(|pr| !gone.contains(&pr.number))
        .map(|pr| (*pr).clone())
        .collect();
    if rest.is_empty() {
        return Ok(Tick::Dropped {
            dropped: gone,
            rebuilt: None,
        });
    }
    match build_round(p, rest, marker.round + 1, Some(run), Some(head.to_string()))? {
        Tick::Built { head, .. } => Ok(Tick::Dropped {
            dropped: gone,
            rebuilt: Some(head),
        }),
        other => Ok(other),
    }
}

fn names(f: &[&Failure]) -> String {
    f.iter().map(|x| x.what.clone()).collect::<Vec<_>>().join(", ")
}

fn finish_landing(p: &mut Ports, head: &str, in_train: &[&Pull]) -> Result<Tick, String> {
    let mut prs = Vec::new();
    for pr in in_train {
        p.ledger.relabel(pr.number, &["+state/landed"])?;
        p.ledger.comment(
            pr.number,
            &format!("queue: landed on main as {} (fast-forward of the tested train).", short(head)),
        )?;
        prs.push(pr.number);
    }
    Ok(Tick::Landed {
        head: head.to_string(),
        prs,
    })
}
