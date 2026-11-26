// ci-runs-reader: the commands over the client: triage, schedule-skip, verdict
//! The `ci` commands over the Gitea Actions API (read-only):
//!
//! - `ci triage <run>`: NEW, ALREADY FAILING ON MAIN or FLAKY for each failed test of a run ([`triage`]); its flake history is
//!   [`flakes`] (the tests nextest and Playwright marked flaky, vitest's failed specs, and tests that failed and then passed on
//!   a rerun of the same commit; the log readers are [`logfind`]). A run whose logs cannot be read is "not measured", never
//!   "no flakes" (ER-33).
//! - `ci verdict --workflow <file> --sha <sha>`: is there a full green run of the workflow on the commit ([`verdict`]; exit 0, 5
//!   red or partial only, 6 not measured). The one reader of the runs for a pass/fail answer; the tag workflow asks it.
//! - `ci schedule-skip`: whether a scheduled run ends early because a train is in CI ([`schedule_skip`]; the job token in
//!   `PLAN_TOKEN` or `GITEA_TOKEN`, and it never fails the run).
//!
//! The API client ([`client`]) reuses the planner's [`Http`](crate::wait::Http) and [`GiteaRuns`](crate::plan_since::GiteaRuns).
//! The token is read from `GITEA_TOKEN`, travels in a header only and is printed nowhere.

pub mod cache_audit;
pub mod cancel;
pub mod client;
pub mod dispatch;
pub mod dispatch_guards;
pub mod fix_items;
pub mod flakes;
pub mod green;
pub mod landing;
pub mod logfind;
pub mod members;
pub mod record;
pub mod reservation;
pub mod reuse;
pub mod schedule_skip;
#[cfg(test)]
mod testutil;
pub mod time;
pub mod triage;
pub mod trial;
pub mod verdict;

use crate::wait::CurlHttp;
use clap::Args;
use client::Client;
use paddock_cli::{CliError, Exit, Report};
use std::path::PathBuf;

const SERVER: &str = "https://dev.decryptology.net";

/// Arguments of `ci cache-audit`.
#[derive(Args)]
pub struct CacheAuditArgs {
    /// The workflow file whose runs are read.
    #[arg(long, default_value = "ci.yml")]
    workflow: String,
    /// How many finished runs to read (the newest first); each run's jobs are read one log at a time.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=20))]
    runs: u32,
    /// A key counts as never saved from this many misses with no hit and no confirmed save.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=1000))]
    min_misses: u32,
    /// Exit 5 when a key was never saved (default: exit 0, report-only, ER-41). A window that shows no `cache-report:` line exits 6.
    #[arg(long)]
    fail_on_never_saved: bool,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `ci test-cost`.
#[derive(Args)]
pub struct TestCostArgs {
    /// A finished run whose job logs are read (every job; the ones with no nextest run are skipped).
    #[arg(long, conflicts_with = "log")]
    run: Option<u64>,
    /// A file holding nextest's output (a saved job log, or a local `cargo nextest run` captured with `2>&1`). Repeatable.
    #[arg(long)]
    log: Vec<PathBuf>,
    /// The charge report the tests wrote (`PADDOCK_TEST_COST_REPORT`, or `<target>/nextest/test-cost/<run id>.jsonl`): every misplaced
    /// `cargo metadata` call is a `::warning::` and a row of the job summary (`GITHUB_STEP_SUMMARY`).
    #[arg(long)]
    report: Option<PathBuf>,
    /// The nextest configuration whose stops the times are judged against (default: tools/paddock-dev/.config/nextest.toml of this
    /// repository), read with the TOML parser.
    #[arg(long)]
    config: Option<PathBuf>,
    /// The nextest profile of the run.
    #[arg(long, default_value = "ci")]
    profile: String,
    /// Also list every assert on elapsed wall time inside paddock-dev's tests (`src`, `tests`): a test a busy host can fail without a bug.
    /// Report-only: a `::warning::` each, with the next step.
    #[arg(long)]
    wall_asserts: bool,
    /// Exit 5 when an unclassed test is over a quarter of its stop (default: exit 0, report-only, ER-41).
    #[arg(long)]
    fail_on_findings: bool,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci test-cost`: the cost classes of paddock-dev's tests against what they took, and the charges the tests recorded (ADR 0121).
/// Report-only: exit 0, with a `::warning::` per finding and a job-summary table. With `--fail-on-findings` (only from the step that
/// records the first green run): 5 with findings, 6 when nothing was measured (no whole nextest run read, or an empty charge report).
pub fn test_cost(a: &TestCostArgs) -> Result<Report, CliError> {
    use crate::nextest_log::{NextestError, durations};
    let mut texts: Vec<(String, String)> = Vec::new();
    let mut unreadable = Vec::new();
    if let Some(run) = a.run {
        let token = token()?;
        let mut http = CurlHttp;
        let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
        let fail = |e: String| {
            CliError::failure(format!("run {run} could not be read: {e}"))
                .with_hint("Check GITEA_TOKEN, the server and the run id, then run it again.")
        };
        let jobs = client.timed_jobs(run).map_err(fail)?.ok_or_else(|| fail("no such run".into()))?;
        for job in jobs {
            match client.log(job.id) {
                Ok(Some(text)) => texts.push((format!("job {} ({})", job.id, job.name), text)),
                Ok(None) => unreadable.push(format!("job {} ({}): no log", job.id, job.name)),
                Err(e) => unreadable.push(format!("job {} ({}): {e}", job.id, job.name)),
            }
        }
    }
    for path in &a.log {
        match std::fs::read(path) {
            Ok(bytes) => texts.push((path.display().to_string(), String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) => unreadable.push(format!("{}: {e}", path.display())),
        }
    }
    let mut timed = Vec::new();
    let mut read = Vec::new();
    for (name, text) in texts {
        match durations(&text) {
            Ok(t) => {
                read.push(name);
                timed.extend(t);
            }
            Err(NextestError::NoRun) => {}
            Err(e) => unreadable.push(format!("{name}: {e}")),
        }
    }
    let timings = a.run.is_some() || !a.log.is_empty();
    if !timings && a.report.is_none() && !a.wall_asserts {
        return Err(CliError::usage("nothing to read").with_hint("Pass --run <id>, --log <file>, --report <file> or --wall-asserts."));
    }
    let root = std::env::current_dir().ok().and_then(|d| find_root(&d));
    let mut wall = String::new();
    if a.wall_asserts {
        let Some(root) = &root else {
            return Err(CliError::usage("--wall-asserts reads the repository").with_hint("Run it inside the checkout."));
        };
        let found = crate::test_cost::wall_asserts_in(&root.join("tools/paddock-dev"), &["src", "tests"]);
        wall.push_str(&format!("\n{} assert(s) on elapsed wall time in paddock-dev's tests", found.len()));
        for w in &found {
            wall.push_str(&format!(
                "\n::warning file=tools/paddock-dev/{},line={}::ci test-cost: {} asserts on elapsed wall time, which a busy host can fail \
                 without a bug (ADR 0121). Next step: inject a clock (pass time in) or bound the CPU time it spends, not the wall time.",
                w.file, w.line, w.test
            ));
        }
    }
    let level = if a.fail_on_findings { "error" } else { "warning" };
    let mut text = String::new();
    let mut summary = String::new();
    let mut charges_found = 0;
    let mut charges_measured = true;
    if let Some(path) = &a.report {
        match std::fs::read_to_string(path) {
            Ok(body) => {
                let rr = crate::test_cost::read_report(&body);
                charges_found = rr.misplaced.len();
                charges_measured = rr.charges > 0;
                let (t, md) = crate::test_cost::render_report(&rr, &path.display().to_string(), level);
                text.push_str(&t);
                summary.push_str(&md);
            }
            Err(e) => {
                charges_measured = false;
                text.push_str(&format!(
                    "::{level}::ci test-cost: not measured: the charge report {} could not be read ({e}): no test of this run charged a cost class, or {} did not reach the tests. Next step: check the variable on the Tests step.",
                    path.display(),
                    crate::test_cost::REPORT_ENV
                ));
                summary.push_str("### Test cost classes (ADR 0121)\n\nNot measured: no charge report was written.\n");
            }
        }
    }
    if let Some(path) = std::env::var("GITHUB_STEP_SUMMARY")
        .ok()
        .filter(|p| !p.is_empty() && !summary.is_empty())
    {
        use std::io::Write;
        let wrote = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(summary.as_bytes()));
        if let Err(e) = wrote {
            text.push_str(&format!(
                "\n::warning::ci test-cost: the job summary {path} could not be written: {e}"
            ));
        }
    }
    if !timings {
        let exit = if a.fail_on_findings && charges_found > 0 {
            Exit::Findings
        } else if a.fail_on_findings && !charges_measured {
            Exit::NotMeasured
        } else {
            Exit::Ok
        };
        text.push_str(&wall);
        let data = serde_json::json!({"misplaced": charges_found, "measured": charges_measured});
        return Ok(Report::new(data, text.trim_start().to_string()).with_exit(exit));
    }
    let config_path = a
        .config
        .clone()
        .or_else(|| root.as_ref().map(|r| r.join("tools/paddock-dev/.config/nextest.toml")))
        .ok_or_else(|| CliError::usage("no nextest configuration").with_hint("Pass --config <file>, or run it inside the checkout."))?;
    let stops = std::fs::read_to_string(&config_path)
        .map_err(|e| e.to_string())
        .and_then(|t| crate::test_cost::stops(&t, &a.profile))
        .map_err(|e| {
            CliError::failure(format!("the stops of {} could not be read: {e}", config_path.display()))
                .with_hint("Point --config at the nextest configuration of the run.")
        })?;
    let r = crate::test_cost::report(&timed, &stops);
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&format!("{} nextest log(s) read, {} paddock-dev test(s)", read.len(), r.tests));
    for u in &unreadable {
        text.push_str(&format!("\n::warning::ci test-cost: {u} (not read, so not counted)"));
    }
    for c in &r.classes {
        match &c.longest {
            Some((name, secs)) => text.push_str(&format!("\n{}: {} test(s), longest {name} {secs:.1} s", c.class, c.tests)),
            None => text.push_str(&format!("\n{}: no test", c.class)),
        }
    }
    for f in &stops.unmodelled {
        text.push_str(&format!(
            "\n::warning::ci test-cost: an override's filter is not modelled, its tests are judged by the base stop: {f}"
        ));
    }
    for (name, secs) in &r.findings {
        let stop = stops.of(name);
        text.push_str(&format!(
            "\n::{level}::ci test-cost: {name} took {secs:.1} s; times the measured contention of {} that passes its {stop:.0} s stop, so a busy \
             host can stop it (ADR 0121). Next step: give it the class and tier its work needs (`tree_` or `graph_`, or a longer tier in \
             .config/nextest.toml with its measurement), or make it cheaper.",
            crate::test_cost::CONTENTION
        ));
    }
    let exit = if r.tests == 0 {
        text.push_str(&format!(
            "\n::{level}::ci test-cost: not measured: no whole nextest run of paddock-dev was read. Next step: point it at the paddock-dev job's log."
        ));
        if a.fail_on_findings { Exit::NotMeasured } else { Exit::Ok }
    } else if a.fail_on_findings && (!r.findings.is_empty() || charges_found > 0) {
        Exit::Findings
    } else if a.fail_on_findings && !charges_measured {
        Exit::NotMeasured
    } else {
        Exit::Ok
    };
    let data = serde_json::json!({
        "logs_read": read.len(),
        "tests": r.tests,
        "unreadable": unreadable,
        "classes": r.classes.iter().map(|c| serde_json::json!({"class": c.class, "tests": c.tests, "longest": c.longest})).collect::<Vec<_>>(),
        "findings": r.findings,
        "misplaced": charges_found,
    });
    text.push_str(&wall);
    Ok(Report::new(data, text).with_exit(exit))
}

/// Arguments of `ci dispatch`.
#[derive(Args)]
pub struct DispatchArgs {
    /// The workflow file in `.gitea/workflows` (`dyno-release.yml`).
    workflow: String,
    /// The branch or tag to run it on.
    #[arg(long = "ref", value_name = "REF", required_unless_present = "trial", conflicts_with = "trial")]
    git_ref: Option<String>,
    /// The trial before approval (ADR 0098 WP-4): merge this branch's head onto the open train (origin/main when none is open) in
    /// the tool's own clone, report a conflict or a merge-check finding instead of dispatching, push `trial/<branch>` and
    /// dispatch it with `base` = the target's sha. The branch's one CI run (rule 50 (a)); the scratch clone lives in
    /// PADDOCK_TRIAL_DIR (required: a folder on a data drive). Needs GITEA_TOKEN with write access.
    #[arg(long, value_name = "BRANCH")]
    trial: Option<String>,
    /// With --trial: merge onto this ref (for example `origin/train/2026-11-13-8`) instead of the open train.
    #[arg(long, value_name = "REF", requires = "trial")]
    onto: Option<String>,
    /// A workflow input, `key=value` (repeat for more).
    #[arg(long = "input", value_name = "KEY=VALUE")]
    inputs: Vec<String>,
    /// The longest to wait for a busy runner, for example 30m. Required: the wait always ends.
    #[arg(long, value_name = "DURATION")]
    max: String,
    /// How often to look (default 15s).
    #[arg(long, value_name = "DURATION")]
    interval: Option<String>,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
    /// The checkout whose `.gitea/workflows` is read (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Coordinator override of the failed-first guard (ER-10): dispatch a full train run although the last full run is red and no
    /// failed-only round is green on the head. The reason is logged.
    #[arg(long, value_name = "REASON")]
    full_anyway: Option<String>,
    /// Coordinator override of the green-main guard: dispatch a train although main's nightly or push run is red in the foundation
    /// or an active app (a train that carries the fix). The reason is logged.
    #[arg(long, value_name = "REASON")]
    red_main_anyway: Option<String>,
    /// Coordinator override of the train-audits guard: dispatch a train although the repository-wide audits failed (or could not run)
    /// on its head. The reason is logged.
    #[arg(long, value_name = "REASON")]
    audits_anyway: Option<String>,
}

/// Arguments of `ci rerun-check`: guard 3 of the failed-only rounds (ER-10, ER-33).
#[derive(Args)]
pub struct RerunCheckArgs {
    /// The rerun's nextest filterset (written by `./dev plan --since-run`).
    #[arg(long)]
    filter: String,
    /// The output of `cargo nextest list --message-format json -E <filter>` (same selection as the run), a file.
    #[arg(long, value_name = "FILE")]
    listed: PathBuf,
}

/// `ci rerun-check`: every failed test a failed-only round names must select a test at this commit; one that matches nothing (renamed or
/// removed since the run that failed) fails the step with an `::error::` naming it, never a warning (run 8868 passed on an empty
/// selection). Exit 5 with the names, 1 when the filter or the listing cannot be read.
pub fn rerun_check(a: &RerunCheckArgs) -> Result<Report, CliError> {
    let fail = |e: String| {
        println!("::error::rerun-check: {e}");
        CliError::failure(e).with_hint("Check that the step lists with the same filter and selection it runs.")
    };
    let text = std::fs::read_to_string(&a.listed).map_err(|e| fail(format!("{} cannot be read: {e}", a.listed.display())))?;
    let listing: serde_json::Value = serde_json::from_str(&text).map_err(|e| fail(format!("{} is not JSON: {e}", a.listed.display())))?;
    let atoms = crate::plan_since::filterset_atoms(&a.filter).map_err(fail)?;
    let missing = crate::plan_since::rerun_unmatched(&a.filter, &listing).map_err(fail)?;
    if missing.is_empty() {
        return Ok(Report::new(
            serde_json::json!({"named": atoms.len(), "missing": []}),
            format!(
                "rerun-check: each of the {} failed test(s) the round names selects a test at this commit",
                atoms.len()
            ),
        ));
    }
    for m in &missing {
        println!(
            "::error::rerun-check: the failed test {m} matches no test at this commit (renamed or removed since the run that failed): a failed-only round must run every test it names (ER-10, ER-33). Next: rename it back, or let the next failed-only round of this run rerun this job whole (its failed step is not a test step)"
        );
    }
    let names: Vec<String> = missing.iter().map(ToString::to_string).collect();
    Ok(Report::new(
        serde_json::json!({"named": atoms.len(), "missing": names}),
        format!(
            "rerun-check: {} of {} failed test(s) match nothing: {}",
            names.len(),
            atoms.len(),
            names.join("; ")
        ),
    )
    .with_exit(Exit::Findings))
}

/// Arguments of `ci guard-record`: ER-41 for the dispatch guards.
#[derive(Args)]
pub struct GuardRecordArgs {
    /// The guard: `failed-first` or `green-main`.
    #[arg(long)]
    guard: String,
    /// The run of its first green use: a finished, green run of a train dispatched through `./dev ci dispatch` (its title carries the
    /// dispatch token) after the guard measured and let it go.
    #[arg(long)]
    run: u64,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci guard-record`: records a dispatch guard's first green use in `docs/ci-dispatch-guards.toml` (of the current repository), which
/// switches it from report-only to blocking in the one step (ER-41). Refuses a run that is not finished and green, not a train's, or not
/// dispatched through `./dev ci dispatch`, and a guard already recorded.
pub fn guard_record(a: &GuardRecordArgs) -> Result<Report, CliError> {
    let guard = dispatch_guards::Guard::parse(&a.guard)
        .ok_or_else(|| CliError::usage(format!("unknown guard {:?}", a.guard)).with_hint("Use failed-first or green-main."))?;
    let root = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let file = root.join(dispatch_guards::REGISTER);
    let text = std::fs::read_to_string(&file)
        .map_err(|e| CliError::failure(format!("{} cannot be read: {e}.", file.display())).with_hint("Run it from the repository root."))?;
    let updated = dispatch_guards::with_first_green(&text, guard, a.run).map_err(CliError::failure)?;
    let token = token()?;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
    let run = client
        .run(a.run)
        .map_err(CliError::failure)?
        .ok_or_else(|| CliError::failure(format!("the server has no run {}.", a.run)))?;
    let why = if !run.finished() || run.conclusion != "success" {
        Some(format!("it is {} {}, not finished and green", run.status, run.conclusion))
    } else if !dispatch::is_train(run.git_ref()) {
        Some(format!("it ran on {}, not on a train", run.git_ref()))
    } else if dispatch::DispatchTag::of_title(&run.title).is_none() {
        Some("its title carries no dispatch token: it was not dispatched through `./dev ci dispatch`".into())
    } else {
        None
    };
    if let Some(why) = why {
        return Err(
            CliError::failure(format!("run {} cannot be recorded for {}: {why}.", a.run, guard.name()))
                .with_hint("Record a green train run that `./dev ci dispatch` sent after this guard measured and let it go."),
        );
    }
    std::fs::write(&file, updated).map_err(|e| CliError::failure(format!("{} cannot be written: {e}.", file.display())))?;
    Ok(Report::new(
        serde_json::json!({"guard": guard.name(), "run": a.run}),
        format!(
            "recorded {} at run {} in {}: the guard now refuses (ER-41). Commit the file.",
            guard.name(),
            a.run,
            dispatch_guards::REGISTER
        ),
    ))
}

/// Arguments of `ci runner-wait` (the `runner-wait` action, the first job of a train run). The run, server and repository
/// default to the job's (`GITHUB_RUN_ID`, `GITHUB_SERVER_URL`, `GITHUB_REPOSITORY`); the token is `PLAN_TOKEN` or `GITEA_TOKEN`.
#[derive(Args)]
pub struct RunnerWaitArgs {
    /// This run's id (default: GITHUB_RUN_ID).
    #[arg(long)]
    run_id: Option<u64>,
    /// The longest to wait, for example 2h. Required: the wait always ends.
    #[arg(long, value_name = "DURATION")]
    max: String,
    /// How often to look (default 30s).
    #[arg(long, value_name = "DURATION")]
    interval: Option<String>,
    /// The repository (default: GITHUB_REPOSITORY, then decryp7/paddock).
    #[arg(long)]
    repo: Option<String>,
    /// The Gitea server (default: GITHUB_SERVER_URL, then the family's).
    #[arg(long)]
    server: Option<String>,
    /// The checkout whose `.gitea/workflows` is read (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

/// Arguments of `ci verdict`.
#[derive(Args)]
pub struct VerdictArgs {
    /// The workflow file whose runs are read (`ci.yml`, `debrief.yml`).
    #[arg(long)]
    workflow: String,
    /// The full 40-hex commit id.
    #[arg(long)]
    sha: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
    /// The answer is for a tag: only a run that tested every package on every OS counts, never an OS-scoped one (ER-15).
    #[arg(long, conflicts_with = "landing")]
    tag: bool,
    /// The app the tag releases: the deciding run must have planned it (it was active, or the run planned every app); a run whose
    /// declared scope lists it as inactive (docs/ci-active-apps.toml) is not its full green run (ER-15).
    #[arg(long)]
    app: Option<String>,
    /// The answer is for a landing: a run that ran exactly its own diff's selection counts, and green with fix items (FLAKY
    /// tests in untouched code, each a fix item) is landable (exit 0). Never for a tag.
    #[arg(long)]
    landing: bool,
}

/// Arguments of `ci trial-verdict`.
#[derive(Args)]
pub struct TrialVerdictArgs {
    /// The workflow file whose runs are read.
    #[arg(long, default_value = "ci.yml")]
    workflow: String,
    /// The member's branch (the trial ref is `trial/<branch>`).
    #[arg(long)]
    branch: String,
    /// The member's head, the full 40-hex commit id: only a trial merge of exactly this head counts.
    #[arg(long)]
    member_sha: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci trial-verdict`: what the member's own trial run (`ci dispatch --trial`) says, through the one reader ([`verdict::trial`]).
/// Exit 0 green, 5 red, 6 still running, no trial run of this head, or not measured (never a pass, ER-33). `--json` carries `state`:
/// `green`, `red`, `running`, `no-run` or `not-measured`, and `run` when there is one.
pub fn trial_verdict(a: &TrialVerdictArgs) -> Result<Report, CliError> {
    if a.member_sha.len() != 40 || !a.member_sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(
            CliError::usage(format!("--member-sha {:?} is not a full 40-hex commit id.", a.member_sha))
                .with_hint("Pass the whole sha (`git rev-parse HEAD`)."),
        );
    }
    let t = match token() {
        Err(_) => verdict::Trial::NotMeasured("GITEA_TOKEN is not set, so the runs cannot be read".into()),
        Ok(token) => {
            let mut http = CurlHttp;
            let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
            verdict::trial(&mut client, &a.workflow, &a.branch, &a.member_sha)
        }
    };
    Ok(trial_report(&t, &a.branch))
}

/// The report for a trial state (the words are the ones `merge check` prints).
pub fn trial_report(t: &verdict::Trial, branch: &str) -> Report {
    use verdict::Trial;
    let (state, run, text, exit) = match t {
        Trial::Green { run } => ("green", Some(*run), format!("trial of {branch}: run {run} is green"), Exit::Ok),
        Trial::Red { run, reason } => (
            "red",
            Some(*run),
            format!("trial of {branch}: run {run} is red: {reason}"),
            Exit::Findings,
        ),
        Trial::Running { run } => (
            "running",
            Some(*run),
            format!("trial of {branch}: run {run} is still running (not a pass)"),
            Exit::NotMeasured,
        ),
        Trial::NoRun => (
            "no-run",
            None,
            format!("trial of {branch}: no trial run of this head (run `./dev merge sync-member <train>` after merging the train)"),
            Exit::NotMeasured,
        ),
        Trial::NotMeasured(why) => (
            "not-measured",
            None,
            format!("trial of {branch}: not measured: {why}"),
            Exit::NotMeasured,
        ),
    };
    Report::new(serde_json::json!({"state": state, "run": run, "branch": branch}), text).with_exit(exit)
}

/// Arguments of `ci fix-items`.
#[derive(Args)]
pub struct FixItemsArgs {
    /// The workflow file whose runs are read (`ci.yml`).
    #[arg(long)]
    workflow: String,
    /// The full 40-hex commit id that landed.
    #[arg(long)]
    sha: String,
    /// The checkout whose docs/lessons.md is written (default: the current folder).
    #[arg(long)]
    root: Option<std::path::PathBuf>,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `ci triage`.
#[derive(Args)]
pub struct TriageArgs {
    /// The finished CI run to triage (an id from the run's URL).
    run: u64,
    /// Compare with this run instead of the last finished main run of the same workflow.
    #[arg(long)]
    against: Option<u64>,
    /// The app whose flake history is read (default: the workflow's own app and the `app <name>` jobs of the run).
    #[arg(long)]
    app: Option<String>,
    /// The window of the flake history in days.
    #[arg(long, default_value_t = 14, value_parser = clap::value_parser!(u32).range(1..=366))]
    days: u32,
    /// Skip the flake history (faster: it reads the failed job logs of the window).
    #[arg(long)]
    no_history: bool,
    /// Also blame the members of a train built by hand: the first-parent merges between this base (for example `origin/main`) and
    /// the commit the run tested, each with the planner's answer for its own diff (ADR 0098). Read-only; run it in the
    /// integrator's checkout, which holds the train.
    #[arg(long, value_name = "BASE")]
    members: Option<String>,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `ci schedule-skip`. The event, server and repository default to the job's (`GITHUB_EVENT_NAME`,
/// `GITHUB_SERVER_URL`, `GITHUB_REPOSITORY`).
#[derive(Args)]
pub struct ScheduleSkipArgs {
    /// The event of the run (default: GITHUB_EVENT_NAME); only `schedule` is ever skipped.
    #[arg(long)]
    event: Option<String>,
    /// The repository (default: GITHUB_REPOSITORY, then decryp7/paddock).
    #[arg(long)]
    repo: Option<String>,
    /// The Gitea server (default: GITHUB_SERVER_URL, then the family's).
    #[arg(long)]
    server: Option<String>,
    /// The workflow file being decided (`nightly.yml`): its own scheduled runs bound the skip.
    #[arg(long)]
    workflow: String,
    /// Never skip when the workflow's last measured scheduled run is older than this (36 h: one nightly at most; a weekly
    /// workflow passes a week and a day). Whatever the limit, a workflow is never skipped twice in a row.
    #[arg(long, default_value_t = 36.0)]
    max_gap_hours: f64,
    /// The app whose own workflow this is (`debrief` for debrief.yml): a scheduled run ends early while the app is not in
    /// docs/ci-active-apps.toml's `active`. The nightly names none: it runs every app.
    #[arg(long)]
    app: Option<String>,
}

/// `ci schedule-skip`: `skip=true|false` to `GITHUB_OUTPUT` (standard output without it), the annotation on standard output,
/// the reason in the job summary. Exit 0 whatever the API says (a scheduled test run fails open).
pub fn schedule_skip(a: &ScheduleSkipArgs) -> Result<Report, CliError> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let event = a.event.clone().or_else(|| env("GITHUB_EVENT_NAME")).unwrap_or_default();
    let server = a
        .server
        .clone()
        .or_else(|| env("GITHUB_SERVER_URL"))
        .unwrap_or_else(|| SERVER.into());
    let repo = a
        .repo
        .clone()
        .or_else(|| env("GITHUB_REPOSITORY"))
        .unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").or_else(|| env("GITEA_TOKEN")).unwrap_or_default();
    let now_ms = time::now_secs() as f64 * 1000.0;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &server, &repo, &token);
    // The checkout's own set (the run's commit): the job's workspace, else the current folder.
    let root = env("GITHUB_WORKSPACE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let set = crate::active_apps::ActiveApps::load(&root);
    if let (Some(app), Err(e)) = (a.app.as_deref(), &set)
        && event == "schedule"
    {
        println!("::warning::schedule skip: whether {app} is active cannot be read ({e}); this scheduled run proceeds (fail open)");
    }
    let d = match schedule_skip::inactive(&event, a.app.as_deref(), set) {
        Some(d) => d,
        None => schedule_skip::decide_scheduled(&mut client, &event, &a.workflow, now_ms, a.max_gap_hours),
    };
    let answer = format!("skip={}", d.skip());
    let mut text: Vec<String> = d.annotation().into_iter().collect();
    match env("GITHUB_OUTPUT") {
        Some(path) => append(&path, &format!("{answer}\n"))?,
        None => text.push(answer),
    }
    if let Some(path) = env("GITHUB_STEP_SUMMARY") {
        append(&path, &d.summary())?;
    }
    Ok(Report::new(
        serde_json::json!({ "skip": d.skip(), "summary": d.summary() }),
        text.join("\n"),
    ))
}

/// Arguments of `ci scope`: the scope line of an app's own workflow (ADR 0113).
#[derive(Args)]
pub struct ScopeArgs {
    /// The workflow file whose plan job declares the scope (`debrief.yml`, `dyno-release.yml`).
    #[arg(long)]
    workflow: String,
    /// The app the workflow gates.
    #[arg(long)]
    app: String,
    /// Whether the run has the workflow's full scope (`true` or `false`: a tag, a push to main, a dispatch with its full input).
    #[arg(long)]
    full: String,
    /// Whether the plan ran its jobs at all (`true` or `false`; a schedule skipped for a train or an unchanged main ran none).
    #[arg(long, default_value = "true")]
    run: String,
    /// The `since_run` input of a rerun of failures (empty: not a rerun).
    #[arg(long, default_value = "")]
    since_run: String,
    /// Whether the run is OS-scoped (`true` or `false`): its Windows and macOS tests covered only the packages with OS-specific code
    /// (`hostTests`), so a tag cannot rest on it (`ci verdict --tag`).
    #[arg(long, default_value = "false")]
    os_scoped: String,
    /// The engines a full run tests, comma separated (a run that is not full tests the first only).
    #[arg(long, default_value = "chromium")]
    full_engines: String,
}

/// `ci scope`: prints the scope line (`plan scope: {json}`) of an app's own workflow; the one reader (`ci verdict`) compares it
/// with a push to main's. A value other than `true` or `false` is a usage error, never a default (ER-33).
pub fn scope(a: &ScopeArgs) -> Result<Report, CliError> {
    let flag = |name: &str, v: &str| match v {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(CliError::usage(format!("--{name} {other:?} is not `true` or `false`."))
            .with_hint("Pass the workflow's own expression for it.")),
    };
    let engines: Vec<String> = a
        .full_engines
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect();
    if engines.is_empty() {
        return Err(CliError::usage("--full-engines names no engine.").with_hint("Pass `chromium` or `chromium,webkit`."));
    }
    let d = crate::run_scope::app_scope(
        &a.workflow,
        &a.app,
        flag("full", &a.full)?,
        flag("run", &a.run)?,
        !a.since_run.trim().is_empty(),
        flag("os-scoped", &a.os_scoped)?,
        &engines,
    );
    // The active apps (crate::active_apps): an inactive app's run names it, so its verdict never reads as the app's (`--app`).
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let set = crate::active_apps::ActiveApps::load(&root).map_err(CliError::failure)?;
    let env = |k: &str| std::env::var(k).ok();
    let tag = env("GITHUB_REF").is_some_and(|r| r.starts_with("refs/tags/"));
    let d = if !crate::active_apps::every_app(&env) && !tag && !set.active.contains(&a.app) {
        d.with_inactive([a.app.clone()].into())
    } else {
        d
    };
    Ok(Report::new(serde_json::json!({ "complete": d.complete().is_ok() }), d.line()))
}

/// Arguments of `ci scope-record`: ER-41 for ADR 0113.
#[derive(Args)]
pub struct ScopeRecordArgs {
    /// The workflow file whose run is recorded (`ci.yml`, `debrief.yml`, or one release workflow for the plan action).
    #[arg(long)]
    workflow: String,
    /// The run: a finished, green run whose plan job declared a complete scope.
    #[arg(long)]
    run: u64,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci scope-record`: records the first green scope run of a producer in `docs/ci-scope-first-runs.toml` (of the current repository)
/// and so switches `ci verdict` from the old line to the declared scope for that producer, in the one step (ER-41). Refuses a run
/// that is not a first green, complete, ran scope, and a producer already recorded. The token is read from `GITEA_TOKEN`.
pub fn scope_record(a: &ScopeRecordArgs) -> Result<Report, CliError> {
    let root = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let file = root.join(crate::run_scope::REGISTER);
    let text = std::fs::read_to_string(&file)
        .map_err(|e| CliError::failure(format!("{} cannot be read: {e}.", file.display())).with_hint("Run it from the repository root."))?;
    let producer = crate::run_scope::producer_of(&a.workflow);
    let updated = crate::run_scope::with_first_run(&text, producer, a.run).map_err(CliError::failure)?;
    let token = token()?;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
    let checked = verdict::first_green_scope(&mut client, &a.workflow, a.run).map_err(|why| {
        CliError::failure(format!("run {} of {} cannot be recorded: {why}.", a.run, a.workflow))
            .with_hint("Record a finished green run whose plan job printed a complete `plan scope:` line.")
    })?;
    std::fs::write(&file, updated).map_err(|e| CliError::failure(format!("{} cannot be written: {e}.", file.display())))?;
    let text = format!(
        "recorded {producer} at run {} in {}: {checked}. The reader now decides on the declared scope for it; delete the producer's old `plan verdict:` block in the same commit.",
        a.run,
        crate::run_scope::REGISTER
    );
    Ok(Report::new(serde_json::json!({ "producer": producer, "run": a.run }), text))
}

/// Arguments of `ci reuse` (the server, repository and token come from the CI environment: `GITHUB_SERVER_URL`,
/// `GITHUB_REPOSITORY`, `PLAN_TOKEN`).
#[derive(Args)]
pub struct ReuseArgs {
    /// The workflow file being decided (`ci.yml`, `debrief.yml`).
    #[arg(long)]
    workflow: String,
    /// The commit (default: GITHUB_SHA), the whole 40-hex id.
    #[arg(long)]
    sha: Option<String>,
    /// The asking run, left out of the runs read (default: GITHUB_RUN_ID).
    #[arg(long)]
    run_id: Option<u64>,
    /// The repository (default: GITHUB_REPOSITORY, then decryp7/paddock).
    #[arg(long)]
    repo: Option<String>,
    /// The Gitea server (default: GITHUB_SERVER_URL, then the family's).
    #[arg(long)]
    server: Option<String>,
    /// Main's tip before this push, the commit the push moved it from (default: the push event's `before`, from GITHUB_EVENT_PATH).
    #[arg(long)]
    before: Option<String>,
}

/// `ci reuse`: `reuse=true|false` and `run=<id>` to `GITHUB_OUTPUT` (standard output without it), one annotation, a summary line.
/// Exit 0 whatever the API says: it fails open to running the commit (ER-49 skips only on proof).
pub fn reuse(a: &ReuseArgs) -> Result<Report, CliError> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let sha = a.sha.clone().or_else(|| env("GITHUB_SHA")).unwrap_or_default();
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(CliError::usage(format!("the commit {sha:?} is not a full 40-hex id."))
            .with_hint("Pass --sha, or run it in a CI job (GITHUB_SHA)."));
    }
    let server = a
        .server
        .clone()
        .or_else(|| env("GITHUB_SERVER_URL"))
        .unwrap_or_else(|| SERVER.into());
    let repo = a
        .repo
        .clone()
        .or_else(|| env("GITHUB_REPOSITORY"))
        .unwrap_or_else(|| "decryp7/paddock".into());
    let current = a.run_id.or_else(|| env("GITHUB_RUN_ID").and_then(|v| v.parse().ok()));
    let token = env("PLAN_TOKEN").or_else(|| env("GITEA_TOKEN")).unwrap_or_default();
    let d = if token.is_empty() {
        reuse::Reuse::Run("no token to read the Actions runs (PLAN_TOKEN, GITEA_TOKEN)".into())
    } else {
        let mut http = CurlHttp;
        let mut client = Client::new(&mut http, &server, &repo, &token);
        let before = a.before.clone().or_else(|| green::push_before(&env));
        reuse::decide(&mut client, &a.workflow, &sha, current, before.as_deref())
    };
    let run = if let reuse::Reuse::Skip { run, .. } = &d {
        run.to_string()
    } else {
        String::new()
    };
    let answer = format!("reuse={}\nrun={run}", d.skip());
    let mut text = vec![d.annotation(&a.workflow, &sha)];
    match env("GITHUB_OUTPUT") {
        Some(path) => append(&path, &format!("{answer}\n"))?,
        None => text.push(answer),
    }
    if let Some(path) = env("GITHUB_STEP_SUMMARY") {
        append(&path, &d.summary(&a.workflow))?;
    }
    Ok(Report::new(serde_json::json!({ "reuse": d.skip(), "run": run }), text.join("\n")))
}

fn append(path: &str, text: &str) -> Result<(), CliError> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(|e| CliError::failure(format!("{path} cannot be written: {e}.")))
}

/// Arguments of `ci record-fixture`.
#[derive(Args)]
pub struct RecordArgs {
    /// The finished runs to record (ids from the runs' URLs).
    #[arg(required = true)]
    runs: Vec<u64>,
    /// The folder the files are written to (create it under the test folder of the tool that reads them).
    #[arg(long)]
    out: PathBuf,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// Arguments of `ci cancel`.
#[derive(Args)]
pub struct CancelArgs {
    /// The run to cancel: the id `./dev ci dispatch` printed (`run <id>`). Never "the newest run".
    run: u64,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci cancel`: stops run `--run` through its own cancel URL ([`cancel::cancel_run`]). Exit 0 sent or already finished, 6 when it could
/// not be read or sent (never a silent pass: the run may still be running).
pub fn cancel_cmd(a: &CancelArgs) -> Result<Report, CliError> {
    let token = token()?;
    let mut http = CurlHttp;
    match cancel::cancel_run(&mut http, &a.server, &a.repo, &token, a.run) {
        Ok(done) => Ok(Report::new(
            serde_json::json!({ "run": a.run, "sent": matches!(done, cancel::Cancelled::Sent { .. }) }),
            done.line(),
        )),
        Err(e) => Err(CliError::new(
            Exit::NotMeasured,
            format!(
                "::error::ci cancel: run {}: {e}. Next step: read the run on the server and cancel it by hand if it still runs.",
                a.run
            ),
        )),
    }
}

/// Arguments of `ci landing-held`.
#[derive(Args)]
pub struct LandingHeldArgs {
    /// The full 40-hex commit that is to land.
    #[arg(long)]
    sha: String,
    /// The workflow whose complete green run is needed.
    #[arg(long, default_value = "ci.yml")]
    workflow: String,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci landing-held`: ER-49's landing needs BOTH the complete green CI run ([`verdict::verdict_landing_at`]) and the green local slow rows
/// (`./dev check --pre-push --member-gate --set slow`, [`crate::pre_push::train_split`]) on the same commit. Exit 0 landable, 5 refused
/// (CI red or a slow row failed), 6 not held (anything unfinished, missing or unread: never a pass, ER-33).
pub fn landing_held(a: &LandingHeldArgs) -> Result<Report, CliError> {
    use crate::pre_push::train_split::{Ci, Landing, SlowState, landing, read};
    if a.sha.len() != 40 || !a.sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(CliError::usage(format!("--sha {:?} is not a full 40-hex commit id.", a.sha))
            .with_hint("Pass the whole sha (`git rev-parse HEAD`)."));
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = find_root(&cwd).unwrap_or(cwd);
    let ci = match token() {
        Err(_) => Ci::NotMeasured("GITEA_TOKEN is not set, so the runs cannot be read".into()),
        Ok(token) => {
            let mut http = CurlHttp;
            let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
            match verdict::verdict_landing_at(&mut client, &a.workflow, &a.sha, &root) {
                verdict::Verdict::Green { .. } | verdict::Verdict::GreenWithFixItems { .. } => Ci::Green,
                verdict::Verdict::Red { reason, .. } => Ci::Red(reason),
                verdict::Verdict::NotMeasured(why) => Ci::NotMeasured(why),
            }
        }
    };
    let slow: SlowState = read(&crate::pre_push::state_dir(&root), &a.sha);
    let (exit, text) = match landing(&ci, &slow) {
        Landing::Landable => (
            Exit::Ok,
            format!("{} may land: the complete green CI run and the green slow rows hold on it", a.sha),
        ),
        Landing::Refused(why) => (Exit::Findings, format!("{} may not land: {why}", a.sha)),
        Landing::NotHeld(why) => (Exit::NotMeasured, format!("{} is not held for landing: {why}", a.sha)),
    };
    Ok(Report::new(serde_json::json!({ "sha": a.sha, "landable": exit == Exit::Ok }), text).with_exit(exit))
}

/// Cancels run `id` of the family's server (the pre-push slow set calls this when a slow row fails): the line to print, or why not.
pub fn cancel_by_id(id: u64) -> Result<String, String> {
    let token = token().map_err(|e| e.to_string())?;
    let mut http = CurlHttp;
    cancel::cancel_run(&mut http, SERVER, "decryp7/paddock", &token, id).map(|d| d.line())
}

fn token() -> Result<String, CliError> {
    token_from(std::env::var("GITEA_TOKEN").ok())
}

fn token_from(value: Option<String>) -> Result<String, CliError> {
    value.filter(|t| !t.is_empty()).ok_or_else(|| {
        CliError::usage("GITEA_TOKEN is not set.").with_hint("Run `source ~/.zshenv`, or export a read token for the Gitea API.")
    })
}

/// The folder containing `.git`, from `start` upwards.
fn find_root(start: &std::path::Path) -> Option<PathBuf> {
    start.ancestors().find(|d| d.join(".git").exists()).map(PathBuf::from)
}

/// The verdict of `workflow` on `sha` from the family's server: the one reader (`ci verdict` and the pre-push shortcut). Without a
/// token it is NotMeasured, never green.
pub fn ci_verdict(workflow: &str, sha: &str) -> verdict::Verdict {
    match token() {
        Err(_) => verdict::Verdict::NotMeasured("GITEA_TOKEN is not set, so the runs cannot be read".into()),
        Ok(token) => {
            let mut http = CurlHttp;
            let mut client = Client::new(&mut http, SERVER, "decryp7/paddock", &token);
            verdict::verdict(&mut client, workflow, sha)
        }
    }
}

/// `ci verdict`: is there a full green run of the workflow on the commit (exit 0), a red or only partial one (5), or is it not
/// measured (6; also without a token)? The token is read from `GITEA_TOKEN`.
pub fn verdict(a: &VerdictArgs) -> Result<Report, CliError> {
    if a.sha.len() != 40 || !a.sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(CliError::usage(format!("--sha {:?} is not a full 40-hex commit id.", a.sha))
            .with_hint("Pass the whole sha (`git rev-parse HEAD`)."));
    }
    let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let v = match token() {
        Err(_) => verdict::Verdict::NotMeasured("GITEA_TOKEN is not set, so the runs cannot be read".into()),
        Ok(token) => {
            let mut http = CurlHttp;
            let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
            let v = if a.tag {
                verdict::verdict_tag(&mut client, &a.workflow, &a.sha)
            } else if a.landing {
                verdict::verdict_landing_at(&mut client, &a.workflow, &a.sha, &root)
            } else {
                verdict::verdict(&mut client, &a.workflow, &a.sha)
            };
            match &a.app {
                Some(app) => verdict::for_app(&mut client, v, app),
                None => v,
            }
        }
    };
    let r = verdict::report_for(&v, &a.workflow, &a.sha, &crate::pre_push::outcome::GitRepo(&root), a.landing);
    if r.exit != Exit::Ok {
        eprintln!("::error::ci verdict: {}", r.text);
    } else if matches!(v, verdict::Verdict::GreenWithFixItems { .. }) {
        eprintln!("::notice::ci verdict: {}", r.text);
    }
    let mut lines = green::take_notices();
    lines.push(r.text.clone());
    green::write_summary(
        &format!("ci verdict: {} on {}", a.workflow, a.sha.get(..10).unwrap_or(&a.sha)),
        &lines,
    );
    Ok(r)
}

/// `ci fix-items`: records the fix items of the landing verdict ([`verdict::verdict_landing_at`], the reader `ci landing-held` and the
/// queue use: a landed commit is decided by a landing run, never a complete one) on `--sha` in `docs/lessons.md` ([`fix_items::record`]). A plain green has none; a
/// red or partial verdict or one that is not measured is reported with its exit code and writes nothing.
pub fn fix_items_cmd(a: &FixItemsArgs) -> Result<Report, CliError> {
    if a.sha.len() != 40 || !a.sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(CliError::usage(format!("--sha {:?} is not a full 40-hex commit id.", a.sha))
            .with_hint("Pass the whole sha (`git rev-parse HEAD`)."));
    }
    let root = a
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")));
    let v = match token() {
        Err(_) => verdict::Verdict::NotMeasured("GITEA_TOKEN is not set, so the runs cannot be read".into()),
        Ok(token) => {
            let mut http = CurlHttp;
            let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
            verdict::verdict_landing_at(&mut client, &a.workflow, &a.sha, &root)
        }
    };
    match v {
        verdict::Verdict::GreenWithFixItems { run, items } => {
            let done = fix_items::record(&root, &a.workflow, run, &a.sha, &items).map_err(|e| {
                CliError::failure(format!("the fix items of run {run} could not be recorded: {e}"))
                    .with_hint("Run it in a checkout of the repository (--root).")
            })?;
            let text = match done {
                fix_items::Recorded::Added => format!("{} fix item(s) of run {run} added to {}", items.len(), fix_items::LESSONS),
                fix_items::Recorded::Already => {
                    format!("the {} fix item(s) of run {run} are in {} already", items.len(), fix_items::LESSONS)
                }
            };
            Ok(Report::new(
                serde_json::json!({"run": run, "items": items.len(), "recorded": done == fix_items::Recorded::Added}),
                text,
            ))
        }
        verdict::Verdict::Green { run } => Ok(Report::new(
            serde_json::json!({"run": run, "items": 0}),
            format!("run {run} is plain green: no fix items"),
        )),
        verdict::Verdict::Red { reason, .. } => {
            Ok(Report::new(serde_json::json!({"verdict": "red"}), format!("not recorded: {reason}")).with_exit(Exit::Findings))
        }
        verdict::Verdict::NotMeasured(why) => {
            Ok(Report::new(serde_json::json!({"verdict": "not_measured"}), format!("not recorded: {why}")).with_exit(Exit::NotMeasured))
        }
    }
}

/// `ci cache-audit`: the cache keys of the last runs that were never saved (ADR 0115). Report-only unless `--fail-on-never-saved`.
pub fn cache_audit(a: &CacheAuditArgs) -> Result<Report, CliError> {
    let token = token()?;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
    let w = cache_audit::read(&mut client, &a.workflow, a.runs as usize).map_err(|e| {
        CliError::failure(format!("the runs of {} could not be read: {e}", a.workflow))
            .with_hint("Check GITEA_TOKEN and the server, then run it again.")
    })?;
    let verdict = cache_audit::judge(&w, a.min_misses as usize);
    let mut text = format!(
        "{} run(s), {} job log(s) read, {} cache report(s)",
        w.runs.len(),
        w.logs_read,
        w.seen.len()
    );
    for u in &w.unreadable {
        text.push_str(&format!("\n::warning::ci cache-audit: {u} (not read, so not counted)"));
    }
    let (kind, exit) = match &verdict {
        cache_audit::Verdict::Clean => ("clean", Exit::Ok),
        cache_audit::Verdict::Broken(why) => {
            text.push_str(&format!(
                "\n::error::ci cache-audit: check broken: {why}. Next step: run one job's log through `grep cache-report:`."
            ));
            ("broken", Exit::NotMeasured)
        }
        cache_audit::Verdict::NeverSaved(lines) => {
            for l in lines {
                text.push_str(&format!(
                    "\n::{}::ci cache-audit: {l}. Next step: read that cache's restore and save steps in a job log (cache server, key, post step).",
                    if a.fail_on_never_saved { "error" } else { "warning" }
                ));
            }
            ("never_saved", if a.fail_on_never_saved { Exit::Findings } else { Exit::Ok })
        }
    };
    let data = serde_json::json!({"verdict": kind, "runs": w.runs, "logs_read": w.logs_read, "reports": w.seen.len(), "unreadable": w.unreadable.len()});
    Ok(Report::new(data, text).with_exit(exit))
}

/// `ci runner-wait`: waits (bounded) until no older run holds this run's single-machine runner. Exit 0 when free or when the
/// runs cannot be read (fail open, with a warning); a failure naming the busy run at `--max`.
pub fn runner_wait(a: &RunnerWaitArgs) -> Result<Report, CliError> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let hint = "Run `./dev ci runner-wait --help`.";
    let own = a
        .run_id
        .or_else(|| env("GITHUB_RUN_ID").and_then(|v| v.parse().ok()))
        .ok_or_else(|| CliError::usage("no run id: pass --run-id or run it in a CI job (GITHUB_RUN_ID).").with_hint(hint))?;
    let max = crate::wait::parse_duration(&a.max).map_err(|e| CliError::usage(e).with_hint(hint))?;
    if max.is_zero() || max > crate::wait::MAX_LIMIT {
        return Err(CliError::usage(format!("--max {} is out of range: use 1s to 24h.", a.max)).with_hint(hint));
    }
    let interval = match &a.interval {
        Some(i) => crate::wait::parse_duration(i).map_err(|e| CliError::usage(e).with_hint(hint))?,
        None => std::time::Duration::from_secs(30),
    }
    .max(crate::wait::MIN_INTERVAL);
    let server = a
        .server
        .clone()
        .or_else(|| env("GITHUB_SERVER_URL"))
        .unwrap_or_else(|| SERVER.into());
    let repo = a
        .repo
        .clone()
        .or_else(|| env("GITHUB_REPOSITORY"))
        .unwrap_or_else(|| "decryp7/paddock".into());
    let token = env("PLAN_TOKEN").or_else(|| env("GITEA_TOKEN")).unwrap_or_default();
    let root = a
        .root
        .clone()
        .or_else(|| std::env::current_dir().ok().and_then(|d| find_root(&d)))
        .ok_or_else(|| CliError::usage("not inside a repository checkout.").with_hint("Pass --root <checkout>."))?;
    let read = |name: &str| std::fs::read_to_string(root.join(".gitea/workflows").join(name)).ok();
    let mut http = CurlHttp;
    let mut clock = dispatch::system_clock();
    let target = dispatch::Target {
        server: &server,
        repo: &repo,
        token: &token,
    };
    let r = dispatch::runner_wait(
        &mut http,
        &target,
        own,
        max,
        interval,
        &read,
        &mut clock,
        &mut std::io::stdout(),
        500,
    )
    .map_err(|e| CliError::failure(e).with_hint("Check the workflow file and the job token's read access to Actions runs."))?;
    match r {
        dispatch::RunnerWait::Free { waited } => Ok(Report::new(
            serde_json::json!({"waitedMs": waited.as_millis() as u64}),
            String::new(),
        )),
        dispatch::RunnerWait::Unreadable(e) => Ok(Report::new(serde_json::json!({"waited": false, "unreadable": e}), String::new())),
        dispatch::RunnerWait::StillBusy { last } => Err(CliError::failure(format!("the runner stayed busy for --max ({last})."))
            .with_hint(
                "Wait for that run with `./dev wait --ci-run <id> --max <duration>` or cancel it if it is stale, then rerun this run.",
            )),
    }
}

/// `ci dispatch`: waits while a run shares the workflow's single-machine runner class, then dispatches.
pub fn dispatch(a: &DispatchArgs) -> Result<Report, CliError> {
    let hint = "Run `./dev ci dispatch --help`.";
    let token =
        token_from(std::env::var("GITEA_TOKEN").ok()).map_err(|e| e.with_hint("The dispatch needs a token that can write Actions."))?;
    let root = a
        .root
        .clone()
        .or_else(|| std::env::current_dir().ok().and_then(|d| find_root(&d)))
        .ok_or_else(|| CliError::usage("not inside a repository checkout.").with_hint("Pass --root <checkout>."))?;
    let read = |name: &str| std::fs::read_to_string(root.join(".gitea/workflows").join(name)).ok();
    let mut inputs = a.inputs.clone();
    let mut git_ref = a.git_ref.clone().unwrap_or_default();
    let mut trial = None;
    // The train always goes first (2026-11-16): while a train's CI sequence holds its reservation, a run that is not a train's
    // (a trial, a member, a manual run) waits, at most --max, before anything is pushed. A `train/*` dispatch never waits here.
    // It runs after the arguments are parsed: bad arguments are refused at once, wherever the tool runs (run 8208).
    let waits = a.trial.is_some() || !dispatch::is_train(&git_ref);
    let wait_for_trains = || -> Result<(), CliError> {
        let max = crate::wait::parse_duration(&a.max).map_err(|e| CliError::usage(e).with_hint(hint))?;
        match reservation::wait_real(max, &mut std::io::stderr()) {
            Ok(reservation::Free::Yes { .. }) => Ok(()),
            Ok(reservation::Free::StillHeld { holder }) => Err(CliError::failure(format!(
                "not dispatched: a train's CI sequence holds the runners ({holder})."
            ))
            .with_hint("Wait for the train's sequence to end, then dispatch again; the train always goes first.")),
            Err(e) => Err(CliError::failure(e).with_hint("Set PADDOCK_TRAIN_CI_LOCK_ROOT, or check `./dev places`.")),
        }
    };
    if let Some(branch) = &a.trial {
        // Everything that can be refused is refused before `trial/<branch>` is pushed: the branch name, the inputs, --max and
        // --interval (parsed with a stand-in ref and base), and the workflow (readable, its runner classes known).
        if !trial::valid_branch(branch) {
            return Err(CliError::usage(format!(
                "`{branch}` is not a branch name --trial accepts (letters, digits, . _ - and /)."
            ))
            .with_hint(hint));
        }
        let mut probe_inputs = inputs.clone();
        probe_inputs.push(format!("base={}", "0".repeat(40)));
        dispatch::parse_request(
            &a.workflow,
            &format!("trial/{branch}"),
            &probe_inputs,
            &a.max,
            a.interval.as_deref(),
        )
        .map_err(|e| CliError::usage(e).with_hint(hint))?;
        dispatch::classes_of(&a.workflow, &read).map_err(|e| CliError::usage(e).with_hint("Check the workflow name and --root."))?;
        wait_for_trains()?;
        let (g, trial_ref, base) = trial_merge(a, branch, &token)?;
        git_ref = trial_ref.clone();
        inputs.push(format!("base={base}"));
        trial = Some((g, trial_ref));
    }
    // A full train dispatch of ci.yml tests the train's diff from main's last complete commit (ADR 0117 WP-8, G1): the base is asked
    // before the request is built. Everything else (a member, a rerun, a tag, another workflow) is left as given.
    if trial.is_none() {
        let target = dispatch::Target {
            server: &a.server,
            repo: &a.repo,
            token: &token,
        };
        let pairs: Vec<(String, String)> = inputs
            .iter()
            .filter_map(|i| i.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect();
        let with = landing::with_real_landing_base(&mut CurlHttp, &target, &root, &a.workflow, &git_ref, pairs.clone());
        inputs.extend(with[pairs.len()..].iter().map(|(k, v)| format!("{k}={v}")));
    }
    let mut req = dispatch::parse_request(&a.workflow, &git_ref, &inputs, &a.max, a.interval.as_deref())
        .map_err(|e| CliError::usage(e).with_hint(hint))?;
    for (flag, reason) in [
        ("--full-anyway", &a.full_anyway),
        ("--red-main-anyway", &a.red_main_anyway),
        ("--audits-anyway", &a.audits_anyway),
    ] {
        if reason.as_deref().is_some_and(|r| r.trim().is_empty()) {
            return Err(
                CliError::usage(format!("{flag} needs a reason.")).with_hint("Say why, for example --full-anyway \"test renamed\".")
            );
        }
    }
    // The train guards (failed-first, green-main): before anything waits or is sent.
    let guards = if trial.is_none() && dispatch::is_train(&req.git_ref) {
        train_guards(a, &root, &read, &token, &req)?
    } else {
        Vec::new()
    };
    // Guard A6 (ER-51): the run is found by the token this dispatch sends, never as "the newest run".
    let (with_id, dispatch_id) = dispatch::with_dispatch_id(&req.workflow, &read, std::mem::take(&mut req.inputs));
    req.inputs = with_id;
    if waits && trial.is_none() {
        wait_for_trains()?;
    }
    // The Linux slot count is declared once (conformance/ci/runner-slots.json); without it a train dispatch waits here.
    match dispatch::train_waiters(&root) {
        Ok(n) => req.train_waiters = n,
        Err(e) => eprintln!("::warning::dispatch: {e}; a train run waits here, not in its first job."),
    }
    let mut http = CurlHttp;
    let mut clock = dispatch::system_clock();
    let mut reach = crate::health::CurlReach;
    let mut health = crate::health::Health::new(crate::health::real_services(&a.server, "dispatch", false), &mut reach);
    // ER-51: one of the two callers of the raw dispatch; the run it starts is then found by its dispatch token (find_run below).
    #[expect(
        clippy::disallowed_methods,
        reason = "`./dev ci dispatch` holds the run it starts: find_run by the dispatch token"
    )]
    let result = dispatch::run(
        &mut http,
        &dispatch::Target {
            server: &a.server,
            repo: &a.repo,
            token: &token,
        },
        &req,
        &read,
        &mut clock,
        &mut std::io::stderr(),
        Some(&mut health),
    );
    // A trial that was not dispatched leaves no `trial/<branch>` behind (best effort; `ci trial-prune` catches the rest).
    if let Some((g, trial_ref)) = &trial
        && !matches!(result, Ok(dispatch::Dispatched::Yes { .. }))
        && let Err(e) = trial::delete(g, trial_ref)
    {
        eprintln!("::warning::trial: {trial_ref} could not be deleted after the dispatch failed ({e}); run `./dev ci trial-prune`");
    }
    match result {
        Ok(dispatch::Dispatched::Yes { waited }) => {
            let sent = format!(
                "Dispatched {} on {} after waiting {}.",
                req.workflow,
                req.git_ref,
                crate::wait::fmt_duration(waited)
            );
            let Some(id) = dispatch_id else {
                eprintln!(
                    "::warning::dispatch: {} declares no {} input, so its run cannot be found by a token; add the input and its run-name line (guard A6)",
                    req.workflow,
                    dispatch::DISPATCH_ID
                );
                return Ok(Report::new(
                    serde_json::json!({"dispatched": true, "waitedMs": waited.as_millis() as u64, "run": null, "guards": guards}),
                    sent,
                ));
            };
            let target = dispatch::Target {
                server: &a.server,
                repo: &a.repo,
                token: &token,
            };
            let found = dispatch::find_run(
                &mut http,
                &target,
                &req.workflow,
                &id,
                &mut clock,
                dispatch::FIND_MAX,
                dispatch::FIND_INTERVAL,
                &mut std::io::stderr(),
            );
            match found {
                Ok(Some(run)) => Ok(Report::new(
                    serde_json::json!({"dispatched": true, "waitedMs": waited.as_millis() as u64, "run": run, "dispatchId": id, "guards": guards}),
                    format!("{sent}\nrun {run}"),
                )),
                Ok(None) | Err(_) => {
                    let why = match found {
                        Err(e) => e,
                        _ => format!("no run showed it within {}", crate::wait::fmt_duration(dispatch::FIND_MAX)),
                    };
                    eprintln!("::error::dispatch: {sent} Its run with dispatch id {id} was not found: {why}. Do not dispatch again.");
                    Err(CliError::new(
                        Exit::NotMeasured,
                        format!("dispatched, but the run with dispatch id {id} was not found ({why})."),
                    )
                    .with_hint("Do not dispatch again: find the run in Gitea's Actions list by `[dispatch <id>]` in its title."))
                }
            }
        }
        Ok(dispatch::Dispatched::ServiceDown { last }) => Err(CliError::failure(format!("not dispatched: {last}."))
            .with_hint("Wait until Gitea answers (the ::error:: line above names it), then dispatch again.")),
        Ok(dispatch::Dispatched::StillBusy { last }) => Err(CliError::failure(format!("not dispatched: the runner stayed busy ({last})."))
            .with_hint("Wait for that run with `./dev wait --ci-run <id> --max <duration>`, then dispatch again.")),
        Err(e) => {
            eprintln!("::error::dispatch: {e}");
            Err(CliError::failure(e).with_hint("Check the workflow name, GITEA_TOKEN's scope and the network."))
        }
    }
}

/// The guards of a train dispatch ([`dispatch_guards::check_train`]) with the coordinator's overrides; a blocking refusal ends the command
/// (exit 4, or 6 when the guard could not look).
fn train_guards(
    a: &DispatchArgs,
    root: &std::path::Path,
    read: dispatch::ReadWorkflow<'_>,
    token: &str,
    req: &dispatch::Request,
) -> Result<Vec<serde_json::Value>, CliError> {
    let target = dispatch::Target {
        server: &a.server,
        repo: &a.repo,
        token,
    };
    let overrides = dispatch_guards::Overrides {
        full_anyway: a.full_anyway.as_deref(),
        red_main_anyway: a.red_main_anyway.as_deref(),
        audits_anyway: a.audits_anyway.as_deref(),
    };
    dispatch_guards::check_train(
        &mut CurlHttp,
        &target,
        root,
        read,
        req,
        overrides,
        time::now_secs() as f64 * 1000.0,
        &mut std::io::stderr(),
    )
    .map_err(|s| {
        CliError::new(if s.not_measured { Exit::NotMeasured } else { Exit::Refused }, s.message)
            .with_hint("The ::error:: line above names the next step.")
    })
}

/// The trial merge of `--trial`: the ref to dispatch and the `base` input. A conflict or a merge-check finding is a failure with
/// its `::error::` line and nothing is pushed.
fn trial_merge(a: &DispatchArgs, branch: &str, token: &str) -> Result<(crate::queue::real::GitTrain, String, String), CliError> {
    use crate::queue::real::GitTrain;
    let scratch = std::env::var_os("PADDOCK_TRIAL_DIR").map(PathBuf::from).ok_or_else(|| {
        CliError::usage("PADDOCK_TRIAL_DIR is not set.")
            .with_hint("Set it to a folder on a data drive for the trial's scratch clone (not under your home).")
    })?;
    let url = format!("{}/{}.git", a.server.trim_end_matches('/'), a.repo);
    let fail = |why: String| {
        eprintln!("::error::trial: {why}");
        CliError::failure(why).with_hint("Check the branch name, GITEA_TOKEN's write scope and the network.")
    };
    let mut g = GitTrain::open(&url, &scratch, Some(token.to_string()), true, false).map_err(fail)?;
    // The trial's push holds while Gitea is down for at most this command's own --max (already validated by parse_request).
    g.push_hold = crate::wait::parse_duration(&a.max).ok();
    let check = |root: &std::path::Path, head: &str| {
        crate::merge_check::command(&crate::merge_check::Request {
            root: Some(root.to_path_buf()),
            commit: Some(head.to_string()),
            branch: None,
            target: None,
            // The trial runs no code of the branch: generator checks and the member gate need a checkout.
            generators: false,
            gates: false,
            write_reverted: None,
            old_base_anyway: None,
            no_trial: None,
            gates_base: None,
        })
        .map(|_| ())
        .map_err(|e| e.message)
    };
    match trial::prepare(&g, branch, a.onto.as_deref(), &check).map_err(fail)? {
        trial::Outcome::Ready {
            trial_ref,
            target,
            target_sha,
            head,
        } => {
            eprintln!(
                "trial: {branch} merged onto {target} ({}) as {}; pushed {trial_ref}",
                &target_sha[..target_sha.len().min(9)],
                &head[..head.len().min(9)]
            );
            Ok((g, trial_ref, target_sha))
        }
        trial::Outcome::Conflict { target, files } => {
            let why = format!(
                "{branch} conflicts with {target} in {} file(s): {}. Next step: send the branch back to its author to merge {target} and resolve them; nothing was dispatched",
                files.len(),
                files.join(", ")
            );
            Err(CliError::new(Exit::Findings, format!("::error::trial: {why}")))
        }
        trial::Outcome::Findings(text) => {
            eprintln!("::error::trial: the merge check found something in {branch} merged onto the train; nothing was dispatched");
            Err(CliError::new(Exit::Findings, text))
        }
    }
}

/// The exit of `ci triage --members`: a blame that could not read the members (a plan, a merge) is NOT MEASURED (exit 6), whatever the
/// failures said; it never leaves the run's own answer standing as if it were the blame's.
fn members_exit(current: Exit, blame_read: bool) -> Exit {
    if blame_read { current } else { Exit::NotMeasured }
}

/// Arguments of `ci trial-prune`.
#[derive(Args)]
pub struct TrialPruneArgs {
    /// List the stale trial refs; delete nothing.
    #[arg(long)]
    dry_run: bool,
    /// The repository.
    #[arg(long, default_value = "decryp7/paddock")]
    repo: String,
    /// The Gitea server.
    #[arg(long, default_value = SERVER)]
    server: String,
}

/// `ci trial-prune`: deletes the `trial/*` refs (ADR 0098 WP-4) of branches that are gone or landed on main. Needs GITEA_TOKEN with
/// write access and PADDOCK_TRIAL_DIR (the trial's scratch clone).
pub fn trial_prune(a: &TrialPruneArgs) -> Result<Report, CliError> {
    use crate::queue::real::GitTrain;
    let token = token()?;
    let scratch = std::env::var_os("PADDOCK_TRIAL_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::usage("PADDOCK_TRIAL_DIR is not set.").with_hint("Set it to the folder of the trial's scratch clone."))?;
    let url = format!("{}/{}.git", a.server.trim_end_matches('/'), a.repo);
    let fail = |why: String| {
        CliError::failure(format!("::error::trial-prune: {why}")).with_hint("Check GITEA_TOKEN's write scope and the network.")
    };
    let g = GitTrain::open(&url, &scratch, Some(token), true, false).map_err(fail)?;
    let stale = trial::prune(&g, a.dry_run).map_err(fail)?;
    let lines: Vec<String> = stale.iter().map(|s| format!("{} ({})", s.trial_ref, s.why)).collect();
    Ok(Report::new(
        serde_json::json!({"deleted": !a.dry_run, "refs": stale.iter().map(|s| &s.trial_ref).collect::<Vec<_>>()}),
        if lines.is_empty() {
            "no stale trial refs".to_string()
        } else {
            format!(
                "{} {} stale trial ref(s):\n{}",
                if a.dry_run { "would delete" } else { "deleted" },
                lines.len(),
                lines.join("\n")
            )
        },
    ))
}

/// `ci triage`: read-only; it quarantines nothing and marks nothing green (ER-12).
pub fn triage(a: &TriageArgs) -> Result<Report, CliError> {
    let token = token()?;
    let mut http = CurlHttp;
    let mut client = Client::new(&mut http, &a.server, &a.repo, &token);
    let req = triage::Request {
        run: a.run,
        against: a.against,
        app: a.app.clone(),
        days: a.days,
        history: !a.no_history,
        repo: a.repo.clone(),
        now_secs: time::now_secs(),
    };
    let report = triage::collect(&mut client, &req, &mut |l| crate::log_text::eline(l)).map_err(|e| {
        CliError::failure(format!("run {} could not be triaged: {e}", a.run))
            .with_hint("Check the network, that GITEA_TOKEN can read the repository, and that the run has finished.")
    })?;
    let (mut text, mut data, mut exit) = (report.text(), report.json(), report.exit());
    if let Some(base) = &a.members {
        // ADR 0098: blame the members of a hand-built train. Read-only; a member that cannot be read ends the blame (ER-33).
        let answer = (|| -> Result<(String, serde_json::Value), String> {
            let root = crate::gates::command::repo_root(None).map_err(|e| e.message)?;
            let git = crate::merge_check::git::Git::new(&root);
            let failures = crate::queue::real::failures_of(&report, a.run, &mut http, &a.server, &a.repo, &token);
            let plan = |first: &str, merge: &str| crate::queue::real::run_planner(&root, first, merge);
            let members = members::read(&git, base, &report.head, &plan, members::needs_files(&failures))?;
            Ok(members::report(base, &report.head, &members, &failures))
        })();
        match answer {
            Ok((t, d)) => {
                text.push_str(&t);
                data["members"] = d;
            }
            Err(why) => {
                text.push_str(&format!(
                    "::error::the members of {base}..{} could not be blamed: {why}. Next step: run this in the checkout that holds the train, with its base fetched; the failures above stand.\n",
                    report.head.chars().take(9).collect::<String>()
                ));
                data["members"] = serde_json::json!({"error": why});
                exit = members_exit(exit, false);
            }
        }
    }
    // An answer with an UNKNOWN row or a gap is not a clean one: exit 5, so a script cannot read it as "nothing to worry about".
    Ok(Report::new(data, text).with_exit(exit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::run(&args, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn a_missing_token_is_a_usage_error_that_names_the_next_step() {
        let e = token_from(None).unwrap_err();
        assert!(
            e.text().contains("GITEA_TOKEN is not set") && e.text().contains("source ~/.zshenv"),
            "{}",
            e.text()
        );
        assert!(token_from(Some(String::new())).is_err());
        assert_eq!(token_from(Some("t".into())).unwrap(), "t");
    }

    #[test]
    fn the_repository_root_is_the_folder_with_git() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a/b")).unwrap();
        assert_eq!(find_root(&d.path().join("a/b")), None);
        std::fs::create_dir(d.path().join(".git")).unwrap();
        assert_eq!(find_root(&d.path().join("a/b")).as_deref(), Some(d.path()));
    }

    #[test]
    fn verdict_needs_a_workflow_and_a_whole_sha_and_never_passes_without_a_token() {
        let (code, _, err) = run(&["ci", "verdict"]);
        assert_eq!(code, Exit::Usage.code(), "{err}");
        let (code, _, err) = run(&["ci", "verdict", "--workflow", "ci.yml", "--sha", "abc1234"]);
        assert_eq!(code, Exit::Usage.code(), "{err}");
        assert!(err.contains("40-hex"), "{err}");
        let (code, out, _) = run(&["ci", "verdict", "--help"]);
        assert_eq!(code, 0);
        for flag in ["--workflow", "--sha", "--repo", "--server"] {
            assert!(out.contains(flag), "{flag} missing in {out}");
        }
    }

    #[test]
    fn a_blame_that_could_not_read_the_members_is_not_measured_whatever_the_run_said() {
        assert_eq!(members_exit(Exit::Ok, false), Exit::NotMeasured);
        assert_eq!(members_exit(Exit::Findings, false), Exit::NotMeasured);
        assert_eq!(members_exit(Exit::Findings, true), Exit::Findings);
        assert_eq!(members_exit(Exit::Ok, true), Exit::Ok);
    }
}
