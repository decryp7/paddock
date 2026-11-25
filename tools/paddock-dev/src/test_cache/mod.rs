//! The test-result cache (ADR 0116, accepted 2026-11-25): a test target whose inputs did not change reuses its pass.
//!
//! One definition of reuse (ER-22, rule 55): the key is [`crate::pre_push::reuse::digest`] over the repository graph's per-node input
//! hash ([`crate::repo_model::hash`], ADR 0117), the local store sits beside the gate rows' records in the git common dir, and the
//! per-target pass is read with the one nextest reader ([`crate::nextest_log`]). What the cache adds is in the parts below:
//! - [`config`]: `docs/test-cache.toml` (epoch, age rule, never-cached list, the shadow switch);
//! - [`key`]: targets, runner classes and the key;
//! - [`result`]: what a finished run proves per target (passes only, a flake is never a pass);
//! - [`store`]: records and the store port, with the local store;
//! - this file: the decision per target, and the trust rules (who may write, when nothing is read).
//!
//! Soundness limits, said plainly: a key covers what the graph's hash covers (ADR 0117 §5); a dirty node (uncommitted change in
//! its files) runs and is never stored; the doc tests of a package are not part of its target in this phase (the gate that runs them
//! runs them as before); coverage, the audits and conformance are never targets.

pub mod cmd;
pub mod config;
pub mod key;
pub mod result;
pub mod store;
#[cfg(test)]
mod tests;

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

/// The key of a candidate under `cfg` on `class`.
pub fn key_of(c: &Candidate, cfg: &Config, class: &RunnerClass) -> String {
    key(&c.target, &c.node_hash, &KeyParts { epoch: cfg.epoch, class, profile: &cfg.profile })
}

/// One decision per candidate, in order, from one bulk lookup. A store that errors turns reuse off for every target (fail open,
/// §5: everything runs, never the other way round); the cause is the second answer, for one notice. `trust` is the reader-side
/// check of a record (§7); `now` is Unix seconds.
pub fn decide(
    cands: &[Candidate],
    cfg: &Config,
    class: &RunnerClass,
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
            asks.push(Lookup { target: c.target.id.clone(), class: class.clone(), key: key_of(c, cfg, class) });
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
