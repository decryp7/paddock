//! The test-result cache (ADR 0116, accepted 2026-11-25): a test target whose inputs did not change reuses its pass.
//!
//! One definition of reuse (ER-22, rule 55): the key is [`crate::pre_push::reuse::digest`] over the repository graph's per-node input
//! hash ([`crate::repo_model::hash`], ADR 0117), the local store sits beside the gate rows' records in the git common dir, and the
//! per-target pass is read with the one nextest reader ([`crate::nextest_log`]). What the cache adds is in the parts below:
//! - [`config`]: `docs/test-cache.toml` (epoch, age rule, never-cached list, the shadow switch);
//! - [`key`]: targets, runner classes and the key;
//! - [`result`]: what a finished run proves per target (passes only, a flake is never a pass);
//! - [`store`]: records and the store port, with the local store; [`s3`]: the RustFS store of CI and of every machine;
//! - this file: the decision per target, and the trust rules (who may write, when nothing is read).
//!
//! Soundness limits, said plainly: a key covers what the graph's hash covers (ADR 0117 §5); a dirty node (uncommitted change in
//! its files) runs and is never stored; the doc tests of a package are not part of its target in this phase (the gate that runs them
//! runs them as before); coverage, the audits and conformance are never targets.

pub mod cmd;
pub mod config;
pub mod key;
pub mod plan;
pub mod result;
pub mod s3;
pub mod store;
#[cfg(test)]
mod tests;
pub mod trust;

use config::Config;
use key::{KeyParts, RunnerClass, Target, key};
use std::time::Duration;
use store::{Lookup, Record, Store};

/// A target with the node hash its key is made of.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The target.
    pub target: Target,
    /// The graph's input hash of its node.
    pub node_hash: String,
    /// A file of the node has uncommitted changes: the hash describes HEAD, so the target runs and is not stored.
    pub dirty: bool,
}

/// Why a target runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Listed in `no_test_cache`.
    Never,
    /// Uncommitted changes under its inputs.
    Dirty,
    /// No record under this key.
    Miss,
    /// A record, older than the limit of the paused nightly (decision D7).
    TooOld,
    /// A record that failed the trust check, and what it said.
    Untrusted(String),
    /// The store did not answer, or reuse is off for this run (the cause): everything runs.
    Off(String),
}

impl Why {
    /// The words of a log line.
    pub fn text(&self) -> String {
        match self {
            Why::Never => "listed in no_test_cache".into(),
            Why::Dirty => "uncommitted changes under its inputs".into(),
            Why::Miss => "no record under this key".into(),
            Why::TooOld => "its record is older than the limit while the nightly is paused".into(),
            Why::Untrusted(w) => format!("its record failed the trust check: {w}"),
            Why::Off(w) => format!("reuse is off: {w}"),
        }
    }
}

/// What to do with one target.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// A valid record exists: the target's pass can be reused.
    Reuse(Record),
    /// Run the target.
    Run(Why),
}

/// The key of a candidate under `cfg` on `class`, for a run narrowed to nextest filterset `filter` (empty: the whole package).
pub fn key_of(c: &Candidate, cfg: &Config, class: &RunnerClass, filter: &str) -> String {
    key(
        &c.target,
        &c.node_hash,
        &KeyParts {
            epoch: cfg.epoch,
            class,
            profile: &cfg.profile,
            filter,
        },
    )
}

/// One decision per candidate, in order, from one bulk lookup. A store that errors turns reuse off for every target (fail open,
/// §5: everything runs, never the other way round); the cause is the second answer, for one notice. `trust` is the reader-side
/// check of a record (§7); `now` is Unix seconds. `filter` is the filterset the run this lookup is for is narrowed to: a record counts
/// only under the same filter.
pub fn decide(
    cands: &[Candidate],
    cfg: &Config,
    class: &RunnerClass,
    filter: &str,
    store: &dyn Store,
    now: u64,
    trust: &dyn Fn(&Record) -> Result<(), String>,
) -> (Vec<Decision>, Option<String>) {
    let mut asks: Vec<Lookup> = Vec::new();
    let mut at: Vec<Option<usize>> = Vec::new();
    for c in cands {
        if cfg.never_cached(&c.target.node) || c.dirty {
            at.push(None);
        } else {
            at.push(Some(asks.len()));
            asks.push(Lookup {
                target: c.target.id.clone(),
                class: class.clone(),
                key: key_of(c, cfg, class, filter),
            });
        }
    }
    let found = match store.get_many(&asks) {
        Ok(f) if f.len() == asks.len() => f,
        Ok(f) => return off(cands, format!("the store answered {} records for {} keys", f.len(), asks.len())),
        Err(e) => return off(cands, e.to_string()),
    };
    let limit = cfg.max_age();
    let out = cands
        .iter()
        .zip(at)
        .map(|(c, i)| {
            let Some(i) = i else {
                return Decision::Run(if cfg.never_cached(&c.target.node) { Why::Never } else { Why::Dirty });
            };
            let Some(r) = &found[i] else { return Decision::Run(Why::Miss) };
            if limit.is_some_and(|l: Duration| now.saturating_sub(r.at) > l.as_secs()) {
                return Decision::Run(Why::TooOld);
            }
            match trust(r) {
                Ok(()) => Decision::Reuse(r.clone()),
                Err(w) => Decision::Run(Why::Untrusted(w)),
            }
        })
        .collect();
    (out, None)
}

fn off(cands: &[Candidate], cause: String) -> (Vec<Decision>, Option<String>) {
    (cands.iter().map(|_| Decision::Run(Why::Off(cause.clone()))).collect(), Some(cause))
}

/// Whether this run may read the cache at all (§6): `Err` names why not, and every target then runs. Never read: a tag push, a
/// release workflow, `tag.yml`'s gates, the nightly (it is the uncached net) and a dispatch with `test_cache=off` (`PLAN_TEST_CACHE`).
pub fn read_allowed(env: &dyn Fn(&str) -> Option<String>) -> Result<(), String> {
    let get = |k: &str| env(k).unwrap_or_default();
    let (r, wf, event) = (get("GITHUB_REF"), get("GITHUB_WORKFLOW_REF"), get("GITHUB_EVENT_NAME"));
    let file = wf.split('@').next().unwrap_or("").rsplit('/').next().unwrap_or("");
    if get("PLAN_TEST_CACHE") == "off" {
        Err("the dispatch says test_cache=off".into())
    } else if r.starts_with("refs/tags/") {
        Err("a tag run proves a release from its own run (ER-15)".into())
    } else if file.ends_with("-release.yml") || file == "tag.yml" {
        Err(format!("{file} is a release workflow (ER-15)"))
    } else if file == "nightly.yml" || event == "schedule" {
        Err("the nightly runs every target uncached: it is the net".into())
    } else {
        Ok(())
    }
}

/// Whether this run may write records (§7). A local run may (its store is its own); in CI only `refs/heads/main` and
/// `refs/heads/train/*` may, and never a run that declared a rerun (`PLAN_SINCE_RUN`: it ran only the failed tests, so it did not run
/// the target in full).
pub fn write_allowed(env: &dyn Fn(&str) -> Option<String>) -> Result<(), String> {
    let get = |k: &str| env(k).unwrap_or_default();
    if get("PLAN_SINCE_RUN").trim() != "" {
        return Err("the run declared a rerun of failed tests, so no target ran in full".into());
    }
    if get("GITHUB_ACTIONS").is_empty() && get("GITEA_ACTIONS").is_empty() {
        return Ok(());
    }
    let r = get("GITHUB_REF");
    if r == "refs/heads/main" || r.starts_with("refs/heads/train/") {
        Ok(())
    } else {
        Err(format!("only main and train runs write records, this run is on `{r}`"))
    }
}

/// The nextest filterset that leaves the reused packages out: `not (package(=a) | package(=b))`, the form of
/// [`crate::plan_ci::package_filter`]; empty when nothing is reused. A gate that narrows its own filter ANDs this with it.
pub fn exclude_filter(reused: &[&Target]) -> String {
    if reused.is_empty() {
        return String::new();
    }
    format!(
        "not ({})",
        reused
            .iter()
            .map(|t| format!("package(={})", t.package))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

/// Whether this run is the nightly (the uncached net, §6): a `schedule` event or `nightly.yml`.
pub fn is_nightly(env: &dyn Fn(&str) -> Option<String>) -> bool {
    let get = |k: &str| env(k).unwrap_or_default();
    get("GITHUB_EVENT_NAME") == "schedule" || get("GITHUB_WORKFLOW_REF").split('@').next().unwrap_or("").ends_with("/nightly.yml")
}

/// The nightly's net (§6): for every target that failed, deletes the record under its current key and returns the deleted ones, so the
/// run can name each cached pass the world no longer supports (an environment drift, or a test that reads something its key lacks) and
/// open a fix item for the key's definition. A target with no record under its key is not in the answer.
pub fn invalidate(store: &dyn Store, failed: &[Lookup]) -> Result<Vec<Record>, store::StoreError> {
    Ok(store.delete_many(failed)?.into_iter().flatten().collect())
}

/// Where a record came from.
#[derive(Debug, Clone)]
pub struct RunMeta {
    /// The run (CI run id, or a local token).
    pub run: String,
    /// The job's display name (`crate (macos-latest)`), which the reader finds among the run's jobs; empty for a local run.
    pub job: String,
    /// The run's attempt the job ran in (a rerun of a run is another attempt); 0 for a local run.
    pub attempt: u64,
    /// The ref.
    pub git_ref: String,
    /// The commit tested.
    pub commit: String,
    /// When, Unix seconds.
    pub at: u64,
    /// The runner's OS build, reported.
    pub os_build: String,
}

/// The records a nextest log proves, and no others (ADR 0116 §3, ER-12, ER-33): one per candidate whose package ran in the log with
/// every test passing on its first try, the log being whole. A package that failed, was flaky, passed on a retry, never appeared, or has
/// uncommitted changes under its inputs gets none. Each record is keyed by `filter`, the filterset the run was narrowed to, so a pass of
/// the selected tests is never read as a pass of the whole package. The one place `record` decides what is stored.
pub fn records_from_log(log: &str, cands: &[Candidate], cfg: &Config, class: &RunnerClass, filter: &str, m: &RunMeta) -> Vec<Record> {
    let asked: Vec<String> = cands.iter().map(|c| c.target.package.clone()).collect();
    let judged = result::judge(log, &asked);
    cands
        .iter()
        .filter(|c| !c.dirty && judged.passed.contains(&c.target.package))
        .map(|c| Record {
            key: key_of(c, cfg, class, filter),
            target: c.target.id.clone(),
            class: class.clone(),
            filter: key::normalize_filter(filter),
            run: m.run.clone(),
            job: m.job.clone(),
            attempt: m.attempt,
            git_ref: m.git_ref.clone(),
            commit: m.commit.clone(),
            tests: judged.tests.get(&c.target.package).copied().unwrap_or(0),
            secs: 0.0,
            at: m.at,
            os_build: m.os_build.clone(),
        })
        .collect()
}

/// What the shadow phase saw in one run (ADR 0116 §8, ER-41): of the targets whose key matched a valid record, how many ran in this run
/// and passed, and which ran and failed. A matched target that fails is a soundness finding: the key missed an input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shadow {
    /// Matched targets that ran in this run's log.
    pub matched: usize,
    /// Of those, the ones that passed whole on the first try.
    pub passed: usize,
    /// The matched targets that ran and did not pass, by target id.
    pub findings: Vec<String>,
}

impl Shadow {
    /// The one line a record step prints and `ci test-cache shadow-run` reads back.
    pub fn line(&self) -> String {
        format!(
            "test-cache shadow: matched {}, passed {}, findings {}",
            self.matched,
            self.passed,
            self.findings.len()
        )
    }
}

/// The shadow outcome of `matched` (the candidates a valid record covered before this run) against what the log proved. A matched target
/// that never appeared in the log did not run (it is not counted); one whose run could not be read as whole is not a finding either
/// (nothing can be said, and the run is not clean for the switch: it prints `matched 0`).
pub fn shadow_outcome(matched: &[&Candidate], judged: &result::Judged) -> Shadow {
    let mut s = Shadow::default();
    for c in matched {
        let p = &c.target.package;
        if judged.passed.contains(p) {
            s.matched += 1;
            s.passed += 1;
        } else if judged.failed.contains(p) {
            s.matched += 1;
            s.findings.push(c.target.id.clone());
        }
    }
    s
}

/// The shadow lines of job logs, summed: how many lines were read, and the totals. `None` when no log has a line (nothing measured).
pub fn read_shadow(logs: &[String]) -> Option<(usize, Shadow)> {
    let mut lines = 0;
    let mut sum = Shadow::default();
    for log in logs {
        for l in log.lines() {
            let l = crate::log_text::plain(l);
            let Some(rest) = l.trim().split("test-cache shadow: ").nth(1) else {
                continue;
            };
            let n = |key: &str| {
                rest.split(&format!("{key} "))
                    .nth(1)
                    .and_then(|r| r.split([',', ' ']).next())
                    .and_then(|v| v.parse::<usize>().ok())
            };
            let (Some(m), Some(p), Some(f)) = (n("matched"), n("passed"), n("findings")) else {
                continue;
            };
            lines += 1;
            sum.matched += m;
            sum.passed += p;
            // The names are in the `::error::` lines; the count is what the switch needs.
            sum.findings.extend((0..f).map(|i| format!("finding {}", i + 1)));
        }
    }
    (lines > 0).then_some((lines, sum))
}

/// The text of `docs/test-cache.toml` with `run` added to `[switch] green_runs`, once (a second call with the same id changes nothing).
/// Reads the file through [`config::Config`] before and after, so a text it cannot read is an error and the result always parses.
pub fn with_green_run(text: &str, run: u64) -> Result<String, String> {
    let before = Config::parse(text)?;
    if before.switch.green_runs.contains(&run) {
        return Ok(text.to_string());
    }
    let mut runs = before.switch.green_runs.clone();
    runs.push(run);
    let list = format!("green_runs = [{}]", runs.iter().map(u64::to_string).collect::<Vec<_>>().join(", "));
    let out = match text.lines().position(|l| l.trim_start().starts_with("green_runs")) {
        Some(i) => {
            let mut v: Vec<String> = text.lines().map(str::to_string).collect();
            v[i] = list;
            let mut t = v.join("\n");
            t.push('\n');
            t
        }
        None => return Err("docs/test-cache.toml has no `green_runs` line under [switch]".into()),
    };
    let after = Config::parse(&out)?;
    if after.switch.green_runs != runs {
        return Err("the edit did not produce the expected green_runs".into());
    }
    Ok(out)
}
