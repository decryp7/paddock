// ci-runs-reader: the one definition of green (ADR 0117 WP-4)
//! The one definition of "green" (ER-15, ADR 0117 section 6, WP-4), read by `ci verdict`, `ci reuse`, the landing queue and the
//! planner's base for a push to main.
//!
//! An outcome is a scope times a result:
//! - [`Scope`]: `Complete` when the run's declared scope covers a push to main's (ADR 0113, `Declared::complete`, read by
//!   `verdict::read_mode`) and every job it declared ran, else `Partial`. A run's base is complete by construction: the base of a
//!   push is picked by [`find_base`], which takes only complete runs, and a run with an explicit base declares less than it needs.
//! - [`GradedResult`]: `Green`; `GreenWithFixItems` when the only failures are FLAKY tests (failed, then passed on the one retry) in
//!   code the change did not touch (each a fix item, recorded in docs/lessons.md by `ci fix-items`); `Red` when a test failed both
//!   tries, a FLAKY test is in touched code or cannot be placed (fail closed), or a step that is not a test failed; `NotMeasured` when
//!   a test report cannot be read (ER-33). "Touched" is the Cargo package that holds the test, judged by the Cargo graph of what
//!   changed, a lockfile change counting for the packages whose resolved dependencies it changed ([`Touched`], [`grade_flaky`]; user
//!   decisions 2026-11-24 and 2026-11-26, decision 95: ER-12 again, the interim strict reading of 2026-11-23 is gone).
//!
//! A skipped or cancelled job is never a pass and never a failure: a declared job that did not run makes the scope `Partial`
//! (`run_scope::check_ran`), and a cancelled run is `Partial` times `NotMeasured` (ER-33); an unfinished run is no evidence.
//!
//! A whole-tree run (a nightly, a tag, a plan of `all`) declares a scope that covers what a push needs without any base, so it is
//! `Complete` whatever its base was: the "complete base" chain of a push ends there. Runs with a base declare less and are `Partial`.
//!
//! The test report is the job log: CI uploads no JUnit or Playwright JSON today, and the nextest and Playwright summaries of the
//! log are read through the one reader, [`super::logfind`] (ER-42).
//!
//! [`find_base`] reads the run list once, in pages, and matches runs to commits in memory: no call per commit (ER-20); a run's jobs
//! and logs are read only for a run that is a candidate.

use super::client::{Client, RunInfo, TimedJob};
use super::logfind;
use super::verdict::{self, Earlier, Mode};
use crate::job_outcome::JobOutcome;
use crate::plan_since::workflow_file;
use crate::run_scope::{ExpectedMap, lookup};
use paddock_ci_config::jobs::{self, PROJECT_OUTPUTS};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Pages of 50 runs the base search reads at most (all branches and events: a train's dispatch run names the commit that lands).
/// Measured 2026-11-23: 37 runs a day on average, 44 at most, so 6 pages (300 runs) reach back about a week (docs/benchmarks.md, "The base
/// search reads the run list in pages"). A base older than that plans everything.
pub const MAX_PAGES: u32 = 6;

/// First-parent commits of main the base search looks at, newest first.
pub const MAX_COMMITS: usize = 200;

/// How much of what a push to main needs the run covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The declared scope covers a push to main's and every declared job ran.
    Complete,
    /// Less than that.
    Partial,
    /// Not complete, but it ran exactly its own diff's selection (some projects their smoke set or affected tests only) and every job
    /// it declared: a landing may rest on it, never a base and never a tag (the user's decision of 2026-11-23, review round 2 P1-A).
    Landing,
}

/// A FLAKY test of a run that landed or may land: what ER-12 asks to record as a fix item, with where to read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixItem {
    /// The job's display name.
    pub job: String,
    /// The job's id (its log is `GET .../jobs/<id>/logs`).
    pub job_id: u64,
    /// The test, as nextest or Playwright names it.
    pub test: String,
    /// The job's page, from the jobs API's `html_url`.
    pub url: String,
}

impl FixItem {
    /// One line for a comment: the job, the test and the link.
    pub fn line(&self, run: u64) -> String {
        format!("run {run}, job {} (id {}): {} ({})", self.job, self.job_id, self.test, self.url)
    }
}

/// What the run's jobs say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GradedResult {
    /// Nothing failed.
    Green,
    /// Only FLAKY tests in untouched targets failed; each is a fix item.
    GreenWithFixItems(Vec<FixItem>),
    /// What to fix.
    Red(String),
    /// What could not be read.
    NotMeasured(String),
}

/// The scope and the result of one run, or of the commit it decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// How much it covered.
    pub scope: Scope,
    /// What it found.
    pub result: GradedResult,
    /// Why the scope is partial, or what was not read (empty for a complete scope that was read).
    pub note: String,
    /// Set when the run's mode could not be read: it might be a newer complete run, so it blocks the commit's decision.
    pub unreadable: bool,
    /// For a [`Scope::Landing`] run: the commit its need was diffed from (a push to main reuses it only for the same diff).
    pub landing_base: Option<String>,
    /// Set when the run is graded but not granted: the newest attempt at a full run that did less than it declared, or whose fix
    /// items were refused. It is no base, and it decides the commit anyway: a newer full run is never passed over for an older green
    /// one (ER-15, "the newest full run decides"). A smaller run (`Partial` without this) is passed over.
    pub blocking: bool,
}

impl Outcome {
    fn of(scope: Scope, result: GradedResult) -> Outcome {
        Outcome {
            scope,
            result,
            note: String::new(),
            unreadable: false,
            blocking: false,
            landing_base: None,
        }
    }
    fn blocking(mut self) -> Outcome {
        self.blocking = true;
        self
    }
    fn noted(mut self, note: impl Into<String>) -> Outcome {
        self.note = note.into();
        self
    }
}

impl Outcome {
    /// Whether the commit may be a base for planning and may land: complete, and green with or without fix items.
    pub fn is_base(&self) -> bool {
        self.scope == Scope::Complete && matches!(self.result, GradedResult::Green | GradedResult::GreenWithFixItems(_))
    }

    /// Whether a tag may be cut from it: complete and plainly green (a tag on fix items takes the one allowed retry, ER-49).
    pub fn is_tag_green(&self) -> bool {
        self.scope == Scope::Complete && self.result == GradedResult::Green
    }

    /// Whether a train may land on it: complete, or a landing run ([`Scope::Landing`]), and green with or without fix items.
    pub fn lands(&self) -> bool {
        matches!(self.scope, Scope::Complete | Scope::Landing)
            && matches!(self.result, GradedResult::Green | GradedResult::GreenWithFixItems(_))
    }
}

/// The targets a change touched, read from the plan of the change.
///
/// ER-12 (user decisions 2026-11-24 and 2026-11-26, decision 95: Google TAP's and Chromium's practice, graded by the reader). Two
/// levels, both read, the narrower one decides:
/// - the job level ([`Touched::reaches`]): a crate workspace or app the plan reached. A test of a project the plan did not reach is
///   untouched;
/// - the package level ([`Touched::reaches_test`]): the Cargo package that holds the test, the narrowest level the planner's graph has.
///   A test exercises its own package and what that package depends on, so the package is touched when it changed or depends on a
///   changed crate (the plan's `packages`, the reverse-dependency closure; a lockfile change names the packages whose resolved
///   dependencies it changed, `plan_cargo_lock`). Only a project the plan reached through Cargo alone (`cargoOnly`) narrows: a UI
///   file, an installer file or a script reaches tests no package list names, so every test of it is touched. A Playwright or vitest
///   test has no package level: its job's project decides. A test the graph cannot place (a crate that is no package of its job's
///   workspace, a workspace whose packages were not read) is [`Reach::Unknown`]: it blocks (fail closed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Touched {
    /// The project flags that were true (`crates`, `ui`, `paddockDev`...).
    pub flags: BTreeSet<String>,
    /// The apps the plan reached.
    pub apps: BTreeSet<String>,
    /// The change cannot be bounded (no base to measure it from): every target counts as reached.
    pub all: bool,
    /// The reached packages of each Cargo workspace folder (`packages` of the plan: the changed crates and their dependents).
    pub packages: BTreeMap<String, BTreeSet<String>>,
    /// The projects the plan reached through Cargo alone (`cargoOnly`: `crates`, `paddock-dev`, `app:<name>`); only these narrow.
    pub cargo_only: BTreeSet<String>,
    /// Every package of each Cargo workspace folder, from the graph; a workspace missing here cannot place a test (fail closed).
    pub members: BTreeMap<String, BTreeSet<String>>,
    /// Each app's Cargo workspace folder ([`crate::plan_ci::app_cargo_dir`]).
    pub app_cargo: BTreeMap<String, String>,
    /// The graded run's `expected` map (its scope line): what a job display name is (its job id, app and projects). Set by the grading
    /// from the run it reads; a name it does not hold is placed on the safe side (reached).
    pub expected: ExpectedMap,
}

thread_local! {
    static NOTICES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Prints one grade as a `::notice::` line and keeps it for the job summary ([`take_notices`]).
pub fn notice(line: &str) {
    eprintln!("::notice::{line}");
    NOTICES.with(|n| n.borrow_mut().push(line.to_string()));
}

/// The notices printed since the last call; a command appends them to its job summary ([`write_summary`]).
pub fn take_notices() -> Vec<String> {
    NOTICES.with(|n| std::mem::take(&mut *n.borrow_mut()))
}

/// Appends `lines` to `$GITHUB_STEP_SUMMARY` when it is set (a CI step); an unwritable file is a warning, never an error.
pub fn write_summary(heading: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY").filter(|p| !p.is_empty()) else {
        return;
    };
    let mut text = format!("\n### {heading}\n\n");
    for l in lines {
        text.push_str(&format!("- {l}\n"));
    }
    let res = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, text.as_bytes()));
    if let Err(e) = res {
        eprintln!("::warning::the job summary cannot be written ({e}); the notices above are the record");
    }
}

/// Where a FLAKY test stands against a change, with the reason in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// The change touched the test or what it exercises: a FLAKY test blocks.
    Touched(String),
    /// It did not: a FLAKY test is a fix item and does not block.
    Untouched(String),
    /// The graph cannot place the test: it blocks (fail closed).
    Unknown(String),
}

/// `(binary id, test name)` of a nextest test as the log reader names it (`paddock-chromium::it real_browser::a_test`); `None` for a
/// Playwright or vitest name (`[chromium] › e2e/x.spec.ts:3:5 › title`).
pub fn nextest_parts(test: &str) -> Option<(&str, &str)> {
    if test.contains(" › ") {
        return None;
    }
    test.split_once(' ').filter(|(b, n)| !b.is_empty() && !n.is_empty())
}

impl Touched {
    /// From a plan (`./dev plan` JSON).
    pub fn from_plan(plan: &Value) -> Touched {
        let flags = plan
            .as_object()
            .map(|o| {
                o.iter()
                    .filter(|(_, v)| v.as_bool() == Some(true))
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default();
        let strings = |v: &Value| -> BTreeSet<String> {
            v.as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default()
        };
        // `gradePackages`: the changed packages without the crates whose only change is manifest metadata (cargo ignores it); a plan
        // without it (an older recording) is read as `packages`.
        let packages = plan
            .get("gradePackages")
            .filter(|v| v.is_object())
            .unwrap_or(&plan["packages"])
            .as_object()
            .map(|o| o.iter().map(|(w, l)| (w.clone(), strings(l))).collect())
            .unwrap_or_default();
        Touched {
            flags,
            apps: strings(&plan["apps"]),
            all: false,
            packages,
            // A reason that cannot alter a test, or only the named packages' (`packageGraded`: a lockfile whose changed packages the plan
            // read, a ci.yml change that runs no test), narrows the grade like a Cargo reach.
            cargo_only: strings(&plan["cargoOnly"])
                .union(&strings(&plan["packageGraded"]))
                .cloned()
                .collect(),
            ..Touched::default()
        }
    }

    /// With the packages of every Cargo workspace and each app's workspace folder, read from the graph the plan was made on.
    pub fn with_graph(mut self, graph: &crate::plan::RepoGraph) -> Touched {
        for (_, n) in graph.cargo.iter() {
            self.members.entry(n.workspace.clone()).or_default().insert(n.name.clone());
        }
        for (app, config) in &graph.configs {
            let dir = crate::plan_ci::app_cargo_dir(config);
            if !dir.is_empty() {
                self.app_cargo.insert(app.clone(), dir);
            }
        }
        self
    }

    /// Where the FLAKY test `test` of CI job `job` stands (the package level, see [`Touched`]).
    pub fn reaches_test(&self, job: &str, test: &str) -> Reach {
        if self.all {
            return Reach::Touched("the change cannot be bounded (no good base to measure it from)".into());
        }
        if !self.reaches(job) {
            return Reach::Untouched(format!("the plan did not reach the project of job {job:?}"));
        }
        let Some((binary, _)) = nextest_parts(test) else {
            return Reach::Touched(format!(
                "{test:?} is not a nextest test: its job's project is the narrowest level, and it was reached"
            ));
        };
        let found = lookup(&self.expected, job, None);
        let leg_app = found.as_ref().and_then(|f| f.leg).and_then(|l| l.get("app")).cloned();
        let (project, workspace) = match (found.as_ref().map(|f| f.job.as_str()), leg_app) {
            (Some("crate" | "crates"), _) => ("crates".to_string(), "crates".to_string()),
            (Some("paddock-dev"), _) => ("paddock-dev".to_string(), "tools/paddock-dev".to_string()),
            (Some("app"), Some(app)) => match self.app_cargo.get(&app) {
                Some(dir) => (format!("app:{app}"), dir.clone()),
                None if self.members.is_empty() => {
                    return Reach::Unknown(format!("the Cargo workspace of app {app} was not read"));
                }
                None => return Reach::Touched(format!("app {app} has no Cargo workspace: the app is the narrowest level")),
            },
            _ => return Reach::Touched(format!("job {job:?} has no package level and its project was reached")),
        };
        let krate = binary.split("::").next().unwrap_or(binary);
        if !self.cargo_only.contains(&project) {
            return Reach::Touched(format!(
                "the plan reached {project} for a reason that is not a Cargo reach (a UI file, a lockfile, a script): every test of it"
            ));
        }
        let Some(members) = self.members.get(&workspace) else {
            return Reach::Unknown(format!("the packages of the Cargo workspace {workspace} were not read"));
        };
        if !members.contains(krate) {
            return Reach::Unknown(format!("{krate} is not a package of the Cargo workspace {workspace}"));
        }
        if self.packages.get(&workspace).is_some_and(|p| p.contains(krate)) {
            Reach::Touched(format!("package {krate} changed or depends on a changed crate"))
        } else {
            Reach::Untouched(format!(
                "package {krate} of {workspace} neither changed nor depends on a changed crate"
            ))
        }
    }

    /// Whether the change reached what a CI job tests. A job this cannot place counts as touched: the safe side is red.
    pub fn reaches(&self, job: &str) -> bool {
        if self.all {
            return true;
        }
        // A name the run's line does not hold, or a job it gives no project, cannot be placed: reached, the safe side.
        let Some(found) = lookup(&self.expected, job, None) else {
            return true;
        };
        if let Some(app) = found.leg.and_then(|l| l.get("app")) {
            return self.apps.contains(app);
        }
        let projects = match found.projects {
            Some(p) if !p.is_empty() => p,
            _ => return true,
        };
        projects.iter().any(|p| {
            self.flags.contains(p.as_str())
                || (p == jobs::APPS_PROJECT && !self.apps.is_empty())
                || PROJECT_OUTPUTS.iter().any(|(_, id, key)| id == p && self.flags.contains(*key))
        })
    }
}

/// A failed job of a finished complete run, as the grading reads it.
pub struct FailedJob {
    /// Its display name.
    pub name: String,
    /// Its id.
    pub id: u64,
    /// Its page (`html_url`).
    pub url: String,
    /// The names of its failed steps (the jobs API's step conclusions).
    pub failed_steps: Vec<String>,
    /// Its log, `None` when it could not be read.
    pub log: Option<String>,
}

/// Grades the failed jobs of a finished complete run of `workflow`. A job is test-only when every failed step is a test step
/// ([`crate::plan_since::failure_is_tests`]) and its log shows only FLAKY tests; any other failed step is red. `touched` is
/// asked at most once, and only when a FLAKY test needs it.
pub fn grade_failures(
    workflow: &str,
    jobs: &[FailedJob],
    expected: &ExpectedMap,
    touched: &mut dyn FnMut() -> Option<Touched>,
) -> GradedResult {
    use crate::plan_since::{StepProblem, failure_is_tests, job_key};
    let mut red = Vec::new();
    let mut unread = Vec::new();
    let mut items = Vec::new();
    let mut touch: Option<Option<Touched>> = None;
    for job in jobs {
        let FailedJob {
            name, failed_steps, log, ..
        } = job;
        let Some(log) = log else {
            unread.push(format!("job {name:?} has no log"));
            continue;
        };
        let f = logfind::read(log);
        if let Some(why) = f.broken {
            unread.push(format!("the test report of job {name:?} cannot be read: {why}"));
            continue;
        }
        if !f.failed.is_empty() || !f.vitest_failed.is_empty() {
            let t = f.failed.iter().chain(f.vitest_failed.iter()).next().cloned().unwrap_or_default();
            red.push(format!("job {name:?}: a test failed both tries ({t})"));
            continue;
        }
        if f.flaky.is_empty() {
            red.push(format!("job {name:?} failed in a step that is not a test"));
            continue;
        }
        // The log shows only FLAKY tests: the job is test-only only when no other step failed (ER-33).
        match job_key(log)
            .ok_or_else(|| StepProblem::Unknown(format!("the log of job {name:?} names no workflow job")))
            .and_then(|key| failure_is_tests(workflow, &key, failed_steps, log))
        {
            Ok(()) => {}
            Err(StepProblem::Other(step)) => {
                red.push(format!("job {name:?}: {step:?} failed and is not a test step or test command"));
                continue;
            }
            Err(StepProblem::Unknown(why)) => {
                unread.push(format!("job {name:?}: {why}"));
                continue;
            }
        }
        let t = touch.get_or_insert_with(|| {
            touched().map(|t| Touched {
                expected: expected.clone(),
                ..t
            })
        });
        let Some(t) = t else {
            unread.push(format!(
                "the targets the change touched cannot be read (job {name:?} has a FLAKY test)"
            ));
            continue;
        };
        for test in &f.flaky {
            match grade_flaky(t, name, test) {
                Ok(()) => items.push(FixItem {
                    job: name.clone(),
                    job_id: job.id,
                    test: test.clone(),
                    url: job.url.clone(),
                }),
                Err(why) => red.push(why),
            }
        }
    }
    if let Some(r) = red.into_iter().next() {
        GradedResult::Red(r)
    } else if let Some(u) = unread.into_iter().next() {
        GradedResult::NotMeasured(u)
    } else {
        GradedResult::GreenWithFixItems(items)
    }
}

/// The grade of one FLAKY test of job `job`: `Ok` makes it a fix item that does not block, `Err` blocks with the reason. The package
/// level decides ([`Touched::reaches_test`], ER-12). The grade is printed as a `::notice::` line and to the job summary.
pub fn grade_flaky(t: &Touched, job: &str, test: &str) -> Result<(), String> {
    let package = t.reaches_test(job, test);
    let (word, why) = match &package {
        Reach::Touched(w) => ("blocks (touched)", w),
        Reach::Untouched(w) => ("fix item (untouched)", w),
        Reach::Unknown(w) => ("blocks (cannot be placed)", w),
    };
    notice(&format!("flaky grade: job {job:?}, {test}: {word}: {why}"));
    match package {
        Reach::Untouched(_) => Ok(()),
        Reach::Touched(w) => Err(format!("job {job:?}: a FLAKY test in touched code ({test}): {w}")),
        Reach::Unknown(w) => Err(format!(
            "job {job:?}: a FLAKY test the graph cannot place ({test}): {w}; it blocks (fail closed)"
        )),
    }
}

/// Reads the logs of the failed jobs of a run of `workflow` and grades them. `touched` is given the client, because what a change
/// touched can need the runs again (the last good commit it is measured from).
pub fn read_failures(
    client: &mut Client,
    workflow: &str,
    jobs: &[TimedJob],
    expected: &ExpectedMap,
    touched: &mut dyn FnMut(&mut Client) -> Option<Touched>,
) -> GradedResult {
    let failed: Vec<FailedJob> = jobs
        .iter()
        .filter(|j| j.outcome == JobOutcome::Failed && !j.is_caller())
        .map(|j| FailedJob {
            name: j.name.clone(),
            id: j.id,
            url: j.html_url.clone(),
            failed_steps: j.failed_steps.clone(),
            log: client.log(j.id).ok().flatten(),
        })
        .collect();
    if failed.is_empty() {
        return GradedResult::Red("the run failed and no job names a failure".into());
    }
    grade_failures(workflow, &failed, expected, &mut || touched(&mut *client))
}

/// The outcome of one run. A cancelled or skipped run is `Partial` times `NotMeasured`, and one still going is `NotMeasured` on
/// the scope it declared: never a pass and never a failure (ER-33). The one function that grades a run: `ci verdict`, `ci reuse`,
/// the landing queue and the planner's base all read its answer.
pub fn grade_run(
    client: &mut Client,
    run: &RunInfo,
    scope_decisive: bool,
    touched: &mut dyn FnMut(&mut Client) -> Option<Touched>,
) -> Outcome {
    grade_run_for(client, run, scope_decisive, false, touched)
}

/// [`grade_run`] with the tag rule given: for a tag an OS-scoped run is partial (`verdict::read_mode_jobs_for`).
pub fn grade_run_for(
    client: &mut Client,
    run: &RunInfo,
    scope_decisive: bool,
    tag: bool,
    touched: &mut dyn FnMut(&mut Client) -> Option<Touched>,
) -> Outcome {
    let mut landing_base = None;
    let mut o = grade_run_inner(client, run, scope_decisive, tag, touched, &mut landing_base);
    if o.scope == Scope::Landing {
        o.landing_base = landing_base;
    }
    o
}

fn grade_run_inner(
    client: &mut Client,
    run: &RunInfo,
    scope_decisive: bool,
    tag: bool,
    touched: &mut dyn FnMut(&mut Client) -> Option<Touched>,
    landing_base: &mut Option<String>,
) -> Outcome {
    if matches!(run.conclusion.as_str(), "cancelled" | "skipped") {
        let why = format!("run {} was {}", run.id, run.conclusion);
        return Outcome::of(Scope::Partial, GradedResult::NotMeasured(why.clone())).noted(why);
    }
    let read = verdict::read_mode_jobs_for(client, run, scope_decisive, tag);
    if let Some(b) = &read.landing {
        landing_base.clone_from(b);
    }
    let (scope, why) = match read.mode {
        Mode::Full => (Scope::Complete, String::new()),
        // A narrowed run of its own diff's selection: graded in full, for a landing only.
        Mode::Partial(why) if read.landing.is_some() => (Scope::Landing, why),
        Mode::Partial(why) => (Scope::Partial, why),
        Mode::Incomplete(why) => {
            let state = if run.conclusion.is_empty() {
                run.status.as_str()
            } else {
                run.conclusion.as_str()
            };
            let result = if run.finished() {
                GradedResult::Red(format!("{state}; {why}"))
            } else {
                GradedResult::NotMeasured(format!("run {} is {}", run.id, run.status))
            };
            return Outcome::of(Scope::Partial, result).noted(why).blocking();
        }
        Mode::Unknown(why) => {
            let mut o = Outcome::of(Scope::Partial, GradedResult::NotMeasured(why.clone())).noted(why);
            o.unreadable = true;
            return o;
        }
    };
    if !run.finished() {
        let w = format!("run {} is {}", run.id, run.status);
        return Outcome::of(scope, GradedResult::NotMeasured(w)).noted(why);
    }
    let state = if run.conclusion.is_empty() {
        run.status.as_str()
    } else {
        run.conclusion.as_str()
    };
    if scope == Scope::Partial {
        let result = if state == "success" {
            GradedResult::Green
        } else {
            GradedResult::Red(format!("{state}; {why}"))
        };
        return Outcome::of(scope, result).noted(why);
    }
    // A rerun in place keeps the run id: an earlier attempt's failed job makes it red, an unreadable one not measured.
    if run.run_attempt > 1 {
        match verdict::read_earlier(client, run) {
            Earlier::Clean => {}
            Earlier::Failed(w) => {
                let r = GradedResult::Red(format!("it ended {state} after a rerun of failed jobs ({w})"));
                return Outcome::of(scope, r).noted(why);
            }
            Earlier::Unknown(w) => return Outcome::of(scope, GradedResult::NotMeasured(w)).noted(why),
        }
    }
    if state == "success" {
        return Outcome::of(scope, GradedResult::Green).noted(why);
    }
    // A job that was cancelled (or ended in no state) is neither a pass nor a failure; one the plan declared and that was skipped
    // is caught by the declared scope below, and one it did not declare is off.
    let unmeasured = |o: &JobOutcome| match o {
        JobOutcome::Passed | JobOutcome::Failed | JobOutcome::Skipped => None,
        JobOutcome::Unfinished => Some("in no state".to_string()),
        JobOutcome::Cancelled | JobOutcome::Unknown(_) => Some(o.to_string()),
    };
    if let Some((j, ended)) = read
        .jobs
        .iter()
        .filter(|j| !j.is_caller())
        .find_map(|j| unmeasured(&j.outcome).map(|e| (j, e)))
    {
        let w = format!("job {:?} ended {ended}", j.name);
        return Outcome::of(scope, GradedResult::NotMeasured(w)).noted(why);
    }
    let result = read_failures(client, run.workflow(), &read.jobs, &read.expected, touched);
    // Fix items are granted only on the declared scope that ran, whichever basis decides "complete" (ER-41's report-only phase).
    if matches!(result, GradedResult::GreenWithFixItems(_)) {
        match read.scope_view {
            Mode::Full => {}
            // A landing run's declared scope ran (read.landing checked every declared job).
            Mode::Partial(_) if scope == Scope::Landing => {}
            Mode::Partial(w) => {
                let n = GradedResult::NotMeasured(format!("fix items need the declared scope to have run: {w}"));
                return Outcome::of(Scope::Partial, n).noted(w).blocking();
            }
            Mode::Incomplete(w) => {
                let n = GradedResult::NotMeasured(format!("fix items need the declared scope to have run: {w}"));
                return Outcome::of(Scope::Partial, n).noted(w).blocking();
            }
            Mode::Unknown(w) => {
                return Outcome::of(scope, GradedResult::NotMeasured(format!("the declared scope cannot be read: {w}"))).noted(why);
            }
        }
    }
    // A landing run keeps why it is not complete (`note`), so a reader that passes it over says why (run 8983: "8983 ()").
    Outcome::of(scope, result).noted(why)
}

/// What decides a commit: its deciding run and outcome, and the runs that were partial on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOutcome {
    /// The run that decides.
    pub run: u64,
    /// Its outcome.
    pub outcome: Outcome,
    /// The partial runs met before it, `id (why)`, newest first.
    pub partial: Vec<String>,
}

/// The outcome of a commit from its runs: the newest run of the workflow that is complete decides (a newer red one beats an older
/// green one; a newer cancelled run, or a push that only reused another run and so declared less, does not), and a run whose mode
/// cannot be read, or that was graded but not granted (`Outcome::blocking`), blocks it. Without a complete run, the newest partial one. `None`: no run.
pub fn commit_outcome(
    client: &mut Client,
    runs: &[&RunInfo],
    scope_decisive: bool,
    touched: &mut dyn FnMut(&mut Client) -> Option<Touched>,
) -> Option<CommitOutcome> {
    commit_outcome_for(client, runs, scope_decisive, false, false, touched)
}

/// [`commit_outcome`] with the tag rule given (`tag`: an OS-scoped run is partial, so an older every-OS run may decide).
/// With `landing` a [`Scope::Landing`] run decides like a complete one (the landing queue and a push's reuse); without it, it is
/// passed over like any partial run (a base, a tag).
pub fn commit_outcome_for(
    client: &mut Client,
    runs: &[&RunInfo],
    scope_decisive: bool,
    tag: bool,
    landing: bool,
    touched: &mut dyn FnMut(&mut Client) -> Option<Touched>,
) -> Option<CommitOutcome> {
    let mut newest = runs.to_vec();
    newest.sort_by_key(|r| std::cmp::Reverse(r.id));
    let mut partial = Vec::new();
    let mut fallback: Option<(u64, Outcome)> = None;
    for r in newest {
        let o = grade_run_for(client, r, scope_decisive, tag, touched);
        if o.unreadable || o.blocking || o.scope == Scope::Complete || (landing && o.scope == Scope::Landing) {
            return Some(CommitOutcome {
                run: r.id,
                outcome: o,
                partial,
            });
        }
        partial.push(if o.scope == Scope::Landing {
            format!(
                "{} (a landing run: it ran its own diff's selection, which counts for its landing and the push to main after it, never as a full run: {})",
                r.id, o.note
            )
        } else {
            format!("{} ({})", r.id, o.note)
        });
        fallback.get_or_insert((r.id, o));
    }
    fallback.map(|(run, outcome)| CommitOutcome { run, outcome, partial })
}

/// The commit a plan or a landing is based on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// The commit.
    pub sha: String,
    /// The run that decided it.
    pub run: u64,
    /// Its outcome (always a base outcome).
    pub outcome: Outcome,
}

/// The runs of one workflow, by the commit they tested: the run list read once (up to [`MAX_PAGES`] pages, all branches and events),
/// so a search matches runs to commits in memory and never asks the server per commit.
pub struct Runs(BTreeMap<String, Vec<RunInfo>>);

impl Runs {
    /// Reads the list.
    pub fn read(client: &mut Client, workflow: &str) -> Result<Runs, String> {
        let mut by_sha: BTreeMap<String, Vec<RunInfo>> = BTreeMap::new();
        for page in 1..=MAX_PAGES {
            let p = client
                .runs_page(page)
                .map_err(|e| format!("the runs of {workflow} could not be listed: {e}"))?;
            let n = p.runs.len();
            for r in p.runs.into_iter().filter(|r| workflow_file(&r.path) == workflow) {
                by_sha.entry(r.head_sha.clone()).or_default().push(r);
            }
            if n < 50 {
                break;
            }
        }
        Ok(Runs(by_sha))
    }

    /// The commits main has been at, newest first: those of push runs on main (a train lands by fast-forward, so each landing is one push
    /// of main's branch). Main's first-parent line does not hold them ([`main_line`]).
    pub fn main_tips(&self) -> Vec<String> {
        let mut pushes: Vec<(u64, &str)> = self
            .0
            .values()
            .flatten()
            .filter(|r| r.event == "push" && r.head_branch == "main")
            .map(|r| (r.id, r.head_sha.as_str()))
            .collect();
        pushes.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
        let mut seen = BTreeSet::new();
        pushes
            .into_iter()
            .filter(|(_, sha)| seen.insert(*sha))
            .map(|(_, sha)| sha.to_string())
            .collect()
    }
}

/// The last first-parent commit (`first_parent`: newest first, the asking commit itself left out) whose outcome is complete and green
/// with or without fix items. The runs are read once ([`Runs`]); `touched(client, sha)` is asked only for a commit with a FLAKY-only
/// failure. `Ok(None)`: no such commit among them.
pub fn find_base(
    client: &mut Client,
    workflow: &str,
    first_parent: &[String],
    scope_decisive: bool,
    touched: &mut dyn FnMut(&mut Client, &str) -> Option<Touched>,
) -> Result<Option<Base>, String> {
    find_base_for(client, workflow, first_parent, scope_decisive, false, touched)
}

/// [`find_base`] with the tag rule given (`tag`): a commit counts only when a run of it is complete on every OS and green without fix
/// items ([`commit_outcome_for`] with `tag`; a tag needs a run without them), the rule `ci verdict --tag` applies to the commit itself.
pub fn find_base_for(
    client: &mut Client,
    workflow: &str,
    first_parent: &[String],
    scope_decisive: bool,
    tag: bool,
    touched: &mut dyn FnMut(&mut Client, &str) -> Option<Touched>,
) -> Result<Option<Base>, String> {
    let runs = Runs::read(client, workflow)?;
    Ok(find_base_in_for(
        client,
        &runs,
        first_parent,
        scope_decisive,
        tag,
        &mut |c, _, s| touched(c, s),
    ))
}

/// [`find_base`] over runs already read; `touched` is given them too, so a measure of what a change touched searches the same list
/// instead of reading it again ([`touched_since_base`]).
pub fn find_base_in(
    client: &mut Client,
    runs: &Runs,
    first_parent: &[String],
    scope_decisive: bool,
    touched: &mut dyn FnMut(&mut Client, &Runs, &str) -> Option<Touched>,
) -> Option<Base> {
    find_base_in_for(client, runs, first_parent, scope_decisive, false, touched)
}

/// [`find_base_in`] with the tag rule given (see [`find_base_for`]).
fn find_base_in_for(
    client: &mut Client,
    runs: &Runs,
    first_parent: &[String],
    scope_decisive: bool,
    tag: bool,
    touched: &mut dyn FnMut(&mut Client, &Runs, &str) -> Option<Touched>,
) -> Option<Base> {
    for sha in first_parent.iter().take(MAX_COMMITS) {
        let Some(of_sha) = runs.0.get(sha) else { continue };
        let refs: Vec<&RunInfo> = of_sha.iter().collect();
        let mut t = |c: &mut Client| touched(c, runs, sha);
        if let Some(CommitOutcome { run, outcome, .. }) = commit_outcome_for(client, &refs, scope_decisive, tag, false, &mut t)
            && outcome.is_base()
            && (!tag || outcome.result == GradedResult::Green)
        {
            return Some(Base {
                sha: sha.clone(),
                run,
                outcome,
            });
        }
    }
    None
}

/// What the change since the last good commit touched (ADR 0117 WP-4): the plan from the base the one definition of green finds in
/// `first_parent` (the commits before `sha`, newest first) to `sha`. A train lands by fast-forward, so `sha^1` is only its last
/// member; the last good commit before the whole train is the base of everything it added. With no such commit the change cannot
/// be bounded and every target counts as reached. `plan_of(base, head)` plans a range (`None`: it cannot). The search reads `runs`,
/// the list the caller already has: no second read of the run list.
pub fn touched_since_base(
    client: &mut Client,
    runs: &Runs,
    first_parent: &[String],
    sha: &str,
    scope_decisive: bool,
    plan_of: &mut dyn FnMut(&str, &str) -> Option<Touched>,
) -> Option<Touched> {
    // The candidates are graded without touched targets, so a commit with a FLAKY is not a base here (no recursion): the range only
    // grows, and a larger range reaches more, which is the safe side.
    match find_base_in(client, runs, first_parent, scope_decisive, &mut |_, _, _| None) {
        None => Some(Touched {
            all: true,
            ..Touched::default()
        }),
        Some(base) => plan_of(&base.sha, sha),
    }
}

/// The first-parent commits before `sha`, newest first, at most [`MAX_COMMITS`] (`git rev-list --first-parent`); empty when git cannot
/// say (then no base is found: everything runs, the safe side).
pub fn first_parent_before(root: &std::path::Path, sha: &str) -> Vec<String> {
    let range = format!("{sha}^");
    let n = MAX_COMMITS.to_string();
    crate::plan_repo::git_text(root, &["rev-list", "--first-parent", "-n", &n, &range])
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// The candidate bases of a push to main that moved it from `before` to `sha`, newest first: main's own first-parent line from `before`
/// (included), at most [`MAX_COMMITS`]. A train lands by fast-forward, so the first-parent line of `sha` is the train's: it holds the
/// train's merges of main but never main's previous tip, and a search over it skipped that tip and found an older base (push run
/// 9056 read b3ad2dd2d, two mains back, while train g's landing run 9054 had tested the diff from 4284e23d5, the main it moved from).
/// Without a usable `before` (none, the zero id of a new branch, not a commit of this clone, or not an ancestor of `sha`: a forced
/// push) it is [`first_parent_before`], whose base is only older: a wider diff, never a missed one.
pub fn push_chain(root: &std::path::Path, sha: &str, before: Option<&str>) -> Vec<String> {
    let usable = before.filter(|b| b.len() == 40 && b.bytes().all(|c| c.is_ascii_hexdigit()) && b.bytes().any(|c| c != b'0') && *b != sha);
    // Exit 0 of `merge-base --is-ancestor` is the only yes; exit 1 (no) and an unknown commit are both a failure here.
    if let Some(b) = usable.filter(|b| crate::plan_repo::git_text(root, &["merge-base", "--is-ancestor", b, sha]).is_some()) {
        let n = MAX_COMMITS.to_string();
        let chain: Vec<String> = crate::plan_repo::git_text(root, &["rev-list", "--first-parent", "-n", &n, b])
            .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
            .unwrap_or_default();
        if !chain.is_empty() {
            return chain;
        }
    }
    first_parent_before(root, sha)
}

/// The candidate bases on main's own line, newest first: the commits main has been at (`tips`, [`Runs::main_tips`]) that are ancestors of
/// `head` (`head` itself only with `include_head`), then the first-parent line (from `head`'s parent for a push, whose `before` is given
/// to [`push_chain`]; from `head` for a landing dispatch). A train lands by fast-forward and merges main's previous tip as a second parent
/// (3258361e7 into train j), so the first-parent line of main's tip never meets it: the push's reuse and the next dispatch both lost the
/// last complete commit that way. The one candidate line of the push's reuse, the planner's base and a landing dispatch's base.
pub fn main_line(root: &std::path::Path, head: &str, include_head: bool, before: Option<&str>, tips: &[String]) -> Vec<String> {
    // Every ancestor, not the newest MAX_COMMITS of them: a train merges many commits, so a tip main has been at can lie far below `head`
    // in the walk (train r's push: 222 commits down, past 200, and the push took an older base than its landing run had). One
    // `rev-list` of hashes, about 30 ms for 10,000 commits; the candidates stay bounded by the tips (at most MAX_PAGES pages of runs)
    // and the first-parent line below.
    let ancestors = |rev: &str| -> Vec<String> {
        crate::plan_repo::git_text(root, &["rev-list", rev])
            .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
            .unwrap_or_default()
    };
    let Some(head_sha) = crate::plan_repo::git_text(root, &["rev-parse", "--verify", "--quiet", &format!("{head}^{{commit}}")])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    else {
        return Vec::new();
    };
    let reach: BTreeSet<String> = ancestors(&head_sha).into_iter().collect();
    let below = if include_head {
        let mut line = vec![head_sha.clone()];
        line.extend(first_parent_before(root, &head_sha));
        line
    } else {
        push_chain(root, &head_sha, before)
    };
    let mut seen = BTreeSet::new();
    tips.iter()
        .filter(|t| reach.contains(*t) && (include_head || **t != head_sha))
        .cloned()
        .chain(below)
        .filter(|c| seen.insert(c.clone()))
        .collect()
}

/// Main's tip before this push: `before` of the push event (`GITHUB_EVENT_PATH`, read with serde) when the event is a push; `None`
/// otherwise or when it cannot be read ([`push_chain`] then falls back to the first-parent line, the safe side).
pub fn push_before(env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if env("GITHUB_EVENT_NAME").as_deref() != Some("push") {
        return None;
    }
    let text = std::fs::read_to_string(env("GITHUB_EVENT_PATH")?).ok()?;
    let event: serde_json::Value = serde_json::from_str(&text).ok()?;
    event.get("before")?.as_str().map(str::to_string)
}

/// Whether a run that tested the diff `from..head` tested at least what the diff `base..head` holds: `from` is an ancestor of `base`
/// and every path `base..head` changes is changed in `from..head` too (the planner plans from the changed paths). `false` whenever git
/// cannot say: the push then runs its own.
pub fn diff_covers(root: &std::path::Path, from: &str, base: &str, head: &str) -> bool {
    if crate::plan_repo::git_text(root, &["merge-base", "--is-ancestor", from, base]).is_none() {
        return false;
    }
    let paths = |a: &str| -> Option<BTreeSet<String>> {
        crate::plan_repo::git_text(root, &["diff", "--name-only", "-z", "--no-renames", a, head])
            .map(|t| t.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
    };
    match (paths(from), paths(base)) {
        (Some(wide), Some(push)) => push.is_subset(&wide),
        _ => false,
    }
}

/// What the commits `base..head` touched: the plan of that range. `None` when it cannot be planned (a commit that is not in this
/// clone), which makes a FLAKY test not measured, never untouched.
pub fn plan_touched_range(root: &std::path::Path, base: &str, head: &str) -> Option<Touched> {
    use crate::plan::{Request, plan_with_graph};
    let history = paddock_repo_graph::GitHistory::new(root, std::time::Duration::from_secs(120));
    let req = Request {
        files: Vec::new(),
        all: false,
        explain: false,
        root: None,
        base: Some(base.to_string()),
        head: head.to_string(),
    };
    plan_with_graph(root, &history, &req)
        .ok()
        .map(|(graph, plan)| Touched::from_plan(&plan).with_graph(&graph))
}

/// THE candidate bases of a commit's readers (ER-21, ER-44): main's own line below `sha` (the tips main has been at that are ancestors,
/// newest push run first, then the first-parent line; every ancestor is searched, never the newest N). The push's reuse, the planner,
/// a landing dispatch, the measure of what a commit touched and the verdict's "last fully tested commit" hint all read the base from
/// this one line, so they name the same commit (run 9150: 62049e300 here and 3258361e7 for the landing, and the hint said 62049e300).
/// A push's reuse and planner add the push's `before` through [`main_line`]; this form has none.
pub fn candidate_line(root: &std::path::Path, sha: &str, runs: &Runs) -> Vec<String> {
    main_line(root, sha, false, None, &runs.main_tips())
}

/// [`candidate_line`] with the runs read here, once; empty when they cannot be read (no base is found: the safe side).
pub fn candidate_line_of(client: &mut Client, workflow: &str, root: &std::path::Path, sha: &str) -> Vec<String> {
    Runs::read(client, workflow)
        .map(|runs| candidate_line(root, sha, &runs))
        .unwrap_or_default()
}

/// The touched reader of the production paths (`ci verdict`, `ci reuse`, the queue, the planner): [`touched_since_base`] over the
/// first-parent history of `root`, planned by [`plan_touched_range`]; with no good base, everything counts as reached, and a range
/// that cannot be planned is `None` (not measured). `runs` is the list the caller already read (the planner's base search); without
/// it the list is read here, once.
pub fn touched_in(
    client: &mut Client,
    workflow: &str,
    root: &std::path::Path,
    sha: &str,
    scope_decisive: bool,
    runs: Option<&Runs>,
) -> Option<Touched> {
    let own;
    let runs = match runs {
        Some(r) => r,
        None => {
            own = Runs::read(client, workflow).ok()?;
            &own
        }
    };
    let first_parent = candidate_line(root, sha, runs);
    touched_since_base(client, runs, &first_parent, sha, scope_decisive, &mut |b, h| {
        plan_touched_range(root, b, h)
    })
}

#[cfg(test)]
mod tests;
