//! The `only_tests` workflow input (`train release`'s rung 1, the failed tests only): a JSON object from a job's display name to the
//! ids of the tests to run in it, read by the plan job together with `since_run` and turned into the same outputs a narrowed rerun
//! writes ([`crate::plan_since::SincePlan`]): a nextest filterset, a vitest file list or Playwright `file:line` locations. A job the
//! input does not name is not rerun. Every id is read from a `ci triage --json` row, so it is written the way the runner printed
//! it: nextest `<binary> <test>`, a vitest spec file, a Playwright `[project] › file:line:col › title`.
//!
//! A name this cannot honour (a job that did not fail in the run, one whose runner takes no test list, an id of the wrong shape)
//! is an error, never a quiet rerun of the whole job: a rung that claims to run named tests and runs others is not measured
//! (ER-33).

use crate::plan_since::{FailedTest, PLAIN_SPEC, PLAYWRIGHT_STEPS, Scope, SincePlan, TEST_STEPS, VITEST_STEPS, filterset};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// The most test ids one `only_tests` input names (a workflow input is a short string; a bigger set goes to the next rung).
pub const MAX_IDS: usize = 200;

/// What kind of runner a test id belongs to, and a job's steps take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// cargo nextest: `<binary> <test>`.
    Nextest,
    /// vitest: a spec file.
    Vitest,
    /// Playwright: `[project] › file:line:col › title`.
    Playwright,
}

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

static PW_ID: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:\[[^\]]+\] › )?([A-Za-z0-9_./-]+):(\d+):\d+ › \S"));
static SPEC_ID: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_./-]+\.(?:spec|test)\.[cm]?[jt]sx?$"));

/// The kind of runner an id's shape says, `None` for a shape no runner of ours prints.
pub fn id_kind(id: &str) -> Option<Kind> {
    if PW_ID.is_match(id) {
        Some(Kind::Playwright)
    } else if SPEC_ID.is_match(id) {
        Some(Kind::Vitest)
    } else if id
        .split_once(' ')
        .is_some_and(|(b, t)| !b.is_empty() && !t.is_empty() && !t.contains(' '))
    {
        Some(Kind::Nextest)
    } else {
        None
    }
}

/// The kind of test list a job of a workflow takes (`None`: its runner takes none, the whole job reruns).
pub fn job_kind(workflow: &str, job_key: &str) -> Option<Kind> {
    if TEST_STEPS.iter().any(|t| t.0 == workflow && t.1 == job_key) {
        Some(Kind::Nextest)
    } else if VITEST_STEPS.iter().any(|t| t.0 == workflow && t.1 == job_key) {
        Some(Kind::Vitest)
    } else if PLAYWRIGHT_STEPS.iter().any(|t| t.0 == workflow && t.1 == job_key) {
        Some(Kind::Playwright)
    } else {
        None
    }
}

/// The location Playwright takes for a test id: `file:line` (the column and the title are not part of a location).
pub fn playwright_location(id: &str) -> Option<String> {
    PW_ID.captures(id).map(|m| format!("{}:{}", &m[1], &m[2]))
}

/// The tests to run, by job display name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OnlyTests {
    jobs: BTreeMap<String, Vec<String>>,
}

impl OnlyTests {
    /// Reads the input's JSON: `{"crate (ubuntu-latest)": ["binary test", ...], ...}`. Every job names at least one id, every id
    /// has a shape of one runner, and the whole input names at most [`MAX_IDS`].
    pub fn parse(text: &str) -> Result<OnlyTests, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("only_tests is not JSON: {e}"))?;
        let obj = v.as_object().ok_or("only_tests is not a JSON object of job name to test ids")?;
        let mut jobs = BTreeMap::new();
        let mut total = 0;
        for (job, ids) in obj {
            let ids = ids
                .as_array()
                .ok_or_else(|| format!("only_tests: {job:?} is not a list of test ids"))?;
            let mut set = BTreeSet::new();
            for id in ids {
                let id = id
                    .as_str()
                    .ok_or_else(|| format!("only_tests: {job:?} has an id that is not text"))?;
                if id_kind(id).is_none() {
                    return Err(format!(
                        "only_tests: {job:?}: {id:?} is not a test id of any runner (nextest, vitest, Playwright)"
                    ));
                }
                set.insert(id.to_string());
            }
            if set.is_empty() {
                return Err(format!("only_tests: {job:?} names no test: an empty list would run nothing"));
            }
            total += set.len();
            jobs.insert(job.clone(), set.into_iter().collect());
        }
        if jobs.is_empty() {
            return Err("only_tests names no job: an empty input would run nothing".into());
        }
        if total > MAX_IDS {
            return Err(format!("only_tests names {total} tests, more than the {MAX_IDS} one input carries"));
        }
        Ok(OnlyTests { jobs })
    }

    /// The input of these ids by job (the ladder's side); [`OnlyTests::parse`] checks it again on the workflow's side.
    pub fn new(mut jobs: BTreeMap<String, Vec<String>>) -> OnlyTests {
        for ids in jobs.values_mut() {
            ids.sort();
            ids.dedup();
        }
        OnlyTests { jobs }
    }

    /// The input's text.
    pub fn encode(&self) -> String {
        serde_json::to_string(&self.jobs).unwrap_or_default()
    }

    /// The ids by job.
    pub fn jobs(&self) -> &BTreeMap<String, Vec<String>> {
        &self.jobs
    }

    /// How many test ids the input names.
    pub fn count(&self) -> usize {
        self.jobs.values().map(Vec::len).sum()
    }
}

impl SincePlan {
    /// Narrows this rerun to the named tests: each named job's scope becomes exactly its ids, every other job does not rerun.
    /// An error (and no change) for a job that did not fail in the run, one with no known workflow job, one whose runner takes no
    /// test list, an id of another runner's shape, or a plan that already reruns everything.
    pub fn apply_only_tests(&mut self, only: &OnlyTests) -> Result<(), String> {
        if let Some(why) = &self.everything {
            return Err(format!(
                "only_tests cannot narrow the rerun of run {}: it reruns everything ({why})",
                self.run
            ));
        }
        let mut scopes: Vec<(u64, Scope)> = Vec::new();
        for (name, ids) in only.jobs() {
            let r = self
                .reruns
                .iter()
                .find(|r| &r.name == name)
                .ok_or_else(|| format!("only_tests names the job {name:?}, which did not fail in run {}", self.run))?;
            let key = r
                .key
                .as_deref()
                .ok_or_else(|| format!("only_tests: the workflow job of {name:?} is unknown"))?;
            let kind = job_kind(&self.workflow, key)
                .ok_or_else(|| format!("only_tests: job {key} ({name:?}) runs no step that takes a test list; rerun it whole"))?;
            if let Some(id) = ids.iter().find(|id| id_kind(id) != Some(kind)) {
                return Err(format!("only_tests: {id:?} is not a {kind:?} test id, and job {key} runs {kind:?}"));
            }
            let scope = match kind {
                Kind::Nextest => {
                    let tests: Vec<FailedTest> = ids
                        .iter()
                        .filter_map(|id| id.split_once(' '))
                        .map(|(binary, test)| FailedTest {
                            binary: binary.to_string(),
                            test: test.to_string(),
                        })
                        .collect();
                    let filter = filterset(&tests)
                        .ok_or_else(|| format!("only_tests: a test binary of {name:?} cannot be written in a filterset"))?;
                    Scope::Tests { filter, tests }
                }
                Kind::Vitest => {
                    if let Some(f) = ids.iter().find(|f| !PLAIN_SPEC.is_match(f)) {
                        return Err(format!("only_tests: the spec file {f:?} cannot be passed plainly"));
                    }
                    Scope::Specs(ids.clone())
                }
                Kind::Playwright => {
                    let mut locations: Vec<String> = ids.iter().filter_map(|id| playwright_location(id)).collect();
                    locations.sort();
                    locations.dedup();
                    Scope::Playwright(locations)
                }
            };
            scopes.push((r.id, scope));
        }
        for (id, scope) in scopes {
            if let Some(r) = self.reruns.iter_mut().find(|r| r.id == id) {
                r.scope = scope;
            }
        }
        self.reruns.retain(|r| only.jobs().contains_key(&r.name));
        Ok(())
    }
}

#[cfg(test)]
mod tests;
