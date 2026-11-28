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
#[cfg(test)]
mod declared_tests;
pub mod key;
pub mod plan;
pub mod probe;
pub mod reasons;
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
    /// The reads of its crate the graph cannot resolve (a manifest folder in a variable, `ancestors`, a join with a non-literal, ...),
    /// one line each: its key cannot say what it reads, so it is never reused (ADR 0116 §2), and its pass is not stored either: it is not looked
    /// up, so a record would be written over an unread one, and nothing reads it (user, 2026-11-28).
    pub blind: Vec<String>,
}

/// Why a target runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// Listed in `no_test_cache`.
    Never,
    /// Uncommitted changes under its inputs.
    Dirty,
    /// Its crate reads something the graph cannot resolve, so its key may miss an input: the reasons.
    Blind(String),
    /// No record under this key.
    Miss,
    /// A record, older than the limit of the paused nightly (decision D7).
    TooOld,
    /// A record that failed the trust check, and what it said.
    Untrusted(String),
    /// A record whose trust check could not answer (the API failed, no token): it runs, and its record is not replaced.
    Undecided(String),
    /// The store did not answer, or reuse is off for this run (the cause): everything runs.
    Off(String),
}

impl Why {
    /// The words of a log line.
    pub fn text(&self) -> String {
        match self {
            Why::Never => "listed in no_test_cache".into(),
            Why::Dirty => "uncommitted changes under its inputs".into(),
            Why::Blind(w) => format!("it reads inputs that cannot be resolved ({w})"),
            Why::Miss => "no record under this key".into(),
            Why::TooOld => "its record is older than the limit while the nightly is paused".into(),
            Why::Untrusted(w) => format!("its record failed the trust check: {w}"),
            Why::Undecided(w) => format!("its record's trust could not be decided: {w}"),
            Why::Off(w) => format!("reuse is off: {w}"),
        }
    }
}

/// What to do with one target.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// A valid record exists: the target's pass can be reused.
    Reuse(Box<Record>),
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

/// One decision per candidate, in order, from one bulk lookup. A key the store could not answer turns reuse off for that target only
/// ([`Why::Off`], fail open per item, §5 as amended 2026-11-28: it runs, never the other way round) and the others keep their answers; the
/// second answer is one sentence naming how many failed and the first cause, for one notice, `None` when none failed. `trust` is the reader-side
/// check of a record (§7); `now` is Unix seconds. `filter` is the filterset the run this lookup is for is narrowed to: a record counts
/// only under the same filter.
pub fn decide(
    cands: &[Candidate],
    cfg: &Config,
    class: &RunnerClass,
    filter: &str,
    store: &dyn Store,
    now: u64,
    trust: &dyn Fn(&Record) -> Result<(), trust::Distrust>,
) -> (Vec<Decision>, Option<String>) {
    let mut asks: Vec<Lookup> = Vec::new();
    let mut at: Vec<Option<usize>> = Vec::new();
    for c in cands {
        if cfg.never_cached(&c.target.node) || c.dirty || !c.blind.is_empty() {
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
    let found = store.get_many(&asks);
    if found.len() != asks.len() {
        return off(cands, format!("the store answered {} records for {} keys", found.len(), asks.len()));
    }
    let failed: Vec<&store::StoreError> = found.iter().filter_map(|f| f.as_ref().err()).collect();
    let notice = failed.first().map(|e| {
        format!(
            "the store lookup failed for {} of {} keys: {e}. Those targets run; the others keep their answers.",
            failed.len(),
            asks.len()
        )
    });
    let limit = cfg.max_age();
    let out = cands
        .iter()
        .zip(at)
        .map(|(c, i)| {
            let Some(i) = i else {
                return Decision::Run(if cfg.never_cached(&c.target.node) {
                    Why::Never
                } else if !c.blind.is_empty() {
                    Why::Blind(blind_text(&c.target.id, &c.blind))
                } else {
                    Why::Dirty
                });
            };
            let r = match &found[i] {
                Ok(Some(r)) => r,
                Ok(None) => return Decision::Run(Why::Miss),
                Err(e) => return Decision::Run(Why::Off(e.to_string())),
            };
            if limit.is_some_and(|l: Duration| now.saturating_sub(r.at) > l.as_secs()) {
                return Decision::Run(Why::TooOld);
            }
            match trust(r) {
                Ok(()) => Decision::Reuse(Box::new(r.clone())),
                Err(trust::Distrust::Refused(w)) => Decision::Run(Why::Untrusted(w)),
                Err(trust::Distrust::Unknown(w)) => Decision::Run(Why::Undecided(w)),
            }
        })
        .collect();
    (out, notice)
}

/// The blind reads of each crate, split by what covers them (ADR 0116 §2, user decision 122).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// The crates that stay blind (their sites, and the problems of a declaration that does not hold), by folder: the input of
    /// [`spread_blind`].
    pub open: std::collections::BTreeMap<String, Vec<String>>,
    /// The sites a crate's `test-reads` declaration covers, by folder: the crate is cacheable again, its key holds the declared files.
    pub covered: std::collections::BTreeMap<String, Vec<String>>,
}

/// A blind site counts as covered when its crate declares its test reads (`[package.metadata.paddock] test-reads`) and the declaration
/// holds: a glob that matches no tracked file, or an empty list, makes the declaration a finding and the crate stays blind with that
/// reason first, even when it has no blind site of its own (a stale declaration cannot be trusted either). A crate with sites and no
/// declaration stays blind (never silently cacheable).
pub fn cover_blind(
    own: std::collections::BTreeMap<String, Vec<String>>,
    declared: &std::collections::BTreeMap<String, crate::repo_model::test_reads::Declared>,
) -> Coverage {
    let mut out = Coverage::default();
    let mut own = own;
    for (dir, d) in declared {
        let problems = d.problems();
        let sites = own.remove(dir).unwrap_or_default();
        if !problems.is_empty() {
            out.open.insert(dir.clone(), problems.into_iter().chain(sites).collect());
        } else if !sites.is_empty() {
            out.covered.insert(dir.clone(), sites);
        }
    }
    out.open.extend(own);
    out
}

/// The blind reads of every crate, spread to its dependents (ADR 0116 §2): a crate's tests run its dependencies' code, whose unhashed reads
/// its own key cannot see, so a crate that depends (normal, dev or build, transitively, inside the repository) on a blind crate is blind
/// too, with the reason `blind through <dependency folder>`. `own` is each blind crate's folder and its own reasons.
pub fn spread_blind(
    model: &paddock_repo_graph::model::nodes::Model,
    own: &std::collections::BTreeMap<String, Vec<String>>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out = own.clone();
    for n in model.nodes() {
        let Some(dir) = n.id.strip_prefix("crate:") else { continue };
        for dep in paddock_repo_graph::model::hash::dependencies(model, &n.id) {
            let Some(dep_dir) = dep.strip_prefix("crate:") else { continue };
            if own.contains_key(dep_dir) {
                let r = format!("blind through {dep_dir}");
                let e = out.entry(dir.to_string()).or_default();
                if !e.contains(&r) {
                    e.push(r);
                }
            }
        }
    }
    out
}

/// The reasons of a blind target in one line: the target, how many, and the first two (the full list is `./dev plan --explain`).
fn blind_text(target: &str, reasons: &[String]) -> String {
    let shown: Vec<&str> = reasons.iter().take(2).map(String::as_str).collect();
    let more = reasons.len().saturating_sub(shown.len());
    format!(
        "{target}: {}{}",
        shown.join("; "),
        if more > 0 { format!("; and {more} more") } else { String::new() }
    )
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
    let mut gone = Vec::new();
    for answer in store.delete_many(failed) {
        gone.extend(answer?);
    }
    Ok(gone)
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
        .filter_map(|c| match judged.outcome(&c.target.package) {
            // Uncommitted changes under the inputs: the pass proves nothing about the key.
            result::TestOutcome::Passed { tests } if !c.dirty => Some((c, tests)),
            result::TestOutcome::Passed { .. } | result::TestOutcome::Failed { .. } | result::TestOutcome::NotSeen { .. } => None,
        })
        .map(|(c, tests)| Record {
            key: key_of(c, cfg, class, filter),
            target: c.target.id.clone(),
            class: class.clone(),
            filter: key::normalize_filter(filter),
            run: m.run.clone(),
            job: m.job.clone(),
            attempt: m.attempt,
            git_ref: m.git_ref.clone(),
            commit: m.commit.clone(),
            tests,
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
        match judged.outcome(&c.target.package) {
            result::TestOutcome::Passed { .. } => {
                s.matched += 1;
                s.passed += 1;
            }
            result::TestOutcome::Failed { .. } => {
                s.matched += 1;
                s.findings.push(c.target.id.clone());
            }
            result::TestOutcome::NotSeen { .. } => {}
        }
    }
    s
}

/// What a local `test-cache run` says about the targets it ran that did not pass. `findings` comes from [`shadow_outcome`], the one rule
/// the local run and a CI record step share: a soundness finding is a matched target that ran and failed. Every other target that did
/// not pass is `not_stored` when it ran and failed, or `not_seen` when the run's output has no test of it (outside its filter, without
/// tests, or a log that could not be read whole): never a finding.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunOutcome {
    /// The shadow outcome of the matched targets (the findings and the matched and passed counts).
    pub shadow: Shadow,
    /// Targets that ran and did not pass but are not findings (no valid record matched them), with why.
    pub not_stored: Vec<(String, String)>,
    /// Targets with no verdict in the output: not stored and not a finding.
    pub not_seen: usize,
}

/// The [`RunOutcome`] of `ran` (every target the run was asked for) given `matched` (those a valid record covered before it).
pub fn run_outcome(ran: &[&Candidate], matched: &[&Candidate], judged: &result::Judged) -> RunOutcome {
    let mut out = RunOutcome {
        shadow: shadow_outcome(matched, judged),
        ..RunOutcome::default()
    };
    for c in ran {
        match judged.outcome(&c.target.package) {
            result::TestOutcome::Passed { .. } => {}
            // A matched target that failed is already a finding.
            result::TestOutcome::Failed { .. } if out.shadow.findings.contains(&c.target.id) => {}
            result::TestOutcome::Failed { why } => out.not_stored.push((c.target.id.clone(), why)),
            result::TestOutcome::NotSeen { .. } => out.not_seen += 1,
        }
    }
    out
}

/// The records a run stores: `put` (one per passed target) limited to the targets whose record the lookup proved replaceable: no record,
/// one too old, or one the trust check refused. Write-once while trusted, as remote build caches do: a trusted record ([`Decision::Reuse`])
/// is never replaced, so a red run's leg or a local run cannot make every later lookup untrusted (run 9209 replaced run 9180's records).
/// Nothing is replaced on an answer that is not definite either: a trust check that could not answer ([`Why::Undecided`]) or a lookup
/// that did not run or failed ([`Why::Off`]); a target that was never looked up (never cached, blind, dirty) is not replaced either.
/// The second answer counts the records held back by the reason of their decision ([`reasons::Tally::held_line`]).
pub fn not_overwriting_trusted(put: Vec<Record>, cands: &[Candidate], decisions: &[Decision]) -> (Vec<Record>, reasons::Tally) {
    let decision_of: std::collections::BTreeMap<&str, &Decision> =
        cands.iter().zip(decisions).map(|(c, d)| (c.target.id.as_str(), d)).collect();
    let replaceable = |d: &Decision| matches!(d, Decision::Run(Why::Miss | Why::TooOld | Why::Untrusted(_)));
    let (kept, held): (Vec<Record>, Vec<Record>) = put
        .into_iter()
        .partition(|r| decision_of.get(r.target.as_str()).is_some_and(|d| replaceable(d)));
    let tally = reasons::Tally::of(held.iter().filter_map(|r| decision_of.get(r.target.as_str()).copied()));
    (kept, tally)
}

/// The records a lookup found but did not trust, counted by why: `untrusted 57 (57: job ... did not end success)`, and those whose trust
/// could not be decided (`undecided 3 (...)`), so a `matched 0` never hides records that are there. `None` when there were none.
pub fn untrusted_summary(decisions: &[Decision]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for (word, pick) in [("untrusted", true), ("undecided", false)] {
        let mut by: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for d in decisions {
            match d {
                Decision::Run(Why::Untrusted(w)) if pick => *by.entry(w.as_str()).or_default() += 1,
                Decision::Run(Why::Undecided(w)) if !pick => *by.entry(w.as_str()).or_default() += 1,
                _ => {}
            }
        }
        let n: usize = by.values().sum();
        if n > 0 {
            let why: Vec<String> = by.iter().map(|(w, k)| format!("{k}: {w}")).collect();
            parts.push(format!("{word} {n} ({})", why.join("; ")));
        }
    }
    (!parts.is_empty()).then(|| parts.join(", "))
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
