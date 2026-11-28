//! The escape monitor (ADR 0117 §8 and D12, WP-9): what a nightly's failures say about the plans of the commits since the last nightly.
//!
//! Every CI plan run saves a [`Record`] of its selection (commit, base, per project and OS what ran, and what the change could reach),
//! through [`save`], the one writer. The nightly runs everything, then [`judge`]s each failure against the records of every commit since
//! the previous nightly: a failure of a target that a commit's change could reach (its `reach`) but its plan did not run is an
//! **escape**. When several commits dropped the target, the culprit is bisected within [`MAX_COMMITS`] commits and [`MAX_RUNS`] runs;
//! past the bound, or with a record missing or unreadable, the answer is `not measured`, never a guess and never clean. No database
//! (D13, amended 2026-11-27): the plan job writes its record to a file ([`crate::escape_ci::artifact::FileSink`]) that ci.yml uploads as the run's
//! artifact `plan-record`, kept [`RETENTION_DAYS`] days by the server; the nightly reads the artifacts of the commits' runs through
//! the Actions API ([`crate::escape_ci::artifact::ArtifactStore`]). ci.yml stays read-only: nothing pushes a ref.
//!
//! The old planner's rule tables may be deleted only after [`register::REQUIRED`] consecutive clean nightlies ([`register`]).

pub mod register;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod testutil;

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::Path;

/// The record format version.
pub const VERSION: u64 = 1;
/// The most commits since the last nightly a bisect covers (ADR 0117 §8, review round 2 P2-7).
pub const MAX_COMMITS: usize = 30;
/// The most runs of a failing target a nightly's bisects use together.
pub const MAX_RUNS: usize = 10;
/// How long a record is kept (the artifact's `retention-days`, which a test ties to ci.yml): from one nightly to the next across a weekend and a paused nightly (ADR 0117 §8).
pub const RETENTION_DAYS: i64 = 14;
/// The whole-job target: a job failed and the report names no test.
pub const WHOLE_JOB: &str = "*";

/// What a plan ran for one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sel {
    /// The OS legs that run it.
    pub os: Vec<String>,
    /// The planner's label: `all`, `rust`, `ui`, `tests`, `smoke` or `release`.
    pub runs: String,
    /// The exact targets, when the plan names them (a `tests` selection); `None`: by label.
    pub targets: Option<Vec<String>>,
}

impl Sel {
    /// Whether the plan ran `target` on `os`. The labels `tests`, `smoke` and `release` run no enumerable target, so they cover only
    /// what `targets` names (and the whole-job target, since they run something).
    pub fn covers(&self, os: &str, target: &str) -> bool {
        if !self.os.iter().any(|o| o == os) {
            return false;
        }
        if let Some(t) = &self.targets {
            return target == WHOLE_JOB || t.iter().any(|x| x == target);
        }
        match self.runs.as_str() {
            "all" => true,
            "rust" => target == WHOLE_JOB || target.starts_with("nextest:"),
            "ui" => target == WHOLE_JOB || !target.starts_with("nextest:"),
            _ => target == WHOLE_JOB,
        }
    }
}

/// One plan run's selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The commit the plan was made for.
    pub commit: String,
    /// The base it was diffed against, when it had one.
    pub base: Option<String>,
    /// The commit time (seconds since the epoch).
    pub at: i64,
    /// Per project, why the change could reach it (the graph's reasons); a project not here could not have been affected.
    pub reach: BTreeMap<String, Vec<String>>,
    /// Per project, what ran.
    pub selected: BTreeMap<String, Sel>,
}

impl Record {
    /// The record as JSON.
    pub fn to_json(&self) -> Value {
        let selected: Map<String, Value> = self
            .selected
            .iter()
            .map(|(p, s)| (p.clone(), json!({"os": s.os, "runs": s.runs, "targets": s.targets})))
            .collect();
        json!({"version": VERSION, "commit": self.commit, "base": self.base, "at": self.at, "reach": self.reach, "selected": selected})
    }

    /// Parses a record. An unknown version, a missing field or a wrong type is an error naming it (parse, don't validate).
    pub fn parse(text: &str) -> Result<Record, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        if v["version"].as_u64() != Some(VERSION) {
            return Err(format!("version {} is not {VERSION}", v["version"]));
        }
        let strings = |v: &Value, what: &str| -> Result<Vec<String>, String> {
            v.as_array()
                .ok_or_else(|| format!("{what} is not a list"))?
                .iter()
                .map(|s| s.as_str().map(str::to_string).ok_or_else(|| format!("{what} holds a non-string")))
                .collect()
        };
        let commit = v["commit"]
            .as_str()
            .filter(|c| valid_commit(c))
            .ok_or("commit is missing or not a hex id")?
            .to_string();
        let at = v["at"].as_i64().ok_or("at is missing")?;
        let mut reach = BTreeMap::new();
        for (p, why) in v["reach"].as_object().ok_or("reach is missing")? {
            reach.insert(p.clone(), strings(why, "reach")?);
        }
        let mut selected = BTreeMap::new();
        for (p, s) in v["selected"].as_object().ok_or("selected is missing")? {
            let targets = match &s["targets"] {
                Value::Null => None,
                t => Some(strings(t, "targets")?),
            };
            selected.insert(
                p.clone(),
                Sel {
                    os: strings(&s["os"], "os")?,
                    runs: s["runs"].as_str().ok_or("runs is missing")?.to_string(),
                    targets,
                },
            );
        }
        Ok(Record {
            commit,
            base: v["base"].as_str().map(str::to_string),
            at,
            reach,
            selected,
        })
    }
}

/// A commit id: 7 to 64 hex digits (a path or option never gets through to git).
pub fn valid_commit(c: &str) -> bool {
    (7..=64).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What a store holds for a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load {
    /// A readable record.
    Ok(Box<Record>),
    /// None was saved (the plan run did not finish, or the record expired).
    Missing,
    /// A record that cannot be read, with why.
    Unreadable(String),
}

/// Where the nightly reads records from (D13: no database).
pub trait Store {
    /// The record of a commit.
    fn get(&self, commit: &str) -> Load;
}

/// Where a plan run writes its record.
pub trait Sink {
    /// Saves a record, replacing one of the same commit.
    fn put(&self, r: &Record) -> Result<(), String>;
}

/// The one writer of selection records.
pub fn save(sink: &dyn Sink, r: &Record) -> Result<(), String> {
    if !valid_commit(&r.commit) {
        return Err(format!("{:?} is not a commit id", r.commit));
    }
    sink.put(r)
}

/// The records a nightly judges, oldest first: from `head` back along each record's base until `reached` says the previous nightly's
/// commit is reached. A plan run of a push to main covers `base..commit`, so the chain of bases covers every commit since the last
/// nightly without a record per commit. A commit whose record is missing or unreadable ends the chain and is the last one listed
/// (so [`judge`] reports it); a record with no base ran everything and ends it too. The walk is bounded and never loops.
pub fn chain(store: &dyn Store, head: &str, reached: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = head.to_string();
    while out.len() < 4 * MAX_COMMITS && !reached(&cur) && !out.contains(&cur) {
        out.push(cur.clone());
        match store.get(&cur) {
            Load::Ok(r) => match r.base {
                Some(b) => cur = b,
                None => break,
            },
            _ => break,
        }
    }
    out.reverse();
    out
}

/// A target that failed in the nightly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Failure {
    /// The project id.
    pub project: String,
    /// The OS leg.
    pub os: String,
    /// The target node id, or [`WHOLE_JOB`].
    pub target: String,
}

/// Runs a failing target at a commit, for the bisect.
pub trait Runner {
    /// Whether `f` fails at `commit`; `Err` when it could not be run.
    fn fails_at(&mut self, commit: &str, f: &Failure) -> Result<bool, String>;
}

/// An escape: a failure whose commit's plan dropped it though the change could reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escape {
    /// The commit whose plan dropped the target.
    pub commit: String,
    /// The failure.
    pub failure: Failure,
    /// The graph's reasons the change reached the project.
    pub reason: String,
    /// Whether a bisect found the commit (else it was the only one that dropped it).
    pub bisected: bool,
}

/// What a failure of the nightly came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// An escape.
    Escape(Escape),
    /// Not measured, with why (never clean).
    NotMeasured(Failure, String),
    /// Every commit that could have caused it ran it (or none could reach it): not an escape.
    Covered(Failure),
}

/// The judgement of a nightly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// One per failure.
    pub outcomes: Vec<Outcome>,
    /// The commits whose record is missing or unreadable, with why: the nightly is not measured for them.
    pub unreadable: Vec<(String, String)>,
    /// How many bisect runs were used.
    pub runs_used: usize,
}

impl Judged {
    /// The escapes.
    pub fn escapes(&self) -> Vec<&Escape> {
        self.outcomes
            .iter()
            .filter_map(|o| if let Outcome::Escape(e) = o { Some(e) } else { None })
            .collect()
    }

    /// The failures not measured.
    pub fn not_measured(&self) -> usize {
        self.outcomes.iter().filter(|o| matches!(o, Outcome::NotMeasured(..))).count()
    }

    /// Clean: every record was read, no failure is an escape or unmeasured. The only way to get `true`.
    pub fn clean(&self) -> bool {
        self.unreadable.is_empty() && self.escapes().is_empty() && self.not_measured() == 0
    }

    /// The `::error::` lines: one per escape, one per not-measured failure or record (ER-27: the cause and the next step).
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in self.escapes() {
            out.push(format!(
                "::error::escape: commit {} did not run {} on {} (project {}) although its change could reach it ({}); it fails in the nightly{}. Next: fix the graph edge or declared input that missed it, add a golden for it, and keep the old rule tables (ADR 0117 D12).",
                short(&e.commit),
                e.failure.target,
                e.failure.os,
                e.failure.project,
                e.reason,
                if e.bisected { " (bisected)" } else { "" },
            ));
        }
        for o in &self.outcomes {
            if let Outcome::NotMeasured(f, why) = o {
                out.push(format!(
                    "::error::not measured: {} on {} (project {}) fails in the nightly and the monitor cannot tell if a plan dropped it: {why}",
                    f.target, f.os, f.project
                ));
            }
        }
        for (c, why) in &self.unreadable {
            out.push(format!(
                "::error::no selection record for {}: {why}. The nightly counts as not measured, never clean; the plan run of that commit saves it (dev plan --ci).",
                short(c)
            ));
        }
        out
    }
}

fn short(c: &str) -> &str {
    &c[..c.len().min(9)]
}

/// Judges the nightly's `failures` against the records of `commits` (every commit since the previous nightly, oldest first).
/// `runner` bisects when several commits dropped a target; `runs_left` is the nightly's budget ([`MAX_RUNS`]).
pub fn judge(commits: &[String], store: &dyn Store, failures: &[Failure], runner: &mut dyn Runner) -> Judged {
    let mut records: Vec<Record> = Vec::new();
    let mut unreadable = Vec::new();
    for c in commits {
        match store.get(c) {
            Load::Ok(r) => records.push(*r),
            Load::Missing => unreadable.push((c.clone(), "no record was saved".to_string())),
            Load::Unreadable(why) => unreadable.push((c.clone(), format!("unreadable: {why}"))),
        }
    }
    let mut used = 0;
    let mut outcomes = Vec::new();
    for f in failures {
        if !unreadable.is_empty() {
            let names: Vec<&str> = unreadable.iter().map(|(c, _)| short(c)).collect();
            outcomes.push(Outcome::NotMeasured(
                f.clone(),
                format!("no readable record for {}", names.join(", ")),
            ));
            continue;
        }
        let dropped: Vec<&Record> = records
            .iter()
            .filter(|r| r.reach.contains_key(&f.project) && !r.selected.get(&f.project).is_some_and(|s| s.covers(&f.os, &f.target)))
            .collect();
        let reason = |r: &Record| r.reach.get(&f.project).map(|w| w.join("; ")).unwrap_or_default();
        outcomes.push(match dropped.as_slice() {
            [] => Outcome::Covered(f.clone()),
            [one] => Outcome::Escape(Escape {
                commit: one.commit.clone(),
                failure: f.clone(),
                reason: reason(one),
                bisected: false,
            }),
            many => {
                if commits.len() > MAX_COMMITS {
                    outcomes.push(Outcome::NotMeasured(
                        f.clone(),
                        format!(
                            "{} commits since the last nightly ({}..{}) are over the bisect bound of {MAX_COMMITS}",
                            commits.len(),
                            short(&commits[0]),
                            short(&commits[commits.len() - 1])
                        ),
                    ));
                    continue;
                }
                match bisect(many, f, runner, &mut used) {
                    Ok(r) => Outcome::Escape(Escape {
                        commit: r.commit.clone(),
                        failure: f.clone(),
                        reason: reason(r),
                        bisected: true,
                    }),
                    Err(why) => Outcome::NotMeasured(f.clone(), why),
                }
            }
        });
    }
    Judged {
        outcomes,
        unreadable,
        runs_used: used,
    }
}

/// The first of `dropped` (oldest first) at which `f` fails, by binary search: the target fails at the newest of them (it fails in the
/// nightly, and every later commit ran it), so the search needs about log2(n) runs. Stops with a reason at the budget.
fn bisect<'a>(dropped: &[&'a Record], f: &Failure, runner: &mut dyn Runner, used: &mut usize) -> Result<&'a Record, String> {
    let (mut lo, mut hi) = (0, dropped.len() - 1);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if *used >= MAX_RUNS {
            return Err(format!(
                "the bisect between {} and {} needs more than the {MAX_RUNS} runs a nightly may use",
                short(&dropped[lo].commit),
                short(&dropped[hi].commit)
            ));
        }
        *used += 1;
        match runner.fails_at(&dropped[mid].commit, f) {
            Ok(true) => hi = mid,
            Ok(false) => lo = mid + 1,
            Err(e) => {
                return Err(format!(
                    "{} commits dropped it ({}..{}) and the bisect could not run: {e}",
                    dropped.len(),
                    short(&dropped[0].commit),
                    short(&dropped[dropped.len() - 1].commit)
                ));
            }
        }
    }
    Ok(dropped[lo])
}

/// A runner that cannot run anything: the bisect is `not measured` (no bisect command configured).
pub struct NoRunner;

impl Runner for NoRunner {
    fn fails_at(&mut self, _: &str, _: &Failure) -> Result<bool, String> {
        Err("no bisect command is configured (--bisect-cmd)".to_string())
    }
}

/// A runner that runs a command with the commit, project, OS and target in `ESCAPE_COMMIT`, `ESCAPE_PROJECT`, `ESCAPE_OS` and
/// `ESCAPE_TARGET`: exit 0 means the target passes, 1 that it fails, anything else that it could not be run.
pub struct CommandRunner<'a> {
    /// The program (a script of the repository).
    pub program: &'a Path,
    /// The folder it runs in.
    pub root: &'a Path,
}

impl Runner for CommandRunner<'_> {
    fn fails_at(&mut self, commit: &str, f: &Failure) -> Result<bool, String> {
        let mut cmd = paddock_proc::command(self.program);
        cmd.current_dir(self.root)
            .env("ESCAPE_COMMIT", commit)
            .env("ESCAPE_PROJECT", &f.project)
            .env("ESCAPE_OS", &f.os)
            .env("ESCAPE_TARGET", &f.target);
        let st = cmd.status().map_err(|e| format!("{}: {e}", self.program.display()))?;
        match st.code() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            other => Err(format!("{} exited with {other:?}", self.program.display())),
        }
    }
}
