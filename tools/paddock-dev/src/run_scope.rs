//! The scope of a CI run, declared as data (ADR 0113). A plan job prints one line, `plan scope: {json}`, and `./dev ci verdict`
//! (the one reader, [`crate::ci_runs::verdict`]) decides "complete" by comparing what the run did (`ran`) with what a push to main
//! does for the same tree (`need`): complete means `ran` covers `need`, jobs, engines, apps and whole-workspace tests alike.
//! No workflow decides it any more, so a smaller scope cannot be written as "complete" by a shell line that forgot a dimension
//! (the schedule-skipped and rerun-only runs counted green on 2026-11-12; the WebKit hole of the D4 reuse).
//!
//! Producers: `./dev plan --ci` for ci.yml ([`ci_scope`]), `./dev ci scope` for the app workflows ([`app_scope`]).
//! The reader: [`read`] and [`Declared::complete`].

use crate::job_outcome::JobOutcome;
use crate::log_text::plain;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The prefix of the scope line (after the colours and the timestamp).
pub const SCOPE_LINE: &str = "plan scope: ";

/// The version of the line's shape; a reader refuses another. Version 2 (ADR 0117 §6, the graph's wiring): `ran` names the projects
/// that ran a smoke set or only affected tests, every scope states `os_full`, and the line states whether `need` came from the whole
/// tree (`need_whole`).
pub const VERSION: u64 = 2;

/// The runners the `crate` and `paddock-dev` jobs run on: the planner's one leg set (ADR 0123 §4; WP-2 reads it from the model).
pub const DEFAULT_OS_LEGS: [&str; 3] = crate::plan_ci::OS_LEGS;

/// The project flags of a plan that each decide a job of ci.yml.
const PROJECTS: [&str; 8] = ["crates", "ui", "eslint", "conformance", "c99", "tools", "icons", "paddockDev"];

/// What a run covers: jobs, the browser engines of its tests, apps (a leg is `app@os` with an e2e leg appended) and whether its
/// tests ran whole (not narrowed to packages).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// The jobs the plan switched on (`crates`, `crate@macos-latest`, ...).
    pub jobs: BTreeSet<String>,
    /// The browser engines of the tests (`chromium`, `webkit`).
    pub engines: BTreeSet<String>,
    /// The app legs (`dyno@ubuntu-latest`, `dyno@ubuntu-latest:e2e-webkit`).
    pub apps: BTreeSet<String>,
    /// True when no test run was narrowed to a package list.
    pub whole: bool,
    /// True when the Windows and macOS legs tested every package (the full every-OS scope: the nightly, a release's gates); false for
    /// an OS-scoped run, which tested only the packages with OS-specific code there (`hostTests`). A push to main's scope does not need
    /// it (a landing may be scoped); a tag does ([`Declared::complete_for_tag`]).
    pub os_full: bool,
    /// The projects that ran their declared smoke set or only some contract phases (the graph's selection), not their suite: never
    /// complete (ADR 0117 §6).
    pub smoke: BTreeSet<String>,
    /// The projects that ran only the tests the change reached (affected targets and spec files): never complete.
    pub affected: BTreeSet<String>,
}

fn set(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

impl Scope {
    /// The scope as JSON (sorted lists).
    pub fn json(&self) -> Value {
        json!({"jobs": self.jobs, "engines": self.engines, "apps": self.apps, "whole": self.whole, "os_full": self.os_full,
               "smoke": self.smoke, "affected": self.affected})
    }

    fn parse(v: &Value, what: &str) -> Result<Scope, String> {
        for key in ["jobs", "engines", "apps", "smoke", "affected"] {
            if !v[key].is_array() {
                return Err(format!("{what}.{key} is missing or not a list"));
            }
        }
        let whole = v["whole"]
            .as_bool()
            .ok_or_else(|| format!("{what}.whole is missing or not a boolean"))?;
        Ok(Scope {
            jobs: set(&v["jobs"]),
            engines: set(&v["engines"]),
            apps: set(&v["apps"]),
            whole,
            // A missing key is an error, never "every OS" (review of 8cab10234, P2-2: fail closed).
            os_full: v["os_full"]
                .as_bool()
                .ok_or_else(|| format!("{what}.os_full is missing or not a boolean"))?,
            smoke: set(&v["smoke"]),
            affected: set(&v["affected"]),
        })
    }

    /// What `need` has that `self` lacks, one phrase per dimension; empty when `self` covers it.
    fn missing(&self, need: &Scope) -> Vec<String> {
        let mut out = Vec::new();
        for (name, have, want) in [
            ("jobs", &self.jobs, &need.jobs),
            ("engines", &self.engines, &need.engines),
            ("apps", &self.apps, &need.apps),
        ] {
            let gone: Vec<&String> = want.difference(have).collect();
            if !gone.is_empty() {
                out.push(format!("{name} {}", gone.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
            }
        }
        if need.whole && !self.whole {
            out.push("whole-workspace tests (a member run narrowed them to packages)".into());
        }
        out
    }
}

/// What one plan job declared: the workflow, whether it is a rerun of failures, what it ran and what a push to main needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The workflow file (`ci.yml`): a line of another workflow is not evidence.
    pub workflow: String,
    /// A rerun of an earlier run's failures: never complete, whatever it ran.
    pub rerun: bool,
    /// The one operating system an `os` run was narrowed to (`linux`, `windows`, `macos`; ci.yml's `os` input): such a run is never
    /// the complete run of its commit, whatever it ran (ER-15: only a complete run counts).
    pub os: Option<String>,
    /// What the run did.
    pub ran: Scope,
    /// What a push to main does for this tree.
    pub need: Scope,
    /// Whether `need` is the whole tree's (a dispatch without a base, a tag, the nightly), not the diff of a push or a landing: a
    /// tag counts only such a run (ER-15, review of 8cab10234 P2-3).
    pub need_whole: bool,
    /// The commit `need` was diffed from when it is not the whole tree's (a push's green base, a landing's base): a push to main may
    /// reuse a landing run only when the landing ran the same diff (review round 2, P1-A).
    pub base: Option<String>,
    /// The apps the tree's active set left out of both `ran` and `need` ([`crate::active_apps`]): a complete run is complete for the
    /// foundation and the active apps, never for these ([`Declared::covers_app`]). Written only when some are inactive; a line without
    /// the key (written before the set existed, or by a run that planned every app) has none.
    pub inactive: BTreeSet<String>,
    /// The jobs the plan switched off because every one of their test targets reuses a trusted pass of the same key (ADR 0116 §6), with
    /// one note per target (`rust:crates/a@Linux-X64 from run 8635`). A job here counts as covered in [`Declared::complete`] and is
    /// not expected to have run; a job that runs only some of its targets is in `ran` as usual and its reused targets are named in
    /// the plan log. Written only when non-empty, so a line without the key (an older producer, or a run with no reuse) has none.
    pub reused: BTreeMap<String, Vec<String>>,
}

impl Declared {
    /// This declaration for a run narrowed to one operating system (`None`: every OS).
    pub fn with_os(self, os: Option<&str>) -> Declared {
        Declared {
            os: os.map(str::to_string),
            ..self
        }
    }

    /// The scope line.
    pub fn line(&self) -> String {
        let mut v = json!({"v": VERSION, "workflow": self.workflow, "rerun": self.rerun, "ran": self.ran.json(), "need": self.need.json(),
                           "need_whole": self.need_whole});
        if let Some(b) = &self.base {
            v["base"] = json!(b);
        }
        if let Some(os) = &self.os {
            v["os"] = json!(os);
        }
        if !self.inactive.is_empty() {
            v["inactive"] = json!(self.inactive);
        }
        if !self.reused.is_empty() {
            v["reused"] = json!(self.reused);
        }
        format!("{SCOPE_LINE}{v}")
    }

    /// `Ok` when the run is complete: not a rerun, not one operating system's part (`os`), `need` is not empty (an empty need proves nothing), and `ran` covers `need`.
    /// `Err` names what is missing.
    pub fn complete(&self) -> Result<(), String> {
        if self.rerun {
            return Err("it is a rerun of an earlier run's failures".into());
        }
        if let Some(os) = &self.os {
            return Err(format!(
                "it ran one operating system's part only (the os input: {os}), not every runner"
            ));
        }
        if !self.ran.smoke.is_empty() {
            return Err(format!(
                "it ran a smoke set or some phases only, not the suite, of {} (the graph's selection)",
                self.ran.smoke.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if !self.ran.affected.is_empty() {
            return Err(format!(
                "it ran only the affected tests of {} (the graph's selection)",
                self.ran.affected.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if self.need.jobs.is_empty() && self.need.apps.is_empty() {
            return Err("its declared need is empty (a scope that needs nothing proves nothing)".into());
        }
        // A job switched off because all its targets were reused is covered (ADR 0116 §6): "ran or reused".
        let covered = Scope {
            jobs: self.ran.jobs.iter().chain(self.reused.keys()).cloned().collect(),
            ..self.ran.clone()
        };
        let gone = covered.missing(&self.need);
        if gone.is_empty() {
            Ok(())
        } else {
            Err(format!("it did not cover what a push to main does: {}", gone.join("; ")))
        }
    }
}

impl Declared {
    /// This declaration with the commit its `need` was diffed from (`None`: the whole tree).
    pub fn with_base(self, base: Option<&str>) -> Declared {
        Declared {
            base: base.map(str::to_string),
            ..self
        }
    }

    /// Whether a landing may rest on the run (the user's decision of 2026-11-23, review round 2 P1-A): it is complete, or it ran
    /// exactly its own diff's selection: its `need` is that diff's plan (not the whole tree) and `ran` covers it except that some
    /// projects ran their smoke set or only their affected tests. Such a run lands; it is never complete, never a base and never
    /// for a tag.
    pub fn lands(&self) -> Result<(), String> {
        if self.complete().is_ok() {
            return Ok(());
        }
        if self.need_whole {
            return Err("it was planned for the whole tree, so a narrowed selection is not its own diff's".into());
        }
        let whole = Declared {
            ran: Scope {
                smoke: BTreeSet::new(),
                affected: BTreeSet::new(),
                ..self.ran.clone()
            },
            ..self.clone()
        };
        whole.complete()
    }

    /// [`Declared::complete`], and the run tested every package on every OS: a tag counts only a full every-OS run (ER-15, ADR 0113),
    /// never an OS-scoped one. A landing may be scoped (the nightly and the release gates are the full runs).
    pub fn complete_for_tag(&self) -> Result<(), String> {
        self.complete()?;
        if !self.need_whole {
            return Err("its need was the diff of a push or a landing, not the whole tree; a tag needs a run planned for the whole tree (a dispatch without a base, the nightly or a release's gates)".into());
        }
        if self.ran.os_full {
            Ok(())
        } else {
            Err("it was an OS-scoped run (the Windows and macOS legs tested only the packages with OS-specific code, hostTests); a tag needs a full every-OS run (os_full, the nightly or a release's gates)".into())
        }
    }
}

impl Declared {
    /// This declaration with the jobs the test-result cache switched off, and the targets each rests on.
    pub fn with_reused(self, reused: BTreeMap<String, Vec<String>>) -> Declared {
        Declared { reused, ..self }
    }

    /// One line per reused target, `reused from run N`, for the verdict reader to print: a verdict that relied on a reused pass says so.
    pub fn reuse_notes(&self) -> Vec<String> {
        self.reused
            .iter()
            .flat_map(|(job, ts)| ts.iter().map(move |t| format!("{job}: reused {t}")))
            .collect()
    }

    /// This declaration with the apps its tree's active set left out.
    pub fn with_inactive(self, inactive: BTreeSet<String>) -> Declared {
        Declared { inactive, ..self }
    }

    /// `Ok` when the run planned `app` (it was active, or every app ran: the nightly); `Err` names the set. A tag of an app rests only
    /// on a run that covered it (ER-15).
    pub fn covers_app(&self, app: &str) -> Result<(), String> {
        if self.inactive.contains(app) {
            Err(format!(
                "{app} was inactive in it ({} `active` did not name it), so it ran none of {app}'s jobs",
                crate::active_apps::FILE
            ))
        } else {
            Ok(())
        }
    }
}

/// The declaration a plan job's log holds: `None` when no line starts with [`SCOPE_LINE`], `Err` for a line that cannot be read
/// (not JSON, another version, a missing field) or a second line (ambiguous: only one plan job prints one).
pub fn read(log: &str) -> Option<Result<Declared, String>> {
    let lines: Vec<String> = log
        .lines()
        .map(plain)
        .filter_map(|l| l.trim().strip_prefix(SCOPE_LINE).map(str::to_string))
        .collect();
    let first = lines.first()?;
    if lines.len() > 1 {
        return Some(Err(format!("the plan job logged {} scope lines, not one", lines.len())));
    }
    Some(parse(first))
}

fn parse(text: &str) -> Result<Declared, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("the scope line is not JSON ({e})"))?;
    if v["v"].as_u64() != Some(VERSION) {
        return Err(format!("the scope line has version {}, this reader knows {VERSION}", v["v"]));
    }
    Ok(Declared {
        workflow: v["workflow"].as_str().ok_or("the scope line has no workflow")?.to_string(),
        rerun: v["rerun"].as_bool().ok_or("the scope line has no rerun flag")?,
        os: match &v["os"] {
            Value::Null => None,
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            other => return Err(format!("the scope line's os is {other}, not a name")),
        },
        ran: Scope::parse(&v["ran"], "ran")?,
        need: Scope::parse(&v["need"], "need")?,
        need_whole: v["need_whole"].as_bool().ok_or("the scope line has no need_whole flag")?,
        base: match &v["base"] {
            Value::Null => None,
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            other => return Err(format!("the scope line's base is {other}, not a commit")),
        },
        inactive: match &v["inactive"] {
            Value::Null => BTreeSet::new(),
            Value::Array(a) if a.iter().all(|x| x.as_str().is_some_and(|s| !s.is_empty())) => set(&v["inactive"]),
            other => return Err(format!("the scope line's inactive is {other}, not a list of apps")),
        },
        reused: match &v["reused"] {
            Value::Null => BTreeMap::new(),
            Value::Object(o) => o
                .iter()
                .map(|(job, ts)| {
                    let list: Option<Vec<String>> = ts
                        .as_array()
                        .and_then(|a| a.iter().map(|x| x.as_str().filter(|s| !s.is_empty()).map(str::to_string)).collect());
                    list.filter(|l| !l.is_empty())
                        .map(|l| (job.clone(), l))
                        .ok_or_else(|| format!("the scope line's reused.{job} is not a non-empty list of targets"))
                })
                .collect::<Result<_, _>>()?,
            other => return Err(format!("the scope line's reused is {other}, not a map of jobs")),
        },
    })
}

/// A leg's identity in [`Scope::apps`]: `app@os`, with `:e2e-n` or `:e2e-webkit` for a derived e2e leg.
fn leg_id(leg: &Value) -> String {
    let e2e = leg["e2e_leg"].as_str().unwrap_or("");
    let base = format!("{}@{}", leg["app"].as_str().unwrap_or(""), leg["os"].as_str().unwrap_or(""));
    if e2e.is_empty() { base } else { format!("{base}:{e2e}") }
}

/// The scope of a ci.yml plan: the project jobs it switched on (and the crate and paddock-dev jobs on each runner), the app legs,
/// the engines (WebKit only when `webkit` and a UI is reached), and whether every test ran whole.
pub fn scope_of(plan: &Value, legs: &[Value], webkit: bool, os_legs: &[String], package_filters: &[&str]) -> Scope {
    let mut jobs: BTreeSet<String> = PROJECTS
        .iter()
        .filter(|p| plan[**p].as_bool() == Some(true))
        .map(|p| p.to_string())
        .collect();
    for (flag, job) in [("crates", "crate"), ("paddockDev", "paddock-dev")] {
        if plan[flag].as_bool() == Some(true) {
            jobs.extend(os_legs.iter().map(|os| format!("{job}@{os}")));
        }
    }
    // The reference-app jobs of ci.yml (Chromium and WebKit) run when a UI is reached and the planner wants them (`webkit`: a tag,
    // the nightly, a manual ask).
    if plan["ui"].as_bool() == Some(true) && webkit {
        jobs.insert("e2e".to_string());
        jobs.insert("e2e-webkit".to_string());
    }
    let ui_leg = legs
        .iter()
        .any(|l| l["ui"].as_bool() == Some(true) || l["e2e_leg"].as_str().is_some_and(|e| e.starts_with("e2e")));
    let ui = plan["ui"].as_bool() == Some(true) || ui_leg;
    let mut engines = BTreeSet::new();
    if ui {
        engines.insert("chromium".to_string());
        if webkit {
            engines.insert("webkit".to_string());
        }
    }
    let whole = package_filters.iter().all(|f| f.is_empty()) && legs.iter().all(|l| l["filter"].as_str().unwrap_or("").is_empty());
    Scope {
        jobs,
        engines,
        apps: legs.iter().map(leg_id).collect(),
        whole,
        os_full: true,
        ..Scope::default()
    }
}

/// How a declared job is found among a run's jobs by display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobName {
    /// The whole name (a matrix leg: `crate (macos-latest)`).
    Exact(String),
    /// The start of the name (a job with a long or sharded name: `ui (`, `reference apps (shard`).
    Prefix(&'static str),
    /// Any of these starts: the job's name now and its names before a rename, so a recorded run of the old ci.yml still reads as
    /// covering its project (`quick checks (|C99 (`: C99 was a job of its own until 2026-11-26).
    AnyPrefix(Vec<&'static str>),
    /// The start of the name, built at run time (a leg whose job may carry a partition label: `crate (ubuntu-latest`).
    Starts(String),
}

impl JobName {
    fn matches(&self, name: &str) -> bool {
        match self {
            JobName::Exact(n) => name == n,
            JobName::Prefix(p) => name.starts_with(p),
            JobName::AnyPrefix(ps) => ps.iter().any(|p| name.starts_with(p)),
            JobName::Starts(p) => name.starts_with(p.as_str()),
        }
    }
}

/// The display names of the ci.yml jobs each project flag of a plan switches on (`plan_gitea::CI_JOBS` lists them by id; a test reads
/// ci.yml and checks each prefix names a job there). Jobs whose `if` reads more than the plan (`audit`) are
/// not here: a skipped one is not a scope that was not covered.
pub const PROJECT_JOBS: &[(&str, &[&str])] = &[
    ("crates", &["crates ("]),
    ("ui", &["ui ("]),
    ("eslint", &["ui ("]),
    ("conformance", &["conformance ("]),
    // `a|b`: either name; the first is the job's name now, the rest its names in runs of an older ci.yml (the short checks became one
    // job, `quick`, on 2026-11-26), which the verdict and reuse readers still read.
    ("c99", &["quick checks (|C99 ("]),
    ("tools", &["tools and shared scripts", "quick checks (|heavy tool"]),
    ("icons", &["ui ("]),
    ("e2e", &["reference apps (shard"]),
    ("e2e-webkit", &["reference apps in WebKit"]),
];

/// The job names of one declared job of ci.yml, empty for a key no single job stands for (`paddockDev`: its jobs are the
/// `paddock-dev@os` legs).
fn names_of_job(key: &str) -> Vec<JobName> {
    if let Some((_, prefixes)) = PROJECT_JOBS.iter().find(|(k, _)| *k == key) {
        return prefixes
            .iter()
            .map(|p| match p.split_once('|') {
                Some(_) => JobName::AnyPrefix(p.split('|').collect()),
                None => JobName::Prefix(p),
            })
            .collect();
    }
    match key.split_once('@') {
        // `crate (ubuntu-latest)` or, when its tests are split into parallel jobs, `crate (ubuntu-latest, 2/3)`: every one must have run.
        Some(("crate", os)) => vec![JobName::Starts(format!("crate ({os}"))],
        Some(("paddock-dev", os)) => vec![JobName::Exact(format!("paddock-dev ({os})"))],
        _ => Vec::new(),
    }
}

/// The job name of an app leg (`dyno@ubuntu-latest:e2e-1`): `app dyno (ubuntu-latest, e2e-1)`, as ci.yml's `app` job names it.
fn name_of_leg(leg: &str) -> JobName {
    let (app_os, e2e) = leg.split_once(':').map_or((leg, None), |(a, e)| (a, Some(e)));
    let (app, os) = app_os.split_once('@').unwrap_or((app_os, ""));
    JobName::Exact(match e2e {
        Some(e) => format!("app {app} ({os}, {e})"),
        None => format!("app {app} ({os})"),
    })
}

/// What `check_ran` found wrong with a run's jobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RanProblem {
    /// A declared job did not succeed because it never ran or was cancelled: the run did not do what it declared (partial).
    NotRun(String),
    /// A declared job has no job of that name in the run: the declaration cannot be checked (not measured).
    Missing(String),
}

/// "Ran means ran": every job the run declared in `ran` has a job in the run with a matching name, and none of them was skipped or
/// cancelled. A failed one is left to the run's own conclusion (red). Only ci.yml's jobs are mapped; another workflow's declaration
/// has no job names and passes (its `gates` job is not one job). `jobs` are `(display name, outcome)`.
pub fn check_ran(d: &Declared, jobs: &[(String, JobOutcome)]) -> Result<(), RanProblem> {
    if d.workflow != "ci.yml" {
        return Ok(());
    }
    let declared = d
        .ran
        .jobs
        .iter()
        .map(|k| (k.as_str(), names_of_job(k)))
        .chain(d.ran.apps.iter().map(|a| (a.as_str(), vec![name_of_leg(a)])));
    for (key, names) in declared {
        for name in names {
            let found: Vec<&(String, JobOutcome)> = jobs.iter().filter(|(n, _)| name.matches(n)).collect();
            if found.is_empty() {
                return Err(RanProblem::Missing(format!("{key}: the run has no job named like {name:?}")));
            }
            for (n, o) in found {
                let c = match o {
                    // Ran: a failure is the run's own red, not a partial scope.
                    JobOutcome::Passed | JobOutcome::Failed => continue,
                    JobOutcome::Unfinished => "not finished",
                    JobOutcome::Skipped | JobOutcome::Cancelled | JobOutcome::Unknown(_) => o.as_api(),
                };
                return Err(RanProblem::NotRun(format!("{key}: its job {n:?} was {c}, not success")));
            }
        }
    }
    Ok(())
}

/// What a ci.yml plan job declares. `ran` is the scope the run's own plan switched on (empty for a reused run, which runs no job,
/// and for a rerun); `need` the same planner's answer for a push to main, with every engine on and every test whole. A rerun is
/// never complete, whatever it ran.
pub fn ci_scope(ran: Scope, rerun: bool, need: Scope, need_whole: bool) -> Declared {
    Declared {
        workflow: "ci.yml".into(),
        rerun,
        os: None,
        ran,
        need,
        need_whole,
        base: None,
        inactive: BTreeSet::new(),
        reused: BTreeMap::new(),
    }
}

/// What an app's own workflow declares (`./dev ci scope`): one gates job for the app. `full` is the workflow's own full mode
/// (a push, a tag, the nightly, a dispatch with its full input), `run` whether its plan ran its jobs at all (a schedule skipped
/// for a train, an unchanged main, a tagged commit ran none), `since_run` a rerun of failures. `full_engines` are the engines a
/// full run tests; a run that is not full tests the first one only.
pub fn app_scope(workflow: &str, app: &str, full: bool, run: bool, since_run: bool, os_scoped: bool, full_engines: &[String]) -> Declared {
    let need = Scope {
        jobs: ["gates".to_string()].into(),
        engines: full_engines.iter().cloned().collect(),
        apps: [app.to_string()].into(),
        whole: true,
        os_full: true,
        ..Scope::default()
    };
    let ran = if run && !since_run {
        // A run that is not full ran no gates job (its nightly or installer jobs are not the gates) and one engine.
        Scope {
            jobs: if full { need.jobs.clone() } else { BTreeSet::new() },
            engines: if full {
                need.engines.clone()
            } else {
                full_engines.iter().take(1).cloned().collect()
            },
            os_full: !os_scoped,
            ..need.clone()
        }
    } else {
        Scope::default()
    };
    // The app's need is its whole gates, whatever changed.
    Declared {
        workflow: workflow.into(),
        rerun: since_run,
        os: None,
        ran,
        need,
        need_whole: true,
        base: None,
        inactive: BTreeSet::new(),
        reused: BTreeMap::new(),
    }
}

// ---- the report-only phase (ER-41) ---------------------------------------------------------------------------------------------

/// The register of producers whose first green scope run is recorded, relative to the repository root (`docs/ci-scope-first-runs.toml`).
pub const REGISTER: &str = "docs/ci-scope-first-runs.toml";

/// The producer that prints a workflow's scope line: `ci.yml` (`./dev plan --ci`), `debrief.yml` (`./dev ci scope`), and the shared
/// plan action for every release workflow.
pub fn producer_of(workflow: &str) -> &str {
    match workflow {
        "ci.yml" | "debrief.yml" => workflow,
        _ => "plan-action",
    }
}

/// The run id recorded for `producer` in the register text, `None` when it is not recorded; `Err` for a text that is not the
/// register's shape.
pub fn recorded(text: &str, producer: &str) -> Result<Option<u64>, String> {
    let v: toml::Table = text.parse().map_err(|e| format!("{REGISTER} is not TOML: {e}"))?;
    let Some(table) = v.get("first_green") else {
        return Err(format!("{REGISTER} has no [first_green] table"));
    };
    let Some(table) = table.as_table() else {
        return Err(format!("{REGISTER}: first_green is not a table"));
    };
    match table.get(producer) {
        None => Ok(None),
        Some(id) => id
            .as_integer()
            .filter(|n| *n > 0)
            .map(|n| Some(n as u64))
            .ok_or_else(|| format!("{REGISTER}: first_green.{producer} is not a run id")),
    }
}

/// Whether the reader decides on the declared scope for `workflow`: its producer is recorded in the register of `root`. An absent or
/// unreadable register is report-only (the old basis), with the reason: never a switch by accident.
pub fn decisive_in(root: &std::path::Path, workflow: &str) -> (bool, Option<String>) {
    match std::fs::read_to_string(root.join(REGISTER)) {
        Err(e) => (false, Some(format!("{REGISTER} cannot be read ({e}): the scope is report-only"))),
        Ok(text) => match recorded(&text, producer_of(workflow)) {
            Ok(id) => (id.is_some(), None),
            Err(e) => (false, Some(format!("{e}: the scope is report-only"))),
        },
    }
}

/// The register text with `producer` recorded at `run`; an error when it already is (the first run is the first).
pub fn with_first_run(text: &str, producer: &str, run: u64) -> Result<String, String> {
    if let Some(id) = recorded(text, producer)? {
        return Err(format!(
            "{producer} is already recorded at run {id}: the first green run is recorded once"
        ));
    }
    let mut out = text.trim_end().to_string();
    out.push_str(&format!("\n\"{producer}\" = {run}\n"));
    // The result must read back as what was asked.
    match recorded(&out, producer)? {
        Some(id) if id == run => Ok(out),
        _ => Err(format!("{REGISTER} could not take the record for {producer}")),
    }
}

/// A complete ci.yml scope line, for the tests of the readers (`verdict`, `reuse`): a run that did what a push to main does.
#[cfg(test)]
pub const TEST_COMPLETE: &str = "plan scope: {\"v\":2,\"workflow\":\"ci.yml\",\"rerun\":false,\"ran\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true,\"os_full\":true,\"smoke\":[],\"affected\":[]},\"need\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true,\"os_full\":true,\"smoke\":[],\"affected\":[]},\"need_whole\":true}\n";

/// The line of [`TEST_COMPLETE`] from an `os=linux` run: it covers its `need` here, but it is one operating system's part only.
#[cfg(test)]
pub const TEST_OS_ONLY: &str = "plan scope: {\"v\":2,\"workflow\":\"ci.yml\",\"rerun\":false,\"os\":\"linux\",\"ran\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true,\"os_full\":true,\"smoke\":[],\"affected\":[]},\"need\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true,\"os_full\":true,\"smoke\":[],\"affected\":[]},\"need_whole\":true}\n";

/// A ci.yml scope line of a run that missed one engine (WebKit): not complete.
#[cfg(test)]
pub const TEST_PARTIAL: &str = "plan scope: {\"v\":2,\"workflow\":\"ci.yml\",\"rerun\":false,\"ran\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\"],\"apps\":[],\"whole\":true,\"os_full\":true,\"smoke\":[],\"affected\":[]},\"need\":{\"jobs\":[\"conformance\"],\"engines\":[\"chromium\",\"webkit\"],\"apps\":[],\"whole\":true,\"os_full\":true,\"smoke\":[],\"affected\":[]},\"need_whole\":true}\n";

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn leg(app: &str, os: &str, e2e: &str, ui: bool) -> Value {
        json!({"app": app, "os": os, "e2e_leg": e2e, "ui": ui, "filter": ""})
    }

    fn plan() -> Value {
        json!({"crates": true, "ui": true, "conformance": true, "paddockDev": false})
    }

    fn legs(webkit: bool) -> Vec<Value> {
        let mut l = vec![
            leg("dyno", "ubuntu-latest", "", true),
            leg("dyno", "macos-latest", "", false),
            leg("spotter", "ubuntu-latest", "", true),
        ];
        if webkit {
            l.push(leg("dyno", "ubuntu-latest", "e2e-webkit", true));
        }
        l
    }

    fn os() -> Vec<String> {
        strings(&DEFAULT_OS_LEGS)
    }

    fn declared(ran_webkit: bool, filters: &[&str]) -> Declared {
        let need = scope_of(&plan(), &legs(true), true, &os(), &[]);
        let ran = scope_of(&plan(), &legs(ran_webkit), ran_webkit, &os(), filters);
        ci_scope(ran, false, need, true)
    }

    #[test]
    fn a_run_that_covers_every_job_engine_app_and_test_is_complete() {
        let d = declared(true, &["", ""]);
        assert_eq!(d.complete(), Ok(()), "{}", d.line());
        // The scope has what it should: the crate jobs on each runner, both engines, the e2e WebKit leg.
        assert!(d.need.jobs.contains("crate@macos-latest") && d.need.jobs.contains("ui"));
        assert_eq!(d.need.engines, ["chromium", "webkit"].map(String::from).into());
        assert!(d.need.apps.contains("dyno@ubuntu-latest:e2e-webkit"));
    }

    #[test]
    fn an_os_only_run_is_never_complete_even_when_its_scope_covers_the_need() {
        let d = declared(true, &[]).with_os(Some("linux"));
        assert_eq!(d.ran, d.need, "the control: nothing but the os flag differs from a complete run");
        let why = d.complete().unwrap_err();
        assert!(why.contains("linux") && why.contains("one operating system"), "{why}");
        // It survives the line: the reader sees the flag the producer wrote.
        let read = read(&d.line()).unwrap().unwrap();
        assert_eq!(read.os.as_deref(), Some("linux"));
        assert!(read.complete().is_err());
        // Positive control: the same run with no os is complete, and a line with a malformed os is refused.
        assert_eq!(declared(true, &[]).complete(), Ok(()));
        assert!(read_err(&TEST_OS_ONLY.replace("\"linux\"", "7")).contains("os"));
    }

    /// The run that switched the macOS crate job off because every target of it reused a pass.
    fn reused_job_run() -> Declared {
        let mut d = declared(true, &["", ""]);
        d.ran.jobs.remove("crate@macos-latest");
        d.with_reused(BTreeMap::from([(
            "crate@macos-latest".to_string(),
            strings(&["rust:crates/a@macOS-ARM64 from run 8635"]),
        )]))
    }

    #[test]
    fn a_job_switched_off_by_reuse_is_covered_and_one_that_just_did_not_run_is_not() {
        let d = reused_job_run();
        assert_eq!(d.complete(), Ok(()), "{}", d.line());
        // Positive control: the same run without the reuse entry is missing that job (deleting the reuse rule makes this the answer).
        let mut gone = d.clone();
        gone.reused.clear();
        assert!(gone.complete().unwrap_err().contains("crate@macos-latest"));
        // A reuse entry for another job does not cover it.
        let other = d.clone().with_reused(BTreeMap::from([("ui".to_string(), strings(&["x"]))]));
        assert!(other.complete().is_err());
    }

    #[test]
    fn the_reuse_survives_the_line_and_the_reader_names_the_run_it_relies_on() {
        let d = reused_job_run();
        let back = read(&d.line()).unwrap().unwrap();
        assert_eq!(back, d);
        assert_eq!(
            back.reuse_notes(),
            vec!["crate@macos-latest: reused rust:crates/a@macOS-ARM64 from run 8635".to_string()]
        );
        // A line with no reuse has no key, and a malformed one is refused, never read as no reuse.
        assert!(!declared(true, &[]).line().contains("reused"));
        let bad = d.line().replace("[\"rust:crates/a@macOS-ARM64 from run 8635\"]", "[]");
        assert!(read_err(&bad).contains("reused.crate@macos-latest"), "{}", read_err(&bad));
    }

    #[test]
    fn a_crate_job_split_into_partitions_must_have_every_partition_pass_before_its_declared_job_counts() {
        let d = declared(true, &["", ""]);
        let job = |n: &str, o: JobOutcome| (n.to_string(), o);
        let mut jobs: Vec<(String, JobOutcome)> = d
            .ran
            .jobs
            .iter()
            .filter(|j| !j.starts_with("crate@"))
            .flat_map(|j| names_of_job(j))
            .map(|n| match n {
                JobName::Exact(e) => job(&e, JobOutcome::Passed),
                JobName::Prefix(p) => job(&format!("{p}x)"), JobOutcome::Passed),
                JobName::AnyPrefix(ps) => job(&format!("{}x)", ps[0]), JobOutcome::Passed),
                JobName::Starts(p) => job(&format!("{p})"), JobOutcome::Passed),
            })
            .chain(d.ran.apps.iter().map(|a| match name_of_leg(a) {
                JobName::Exact(e) => job(&e, JobOutcome::Passed),
                _ => unreachable!(),
            }))
            .collect();
        jobs.extend([
            job("crate (windows-latest)", JobOutcome::Passed),
            job("crate (macos-latest)", JobOutcome::Passed),
            job("crate (ubuntu-latest, 1/3)", JobOutcome::Passed),
            job("crate (ubuntu-latest, 2/3)", JobOutcome::Passed),
            job("crate (ubuntu-latest, 3/3)", JobOutcome::Passed),
        ]);
        assert_eq!(check_ran(&d, &jobs), Ok(()), "{jobs:?}");
        // One partition cancelled is a partial scope, not a pass; and a missing job of the whole name is still missing.
        let mut cut = jobs.clone();
        cut.iter_mut().find(|(n, _)| n == "crate (ubuntu-latest, 2/3)").unwrap().1 = JobOutcome::Cancelled;
        assert!(matches!(check_ran(&d, &cut), Err(RanProblem::NotRun(_))));
        jobs.retain(|(n, _)| !n.starts_with("crate (ubuntu-latest"));
        assert!(matches!(check_ran(&d, &jobs), Err(RanProblem::Missing(_))));
    }

    fn read_err(line: &str) -> String {
        read(line).unwrap().unwrap_err()
    }

    #[test]
    fn a_host_leg_with_a_host_scope_is_the_same_job_so_the_scope_does_not_change_and_a_narrowed_filter_still_does() {
        // OS-scoped tests: a host leg tests fewer packages but is still the job the push to main declares, and `need` is computed from
        // the same planner, so `ran` still covers it. Only a member run's package filter (`filter`) makes a run partial.
        let mut scoped = legs(true);
        for l in &mut scoped {
            l["host_packages"] = json!("a b");
            l["host_filter"] = json!("not (package(=a))");
        }
        let need = scope_of(&plan(), &legs(true), true, &os(), &[]);
        let ran = scope_of(&plan(), &scoped, true, &os(), &["", ""]);
        assert_eq!(ran, need);
        assert_eq!(ci_scope(ran, false, need.clone(), true).complete(), Ok(()));
        // Positive control: the same legs with a member filter are partial.
        scoped[1]["filter"] = json!("package(=a)");
        let narrowed = scope_of(&plan(), &scoped, true, &os(), &["", ""]);
        assert!(ci_scope(narrowed, false, need, true).complete().is_err());
    }

    #[test]
    fn a_tag_needs_an_every_os_run_and_a_landing_does_not() {
        let mut scoped = declared(true, &["", ""]);
        scoped.ran.os_full = false;
        assert_eq!(scoped.complete(), Ok(()), "a landing may be scoped");
        let why = scoped.complete_for_tag().unwrap_err();
        assert!(why.contains("OS-scoped"), "{why}");
        // Positive control: the same run with os_full is fine for a tag; a line without the key is an error, never "every OS"
        // (review of 8cab10234, P2-2: fail closed).
        let full = declared(true, &["", ""]);
        assert_eq!(full.complete_for_tag(), Ok(()));
        let old = parse(full.line().replace(",\"os_full\":true", "").trim_start_matches(SCOPE_LINE));
        assert!(old.unwrap_err().contains("os_full"));
        assert!(!full.line().is_empty());
    }

    #[test]
    fn a_run_missing_one_engine_is_not_complete_and_the_positive_control_with_it_is() {
        // The D4 hole: a full run without WebKit. Without the engine the same run, in the same scope function, is incomplete.
        let without = declared(false, &[]);
        let why = without.complete().unwrap_err();
        assert!(why.contains("engines webkit"), "{why}");
        assert!(why.contains("dyno@ubuntu-latest:e2e-webkit"), "the missing leg is named too: {why}");
        assert_eq!(declared(true, &[]).complete(), Ok(()));
    }

    #[test]
    fn a_run_missing_one_app_or_one_runner_is_not_complete() {
        let mut d = declared(true, &[]);
        d.ran.apps.remove("spotter@ubuntu-latest");
        assert!(d.complete().unwrap_err().contains("apps spotter@ubuntu-latest"));
        let mut d = declared(true, &[]);
        d.ran.jobs.remove("crate@windows-latest");
        assert!(d.complete().unwrap_err().contains("jobs crate@windows-latest"));
        // A wider run is complete: extra jobs or apps do no harm.
        let mut d = declared(true, &[]);
        d.ran.apps.insert("pitwall@ubuntu-latest".into());
        assert_eq!(d.complete(), Ok(()));
    }

    #[test]
    fn a_scope_with_inactive_apps_names_them_is_complete_for_the_rest_and_never_covers_them() {
        // The planner leaves the inactive apps out of both `ran` and `need`: the foundation run is complete, and its line names them.
        let d = declared(true, &[]).with_inactive(["pitwall".to_string()].into());
        assert_eq!(d.complete(), Ok(()));
        let line = d.line();
        assert!(line.contains(r#""inactive":["pitwall"]"#), "{line}");
        let back = read(&line).unwrap().unwrap();
        assert_eq!(back, d);
        let why = back.covers_app("pitwall").unwrap_err();
        assert!(
            why.contains("pitwall was inactive") && why.contains("docs/ci-active-apps.toml"),
            "{why}"
        );
        // Positive controls: an active app is covered, and a line without the key (every app planned) covers every app.
        assert_eq!(back.covers_app("dyno"), Ok(()));
        let old = read(TEST_COMPLETE).unwrap().unwrap();
        assert!(old.inactive.is_empty() && old.covers_app("pitwall").is_ok());
        // A malformed list is refused, never read as "none inactive".
        assert!(read_err(&line.replace(r#"["pitwall"]"#, "7")).contains("inactive"));
        assert!(read_err(&line.replace(r#"["pitwall"]"#, r#"[""]"#)).contains("inactive"));
    }

    #[test]
    fn a_member_run_narrowed_to_packages_is_not_whole() {
        let d = declared(true, &["package(=fx-b)", ""]);
        assert!(d.complete().unwrap_err().contains("whole-workspace tests"), "{:?}", d.complete());
        let narrowed_leg = {
            let mut l = legs(true);
            l[0]["filter"] = json!("package(=x)");
            l
        };
        let ran = scope_of(&plan(), &narrowed_leg, true, &os(), &[]);
        assert!(!ran.whole);
    }

    #[test]
    fn a_rerun_and_a_run_that_ran_nothing_are_never_complete() {
        let need = scope_of(&plan(), &legs(true), true, &os(), &[]);
        let rerun = ci_scope(Scope::default(), true, need.clone(), true);
        assert!(rerun.complete().unwrap_err().contains("rerun"), "{:?}", rerun.complete());
        // Even a rerun that claims to have run everything is a rerun.
        let mut claims = declared(true, &[]);
        claims.rerun = true;
        assert!(claims.complete().is_err());
        // A skipped or reused run declares an empty `ran`: it covers nothing.
        let skipped = Declared {
            ran: Scope::default(),
            ..declared(true, &[])
        };
        assert!(skipped.complete().is_err());
        // An empty need proves nothing (ER-33): a run that ran nothing against a need of nothing is not complete.
        let nothing = Declared {
            workflow: "ci.yml".into(),
            rerun: false,
            os: None,
            ran: Scope {
                whole: true,
                os_full: true,
                ..Scope::default()
            },
            need: Scope {
                whole: true,
                os_full: true,
                ..Scope::default()
            },
            need_whole: true,
            base: None,
            inactive: BTreeSet::new(),
            reused: BTreeMap::new(),
        };
        assert!(nothing.complete().unwrap_err().contains("empty"));
    }

    #[test]
    fn a_smoke_or_affected_run_is_never_complete_and_a_tag_needs_a_whole_tree_need() {
        // ADR 0117 §6, the wiring: the graph's narrowed projects are in the line, and the reader refuses them.
        let need = Scope {
            jobs: ["conformance".to_string(), "ui".to_string()].into(),
            engines: ["chromium".to_string()].into(),
            whole: true,
            os_full: true,
            ..Scope::default()
        };
        let full = ci_scope(need.clone(), false, need.clone(), true);
        assert_eq!(full.complete(), Ok(()));
        assert_eq!(full.complete_for_tag(), Ok(()));
        let smoke = ci_scope(
            Scope {
                smoke: ["ui".to_string()].into(),
                ..need.clone()
            },
            false,
            need.clone(),
            true,
        );
        assert!(smoke.complete().unwrap_err().contains("smoke set"));
        let affected = ci_scope(
            Scope {
                affected: ["app:dyno".to_string()].into(),
                ..need.clone()
            },
            false,
            need.clone(),
            true,
        );
        assert!(affected.complete().unwrap_err().contains("affected tests of app:dyno"));
        // The line carries both, and reads back the same.
        for d in [&smoke, &affected] {
            assert_eq!(read(&d.line()).unwrap().unwrap(), *d);
        }
        // A push's or a landing's need is its own diff: complete for a landing, never for a tag (review of 8cab10234, P2-3).
        let push = ci_scope(need.clone(), false, need.clone(), false);
        assert_eq!(push.complete(), Ok(()));
        assert!(push.complete_for_tag().unwrap_err().contains("whole tree"));
        // A line without os_full, without the smoke list or need_whole is unreadable, never "every OS" (P2-2: fail closed); and v1
        // is another version.
        let line = full.line();
        for (from, to) in [
            (",\"os_full\":true", ""),
            (",\"smoke\":[]", ""),
            (",\"need_whole\":true", ""),
            ("\"v\":2", "\"v\":1"),
        ] {
            assert!(line.contains(from), "{from}");
            assert!(read(&line.replacen(from, to, 1)).unwrap().is_err(), "without {from}");
        }
    }

    fn run_jobs(d: &Declared, conclusion: &str) -> Vec<(String, JobOutcome)> {
        // One job per declared key, named as ci.yml names it, all with `conclusion`.
        let mut jobs: Vec<(String, JobOutcome)> = Vec::new();
        for k in &d.ran.jobs {
            for n in names_of_job(k) {
                jobs.push((
                    match n {
                        JobName::Exact(n) => n,
                        JobName::Prefix(p) => format!("{p}x)"),
                        JobName::AnyPrefix(ps) => format!("{}x)", ps[0]),
                        JobName::Starts(p) => format!("{p})"),
                    },
                    JobOutcome::from_api(conclusion),
                ));
            }
        }
        for a in &d.ran.apps {
            if let JobName::Exact(n) = name_of_leg(a) {
                jobs.push((n, JobOutcome::from_api(conclusion)));
            }
        }
        jobs
    }

    /// A run recorded before the short checks became one job (`quick`, 2026-11-26) has its C99 and heavy tool jobs under their old
    /// names: the verdict and reuse readers still read it as covering c99 and tools (run 9054 of tests/ci-runs/reuse-push-base).
    #[test]
    fn a_run_of_the_older_ci_yml_with_its_own_c99_and_heavy_tool_jobs_still_covers_c99_and_tools() {
        let finds = |key: &str, job: &str| names_of_job(key).iter().any(|n| n.matches(job));
        // Today's names.
        assert!(finds("c99", "quick checks (C99, audit, heavy tool)"));
        assert!(finds("tools", "quick checks (C99, audit, heavy tool)") && finds("tools", "tools and shared scripts"));
        // The older ci.yml's own jobs (recorded run 9054 and every run before 2026-11-26).
        assert!(finds("c99", "C99 (no Keychain in a macOS build)"));
        assert!(finds("tools", "heavy tool"));
        // Control: an unrelated job covers neither.
        assert!(!finds("c99", "crates (fmt, workspace hack, unused deps, coverage, deny)"));
        assert!(!finds("tools", "gitleaks (secret scan)"));
    }

    #[test]
    fn ran_means_ran_a_skipped_or_cancelled_declared_job_is_partial_and_a_missing_one_is_not_measured() {
        let d = declared(true, &[]);
        let all = run_jobs(&d, "success");
        assert!(all.len() > 8, "a positive control: the declaration has jobs ({})", all.len());
        assert_eq!(check_ran(&d, &all), Ok(()));
        // The WebKit job skipped (the plan's webkit output dropped): the run declared WebKit and did not run it.
        let mut skipped = all.clone();
        let webkit = skipped.iter_mut().find(|(n, _)| n.starts_with("reference apps in WebKit")).unwrap();
        webkit.1 = JobOutcome::Skipped;
        assert!(
            matches!(check_ran(&d, &skipped), Err(RanProblem::NotRun(m)) if m.contains("e2e-webkit")),
            "{:?}",
            check_ran(&d, &skipped)
        );
        // A cancelled app leg, a job that never finished.
        let mut cancelled = all.clone();
        cancelled.iter_mut().find(|(n, _)| n == "app dyno (macos-latest)").unwrap().1 = JobOutcome::Cancelled;
        assert!(matches!(check_ran(&d, &cancelled), Err(RanProblem::NotRun(m)) if m.contains("cancelled")));
        let mut going = all.clone();
        going.iter_mut().find(|(n, _)| n == "crate (windows-latest)").unwrap().1 = JobOutcome::Unfinished;
        assert!(matches!(check_ran(&d, &going), Err(RanProblem::NotRun(m)) if m.contains("not finished")));
        // A declared job with no job of that name: the declaration cannot be checked.
        let gone: Vec<_> = all
            .iter()
            .filter(|(n, _)| !n.starts_with("reference apps in WebKit"))
            .cloned()
            .collect();
        assert!(matches!(check_ran(&d, &gone), Err(RanProblem::Missing(m)) if m.contains("e2e-webkit")));
        let no_leg: Vec<_> = all
            .iter()
            .filter(|(n, _)| n != "app dyno (ubuntu-latest, e2e-webkit)")
            .cloned()
            .collect();
        assert!(matches!(check_ran(&d, &no_leg), Err(RanProblem::Missing(m)) if m.contains("dyno@ubuntu-latest:e2e-webkit")));
        // A failed job is the run's own red, not a partial scope.
        let mut failed = all.clone();
        failed[0].1 = JobOutcome::Failed;
        assert_eq!(check_ran(&d, &failed), Ok(()));
        // Another workflow's declaration has no job names to check.
        let other = app_scope("debrief.yml", "debrief", true, true, false, false, &["chromium".to_string()]);
        assert_eq!(check_ran(&other, &[]), Ok(()));
    }

    #[test]
    fn every_job_name_the_scope_maps_is_a_job_of_the_real_ci_yml() {
        // The mapping from declared keys to display names is read against ci.yml through the planner's own accessor of its jobs, so a
        // renamed job fails here and not in a tag gate (ER-42).
        let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitea/workflows/ci.yml")).unwrap();
        let wf = crate::plan_gitea::workflow_jobs(&text).unwrap();
        let name_of = |id: &str| -> String {
            let job = &wf
                .jobs
                .iter()
                .find(|(k, _)| k == id)
                .unwrap_or_else(|| panic!("ci.yml has no job {id}"))
                .1;
            job.lines().find_map(|l| l.trim().strip_prefix("name: ")).unwrap().to_string()
        };
        for (key, prefixes) in PROJECT_JOBS {
            assert!(!prefixes.is_empty(), "{key}");
            for p in *prefixes {
                // `now|older`: the first name is today's job; the older ones are only for recorded runs.
                let now = p.split('|').next().unwrap_or(p);
                assert!(
                    wf.jobs.iter().any(|(id, _)| name_of(id).starts_with(now)),
                    "no job of ci.yml is named like {now:?} (the key {key})"
                );
            }
        }
        // The matrix legs: the `name:` templates the exact names are built from.
        assert_eq!(name_of("crate"), "crate (${{ matrix.os }}${{ matrix.part_label }})");
        assert_eq!(name_of("paddock-dev"), "paddock-dev (${{ matrix.os }})");
        assert_eq!(
            name_of("app"),
            "app ${{ matrix.app }} (${{ matrix.os }}${{ matrix.e2e_leg != \'\' && format(\', {0}\', matrix.e2e_leg) || \'\' }})"
        );
        // Positive control: a prefix that names no job is not found.
        assert!(!wf.jobs.iter().any(|(id, _)| name_of(id).starts_with("no such job")));
    }

    #[test]
    fn the_register_decides_per_producer_and_a_missing_one_is_report_only() {
        let empty = "[first_green]\n";
        assert_eq!(recorded(empty, "ci.yml"), Ok(None));
        let with = with_first_run(empty, "ci.yml", 8400).unwrap();
        assert_eq!(recorded(&with, "ci.yml"), Ok(Some(8400)));
        assert_eq!(recorded(&with, "debrief.yml"), Ok(None), "another producer stays report-only");
        // Release workflows share the plan action's producer.
        assert_eq!(producer_of("dyno-release.yml"), "plan-action");
        assert_eq!(producer_of("spotter-release.yml"), producer_of("pitwall-release.yml"));
        assert_eq!(producer_of("ci.yml"), "ci.yml");
        // The first run is recorded once; a register of another shape is an error.
        assert!(
            with_first_run(&with, "ci.yml", 9000)
                .unwrap_err()
                .contains("already recorded at run 8400")
        );
        assert!(recorded("x = 1\n", "ci.yml").unwrap_err().contains("first_green"));
        assert!(recorded("[first_green]\n\"ci.yml\" = \"x\"\n", "ci.yml").is_err());
        // On disk: the real register, then a temp copy that names a producer, then a missing one.
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (real, why) = decisive_in(&repo, "unknown-workflow.yml");
        assert!(why.is_none(), "{why:?}");
        let _ = real;
        let dir = tempfile::tempdir().unwrap();
        assert!(!decisive_in(dir.path(), "ci.yml").0 && decisive_in(dir.path(), "ci.yml").1.is_some());
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join(REGISTER), &with).unwrap();
        assert!(decisive_in(dir.path(), "ci.yml").0, "a recorded producer is decisive");
        assert!(!decisive_in(dir.path(), "debrief.yml").0);
    }

    #[test]
    fn the_test_lines_of_the_readers_are_what_they_say() {
        // The constants the verdict and reuse tests log: one complete, one missing an engine (a positive control for both).
        assert_eq!(read(TEST_COMPLETE).unwrap().unwrap().complete(), Ok(()));
        assert!(
            read(TEST_PARTIAL)
                .unwrap()
                .unwrap()
                .complete()
                .unwrap_err()
                .contains("engines webkit")
        );
    }

    #[test]
    fn webkit_is_an_engine_only_when_a_ui_is_reached() {
        let no_ui = json!({"crates": true, "ui": false});
        let s = scope_of(&no_ui, &[], true, &os(), &[]);
        assert!(s.engines.is_empty(), "a plan with no UI has no engine to miss: {:?}", s.engines);
    }

    #[test]
    fn the_line_round_trips_and_only_one_line_with_the_prefix_counts() {
        let d = declared(true, &[]);
        let line = d.line();
        assert!(line.starts_with(SCOPE_LINE) && !line.contains('\n'));
        let log = format!("2026-11-21T00:00:00.0000000Z \u{1b}[32m{line}\u{1b}[0m\nother\n");
        assert_eq!(read(&log), Some(Ok(d.clone())));
        // No line, a second line, an unreadable one, another version and another shape.
        assert_eq!(read("plan mode: full=true\n"), None);
        assert!(matches!(read(&format!("{line}\n{line}\n")), Some(Err(e)) if e.contains("2 scope lines")));
        assert!(matches!(read("plan scope: nope\n"), Some(Err(_))));
        assert!(matches!(read(&line.replace("\"v\":2", "\"v\":3").to_string()), Some(Err(e)) if e.contains("version")));
        assert!(matches!(
            read("plan scope: {\"v\":2,\"workflow\":\"ci.yml\",\"rerun\":false}\n"),
            Some(Err(_))
        ));
        // The prefix counts only at the start of a line: a mention inside a message is not a declaration.
        assert_eq!(read(&format!("the {line}\n")), None);
    }

    #[test]
    fn an_app_workflow_is_complete_only_when_full_run_and_not_a_rerun() {
        let engines = strings(&["chromium", "webkit"]);
        let at = |full, run, since| app_scope("debrief.yml", "debrief", full, run, since, false, &engines);
        assert_eq!(at(true, true, false).complete(), Ok(()));
        // Positive controls, one dimension each: not full (no WebKit), skipped (no run), a rerun.
        assert!(at(false, true, false).complete().unwrap_err().contains("engines webkit"));
        assert!(at(true, false, false).complete().is_err());
        assert!(at(true, true, true).complete().unwrap_err().contains("rerun"));
        // One engine only: the engine cannot tell a full run from a partial one, the gates job does.
        let one = strings(&["chromium"]);
        assert_eq!(
            app_scope("dyno-release.yml", "dyno", true, true, false, false, &one).complete(),
            Ok(())
        );
        assert!(
            app_scope("dyno-release.yml", "dyno", false, true, false, false, &one)
                .complete()
                .unwrap_err()
                .contains("jobs gates")
        );
    }
    #[test]
    fn a_run_that_ran_its_own_diffs_narrowed_selection_lands_but_never_counts_for_a_tag() {
        // User decision 2026-11-23 (review round 2, P1-A): a landing accepts a narrowed run that ran exactly its own diff's
        // selection; it is never complete, never for a tag.
        let need = Scope {
            jobs: ["conformance".to_string()].into(),
            apps: ["debrief@ubuntu-latest".to_string()].into(),
            whole: true,
            os_full: true,
            ..Scope::default()
        };
        let ran = Scope {
            affected: ["app:debrief".to_string()].into(),
            ..need.clone()
        };
        let landing = ci_scope(ran.clone(), false, need.clone(), false).with_base(Some("abc123"));
        assert!(landing.complete().is_err());
        assert_eq!(landing.lands(), Ok(()));
        assert!(landing.complete_for_tag().is_err());
        assert_eq!(read(&landing.line()).unwrap().unwrap(), landing, "the base reads back");
        assert_eq!(landing.base.as_deref(), Some("abc123"));
        // Controls: a narrowed run planned for the whole tree does not land (its need is not its own diff), nor does a narrowed run
        // that left out a job its need has, nor a rerun.
        assert!(ci_scope(ran.clone(), false, need.clone(), true).lands().is_err());
        let short = Scope {
            jobs: BTreeSet::new(),
            ..ran.clone()
        };
        assert!(ci_scope(short, false, need.clone(), false).lands().is_err());
        assert!(ci_scope(ran, true, need.clone(), false).lands().is_err());
        // A complete run lands too.
        assert_eq!(ci_scope(need.clone(), false, need, false).lands(), Ok(()));
    }
}
