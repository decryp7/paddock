// ci-runs-reader: test-cache: builds the reader-side trust check of cached passes (the run behind a record), not a verdict on a commit
//! `ci test-cache`: `run` (the cache around one Cargo workspace's nextest run), `lookup` (what would be reused, nothing run) and
//! `audit` (the local store's size and oldest record, report-only, ER-51).
//!
//! `run` is the tool a gate, a fix round or an agent calls instead of `cargo nextest run`: it hashes every package's node in the
//! workspace in one bulk read, looks the keys up in one call, runs the packages without a valid record in one nextest run (a
//! filterset of the packages left; nothing when none is), judges the run per package ([`super::result`]) and stores the passes in
//! one call. In shadow mode ([`super::config::Mode`]) every package runs and the would-be reuse is only reported; a package whose key
//! matched a record and then failed is a soundness finding (`::error::`, exit 5), the signal that the key misses an input.

use super::config::{Config, Mode};
use super::key::{RunnerClass, Target};
use super::store::{FsStore, Lookup, Record, Store};
use super::{Candidate, Decision, decide, read_allowed, write_allowed};
use crate::plan_repo::git_text;
use clap::Args;
use paddock_cli::{CliError, Exit, Report};
use paddock_proc::{Limits, run_bounded};
use paddock_repo_graph::GitHistory;
use std::path::{Path, PathBuf};
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(120);
/// A hang detector, not a speed limit (ER-12): a whole workspace's tests on a loaded machine.
const RUN_TIMEOUT: Duration = Duration::from_secs(90 * 60);

/// Arguments of `ci test-cache`.
#[derive(Args)]
pub struct TestCacheArgs {
    /// `run`, `lookup` or `audit`.
    action: String,
    /// The Cargo workspace folder (`crates` or `tools/paddock-dev`).
    #[arg(long, default_value = "crates")]
    workspace: String,
    /// Skip reused targets even in shadow mode (the switch of docs/test-cache.toml is not on yet). For measuring.
    #[arg(long)]
    reuse: bool,
    /// Only these packages, comma separated (a smaller run, for measuring; the default is every package of the workspace).
    #[arg(long)]
    only: Option<String>,
    /// The runner class to key for, `Linux-X64`, `macOS-ARM64` or `Windows-X64` (default: this machine's). A plan job on Linux asks
    /// for the classes of the other runners this way.
    #[arg(long)]
    class: Option<String>,
    /// `shadow-run`: the CI run (id) whose record steps' shadow lines are read.
    #[arg(long)]
    run: Option<u64>,
    /// `shadow-run`: add a clean run's id to `[switch] green_runs` of docs/test-cache.toml (the file is only changed by this flag).
    #[arg(long)]
    write: bool,
    /// `plan`: where to write the reuse plan (`PLAN_TEST_CACHE_FILE` of `./dev plan --ci`).
    #[arg(long)]
    out: Option<PathBuf>,
    /// `record`: the file holding a nextest run's output (stdout and stderr), read as the run's result.
    #[arg(long)]
    log: Option<PathBuf>,
    /// The nextest filterset the run is narrowed to (a host leg's `test_filter`, a member run's reached packages). It is part of the key
    /// (`run`, `lookup`, `record`, `forget`): a pass counts only under the same filter. `run` ANDs it with the packages left to run.
    #[arg(long)]
    filter: Option<String>,
    /// `run`: the nextest configuration file, relative to the workspace (an app's own).
    #[arg(long)]
    nextest_config: Option<String>,
    /// The cargo build folder (default: the workspace's own `target`).
    #[arg(long)]
    build_dir: Option<String>,
    /// The repository root (default: the folder containing .git).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The store folder (default: `<git common dir>/paddock-pre-push/test-cache`).
    #[arg(long)]
    store: Option<PathBuf>,
    /// Use the shared store, the RustFS bucket `test-results-paddock` ([`super::s3::BUCKET`]), instead of the local folder. Without its
    /// credentials the run says so and uses the local store.
    #[arg(long)]
    shared: bool,
}

fn fail(m: String) -> CliError {
    CliError::failure(m)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The local store's folder: `test-cache` beside the gate rows' records in the pre-push state folder
/// ([`crate::pre_push::state_dir`], the one definition of where they live).
pub fn store_dir(root: &Path) -> PathBuf {
    crate::pre_push::state_dir(root).join("test-cache")
}

/// The packages of `workspace` as targets, from the planner's Cargo graph.
fn targets(graph: &paddock_repo_graph::Graph, workspace: &str) -> Vec<Target> {
    let mut t: Vec<Target> = graph
        .iter()
        .filter(|(_, n)| n.workspace == workspace)
        .map(|(dir, n)| Target::rust(dir, &n.name, workspace))
        .collect();
    t.sort();
    t
}

/// The model's blind reads (a plan does not see them) by the Cargo package folder of their referrer, one line each: a package with any is blind
/// (its key cannot say what it reads, ADR 0116 §2).
fn blind_reads(
    cargo: &paddock_repo_graph::Graph,
    unresolved: Vec<&paddock_repo_graph::model::Unresolved>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for u in unresolved {
        if let Some(dir) = cargo.node_of(&u.referrer) {
            out.entry(dir.to_string())
                .or_default()
                .push(format!("{}: {}", u.referrer, u.reference));
        }
    }
    out
}

/// `blind`: every blind read of the workspace's crates, each with the `test-reads` declaration that covers it, or why the crate stays
/// `Never` (ADR 0116 §2, user decision 122). Report-only: nothing is skipped or stored.
fn blind_sites(root: &Path, workspace: &str) -> Result<Report, CliError> {
    let w = world(root)?;
    let in_workspace = |dir: &str| w.graph.cargo.get(dir).is_some_and(|n| n.workspace == workspace);
    let mut own = blind_reads(&w.graph.cargo, w.world.model.blind());
    own.retain(|dir, _| in_workspace(dir));
    let all = own.clone();
    let cov = super::cover_blind(own, &w.world.test_reads);
    let sites: usize = all.values().map(Vec::len).sum();
    let mut text: Vec<String> = Vec::new();
    for (dir, list) in &all {
        let problems = w.world.test_reads.get(dir).map(|d| d.problems()).unwrap_or_default();
        let head = match (cov.covered.contains_key(dir), w.world.test_reads.get(dir)) {
            (true, Some(d)) => format!(
                "{dir}: {} blind site(s) covered by `test-reads` {:?} ({} tracked file(s) in its key)",
                list.len(),
                d.globs,
                d.matched.len()
            ),
            (_, Some(_)) => format!(
                "{dir}: {} blind site(s), NOT covered: its declaration does not hold ({}); it stays Never",
                list.len(),
                problems.join("; ")
            ),
            (_, None) => format!(
                "{dir}: {} blind site(s), NOT covered: no `[package.metadata.paddock] test-reads`; it stays Never",
                list.len()
            ),
        };
        text.push(head);
        text.extend(list.iter().map(|s| format!("    {s}")));
    }
    // A declaration with a problem and no blind site of its own is a finding as well.
    for (dir, d) in &w.world.test_reads {
        if in_workspace(dir) && !all.contains_key(dir) && !d.problems().is_empty() {
            text.push(format!(
                "{dir}: its `test-reads` declaration does not hold ({}); it stays Never",
                d.problems().join("; ")
            ));
        }
    }
    text.push(format!(
        "test-cache: {sites} blind site(s) in {} crate(s) of `{workspace}`: {} covered by a declaration, {} not covered",
        all.len(),
        cov.covered.len(),
        cov.open.len()
    ));
    Ok(Report::new(
        serde_json::json!({"sites": sites, "crates": all.len(), "covered": cov.covered.len(), "open": cov.open.len()}),
        text.join("\n"),
    ))
}

/// The graph and the model of the tree, read once: a plan asks for several workspaces and runner classes.
pub struct World {
    graph: crate::plan::RepoGraph,
    world: crate::repo_model::World,
}

/// Reads the graph and the model of the checkout at `root`.
pub fn world(root: &Path) -> Result<World, CliError> {
    let history = GitHistory::new(root, READ_TIMEOUT);
    let graph = crate::plan::read_graph(root, &history).map_err(|e| fail(e.to_string()))?;
    let world = crate::repo_model::build_world(root, &history, &graph, None).map_err(fail)?;
    Ok(World { graph, world })
}

/// The candidates with their node hashes, in one bulk read.
pub fn candidates(root: &Path, workspace: &str, os: &str) -> Result<Vec<Candidate>, CliError> {
    candidates_in(root, &world(root)?, workspace, os)
}

/// [`candidates`] over a world read before.
pub fn candidates_in(root: &Path, w: &World, workspace: &str, os: &str) -> Result<Vec<Candidate>, CliError> {
    let ts = targets(&w.graph.cargo, workspace);
    if ts.is_empty() {
        return Err(fail(format!(
            "::error::ci test-cache: the Cargo graph has no package in the workspace `{workspace}`. Next step: name a workspace folder such as `crates`."
        )));
    }
    let world = &w.world;
    let text = |f: &str| std::fs::read_to_string(root.join(f)).ok();
    let tree = crate::repo_model::hash::Tree {
        files: &world.files,
        text: &text,
    };
    let vars: Vec<(String, String)> = crate::process_env::vars().collect();
    let env = |k: &str| std::env::var(k).ok();
    let ctx = crate::repo_model::hash::HashEnv {
        env: &env,
        vars: &vars,
        os,
    };
    let nodes: Vec<String> = ts.iter().map(|t| t.node.clone()).collect();
    let hashes = crate::repo_model::hash::node_hashes(&crate::input_hash::Real, &ctx, root, world, &tree, &nodes)?;
    let covered = super::cover_blind(blind_reads(&w.graph.cargo, world.model.blind()), &world.test_reads);
    let blind = super::spread_blind(&world.model, &covered.open);
    ts.into_iter()
        .map(|t| {
            let h = hashes.get(&t.node).ok_or_else(|| {
                fail(format!(
                    "::error::ci test-cache: no input hash for {}. Next step: run `./dev plan --hash {}`.",
                    t.node, t.node
                ))
            })?;
            let blind_here = blind
                .get(t.node.strip_prefix("crate:").unwrap_or(&t.node))
                .cloned()
                .unwrap_or_default();
            Ok(Candidate {
                target: t,
                node_hash: h.hash.clone(),
                dirty: h.dirty,
                blind: blind_here,
            })
        })
        .collect()
}

fn repo_root(root: Option<PathBuf>) -> Result<PathBuf, CliError> {
    if let Some(r) = root {
        return Ok(r);
    }
    let cwd = std::env::current_dir().map_err(|e| fail(format!("The current folder cannot be read: {e}.")))?;
    let out = crate::input_hash::Exec::run(&crate::input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
    Ok(PathBuf::from(out.trim()))
}

/// The filter a run is keyed under: `--filter`, normalized (empty: the whole package).
fn filter_of(a: &TestCacheArgs) -> String {
    super::key::normalize_filter(a.filter.as_deref().unwrap_or(""))
}

fn env_get(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// `ci test-cache <action>`.
pub fn command(a: &TestCacheArgs) -> Result<Report, CliError> {
    let root = repo_root(a.root.clone())?;
    let local = FsStore::new(&a.store.clone().unwrap_or_else(|| store_dir(&root)));
    let env: std::collections::HashMap<String, String> = crate::process_env::vars().collect();
    let shared = if a.shared {
        match super::s3::S3Store::from_env(&env, store_dir(&root).join("scratch")) {
            Ok(s) => Some(s),
            Err(w) => {
                crate::log_text::eline(&format!(
                    "::notice::test-cache: the shared store is not used: {w}. The local store is."
                ));
                None
            }
        }
    } else {
        None
    };
    let store: &dyn Store = match &shared {
        Some(s) => s,
        None => &local,
    };
    match a.action.as_str() {
        "audit" => Ok(match &shared {
            Some(s) => audit_shared(s),
            None => audit(&local),
        }),
        "selfcheck" => selfcheck(shared.as_ref()),
        "blind" => blind_sites(&root, &a.workspace),
        "plan" => plan(a, &root, store),
        "shadow-run" => shadow_run(a, &root),
        "lookup" | "run" | "forget" | "record" => {
            let class = class_of(a);
            let cfg = Config::read(&root).map_err(fail)?;
            let mut cands = candidates(&root, &a.workspace, os_of(&class))?;
            if let Some(only) = &a.only {
                let want: Vec<&str> = only.split(',').collect();
                cands.retain(|c| want.contains(&c.target.package.as_str()));
                if cands.len() != want.len() {
                    return Err(fail(format!(
                        "::error::ci test-cache: --only names a package the workspace `{}` does not have. Next step: check the names.",
                        a.workspace
                    )));
                }
            }
            if a.action == "forget" {
                return forget(&cfg, &cands, store, &filter_of(a));
            }
            if a.action == "record" {
                return record(a, &root, &cfg, &cands, store, &class);
            }
            let ask = |asked: &[String], whole: bool, reused_any: bool| nextest(a, &root, &cfg, asked, whole, reused_any);
            run_with(a, &root, &cfg, &cands, store, &class, &env_get, &trust_check(), &ask)
        }
        other => Err(fail(format!(
            "::error::ci test-cache: unknown action `{other}`. Next step: use `run`, `lookup` or `audit`."
        ))),
    }
}

fn audit(store: &FsStore) -> Report {
    let (bytes, n, oldest) = store.usage();
    let age = oldest.map_or("no record".to_string(), |o| {
        format!("oldest {} days old", now().saturating_sub(o) / 86400)
    });
    let warn = if bytes > (1 << 30) {
        "\n::warning::ci test-cache audit: the store is over 1 GiB. Next step: lower `EVICT_AFTER` or delete the folder."
    } else {
        ""
    };
    Report::new(
        serde_json::json!({"records": n, "bytes": bytes}),
        format!("test-cache store: {n} records, {bytes} bytes, {age}{warn}"),
    )
}

/// One nextest run of the asked packages (every package when `whole`): its output and whether it succeeded. Real: [`nextest`].
type NextestRun<'a> = &'a dyn Fn(&[String], bool, bool) -> Result<(String, bool), CliError>;

/// `ci test-cache run` with the environment, the trust check and the nextest run given (the tests hand stand-ins).
#[allow(clippy::too_many_arguments)]
pub(super) fn run_with(
    a: &TestCacheArgs,
    root: &Path,
    cfg: &Config,
    cands: &[Candidate],
    store: &dyn Store,
    class: &RunnerClass,
    env: &dyn Fn(&str) -> Option<String>,
    trust: &Trust,
    nextest_run: NextestRun<'_>,
) -> Result<Report, CliError> {
    let started = std::time::Instant::now();
    let filter = filter_of(a);
    let mut text: Vec<String> = Vec::new();
    // Whether this run may read, and what the lookup said. A run that may not reuse still looks the records up for the write rule
    // ([`super::not_overwriting_trusted`]), so it seeds and refreshes records; only its reuse is off.
    let looked = decide(cands, cfg, class, &filter, store, now(), trust);
    let read = read_allowed(env);
    let (decisions, notice) = match &read {
        Err(why) => (
            cands.iter().map(|_| Decision::Run(super::Why::Off(why.clone()))).collect(),
            Some(why.clone()),
        ),
        Ok(()) => looked.clone(),
    };
    if let Some(n) = notice {
        text.push(format!("::notice::test-cache: reuse is off for this run: {n}. Every target runs."));
    }
    // A run that may not read still looked the store up: a store that is down is told too, not hidden behind the other cause.
    if read.is_err()
        && let Some(down) = &looked.1
    {
        text.push(format!("::notice::test-cache: the store could not be read either: {down}."));
    }
    let nightly = super::is_nightly(env);
    let skip = cfg.mode() == Mode::On || a.reuse;
    let mut reused = 0;
    let mut why_run: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut left: Vec<&Candidate> = Vec::new();
    let mut matched: Vec<&Candidate> = Vec::new();
    for (c, d) in cands.iter().zip(&decisions) {
        match d {
            Decision::Reuse(r) if skip => {
                reused += 1;
                text.push(r.line());
            }
            Decision::Reuse(r) => {
                matched.push(c);
                text.push(format!("{} (shadow: still runs)", r.line()));
                left.push(c);
            }
            Decision::Run(w) => {
                *why_run.entry(w.text()).or_default() += 1;
                left.push(c);
            }
        }
    }
    // One line per reason, not one per target: a whole workspace is dozens of lines (the reused ones are named above).
    text.extend(why_run.iter().map(|(w, n)| format!("test-cache: run {n} target(s): {w}")));
    if a.action == "lookup" {
        // One line the first CI run shows: how many targets a valid record covers on this runner class (nothing is skipped here).
        let would = reused + matched.len();
        let because = super::reasons::Tally::of(&decisions).ran_because();
        text.push(format!(
            "test-cache: reused {would} of {} targets ({}; nothing is skipped by a lookup){}",
            cands.len(),
            class.0,
            if because.is_empty() {
                String::new()
            } else {
                format!("; ran because: {because}")
            }
        ));
        return Ok(Report::new(
            serde_json::json!({"targets": cands.len(), "reused": would}),
            text.join("\n"),
        ));
    }
    let mut findings = 0;
    let mut ran_secs = 0.0;
    if !left.is_empty() {
        let asked: Vec<String> = left.iter().map(|c| c.target.package.clone()).collect();
        let t0 = std::time::Instant::now();
        let out = nextest_run(&asked, left.len() == cands.len() && a.only.is_none(), reused > 0)?;
        ran_secs = t0.elapsed().as_secs_f64();
        let judged = super::result::judge(&out.0, &asked);
        // One rule with the CI record step (`shadow_outcome`): a finding is a matched target that ran and failed, never one the filter
        // left out (train/2026-11-27-x: 58 not-seen targets were reported as findings).
        let outcome = super::run_outcome(&left, &matched, &judged);
        findings = outcome.shadow.findings.len();
        for t in &outcome.shadow.findings {
            let why = match left.iter().find(|c| &c.target.id == t).map(|c| judged.outcome(&c.target.package)) {
                Some(super::result::TestOutcome::Failed { why } | super::result::TestOutcome::NotSeen { why }) => why,
                Some(super::result::TestOutcome::Passed { .. }) | None => "no verdict".to_string(),
            };
            text.push(format!(
                "::error::test-cache soundness finding: {t} matched a valid record and then failed ({why}). Next step: find the input its key misses (ADR 0116 §8); the switch stays off."
            ));
        }
        for (t, why) in &outcome.not_stored {
            text.push(format!("test-cache: not stored target={t} ({why})"));
        }
        let mut put: Vec<Record> = Vec::new();
        let writes = write_allowed(env);
        for c in &left {
            match judged.outcome(&c.target.package) {
                super::result::TestOutcome::Passed { tests } => {
                    if !c.dirty && writes.is_ok() {
                        put.push(Record {
                            key: super::key_of(c, cfg, class, &filter),
                            target: c.target.id.clone(),
                            class: class.clone(),
                            filter: filter.clone(),
                            run: format!("local-{}", std::process::id()),
                            job: String::new(),
                            attempt: 0,
                            git_ref: env("GITHUB_REF").unwrap_or_default(),
                            commit: git_text(root, &["rev-parse", "HEAD"]).unwrap_or_default().trim().to_string(),
                            tests,
                            secs: ran_secs,
                            at: now(),
                            os_build: os_build(),
                        });
                    }
                }
                // The nightly is the net (§6): a target that ran and failed loses the cached pass under its current key, and the run
                // names it. A target the run never saw (outside its filter, or a log not read whole) keeps its record.
                super::result::TestOutcome::Failed { .. } if nightly => {
                    let ask = Lookup {
                        target: c.target.id.clone(),
                        class: class.clone(),
                        key: super::key_of(c, cfg, class, &filter),
                    };
                    match super::invalidate(store, std::slice::from_ref(&ask)) {
                        Ok(gone) => text.extend(gone.iter().map(|r| {
                            format!(
                                "::warning::test-cache: invalidated target={} (a cached pass from run {} that the nightly no longer supports). Next step: fix the key's definition (ADR 0116 §8).",
                                r.target, r.run
                            )
                        })),
                        Err(e) => text.push(format!("::warning::test-cache: the nightly could not delete a record: {e}")),
                    }
                }
                super::result::TestOutcome::Failed { .. } | super::result::TestOutcome::NotSeen { .. } => {}
            }
        }
        if let Some(why) = &judged.unreadable {
            // Not "outside its filter": nothing of this run was judged, so nothing is known about any target (ER-33).
            text.push(format!(
                "test-cache: not measured: the run's output could not be read whole ({why}); nothing was judged, so nothing is stored"
            ));
        } else if outcome.not_seen > 0 {
            text.push(format!(
                "test-cache: {} target(s) had no test in this run's output (outside its filter, or without tests): not stored",
                outcome.not_seen
            ));
        }
        // Write-once while trusted, as in the record step: a local pass never replaces a trusted record.
        let (put, held) = super::not_overwriting_trusted(put, cands, &looked.0);
        if let Err(w) = &writes {
            text.push(format!("test-cache: nothing stored ({w})"));
        } else if let Err(e) = store.put_many(&put) {
            text.push(format!("::warning::test-cache: records not stored: {e}"));
        } else {
            text.push(format!("test-cache: stored {} pass(es); {}", put.len(), held.held_line()));
        }
        if !out.1 {
            // The whole output: the gate's row reads the failed tests and the first panic from it.
            text.push(format!(
                "{}\n::error::ci test-cache: nextest failed. Next step: fix the test it names above.",
                out.0
            ));
        }
    }
    text.push(format!(
        "test-cache: {} targets, {reused} reused, {} run, {:.1} s ({:.1} s in nextest)",
        cands.len(),
        left.len(),
        started.elapsed().as_secs_f64(),
        ran_secs
    ));
    let data = serde_json::json!({"targets": cands.len(), "reused": reused, "ran": left.len(), "findings": findings});
    let exit = if findings > 0 || text.iter().any(|l| l.contains("nextest failed")) {
        Exit::Findings
    } else {
        Exit::Ok
    };
    Ok(Report::new(data, text.join("\n")).with_exit(exit))
}

/// The runner's OS build as observed (`uname -sr`): reported in each record, never keyed (decision D5).
fn os_build() -> String {
    let mut cmd = paddock_proc::command("uname");
    cmd.arg("-sr");
    run_bounded(cmd, Limits::new(Duration::from_secs(10)))
        .ok()
        .filter(|o| o.success())
        .map(|o| o.stdout_text().trim().to_string())
        .unwrap_or_default()
}

/// One nextest run of `asked` (every package when `whole`), its output and whether it succeeded.
fn nextest(
    a: &TestCacheArgs,
    root: &Path,
    cfg: &Config,
    asked: &[String],
    whole: bool,
    reused_any: bool,
) -> Result<(String, bool), CliError> {
    let mut cmd = paddock_proc::command("cargo");
    cmd.current_dir(root.join(&a.workspace));
    cmd.arg("nextest").arg("run");
    if let Some(d) = &a.build_dir {
        cmd.arg("--config").arg(crate::cargo_target::config_value(d));
    }
    cmd.args([
        "--profile",
        &cfg.profile,
        "--locked",
        "--no-fail-fast",
        // A selection of nothing is a failure, never a pass (ER-33), unless reuse left nothing of it to run.
        if reused_any { "--no-tests=warn" } else { "--no-tests=fail" },
        "--workspace",
    ]);
    if let Some(c) = &a.nextest_config {
        cmd.args(["--config-file", c]);
    }
    if a.workspace == "crates" {
        cmd.arg("--all-features");
    }
    let packages = (!whole).then(|| asked.iter().map(|p| format!("package(={p})")).collect::<Vec<_>>().join(" | "));
    let filter = match (a.filter.as_deref().filter(|f| !f.trim().is_empty()), packages) {
        (Some(user), Some(pk)) => Some(format!("({user}) & ({pk})")),
        (Some(user), None) => Some(user.to_string()),
        (None, Some(pk)) => Some(pk),
        (None, None) => None,
    };
    if let Some(f) = filter {
        cmd.args(["-E", &f]);
    }
    cmd.env("RUSTUP_AUTO_INSTALL", "0");
    let out = run_bounded(cmd, Limits::new(RUN_TIMEOUT).max_output(1 << 28)).map_err(|e| {
        fail(format!(
            "::error::ci test-cache: nextest did not run: {e}. Next step: install cargo-nextest (`cargo install cargo-nextest --locked`)."
        ))
    })?;
    let text = format!("{}\n{}", out.stdout_text(), out.stderr_text());
    Ok((text, out.success()))
}

/// `ci test-cache selfcheck --shared`: put, get, list and delete one scratch object in the shared store. Exit 5 when any step failed,
/// 6 (not measured) when the store cannot be reached or has no credentials: never a pass (ER-33).
fn selfcheck(shared: Option<&super::s3::S3Store>) -> Result<Report, CliError> {
    let Some(s) = shared else {
        return Ok(Report::new(
            serde_json::json!({"verdict": "not_measured"}),
            "::error::ci test-cache selfcheck: no shared store. Next step: pass --shared and set the S3 credentials.".to_string(),
        )
        .with_exit(Exit::NotMeasured));
    };
    match s.probe(&format!("selfcheck-{}-{}", std::process::id(), now())) {
        Err(e) => Ok(Report::new(
            serde_json::json!({"verdict": "not_measured"}),
            format!("::error::ci test-cache selfcheck: {e}. Next step: check the endpoint, the bucket and the key."),
        )
        .with_exit(Exit::NotMeasured)),
        Ok(p) => {
            let ok = p.round_trip && p.listed && p.deleted;
            let text = format!(
                "test-cache selfcheck: round trip {}, listed {}, deleted {}",
                p.round_trip, p.listed, p.deleted
            );
            let r = Report::new(
                serde_json::json!({"round_trip": p.round_trip, "listed": p.listed, "deleted": p.deleted}),
                text,
            );
            Ok(if ok { r } else { r.with_exit(Exit::Findings) })
        }
    }
}

/// `audit --shared`: how many records the bucket lists under `v<schema>/` (one page), report-only (ER-51).
fn audit_shared(s: &super::s3::S3Store) -> Report {
    match s.list(&format!("v{}/", super::key::SCHEMA)) {
        Ok(k) => Report::new(
            serde_json::json!({"records": k.len()}),
            format!("test-cache shared store: {} record(s) listed (first page)", k.len()),
        ),
        Err(e) => Report::new(
            serde_json::json!({"verdict": "not_measured"}),
            format!("::error::ci test-cache audit: {e}. Next step: check the endpoint, the bucket and the key."),
        )
        .with_exit(Exit::NotMeasured),
    }
}

/// `forget`: deletes the records under the current keys of the selected targets (a mistaken or scratch record; the nightly's invalidation
/// does the same for a failing target). Names each one it deleted.
fn forget(cfg: &Config, cands: &[Candidate], store: &dyn Store, filter: &str) -> Result<Report, CliError> {
    let class = RunnerClass::here();
    let asks: Vec<Lookup> = cands
        .iter()
        .map(|c| Lookup {
            target: c.target.id.clone(),
            class: class.clone(),
            key: super::key_of(c, cfg, &class, filter),
        })
        .collect();
    let gone = store
        .delete_many(&asks)
        .map_err(|e| fail(format!("::error::ci test-cache forget: {e}. Next step: check the store.")))?;
    let n = gone.iter().flatten().count();
    let text = gone
        .iter()
        .flatten()
        .map(|r| format!("test-cache: forgot target={} (from run {})", r.target, r.run))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Report::new(
        serde_json::json!({"forgot": n}),
        format!("{text}\ntest-cache: {n} record(s) forgotten"),
    ))
}

/// The runner class to key for: `--class`, else this machine's.
fn class_of(a: &TestCacheArgs) -> RunnerClass {
    a.class.as_ref().map_or_else(RunnerClass::here, |c| RunnerClass(c.clone()))
}

/// The OS a class hashes for (the `os` of the input hash, Rust's name).
fn os_of(c: &RunnerClass) -> &'static str {
    match c.0.split('-').next().unwrap_or("") {
        "macOS" => "macos",
        "Windows" => "windows",
        _ => "linux",
    }
}

/// A record's reader-side check.
pub(super) type Trust = Box<dyn Fn(&Record) -> Result<(), super::trust::Distrust>>;

/// The reader-side trust check (§7): in CI every record's run is asked of the Gitea API; a local run reads its own store as is. With no
/// token nothing can be verified: every record is undecided (never trusted, and never replaced).
fn trust_check() -> Trust {
    let ci = env_get("GITHUB_ACTIONS").is_some_and(|v| !v.is_empty()) || env_get("GITEA_ACTIONS").is_some_and(|v| !v.is_empty());
    if !ci {
        return Box::new(|_| Ok(()));
    }
    let token = env_get("PLAN_TOKEN").or_else(|| env_get("GITEA_TOKEN")).unwrap_or_default();
    let server = env_get("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env_get("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let p = super::trust::Provenance::new(move |id, attempt| {
        if token.is_empty() {
            return Err("no API token (PLAN_TOKEN or GITEA_TOKEN)".into());
        }
        let mut http = crate::wait::CurlHttp;
        let mut client = crate::ci_runs::client::Client::new(&mut http, &server, &repo, &token);
        super::trust::facts(&mut client, id, attempt)
    });
    Box::new(move |r| p.check(r))
}

/// `record --log <file>`: stores the pass of every package the log proves (the CI record step, §3, §7). Packages with no test in the
/// log are not stored (a narrowed run names only its own), and so are the dirty ones. Needs the writer guard; report-only on failure
/// of the store (the job's own result is the tests').
fn record(
    a: &TestCacheArgs,
    root: &Path,
    cfg: &Config,
    cands: &[Candidate],
    store: &dyn Store,
    class: &RunnerClass,
) -> Result<Report, CliError> {
    if let Err(w) = write_allowed(&env_get) {
        return Ok(Report::new(
            serde_json::json!({"stored": 0}),
            format!("test-cache: nothing stored ({w})"),
        ));
    }
    let path = a
        .log
        .as_ref()
        .ok_or_else(|| fail("::error::ci test-cache record: --log <file> is required. Next step: pass the nextest output file.".into()))?;
    let log = std::fs::read_to_string(path).map_err(|e| {
        fail(format!(
            "::error::ci test-cache record: {} cannot be read: {e}. Next step: check the tee step.",
            path.display()
        ))
    })?;
    let meta = super::RunMeta {
        run: env_get("GITHUB_RUN_ID").unwrap_or_else(|| format!("local-{}", std::process::id())),
        // The job's display name and the run's attempt, which the CI step passes from its matrix (the reader finds the job by them,
        // trust.rs). Without them the record is untrusted in CI: fail closed.
        job: env_get("PADDOCK_JOB_NAME").unwrap_or_default().trim().to_string(),
        attempt: env_get("PADDOCK_RUN_ATTEMPT").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
        git_ref: env_get("GITHUB_REF").unwrap_or_default(),
        commit: git_text(root, &["rev-parse", "HEAD"]).unwrap_or_default().trim().to_string(),
        at: now(),
        os_build: os_build(),
    };
    let filter = filter_of(a);
    let (lines, stored) = record_with(&log, cands, cfg, class, &filter, &meta, store, &trust_check());
    Ok(Report::new(serde_json::json!({"stored": stored}), lines.join("\n")))
}

/// The record step over its ports (the log's text, the store and the trust check), for [`record`] and its tests: the shadow line, then
/// the passes stored under the write-once rule ([`super::not_overwriting_trusted`]). Returns the lines and how many records were stored.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_with(
    log: &str,
    cands: &[Candidate],
    cfg: &Config,
    class: &RunnerClass,
    filter: &str,
    meta: &super::RunMeta,
    store: &dyn Store,
    trust: &dyn Fn(&Record) -> Result<(), super::trust::Distrust>,
) -> (Vec<String>, usize) {
    let asked: Vec<String> = cands.iter().map(|c| c.target.package.clone()).collect();
    // The shadow phase (ER-41): of the targets a valid record already covered, how many ran here and passed, and which ran and failed (a
    // soundness finding: the key missed an input). Read before this run's own records are stored, and only when the lookup answered.
    let mut lines: Vec<String> = Vec::new();
    let (decisions, notice) = decide(cands, cfg, class, filter, store, meta.at, trust);
    let judged = super::result::judge(log, &asked);
    match &notice {
        // A lookup that did not answer measured nothing (ER-33): no shadow line, so `shadow-run` reads this job as not measured, and the cause
        // is said here (run 9335: every Linux and Windows record step held its records back without a word).
        Some(cause) => lines.push(format!(
            "::notice::test-cache: the store lookup failed: {cause}. Nothing was measured and no record is replaced (no shadow line). Next step: check the store's load and reachability from this runner."
        )),
        None => {
            let matched: Vec<&Candidate> = cands
                .iter()
                .zip(&decisions)
                .filter(|(_, d)| matches!(d, Decision::Reuse(_)))
                .map(|(c, _)| c)
                .collect();
            let shadow = super::shadow_outcome(&matched, &judged);
            lines.extend(shadow.findings.iter().map(|t| {
                format!("::error::test-cache soundness finding: {t} matched a valid record and then failed in this run. Next step: find the input its key misses (ADR 0116 section 8); the switch stays off.")
            }));
            // Records found but not trusted are named with why, so `matched 0` cannot hide them.
            lines.push(match super::untrusted_summary(&decisions) {
                Some(u) => format!("{}, {u}", shadow.line()),
                None => shadow.line(),
            });
        }
    }
    // What the lookup said of every candidate, counted by reason (the shadow report's "reused R; ran because: ...").
    lines.push(format!(
        "test-cache lookup: {}",
        super::reasons::Tally::of(&decisions).lookup_line()
    ));
    let (mut passed, mut not_in_leg, mut failed) = (0, 0, 0);
    for c in cands {
        match judged.outcome(&c.target.package) {
            super::result::TestOutcome::Passed { .. } => passed += 1,
            super::result::TestOutcome::NotSeen { .. } => not_in_leg += 1,
            super::result::TestOutcome::Failed { .. } => failed += 1,
        }
    }
    let (put, held) = super::not_overwriting_trusted(super::records_from_log(log, cands, cfg, class, filter, meta), cands, &decisions);
    let stored = match store.put_many(&put) {
        Ok(()) => {
            lines.push(format!(
                "test-cache: recorded {} packages of {} candidates ({passed} passed here; not in this leg's partition or without a test in the log: {not_in_leg}; failed or not clean: {failed}); {}",
                put.len(),
                cands.len(),
                held.held_line()
            ));
            put.len()
        }
        Err(e) => {
            lines.push(format!("::warning::test-cache: records not stored: {e}"));
            0
        }
    };
    (lines, stored)
}

/// The filters the legs of `workspace` on `class` carry, normalized and without repeats: the whole package, and the filterset of a host
/// leg's scope (`hostTests`), read through the planner's own leg builder ([`crate::plan_ci::workspace_legs`]) so the plan and the legs
/// cannot disagree. A member run's own filter is not here: its legs find no entry and run everything.
fn leg_filters(root: &Path, w: &World, workspace: &str, class: &RunnerClass) -> Result<Vec<String>, String> {
    let contract = if workspace == "crates" {
        crate::plan_cmd::CRATES_CONTRACT
    } else {
        crate::plan_cmd::PADDOCK_DEV_CONTRACT
    };
    let host = crate::plan_cmd::read_host_tests(root, contract, &w.graph.derived(workspace))?;
    let mut out = vec![String::new()];
    for leg in crate::plan_ci::workspace_legs(&crate::plan_ci::os_legs(), host.as_ref(), "", None) {
        if super::plan::class_of_runner(leg["os"].as_str().unwrap_or("")) == class.0 {
            out.push(super::key::normalize_filter(leg["test_filter"].as_str().unwrap_or("")));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// `plan --out <file>`: for each runner class and each of the two workspaces, the packages with a valid record, as the
/// [`super::plan::ReusePlan`] `./dev plan --ci` reads. While the switch is off (`docs/test-cache.toml`, shadow) and `--reuse` is not given,
/// the file holds an empty plan and the lines say what would be reused: nothing is skipped (ER-41). Fails open: a store that cannot be
/// read, a run that may not read (tag, release, nightly) or a workspace that cannot be hashed gives no reuse for it, with the cause.
fn plan(a: &TestCacheArgs, root: &Path, store: &dyn Store) -> Result<Report, CliError> {
    let cfg = Config::read(root).map_err(fail)?;
    let skip = cfg.mode() == Mode::On || a.reuse;
    let mut out = super::plan::ReusePlan::default();
    let mut text: Vec<String> = Vec::new();
    let allowed = read_allowed(&env_get);
    if let Err(w) = &allowed {
        text.push(format!("::notice::test-cache: reuse is off for this run: {w}. Every target runs."));
    }
    let trust = trust_check();
    let mut would = 0;
    let w = if allowed.is_ok() { Some(world(root)?) } else { None };
    if let Some(w) = &w {
        for class in ["Linux-X64", "macOS-ARM64", "Windows-X64"] {
            let class = RunnerClass(class.to_string());
            for ws in ["crates", "tools/paddock-dev"] {
                let cands = match candidates_in(root, w, ws, os_of(&class)) {
                    Ok(c) => c,
                    Err(e) => {
                        text.push(format!("::notice::test-cache: {ws} on {} not planned: {e}", class.0));
                        continue;
                    }
                };
                // One lookup per filter a leg of this class may carry (the whole package, and a host leg's scope).
                let filters = match leg_filters(root, w, ws, &class) {
                    Ok(f) => f,
                    Err(e) => {
                        text.push(format!(
                            "::notice::test-cache: {ws} on {} has no filtered legs planned: {e}",
                            class.0
                        ));
                        vec![String::new()]
                    }
                };
                for filter in filters {
                    let (decisions, notice) = decide(&cands, &cfg, &class, &filter, store, now(), &trust);
                    if let Some(n) = notice {
                        text.push(format!("::notice::test-cache: reuse is off for {} {ws}: {n}.", class.0));
                    }
                    // The shadow report says why the rest ran: a store that timed out reads `reuse off 61 (curl: (28) ...)`, never a bare 0
                    // (run 9335: Linux-X64 went from 56 reused to 0 with the cause only in a notice).
                    text.push(super::reasons::plan_line(
                        &class.0,
                        ws,
                        &filter,
                        &super::reasons::Tally::of(&decisions),
                    ));
                    for (c, d) in cands.iter().zip(decisions) {
                        if let Decision::Reuse(r) = d {
                            would += 1;
                            if skip {
                                out.by_class
                                    .entry(class.0.clone())
                                    .or_default()
                                    .entry(filter.clone())
                                    .or_default()
                                    .insert(c.target.package.clone(), format!("{}@{} from run {}", r.target, class.0, r.run));
                            }
                        }
                    }
                }
            }
        }
    }
    let line = if skip {
        format!("test-cache plan: reuse is ON: {would} package(s) skipped across the runner classes")
    } else {
        format!(
            "test-cache plan: shadow: {would} package(s) would be skipped; nothing is skipped until docs/test-cache.toml records two clean shadow runs"
        )
    };
    text.push(line);
    if let Some(p) = &a.out {
        std::fs::write(p, out.json().to_string()).map_err(|e| {
            fail(format!(
                "::error::ci test-cache plan: {} cannot be written: {e}. Next step: check the path.",
                p.display()
            ))
        })?;
    }
    Ok(Report::new(serde_json::json!({"would": would, "skipping": skip}), text.join("\n")))
}

/// The step whose log carries a job's shadow line.
const RECORD_STEP: &str = "Record test results (test cache)";

/// `shadow-run --run <id> [--write]`: is this CI run a clean shadow run (ADR 0116 §8, ER-41)? It must be a green run of ci.yml on a
/// train branch, its record steps must have printed shadow lines, at least one target must have matched a record (a run that matched
/// nothing proved nothing) and no matched target may have failed. Exit 0 clean (the run's id is added to docs/test-cache.toml with
/// `--write`), 5 a soundness finding, 6 not measured (never a pass). Two distinct clean runs switch reuse on.
fn shadow_run(a: &TestCacheArgs, root: &Path) -> Result<Report, CliError> {
    let not_measured = |why: String| {
        Ok(Report::new(
            serde_json::json!({"verdict": "not_measured"}),
            format!("::error::ci test-cache shadow-run: not measured: {why}. Next step: read the run's record steps."),
        )
        .with_exit(Exit::NotMeasured))
    };
    let Some(id) = a.run else {
        return Err(fail(
            "::error::ci test-cache shadow-run: --run <id> is required. Next step: pass the train run's id.".into(),
        ));
    };
    let token = env_get("GITEA_TOKEN").or_else(|| env_get("PLAN_TOKEN")).unwrap_or_default();
    if token.is_empty() {
        return not_measured("no API token (GITEA_TOKEN)".into());
    }
    let server = env_get("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env_get("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let mut http = crate::wait::CurlHttp;
    let mut client = crate::ci_runs::client::Client::new(&mut http, &server, &repo, &token);
    let run = match client.run(id) {
        Ok(Some(r)) => r,
        Ok(None) => return not_measured(format!("run {id} does not exist")),
        Err(e) => return not_measured(format!("run {id} could not be read: {e}")),
    };
    if run.workflow() != "ci.yml" || !run.git_ref().starts_with("refs/heads/train/") || run.conclusion != "success" {
        return not_measured(format!(
            "run {id} is not a green ci.yml run on a train ({} on {}, {})",
            run.workflow(),
            run.git_ref(),
            run.conclusion
        ));
    }
    let jobs = match client.timed_jobs(id) {
        Ok(Some(j)) => j,
        Ok(None) => return not_measured(format!("run {id} has no job list")),
        Err(e) => return not_measured(format!("the jobs of run {id} could not be read: {e}")),
    };
    let mut logs = Vec::new();
    for j in jobs.iter().filter(|j| j.steps.iter().any(|(n, _)| n == RECORD_STEP)) {
        match client.log(j.id) {
            Ok(Some(l)) => logs.push(l),
            Ok(None) => return not_measured(format!("job {} has no log", j.id)),
            Err(e) => return not_measured(format!("the log of job {} could not be read: {e}", j.id)),
        }
    }
    let Some((lines, sum)) = super::read_shadow(&logs) else {
        return not_measured(format!(
            "none of the {} record job log(s) of run {id} has a `test-cache shadow:` line",
            logs.len()
        ));
    };
    let summary = format!(
        "run {id}: {lines} shadow line(s), matched {}, passed {}, findings {}",
        sum.matched,
        sum.passed,
        sum.findings.len()
    );
    if !sum.findings.is_empty() {
        return Ok(Report::new(
            serde_json::json!({"verdict": "finding", "run": id}),
            format!(
                "::error::ci test-cache shadow-run: {summary}. A matched target failed: the key misses an input; the switch stays off."
            ),
        )
        .with_exit(Exit::Findings));
    }
    if sum.matched == 0 {
        return not_measured(format!("{summary}: nothing matched a record, so nothing was proven"));
    }
    let mut text = format!("test-cache shadow-run: clean: {summary}");
    if a.write {
        let path = root.join(super::config::FILE);
        let cur = std::fs::read_to_string(&path).map_err(|e| fail(format!("{} cannot be read: {e}", path.display())))?;
        let next = super::with_green_run(&cur, id).map_err(fail)?;
        if next != cur {
            std::fs::write(&path, next).map_err(|e| fail(format!("{} cannot be written: {e}", path.display())))?;
            text.push_str(&format!("\ntest-cache shadow-run: run {id} added to {}", super::config::FILE));
        }
    }
    Ok(Report::new(serde_json::json!({"verdict": "clean", "run": id}), text))
}
