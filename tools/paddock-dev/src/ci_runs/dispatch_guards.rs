// ci-runs-reader: ci dispatch's guards read a train's full runs and failed-only rounds, and main's newest nightly and push run
//! The guards `./dev ci dispatch` runs before a train dispatch (user, 2026-11-24).
//!
//! - **failed-first** (ER-10): on a `train/*` ref, a full dispatch of a workflow that takes `since_run` is refused while the train's
//!   last full run of that workflow is red and no failed-only round is green on the branch's current head. The refusal prints the
//!   exact failed-only command (`--input since_run=<the newest red run>`). The coordinator's override is `--full-anyway "<reason>"`,
//!   which is logged.
//! - **green-main**: a train dispatch (a failed-only round of a train already in CI excepted) reads the newest finished nightly on main
//!   and the newest finished push run of ci.yml on main and sorts their failed jobs with [`active_apps::classify_nightly`] under the
//!   active set of each run's commit: a failure of the foundation or an active app refuses (the jobs and the next step are named); a
//!   failure of an inactive app is printed as that app's fix item and does not refuse. The override is `--red-main-anyway "<reason>"`
//!   (a train that carries the fix), logged.
//!
//! Both start report-only (ER-41): they print what they would refuse as a `::warning::` and the dispatch goes on, until their first
//! green use is recorded by run id in [`DISPATCH_GUARDS`] (`./dev ci guard-record`, which switches the guard to blocking in the same step).
//! Anything unreadable is "not measured", never a pass (ER-33): report-only prints it, blocking refuses it.
//!
//! A run's kind is read from its dispatch token ([`DispatchTag`], in the title through the workflow's `run-name`); a run without one
//! (dispatched before the token, or by another tool) is read from its plan job's mode line ([`read_marker`]): `full=true` is a full
//! run, anything else is not a full run and never counts as a failed-only round.

use super::client::{Client, RunInfo, TimedJob, encode_path};
use super::dispatch::{DispatchTag, ReadWorkflow, Request, RunKind, Target, is_train};
use crate::active_apps;
use crate::job_outcome::JobOutcome;
use crate::plan_since::read_marker;
use crate::registers::DISPATCH_GUARDS;
use crate::wait::Http;
use std::collections::{BTreeMap, BTreeSet};

/// A guard of `ci dispatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// A failed-only round before a full run (ER-10).
    FailedFirst,
    /// Main green in the foundation and the active apps before a train.
    GreenMain,
    /// The repository-wide audits (the member audits and the conformance line) green on the train's head before a dispatch (user,
    /// 2026-11-25): 4 of 8 red train rounds were audits that failed only on the merged train.
    TrainAudits,
}

impl Guard {
    /// Its name in [`DISPATCH_GUARDS`] and the log.
    pub fn name(self) -> &'static str {
        match self {
            Guard::FailedFirst => "failed-first",
            Guard::GreenMain => "green-main",
            Guard::TrainAudits => "train-audits",
        }
    }

    /// The guard of a name.
    pub fn parse(name: &str) -> Option<Guard> {
        [Guard::FailedFirst, Guard::GreenMain, Guard::TrainAudits]
            .into_iter()
            .find(|g| g.name() == name)
    }

    /// Its override flag.
    pub fn override_flag(self) -> &'static str {
        match self {
            Guard::FailedFirst => "--full-anyway",
            Guard::GreenMain => "--red-main-anyway",
            Guard::TrainAudits => "--audits-anyway",
        }
    }
}

/// Whether a guard refuses or only reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Report-only (ER-41): prints what it would refuse, the dispatch goes on. The text says why (no record yet, or an unreadable
    /// register).
    ReportOnly(String),
    /// Blocking since its first green use.
    Blocking {
        /// The run of that use.
        first_green: u64,
    },
}

/// The run id recorded for `guard` in the register text; `Err` for a text that is not the register's shape.
pub fn recorded(text: &str, guard: Guard) -> Result<Option<u64>, String> {
    let v: toml::Table = text.parse().map_err(|e| format!("{DISPATCH_GUARDS} is not TOML: {e}"))?;
    let table = v
        .get("first_green")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{DISPATCH_GUARDS} has no [first_green] table"))?;
    if let Some(k) = table.keys().find(|k| Guard::parse(k).is_none()) {
        return Err(format!(
            "{DISPATCH_GUARDS}: first_green.{k} is not a guard (failed-first, green-main, train-audits)"
        ));
    }
    match table.get(guard.name()) {
        None => Ok(None),
        Some(id) => id
            .as_integer()
            .filter(|n| *n > 0)
            .map(|n| Some(n as u64))
            .ok_or_else(|| format!("{DISPATCH_GUARDS}: first_green.{} is not a run id", guard.name())),
    }
}

/// The mode of `guard` from the register text (`None`: the file is missing or unreadable, with the reason): report-only unless a run
/// is recorded. An unreadable register never switches a guard on or off by accident: it stays report-only, with the reason.
pub fn mode_of(text: Result<&str, &str>, guard: Guard) -> Mode {
    // The audits are established tests the member gate already blocks on (user, 2026-11-17), not a new check: blocking from the start.
    if guard == Guard::TrainAudits {
        return Mode::Blocking { first_green: 0 };
    }
    match text.map_err(str::to_string).and_then(|t| recorded(t, guard)) {
        Ok(Some(first_green)) => Mode::Blocking { first_green },
        Ok(None) => Mode::ReportOnly(format!("no first green use is recorded in {DISPATCH_GUARDS}")),
        Err(e) => Mode::ReportOnly(format!("{DISPATCH_GUARDS} cannot be read ({e})")),
    }
}

/// The register text with `run` recorded for `guard`; refuses a guard already recorded (the first green use is recorded once).
pub fn with_first_green(text: &str, guard: Guard, run: u64) -> Result<String, String> {
    if let Some(id) = recorded(text, guard)? {
        return Err(format!(
            "{} is already recorded at run {id}: the first green use is recorded once",
            guard.name()
        ));
    }
    let mut out = text.trim_end().to_string();
    out.push_str(&format!("\n\"{}\" = {run}\n", guard.name()));
    match recorded(&out, guard)? {
        Some(id) if id == run => Ok(out),
        _ => Err(format!("{DISPATCH_GUARDS} could not take the record for {}", guard.name())),
    }
}

/// What a guard found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Nothing to refuse; the text says what was seen.
    Pass(String),
    /// The dispatch should not go out.
    Refuse {
        /// Why.
        why: String,
        /// The next step, with the exact command where there is one.
        next: String,
    },
    /// The guard could not look (ER-33): never a pass.
    NotMeasured(String),
}

/// What the dispatch does with a guard's finding.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// The dispatch goes on; the JSON row of the guard for the report.
    Go(serde_json::Value),
    /// The dispatch stops: blocking mode and a refusal (`not_measured: false`) or a finding that could not look (`true`).
    Stop {
        /// Whether the guard could not look (exit 6) rather than refused (exit 4).
        not_measured: bool,
        /// The message, the next step included.
        message: String,
    },
}

/// Applies a guard's mode and the coordinator's override (a non-empty reason) to its finding, and logs it: a pass as a plain line, a
/// refusal or not-measured as a `::warning::` (report-only, or overridden with the reason) or an `::error::` (blocking).
pub fn decide(guard: Guard, mode: &Mode, finding: &Finding, override_reason: Option<&str>, log: &mut dyn std::io::Write) -> Decision {
    let name = guard.name();
    let (mode_name, mode_why) = match mode {
        Mode::ReportOnly(why) => ("report-only", why.clone()),
        Mode::Blocking { first_green: 0 } => ("blocking", "blocking: established tests (user, 2026-11-17)".to_string()),
        Mode::Blocking { first_green } => ("blocking", format!("blocking since run {first_green}")),
    };
    let row = |result: &str, detail: &str, overridden: Option<&str>| serde_json::json!({"guard": name, "mode": mode_name, "result": result, "detail": detail, "overridden": overridden});
    let (result, why, next) = match finding {
        Finding::Pass(seen) => {
            let _ = writeln!(log, "dispatch guard {name}: pass: {seen}");
            return Decision::Go(row("pass", seen, None));
        }
        Finding::Refuse { why, next } => ("refuse", why.clone(), next.clone()),
        Finding::NotMeasured(why) => (
            "not-measured",
            format!("not measured, never a pass (ER-33): {why}"),
            match guard {
                Guard::TrainAudits => "dispatch from a clean worktree at the train's head, or fix what the line above names".to_string(),
                _ => "make the runs readable (GITEA_TOKEN, the server) and dispatch again".to_string(),
            },
        ),
    };
    if let Some(reason) = override_reason {
        let _ = writeln!(
            log,
            "::warning::dispatch guard {name}: overridden by the coordinator ({} \"{reason}\"): {why}",
            guard.override_flag()
        );
        return Decision::Go(row(result, &why, Some(reason)));
    }
    match mode {
        Mode::ReportOnly(_) => {
            let _ = writeln!(
                log,
                "::warning::dispatch guard {name} (report-only, ER-41: {mode_why}; the dispatch goes on) would refuse: {why}. Next: {next}."
            );
            Decision::Go(row(result, &why, None))
        }
        Mode::Blocking { .. } => {
            let message = format!("dispatch: refused by guard {name} ({mode_why}): {why}. Next: {next}.");
            let _ = writeln!(log, "::error::{message}");
            Decision::Stop {
                not_measured: matches!(finding, Finding::NotMeasured(_)),
                message,
            }
        }
    }
}

/// The coordinator's overrides, each with its logged reason (the queue has none).
#[derive(Debug, Clone, Copy, Default)]
pub struct Overrides<'a> {
    /// `--full-anyway`: the failed-first guard.
    pub full_anyway: Option<&'a str>,
    /// `--red-main-anyway`: the green-main guard.
    pub red_main_anyway: Option<&'a str>,
    /// `--audits-anyway`: the train-audits guard.
    pub audits_anyway: Option<&'a str>,
}

/// What the repository-wide audits answered on a checkout: the exit code of `check --pre-push` (`None`: ended by its bound or a
/// signal) and its output's tail.
pub type AuditRun = Result<(Option<i32>, String), String>;

/// The checkout the dispatch runs from, as the train-audits guard needs it: its HEAD and whether it has changes of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    /// `git rev-parse HEAD`.
    pub head: String,
    /// Whether `git status --porcelain` lists anything.
    pub dirty: bool,
}

/// The rows the train-audits guard runs: the repository-wide audits of tools/paddock-dev and the conformance line of ci.yml, through
/// the push gate (`./dev check --pre-push --member-gate --only`), which runs each through the heavy slot as the member gate does.
pub const AUDIT_ROWS: &str = "member-audits,conformance";

/// The train-audits guard: the audits ran on the train's head as the server has it. `local` is the checkout the dispatch runs from
/// (it must be at that head and clean: otherwise the audits would judge another tree, never a pass); `run` runs them there.
pub fn train_audits(local: Result<Checkout, String>, server_head: &str, branch: &str, run: &mut dyn FnMut() -> AuditRun) -> Finding {
    let local = match local {
        Ok(c) => c,
        Err(e) => return Finding::NotMeasured(format!("the checkout cannot be read: {e}")),
    };
    if local.head != server_head {
        return Finding::NotMeasured(format!(
            "this checkout is at {}, the head of {branch} is {}: the audits must run on the train's head (dispatch from the integrator's worktree at that head)",
            short(&local.head),
            short(server_head)
        ));
    }
    if local.dirty {
        return Finding::NotMeasured("this checkout has changes of its own: the audits would not judge the train's head".into());
    }
    match run() {
        Err(e) => Finding::NotMeasured(format!("the audits could not run: {e}")),
        Ok((Some(0), _)) => Finding::Pass(format!(
            "the member audits and the conformance line passed on {}",
            short(server_head)
        )),
        Ok((Some(6), tail)) => Finding::NotMeasured(format!("the audits could not look (exit 6): {}", tail.trim())),
        Ok((code, tail)) => Finding::Refuse {
            why: format!(
                "the repository-wide audits failed on the merged train head {} (exit {}): {}",
                short(server_head),
                code.map_or("none".to_string(), |c| c.to_string()),
                tail.trim()
            ),
            next: format!(
                "fix the audit it names in the member that clashes, or pull that member (revert its merge); re-run them with `./dev check --pre-push --member-gate --only {AUDIT_ROWS} --base origin/main`"
            ),
        },
    }
}

/// The checkout at `root`: its HEAD and whether it is dirty.
pub fn checkout(root: &std::path::Path) -> Result<Checkout, String> {
    let git = crate::git::Git::new(root);
    Ok(Checkout {
        head: git.text(&["rev-parse", "HEAD"])?.trim().to_string(),
        dirty: !git.text(&["status", "--porcelain", "--untracked-files=no"])?.trim().is_empty(),
    })
}

/// Runs the audit rows in `root` through this program (or `PADDOCK_DISPATCH_AUDITS`, a test's stand-in): its exit code and the last
/// lines it printed. Bounded like the member gate (each row has its own cap; this bound only ends a hang of the gate itself).
pub fn run_audits(root: &std::path::Path) -> AuditRun {
    let program = match std::env::var_os("PADDOCK_DISPATCH_AUDITS") {
        Some(p) => std::path::PathBuf::from(p),
        None => crate::tool_cache::self_exe().map_err(|e| format!("this program cannot be found: {e}"))?,
    };
    let mut cmd = paddock_proc::command(program);
    cmd.args([
        "check",
        "--pre-push",
        "--member-gate",
        "--only",
        AUDIT_ROWS,
        "--base",
        "origin/main",
        "--root",
    ])
    .arg(root)
    .current_dir(root);
    let out = paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(AUDITS_TIMEOUT)).map_err(|e| e.to_string())?;
    let code = match out.outcome {
        paddock_proc::Outcome::Exited(st) => st.code(),
        _ => None,
    };
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), out.stderr_text());
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    Ok((code, lines[lines.len().saturating_sub(12)..].join("\n")))
}

/// The backstop of [`run_audits`]: the member gate's own outer bound (merge_check::gates, measured there), as the rows are the same.
const AUDITS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// A blocking guard stopped the dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// Whether the guard could not look (exit 6) rather than refused (exit 4).
    pub not_measured: bool,
    /// The message, the next step included (also logged as an `::error::`).
    pub message: String,
}

/// The guards of one dispatch, the one entry point of `ci dispatch` and the landing queue: nothing for a ref that is not a train's;
/// failed-first for a full run of a workflow that takes `since_run`; green-main for every train dispatch but a failed-only round. Each
/// is decided by its mode in [`DISPATCH_GUARDS`] under `root` and its override. The JSON rows of the guards that let it go, or the stop.
#[allow(clippy::too_many_arguments)]
pub fn check_train(
    http: &mut dyn Http,
    target: &Target<'_>,
    root: &std::path::Path,
    read: ReadWorkflow<'_>,
    req: &Request,
    overrides: Overrides<'_>,
    now_ms: f64,
    log: &mut dyn std::io::Write,
) -> Result<Vec<serde_json::Value>, Stopped> {
    if !is_train(&req.git_ref) {
        return Ok(Vec::new());
    }
    let register = std::fs::read_to_string(root.join(DISPATCH_GUARDS)).map_err(|e| e.to_string());
    let mode = |g: Guard| mode_of(register.as_deref().map_err(String::as_str), g);
    let kind = RunKind::of(&req.inputs);
    let branch = req.git_ref.strip_prefix("refs/heads/").unwrap_or(&req.git_ref).to_string();
    let Target { server, repo, token } = *target;
    let mut rows = Vec::new();
    let mut go = |d: Decision| match d {
        Decision::Go(row) => {
            rows.push(row);
            Ok(())
        }
        Decision::Stop { not_measured, message } => Err(Stopped { not_measured, message }),
    };
    if kind == RunKind::Full && takes_since_run(&req.workflow, read) {
        let mut client = Client::new(http, server, repo, token);
        let finding = match client.branch_head(&branch) {
            Ok(Some(head)) => match read_train(&mut client, &req.workflow, &branch) {
                Ok(runs) => {
                    let finding = failed_first(&runs, &head, &req.workflow, &branch);
                    match (&finding, red_chain(&runs)) {
                        (Finding::Refuse { .. }, Some((full, newest_red))) => {
                            let (full_head, newest_red) = (full.head.clone(), newest_red);
                            drop(client);
                            let revert = clean_reverts(root, &full_head, &head);
                            let round = round_is_full(http, target, newest_red, read);
                            without_round(finding, revert, round)
                        }
                        _ => finding,
                    }
                }
                Err(e) => Finding::NotMeasured(format!("the runs of {} on {branch} cannot be read: {e}", req.workflow)),
            },
            Ok(None) => Finding::NotMeasured(format!("branch {branch} is not on the server")),
            Err(e) => Finding::NotMeasured(format!("the head of {branch} cannot be read: {e}")),
        };
        go(decide(
            Guard::FailedFirst,
            &mode(Guard::FailedFirst),
            &finding,
            overrides.full_anyway,
            log,
        ))?;
    }
    if !matches!(kind, RunKind::Since(_)) {
        let apps = apps_in(root);
        let main = {
            let mut client = Client::new(http, server, repo, token);
            read_main(&mut client)
        };
        let green = match main {
            Ok((nightly, push)) => green_main(nightly.as_ref(), push.as_ref(), &apps, now_ms, &mut |sha| {
                active_at_remote(http, server, repo, token, sha, &apps)
            }),
            Err(e) => GreenMain {
                finding: Finding::NotMeasured(format!("main's runs cannot be read: {e}")),
                fix_items: BTreeMap::new(),
            },
        };
        for (app, items) in &green.fix_items {
            for item in items {
                let _ = writeln!(
                    log,
                    "::notice::green-main: fix item of {app} (inactive, does not stop the train): {item}"
                );
            }
        }
        go(decide(
            Guard::GreenMain,
            &mode(Guard::GreenMain),
            &green.finding,
            overrides.red_main_anyway,
            log,
        ))?;
    }
    // Last: the audits take minutes, so they run only once the guards that read the API let the dispatch go, and only before a full
    // run of the train (its first dispatch and its landing run): a failed-only round reruns what failed, a member run is narrowed.
    if kind == RunKind::Full {
        let head = Client::new(http, server, repo, token).branch_head(&branch);
        let finding = match head {
            Ok(Some(h)) => train_audits(checkout(root), &h, &branch, &mut || {
                let _ = writeln!(log, "dispatch guard train-audits: running {AUDIT_ROWS} on the train head (minutes)");
                run_audits(root)
            }),
            Ok(None) => Finding::NotMeasured(format!("branch {branch} is not on the server")),
            Err(e) => Finding::NotMeasured(format!("the head of {branch} cannot be read: {e}")),
        };
        go(decide(
            Guard::TrainAudits,
            &mode(Guard::TrainAudits),
            &finding,
            overrides.audits_anyway,
            log,
        ))?;
    }
    Ok(rows)
}

/// One run of a train's workflow, newest first, as the failed-first guard needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainRun {
    /// The run's id.
    pub id: u64,
    /// The commit it ran on.
    pub head: String,
    /// Whether it has finished.
    pub finished: bool,
    /// `success`, `failure`, `cancelled`... (empty while it runs).
    pub conclusion: String,
    /// Its kind; `None` when it cannot be told (no token and no readable mode line).
    pub kind: Option<RunKind>,
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

/// The failed-first guard over a train's runs of `workflow` (newest first) and the branch's current head.
pub fn failed_first(runs: &[TrainRun], head: &str, workflow: &str, git_ref: &str) -> Finding {
    let counted = |r: &TrainRun| r.finished && !matches!(r.conclusion.as_str(), "cancelled" | "skipped" | "");
    let mut last_full = None;
    for r in runs {
        match &r.kind {
            Some(RunKind::Full) if counted(r) => {
                last_full = Some(r);
                break;
            }
            None if counted(r) => {
                return Finding::NotMeasured(format!(
                    "run {} of {workflow} on {git_ref} has no dispatch token and no readable plan mode line: it may be a newer full run",
                    r.id
                ));
            }
            _ => {}
        }
    }
    let Some(full) = last_full else {
        return Finding::Pass(format!("no finished full run of {workflow} on {git_ref} yet"));
    };
    if full.conclusion == "success" {
        return Finding::Pass(format!("the last full run {} of {workflow} on {git_ref} is green", full.id));
    }
    let rounds: Vec<&TrainRun> = runs
        .iter()
        .filter(|r| r.id > full.id && matches!(r.kind, Some(RunKind::Since(_))))
        .collect();
    if let Some(g) = rounds.iter().find(|r| r.finished && r.conclusion == "success" && r.head == head) {
        return Finding::Pass(format!(
            "the last full run {} is {}, and failed-only round {} is green on the head {}",
            full.id,
            full.conclusion,
            g.id,
            short(head)
        ));
    }
    let newest_red = rounds
        .iter()
        .find(|r| r.finished && r.conclusion == "failure")
        .map_or(full.id, |r| r.id);
    let going: Vec<String> = rounds.iter().filter(|r| !r.finished).map(|r| r.id.to_string()).collect();
    let mut next =
        format!("run the failed-only round first: ./dev ci dispatch {workflow} --ref {git_ref} --input since_run={newest_red} --max 2h");
    if !going.is_empty() {
        next = format!(
            "wait for failed-only round(s) {} (`./dev wait --ci-run <id> --max 2h`), or {next}",
            going.join(", ")
        );
    }
    next.push_str(
        "; when that round is green on this head, dispatch the full run again (a coordinator who must skip it: --full-anyway \"<reason>\")",
    );
    Finding::Refuse {
        why: format!(
            "the last full run {} of {workflow} on {git_ref} is {} and no failed-only round after it is green on the head {} (ER-10)",
            full.id,
            full.conclusion,
            short(head)
        ),
        next,
    }
}

/// The red full run the failed-first guard measures from, and the newest red run of its chain (the full run, or a failed-only round
/// after it): `None` when the last finished full run is green or there is none.
pub fn red_chain(runs: &[TrainRun]) -> Option<(&TrainRun, u64)> {
    let counted = |r: &TrainRun| r.finished && !matches!(r.conclusion.as_str(), "cancelled" | "skipped" | "");
    let full = runs.iter().find(|r| r.kind == Some(RunKind::Full) && counted(r))?;
    if full.conclusion == "success" {
        return None;
    }
    let newest_red = runs
        .iter()
        .filter(|r| r.id > full.id && matches!(r.kind, Some(RunKind::Since(_))))
        .find(|r| r.finished && r.conclusion == "failure")
        .map_or(full.id, |r| r.id);
    Some((full, newest_red))
}

/// The failed-first finding with its two allowances (user, 2026-11-25), recorded in [`DISPATCH_GUARDS`]: a full run may go without a
/// failed-only round when the round would add nothing, because every commit since the red full run cleanly reverts a member's merge
/// (`revert`), or because the round would rerun everything on the same head anyway (`round_full`: its rerun plan is the whole run).
/// Each is `Ok(Some(why))` when it holds; `Ok(None)` or `Err` (it could not be told) leaves the refusal as it is. Only a refusal
/// changes; the full run before landing stays mandatory whatever this says (ER-10).
pub fn without_round(finding: Finding, revert: Result<Option<String>, String>, round_full: Result<Option<String>, String>) -> Finding {
    let Finding::Refuse { why, .. } = &finding else { return finding };
    let allowed = match (revert, round_full) {
        (Ok(Some(w)), _) => w,
        (_, Ok(Some(w))) => w,
        _ => return finding,
    };
    Finding::Pass(format!(
        "{why}, but no failed-only round is needed: {allowed}; the full run before landing is this run (ER-10)"
    ))
}

/// Whether every commit of `from..to` (first parent) cleanly reverts a member's merge: a commit with one parent that names the merge
/// it pulls (git's `This reverts commit <sha>` line, or the train's own `(merge <sha>)` in the subject) and whose change is exactly the
/// inverse of that merge's change against its first parent (the same files, the same removed and added lines). `Ok(None)` when one is
/// not, or there is no commit; `Err` when git cannot tell (a commit missing here: fetch the train first).
pub fn clean_reverts(root: &std::path::Path, from: &str, to: &str) -> Result<Option<String>, String> {
    let git = crate::git::Git::new(root);
    let range = format!("{from}..{to}");
    let commits: Vec<String> = git
        .text(&["rev-list", "--first-parent", "--reverse", &range])?
        .lines()
        .map(str::to_string)
        .collect();
    if commits.is_empty() {
        return Ok(None);
    }
    let parents = |c: &str| -> Result<usize, String> {
        Ok(git
            .text(&["rev-list", "--parents", "-n", "1", c])?
            .split_whitespace()
            .count()
            .saturating_sub(1))
    };
    let mut said = Vec::new();
    for c in &commits {
        if parents(c)? != 1 {
            return Ok(None);
        }
        let message = git.text(&["log", "-1", "--format=%B", c])?;
        let mut named = crate::merge_check::reverted_base::trailer_targets(&message);
        named.extend(merge_named(&message));
        let undone = format!("{c}^");
        let mut clean = None;
        for x in named {
            let Ok(full) = git.text(&["rev-parse", "--verify", &format!("{x}^{{commit}}")]) else {
                continue;
            };
            let x = full.trim().to_string();
            if parents(&x)? < 2 {
                continue;
            }
            let merged_diff = git.text(&["diff", "--binary", "--no-color", &format!("{x}^1"), &x])?;
            let put_back_diff = git.text(&["diff", "--binary", "--no-color", c, &undone])?;
            let (merged, put_back) = (changed_lines(&merged_diff), changed_lines(&put_back_diff));
            if !merged.is_empty() && merged == put_back {
                clean = Some(x);
                break;
            }
        }
        match clean {
            Some(x) => said.push(format!("{} reverts the merge {} exactly", short(c), short(&x))),
            None => return Ok(None),
        }
    }
    Ok(Some(said.join("; ")))
}

/// The merges a train's own pull-out names in its subject: `revert: pull <branch> (merge <sha>) from <train>`.
fn merge_named(message: &str) -> Vec<String> {
    let first = message.lines().next().unwrap_or("");
    first
        .match_indices("(merge ")
        .filter_map(|(i, m)| {
            let hex: String = first[i + m.len()..].chars().take_while(char::is_ascii_hexdigit).collect();
            first[i + m.len() + hex.len()..].starts_with(')').then_some(hex)
        })
        .filter(|h| (7..=64).contains(&h.len()))
        .collect()
}

/// What a diff changes, without its positions: the file headers and the removed and added lines, in order (hunk headers and index
/// lines dropped, so the same change at shifted lines compares equal).
fn changed_lines(diff: &str) -> Vec<&str> {
    diff.lines()
        .filter(|l| {
            l.starts_with("diff --git ") || ((l.starts_with('+') || l.starts_with('-')) && !l.starts_with("+++") && !l.starts_with("---"))
        })
        .collect()
}

/// Whether the failed-only round of `run` would rerun the whole run anyway (its rerun plan is "everything": the plan job failed, or a
/// failed job this ci.yml cannot gate alone), so a full run on the same head is the same run.
pub fn round_is_full(http: &mut dyn Http, target: &Target<'_>, run: u64, read: ReadWorkflow<'_>) -> Result<Option<String>, String> {
    let mut runs = crate::plan_since::GiteaRuns::new(http, target.server, target.repo, target.token.to_string());
    let plan = crate::plan_since::since_run(&mut runs, run, read, &mut |_| {})?;
    Ok(plan
        .everything
        .map(|why| format!("the failed-only round of run {run} would rerun everything ({why})")))
}

/// One run of main as the green-main guard reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct MainRun {
    /// The run's id.
    pub id: u64,
    /// Its workflow file (`nightly.yml`, `ci.yml`).
    pub workflow: String,
    /// The commit it tested.
    pub sha: String,
    /// The jobs that failed (a cancelled one says so in its name: it was not measured).
    pub failed: Vec<String>,
    /// Its scope line's `expected` map (display name to job, leg and projects), read only for a run with failed jobs.
    pub expected: crate::run_scope::ExpectedMap,
    /// When it was created, milliseconds since the epoch (`None`: not given).
    pub created_ms: Option<f64>,
}

/// The oldest a nightly may be and still speak for main: the nightly runs every night and a skip never leaves main unmeasured for
/// more than 36 h (nightly.yml); 48 h leaves a late night room. An older one means the schedule is off or failing to start.
pub const NIGHTLY_MAX_AGE_MS: f64 = 48.0 * 3600.0 * 1000.0;

/// What the green-main guard found, with the inactive apps' fix items it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreenMain {
    /// The verdict.
    pub finding: Finding,
    /// The failed jobs of each inactive app (`workflow: job`): its fix items, never a refusal.
    pub fix_items: BTreeMap<String, Vec<String>>,
}

/// The green-main guard over main's newest finished nightly and push run at `now_ms`. `active_at(sha)` is the active set at a commit.
/// A nightly older than [`NIGHTLY_MAX_AGE_MS`] (or of unknown age) speaks for an older main: not measured.
pub fn green_main(
    nightly: Option<&MainRun>,
    push: Option<&MainRun>,
    apps: &BTreeSet<String>,
    now_ms: f64,
    active_at: &mut dyn FnMut(&str) -> Result<BTreeSet<String>, String>,
) -> GreenMain {
    let mut fix_items: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let (Some(nightly), Some(push)) = (nightly, push) else {
        let missing = if nightly.is_none() { "nightly" } else { "push run of ci.yml" };
        return GreenMain {
            finding: Finding::NotMeasured(format!(
                "no finished {missing} on main among the newest runs (is the nightly schedule enabled?)"
            )),
            fix_items,
        };
    };
    if nightly.created_ms.is_none_or(|c| now_ms - c > NIGHTLY_MAX_AGE_MS) {
        let age = nightly
            .created_ms
            .map_or("of unknown age".to_string(), |c| format!("{:.0} h old", (now_ms - c) / 3_600_000.0));
        return GreenMain {
            finding: Finding::NotMeasured(format!(
                "the newest finished nightly on main (run {}) is {age}: the nightly schedule is off or not starting, so main is not measured",
                nightly.id
            )),
            fix_items,
        };
    }
    let mut blocking = Vec::new();
    for run in [nightly, push] {
        let active = match active_at(&run.sha) {
            Ok(a) => a,
            Err(e) => {
                return GreenMain {
                    finding: Finding::NotMeasured(format!(
                        "the active apps at {} (run {}) cannot be read: {e}",
                        short(&run.sha),
                        run.id
                    )),
                    fix_items,
                };
            }
        };
        let pairs: Vec<(String, String)> = run.failed.iter().map(|j| (run.workflow.clone(), j.clone())).collect();
        let red = active_apps::classify_nightly(&pairs, &active, apps, &run.expected);
        blocking.extend(
            red.blocking
                .into_iter()
                .map(|j| format!("run {} ({}): {j}", run.id, short(&run.sha))),
        );
        for (app, items) in red.fix_items {
            fix_items.entry(app).or_default().extend(items);
        }
    }
    let seen = format!(
        "nightly run {} ({}) and push run {} ({})",
        nightly.id,
        short(&nightly.sha),
        push.id,
        short(&push.sha)
    );
    let finding = if blocking.is_empty() {
        Finding::Pass(format!("{seen}: green in the foundation and the active apps"))
    } else {
        Finding::Refuse {
            why: format!(
                "main is red in the foundation or an active app ({seen}): {}",
                blocking.join("; ")
            ),
            next: "fix those jobs on main first (a fix branch, its train landed); a train that carries the fix dispatches with --red-main-anyway \"<the fix it carries>\"".into(),
        }
    };
    GreenMain { finding, fix_items }
}

/// Jobs whose failure only restates other jobs' (the nightly's installers verdict): left out when another job failed too.
const AGGREGATES: &[&str] = &["installers verdict"];

/// The failed jobs of a finished run's job list: failures, and cancelled jobs marked as not measured; a reusable workflow's caller
/// (it spans its children, which are listed) and an aggregate whose inputs also failed are left out.
pub fn failed_jobs(jobs: &[TimedJob]) -> Vec<String> {
    let mut out: Vec<(bool, String)> = jobs
        .iter()
        .filter(|j| !j.is_caller())
        .filter_map(|j| match j.outcome {
            JobOutcome::Failed => Some((AGGREGATES.iter().any(|a| j.name.starts_with(a)), j.name.clone())),
            JobOutcome::Cancelled => Some((false, format!("{} (cancelled: not measured)", j.name))),
            // A finished run's job: a pass, a skip (the gate or a failed need left it out), or a word the reader does not know.
            JobOutcome::Passed | JobOutcome::Skipped | JobOutcome::Unfinished | JobOutcome::Unknown(_) => None,
        })
        .collect();
    if out.iter().any(|(agg, _)| !agg) {
        out.retain(|(agg, _)| !agg);
    }
    out.into_iter().map(|(_, n)| n).collect()
}

/// Whether a finished nightly measured anything: a job other than its gates ran to a success or a failure (a night the gate skipped
/// measured nothing: main unchanged since the last green nightly, or a train in CI).
fn measured(jobs: &[TimedJob]) -> bool {
    jobs.iter()
        .filter(|j| !j.name.starts_with("gate") && !j.name.starts_with("installers gate"))
        .any(|j| matches!(j.outcome, JobOutcome::Passed | JobOutcome::Failed))
}

/// How many pages of 50 runs the readers look through at most.
const PAGES: u32 = 4;
/// How many finished nightlies that measured nothing are passed over at most.
const SKIPPED_NIGHTS: usize = 10;

fn on_branch(r: &RunInfo, branch: &str) -> bool {
    r.git_ref().strip_prefix("refs/heads/").unwrap_or(r.git_ref()) == branch
}

/// The newest finished nightly on main that measured something, and the newest finished push run of ci.yml on main, with their failed
/// jobs. A cancelled run is passed over (it measured nothing whole).
pub fn read_main(client: &mut Client) -> Result<(Option<MainRun>, Option<MainRun>), String> {
    let mut nightly = None;
    let mut push = None;
    let mut skipped = 0;
    'pages: for page in 1..=PAGES {
        let p = client.runs_page_where(page, "branch=main")?;
        let n = p.runs.len();
        for r in p.runs {
            if !on_branch(&r, "main") || !r.finished() || matches!(r.conclusion.as_str(), "cancelled" | "skipped") {
                continue;
            }
            let want_nightly = nightly.is_none() && r.workflow() == "nightly.yml" && skipped < SKIPPED_NIGHTS;
            let want_push = push.is_none() && r.workflow() == "ci.yml" && r.event == "push";
            if !want_nightly && !want_push {
                continue;
            }
            let jobs = client.timed_jobs(r.id)?.ok_or_else(|| format!("run {} has no job list", r.id))?;
            if want_nightly && !measured(&jobs) {
                skipped += 1;
                continue;
            }
            let failed = failed_jobs(&jobs);
            let expected = if failed.is_empty() {
                crate::run_scope::ExpectedMap::new()
            } else {
                super::verdict::run_expected(client, &jobs)
            };
            let run = MainRun {
                id: r.id,
                workflow: r.workflow().to_string(),
                sha: r.head_sha.clone(),
                failed,
                expected,
                created_ms: super::time::ts(&r.created_at),
            };
            if want_nightly {
                nightly = Some(run);
            } else {
                push = Some(run);
            }
            if nightly.is_some() && push.is_some() {
                break 'pages;
            }
        }
        if n < 50 {
            break;
        }
    }
    Ok((nightly, push))
}

/// The active apps at `sha`, read from the server (`raw/docs/ci-active-apps.toml?ref=<sha>`, a 404 is "no file": every app).
pub fn active_at_remote(
    http: &mut dyn Http,
    server: &str,
    repo: &str,
    token: &str,
    sha: &str,
    apps: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let url = format!(
        "{}/api/v1/repos/{repo}/raw/{}?ref={sha}",
        server.trim_end_matches('/'),
        encode_path(active_apps::FILE)
    );
    let text = match http.get_text(&url, token) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.contains("error: 404") || e.contains("HTTP 404") => Ok(None),
        Err(e) => Err(format!("{} at {} cannot be read: {e}", active_apps::FILE, short(sha))),
    };
    active_apps::active_at(apps, &|_| text.clone())
}

/// The train's runs of `workflow` on `branch`, newest first, down to and including its last finished full run; each run's kind read
/// from its token, else from its plan job's mode line.
pub fn read_train(client: &mut Client, workflow: &str, branch: &str) -> Result<Vec<TrainRun>, String> {
    let mut out = Vec::new();
    let query = format!("event=workflow_dispatch&branch={}", encode_path(branch));
    for page in 1..=PAGES {
        let p = client.runs_page_where(page, &query)?;
        let n = p.runs.len();
        for r in p.runs.into_iter().filter(|r| r.workflow() == workflow && on_branch(r, branch)) {
            let kind = match DispatchTag::of_title(&r.title) {
                Some(t) => Some(t.kind),
                None if !r.finished() => Some(RunKind::Part),
                None => kind_from_plan_log(client, r.id),
            };
            let full = kind == Some(RunKind::Full) && r.finished() && !matches!(r.conclusion.as_str(), "cancelled" | "skipped");
            let unknown = kind.is_none() && r.finished();
            out.push(TrainRun {
                id: r.id,
                head: r.head_sha.clone(),
                finished: r.finished(),
                conclusion: r.conclusion.clone(),
                kind,
            });
            if full || unknown {
                return Ok(out);
            }
        }
        if n < 50 {
            break;
        }
    }
    Ok(out)
}

/// A run's kind from its plan job's `plan mode: full=` line: `full=true` a full run, `false` a narrowed one; `None` when the job or
/// its log cannot be read or has no such line.
fn kind_from_plan_log(client: &mut Client, run: u64) -> Option<RunKind> {
    let jobs = client.timed_jobs(run).ok()??;
    let plan = jobs.iter().find(|j| j.name.starts_with("plan"))?;
    let log = client.log(plan.id).ok()??;
    match read_marker(&log)? {
        Ok(true) => Some(RunKind::Full),
        Ok(false) => Some(RunKind::Part),
        Err(_) => None,
    }
}

/// Whether `workflow` takes the `since_run` input (its failures can be rerun alone): only such a workflow has the failed-first guard.
pub fn takes_since_run(workflow: &str, read: ReadWorkflow<'_>) -> bool {
    read(workflow)
        .and_then(|text| paddock_ci_config::jobs::triggers(&text).ok())
        .is_some_and(|t| t.takes_input("since_run"))
}

/// The apps of the checkout at `root` (`apps/<app>/paddock.ci.json`).
pub fn apps_in(root: &std::path::Path) -> BTreeSet<String> {
    std::fs::read_dir(root.join("apps"))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.path().join("paddock.ci.json").is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
