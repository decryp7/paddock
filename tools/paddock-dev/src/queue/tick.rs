//! One tick of the merge queue: read the ledger, take one step, stop. The state is the pull requests' labels and the train
//! branch (its marker commit), never a local file, so a tick can end anywhere and the next one carries on (idempotent).
//!
//! The step, by what the ledger shows:
//!
//! - a `queue/hold` label anywhere: nothing ([`Tick::Held`], a non-zero exit); a hold never outlives its closed pull request;
//! - a pull request is taken only with an official approving review of its CURRENT head (a label is not an approval), and never
//!   while it carries an `owes/*` label (the Windows leg and the rule 54 build are not built yet); anything else goes back to
//!   review;
//! - no `state/in-train` pull request: take up to [`MAX_BATCH`] approved ones, merge them into the train branch (git and the
//!   merge check only: **the tick runs no code of any pull request**, the local gates are the first jobs of the dispatched
//!   workflow), drop a conflict or a merge-check finding, push the train branch, label the rest `state/in-train`;
//! - a train whose branch is stale or does not match the labels (main moved, a head moved, a drop was interrupted): rebuild it;
//! - a train with no run of a reached workflow on its head: dispatch it (`ci.yml`, and each app workflow the plan reaches; a rebuild
//!   after a drop reruns only what failed, `since_run`), unless the backup window or a busy runner says wait;
//! - a run going: wait;
//! - a failed run: triage it, drop every pull request whose affected projects hold a failure that is new (Q3: all suspects, no
//!   bisection) with a comment naming the failed tests, and rebuild the rest; a run whose failures are all main's own marks the
//!   train `queue/blocked` and is not triaged again until main moves or another pull request is approved;
//! - a green run that is not a full one: dispatch the full run (ER-8); main is fast-forwarded only when EVERY reached workflow has
//!   a full green run per `ci verdict`, outside the backup window, with no hold. A verdict that is not measured never lands.

use super::blame;
use super::ports::{Blame, Built, Ci, Failure, Ledger, Marker, RunState, Sent, TRAIN_BRANCH, Train, TrainMember, label};
use crate::ci_runs::verdict::Verdict;
use crate::pr::gitea::Pull;

/// Pull requests per train (Q2).
pub const MAX_BATCH: usize = 6;
/// Runs of `ci.yml` on one train head before the queue stops asking for more (the affected run, the full run, one spare).
pub const MAX_RUNS_PER_HEAD: usize = 3;
/// The Windows backup window, Asia/Singapore hours: no dispatch and no landing (a fast-forward starts the push-to-main runs)
/// from 00:00 up to 04:00 (docs/ci-schedules.md).
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
        /// The workflow.
        workflow: String,
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
            Tick::Dispatched { head, workflow, inputs } => format!(
                "queue: dispatched {workflow} on {} ({})",
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
    /// The Asia/Singapore hour (0-23) for the backup window; `None` when it could not be read: nothing is dispatched or landed.
    pub hour: Option<u32>,
}

/// The three ports of a tick.
pub struct Ports<'a> {
    /// Gitea.
    pub ledger: &'a mut dyn Ledger,
    /// The working copy.
    pub train: &'a mut dyn Train,
    /// CI.
    pub ci: &'a mut dyn Ci,
}

fn has(p: &Pull, l: &str) -> bool {
    p.labels.iter().any(|x| x == l)
}

fn owes(p: &Pull) -> Option<&String> {
    p.labels.iter().find(|l| l.starts_with("owes/"))
}

fn numbers(prs: &[&Pull]) -> Vec<u64> {
    let mut v: Vec<u64> = prs.iter().map(|p| p.number).collect();
    v.sort_unstable();
    v
}

/// The window or the unread hour, when either forbids a dispatch or a landing.
fn window_guard(env: &Env) -> Option<Tick> {
    match env.hour {
        None => Some(Tick::Stuck {
            why: "the hour (Asia/Singapore) could not be read, so the backup window is not measured: nothing is dispatched or landed. Next step: the system clock could not be read, the next tick asks again".into(),
            measured: false,
        }),
        Some(h) if WINDOW.contains(&h) => Some(Tick::Window),
        Some(_) => None,
    }
}

/// Sends a pull request back to review with the reason; the author approves it again when it is ready.
fn send_back(p: &mut Ports, number: u64, why: &str) -> Result<(), String> {
    p.ledger.relabel(number, &["+state/built"])?;
    p.ledger.comment(
        number,
        &format!("queue: not taken, {why} Approve it again (a review on the current head) when it is ready."),
    )
}

/// Whether the queue may take this pull request now; when not, it goes back to review with a comment.
fn admit(p: &mut Ports, pr: &Pull) -> Result<bool, String> {
    if let Some(debt) = owes(pr) {
        send_back(
            p,
            pr.number,
            &format!(
                "it carries `{debt}`: the queue has no Windows leg and no rule 54 build yet, so a pull request that owes either cannot land through it."
            ),
        )?;
        return Ok(false);
    }
    if !p.ledger.approved_head(pr)? {
        send_back(
            p,
            pr.number,
            &format!(
                "no approving review exists for its current head {}; a label alone is not an approval, and an approval does not follow a new commit.",
                short(&pr.head)
            ),
        )?;
        return Ok(false);
    }
    Ok(true)
}

/// One tick. An `Err` is a failure to read or write (the network, git); a state the queue cannot leave is [`Tick::Stuck`].
pub fn tick(p: &mut Ports, env: &Env) -> Result<Tick, String> {
    // A hold does not outlive its pull request: closed, it stops nobody and says so.
    for n in p.ledger.closed_held()? {
        p.ledger.relabel(n, &["-queue/hold"])?;
        p.ledger.comment(
            n,
            "queue: this pull request is closed, so its `queue/hold` is released; the queue is not held by it any more.",
        )?;
    }
    let pulls = p.ledger.open_pulls()?;
    if let Some(h) = pulls.iter().find(|x| has(x, label::HOLD)) {
        return Ok(Tick::Held(h.number));
    }
    let mut labelled: Vec<&Pull> = pulls.iter().filter(|x| x.base == "main" && has(x, label::IN_TRAIN)).collect();
    labelled.sort_by_key(|x| x.number);
    // A hand-set state/in-train is no approval: every pull request in the train is checked like one that is taken.
    let mut in_train: Vec<&Pull> = Vec::new();
    for pr in labelled {
        if admit(p, pr)? {
            in_train.push(pr);
        }
    }
    if in_train.is_empty() {
        let mut approved: Vec<&Pull> = pulls.iter().filter(|x| x.base == "main" && has(x, label::APPROVED)).collect();
        approved.sort_by_key(|x| x.number);
        let mut taken: Vec<&Pull> = Vec::new();
        for pr in approved {
            if taken.len() == MAX_BATCH {
                break;
            }
            if admit(p, pr)? {
                taken.push(pr);
            }
        }
        if taken.is_empty() {
            return Ok(Tick::Idle("no pull request is approved for its current head, or in a train".into()));
        }
        let mut branches = vec!["main".to_string(), TRAIN_BRANCH.to_string()];
        branches.extend(taken.iter().map(|x| x.branch.clone()));
        p.train.fetch(&branches)?;
        let old = p.train.train_head()?;
        let prs: Vec<Pull> = taken.into_iter().cloned().collect();
        return build_round(p, prs, 1, None, None, old);
    }
    if in_train.iter().any(|x| has(x, label::BLOCKED)) {
        // A blocked train (every failure was main's) waits for main to move; another approved pull request may be the fix, so it
        // releases the train and everything is built again together.
        let mut newcomers = false;
        for pr in pulls.iter().filter(|x| x.base == "main" && has(x, label::APPROVED)) {
            newcomers |= admit(p, pr)?;
        }
        if newcomers {
            for pr in &in_train {
                p.ledger.relabel(pr.number, &["+state/approved", "-queue/blocked"])?;
                p.ledger.comment(pr.number, "queue: the blocked train is released because another pull request was approved (it may fix main); everything is built together again.")?;
            }
            return Ok(Tick::Idle(
                "the blocked train was released for a newly approved pull request".into(),
            ));
        }
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
        // Pushed to main, then the tick ended before the labels: finish the landing, when the head is the train the queue built
        // (a forged marker on an old commit must not label pull requests landed).
        if p.train.verify_train(None, h, &members(in_train), m)?.is_none() {
            return finish_landing(p, h, in_train);
        }
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
    // A branch anyone with write access can push to is not a train until its structure says so: one --no-ff merge per pull request
    // at its approved head, each merge exactly what merging its parents gives, then the marker commit on an unchanged tree.
    let mut valid = valid;
    if valid && let (Some(h), Some(m)) = (&head, &marker) {
        valid = p.train.verify_train(Some(&main), h, &members(in_train), m)?.is_none();
    }
    // A head or marker the queue did not write steers nothing: the rebuild uses only the labels and the ledger.
    if !valid {
        return rebuild_stale(p, in_train, head, &main);
    }
    if in_train.iter().any(|x| has(x, label::BLOCKED)) {
        return Ok(Tick::Stuck {
            why: "the train is blocked: every failure of its run also fails on main, and main has not moved. Not triaged again until main moves or another pull request is approved. Next step: fix main".into(),
            measured: true,
        });
    }
    let (head, marker) = (
        head.unwrap_or_default(),
        marker.unwrap_or(Marker {
            prs: want,
            round: 1,
            since_run: None,
            since_workflow: None,
        }),
    );
    judge(p, env, in_train, &head, &marker, &main)
}

/// The train does not match the labels, or is not one the queue built: rebuild it from the labelled pull requests alone (a fresh
/// round, no `since_run`): nothing a head or a marker says on the train branch steers the rebuild. A pull request whose head moved
/// is already back in review, because its new head has no approval.
fn rebuild_stale(p: &mut Ports, in_train: &[&Pull], old: Option<String>, main: &str) -> Result<Tick, String> {
    let _ = main;
    let prs: Vec<Pull> = in_train.iter().map(|x| (*x).clone()).collect();
    if prs.is_empty() {
        return Ok(Tick::Idle("the train was stale and no pull request is left in it".into()));
    }
    build_round(p, prs, 1, None, None, old)
}

fn members(in_train: &[&Pull]) -> Vec<TrainMember> {
    in_train
        .iter()
        .map(|x| TrainMember {
            number: x.number,
            branch: x.branch.clone(),
            head: x.head.clone(),
        })
        .collect()
}

/// Merges the batch into a fresh train and pushes it. No code of any pull request runs: git and the merge check only.
fn build_round(
    p: &mut Ports,
    prs: Vec<Pull>,
    round: u32,
    since_run: Option<u64>,
    since_wf: Option<String>,
    old: Option<String>,
) -> Result<Tick, String> {
    let main = p.train.main_head()?;
    let marker = Marker {
        prs: Vec::new(),
        round,
        since_run,
        since_workflow: since_wf.filter(|_| since_run.is_some()),
    };
    let Built { head, merged, dropped } = p.train.build(&main, &prs, &marker)?;
    for (n, why) in &dropped {
        drop_pr(p, *n, &format!("it could not join the train: {why}"))?;
    }
    if merged.is_empty() {
        return Ok(Tick::Dropped {
            dropped: dropped.iter().map(|(n, _)| *n).collect(),
            rebuilt: None,
        });
    }
    p.train.push_train(&head, old.as_deref())?;
    for n in &merged {
        p.ledger.relabel(*n, &["+state/in-train", "-queue/blocked"])?;
    }
    Ok(Tick::Built {
        head,
        prs: merged,
        dropped,
    })
}

fn drop_pr(p: &mut Ports, number: u64, why: &str) -> Result<(), String> {
    p.ledger.relabel(number, &["+state/fixes"])?;
    p.ledger.comment(
        number,
        &format!("queue: dropped from the train, {why}\n\nFix it and approve the branch again (a review on the new head) to queue it."),
    )
}

/// The inputs of a dispatch of `workflow`: the affected run against main, or the full run (ER-8); a rerun of failures adds
/// `since_run` when the failed run was of this workflow. ci.yml takes `base`; an app workflow plans against main itself and takes
/// `bundles` for its full run.
pub fn inputs_for(workflow: &str, full: bool, since: Option<u64>, main: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    if workflow == "ci.yml" {
        if !full {
            // A commit id, never a ref name: the commit lint and the plan resolve `base` as one.
            v.push(("base".into(), main.to_string()));
            v.extend(since.map(|r| ("since_run".to_string(), r.to_string())));
        }
    } else if full {
        v.push(("bundles".into(), "true".into()));
    } else {
        v.extend(since.map(|r| ("since_run".to_string(), r.to_string())));
    }
    v
}

fn dispatch(p: &mut Ports, env: &Env, head: &str, workflow: &str, inputs: Vec<(String, String)>) -> Result<Tick, String> {
    if let Some(t) = window_guard(env) {
        return Ok(t);
    }
    match p.ci.dispatch(workflow, &inputs)? {
        Sent::Yes => Ok(Tick::Dispatched {
            head: head.to_string(),
            workflow: workflow.to_string(),
            inputs,
        }),
        Sent::Busy(who) => Ok(Tick::RunnerBusy(who)),
    }
}

fn judge(p: &mut Ports, env: &Env, in_train: &[&Pull], head: &str, marker: &Marker, main: &str) -> Result<Tick, String> {
    for wf in p.train.reached_workflows(main, head)? {
        let runs = p.ci.runs(&wf, head)?;
        let since = marker.since_workflow.as_deref().filter(|w| *w == wf).and(marker.since_run);
        let Some(newest) = runs.first() else {
            return dispatch(p, env, head, &wf, inputs_for(&wf, false, since, main));
        };
        match &newest.state {
            RunState::Running => return Ok(Tick::Waiting(newest.id)),
            RunState::Success => match p.ci.verdict(&wf, head) {
                Verdict::Green { .. } => {}
                Verdict::NotMeasured(why) => {
                    return Ok(Tick::Stuck {
                        why: format!(
                            "the verdict of {wf} on {} is not measured ({why}); nothing lands on a verdict that was not measured. Next step: fix what is named, the next tick asks again",
                            short(head)
                        ),
                        measured: false,
                    });
                }
                Verdict::Red { reason, .. } => {
                    if runs.len() >= MAX_RUNS_PER_HEAD {
                        return Ok(Tick::Stuck {
                            why: format!(
                                "{} runs of {wf} on {} and still no full green run ({reason}). Next step: a person decides",
                                runs.len(),
                                short(head)
                            ),
                            measured: true,
                        });
                    }
                    return dispatch(p, env, head, &wf, inputs_for(&wf, true, None, main));
                }
            },
            RunState::Failed(how) => {
                let (id, how) = (newest.id, how.clone());
                return failed_run(p, in_train, head, marker, main, &wf, id, &how);
            }
        }
    }
    // Every reached workflow is full green. A fast-forward starts the push-to-main runs, so the backup window holds it back too.
    if let Some(t) = window_guard(env) {
        return Ok(t);
    }
    // Labels are read again: a hold set while the runs went must stop the landing.
    let fresh = p.ledger.open_pulls()?;
    if let Some(h) = fresh.iter().find(|x| has(x, label::HOLD)) {
        return Ok(Tick::Held(h.number));
    }
    p.train.land(head)?;
    finish_landing(p, head, in_train)
}

#[allow(clippy::too_many_arguments)]
fn failed_run(
    p: &mut Ports,
    in_train: &[&Pull],
    head: &str,
    marker: &Marker,
    main: &str,
    workflow: &str,
    run: u64,
    how: &str,
) -> Result<Tick, String> {
    if how == "cancelled" {
        return Ok(Tick::Stuck {
            why: format!(
                "run {run} of {workflow} on {} was cancelled; the queue does not guess. Next step: dispatch it again or cancel the train (remove state/in-train)",
                short(head)
            ),
            measured: true,
        });
    }
    let failures: Vec<Failure> = p.ci.failures(run).map_err(|e| format!("run {run} could not be triaged: {e}"))?;
    if failures.is_empty() || failures.iter().any(|f| f.blame == Blame::Unknown) {
        return Ok(Tick::Stuck {
            why: format!(
                "run {run} of {workflow} failed and its failures could not all be named ({} rows); the queue blames nobody it cannot prove. Next step: `./dev ci triage {run}`",
                failures.len()
            ),
            measured: false,
        });
    }
    let blamed: Vec<&Failure> = failures.iter().filter(|f| f.blame != Blame::OnMain).collect();
    if blamed.is_empty() {
        for pr in in_train {
            p.ledger.relabel(pr.number, &["+queue/blocked"])?;
        }
        if let Some(first) = in_train.first() {
            p.ledger.comment(first.number, &format!("queue: every failure of run {run} of {workflow} also fails on main, so the train is blocked (`queue/blocked`) and is not triaged again until main moves or another pull request is approved."))?;
        }
        return Ok(Tick::Stuck {
            why: format!(
                "every failure of run {run} also fails on main; no pull request is to blame. Next step: fix main, then the train reruns"
            ),
            measured: true,
        });
    }
    // The rule is `blame::suspects`, shared with `./dev ci triage --members`: a pull request whose plan runs everything (`*`)
    // cannot be told apart from the others, so it is a suspect only for lack of a precise one, and then the queue stops.
    let blamed_refs = &blamed;
    let mut reaches = Vec::new();
    for pr in in_train {
        // A gate job (train gates, commit messages) fails on files, not on a project: its log's paths map it to the pull requests
        // that change those files.
        reaches.push(blame::Reach {
            projects: p.train.affected(main, &pr.head)?,
            files: if blame::needs_files(blamed_refs) {
                p.train.pr_files(main, &pr.head)?
            } else {
                Vec::new()
            },
        });
    }
    let found = blame::suspects(&reaches, blamed_refs);
    let precise: Vec<(&Pull, Vec<&Failure>)> = found.precise.into_iter().map(|(i, hit)| (in_train[i], hit)).collect();
    let wild: Vec<&Pull> = found.wild.into_iter().map(|i| in_train[i]).collect();
    if precise.is_empty() {
        let why = if wild.is_empty() {
            format!(
                "run {run} failed in {} but no pull request of the train reaches those projects. Next step: `./dev ci triage {run}`",
                names(&blamed)
            )
        } else {
            format!(
                "run {run} failed in {} and only pull requests whose plan runs everything (`*`) could be the cause; the queue cannot tell them apart. Next step: `./dev ci triage {run}`",
                names(&blamed)
            )
        };
        return Ok(Tick::Stuck { why, measured: true });
    }
    for (pr, hit) in &precise {
        let tests = hit.iter().map(|f| format!("- {}", f.what)).collect::<Vec<_>>().join("\n");
        let also = if precise.len() > 1 {
            format!(
                "\n\n{} pull requests reach these projects; all of them are dropped and the rest retried (no bisection).",
                precise.len()
            )
        } else {
            String::new()
        };
        drop_pr(
            p,
            pr.number,
            &format!(
                "run {run} of {workflow} on train {} failed in projects this branch reaches:\n{tests}{also}",
                short(head)
            ),
        )?;
    }
    let gone: Vec<u64> = precise.iter().map(|(pr, _)| pr.number).collect();
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
    match build_round(
        p,
        rest,
        marker.round + 1,
        Some(run),
        Some(workflow.to_string()),
        Some(head.to_string()),
    )? {
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

/// Labels what landed. A pull request whose head moved after the run (its fresh head is not in the landed commit) is not marked
/// landed: its new commits did not land, so it goes back to review.
fn finish_landing(p: &mut Ports, head: &str, in_train: &[&Pull]) -> Result<Tick, String> {
    let fresh = p.ledger.open_pulls()?;
    let mut prs = Vec::new();
    for pr in in_train {
        let now = fresh
            .iter()
            .find(|x| x.number == pr.number)
            .map_or(pr.head.as_str(), |x| x.head.as_str());
        if p.train.contains(now, head)? {
            p.ledger.relabel(pr.number, &["+state/landed"])?;
            p.ledger.comment(
                pr.number,
                &format!("queue: landed on main as {} (fast-forward of the tested train).", short(head)),
            )?;
            prs.push(pr.number);
        } else {
            p.ledger.relabel(pr.number, &["+state/built"])?;
            p.ledger.comment(pr.number, &format!("queue: the tested commit of this branch landed on main as {}, but the branch has new commits since, which did not. It is not marked landed; approve its new head to queue it again.", short(head)))?;
        }
    }
    Ok(Tick::Landed {
        head: head.to_string(),
        prs,
    })
}
