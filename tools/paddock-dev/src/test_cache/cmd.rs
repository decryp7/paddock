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
    /// `record`: the file holding a nextest run's output (stdout and stderr), read as the run's result.
    #[arg(long)]
    log: Option<PathBuf>,
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

/// The local store's folder: `<git common dir>/paddock-pre-push/test-cache`, beside the gate rows' records.
pub fn store_dir(root: &Path) -> PathBuf {
    git_text(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .map(|t| PathBuf::from(t.trim()))
        .unwrap_or_else(|| root.join(".git"))
        .join("paddock-pre-push")
        .join("test-cache")
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

/// The candidates with their node hashes, in one bulk read.
pub fn candidates(root: &Path, workspace: &str, os: &str) -> Result<Vec<Candidate>, CliError> {
    let history = GitHistory::new(root, READ_TIMEOUT);
    let graph = crate::plan::read_graph(root, &history).map_err(|e| fail(e.to_string()))?;
    let ts = targets(&graph.cargo, workspace);
    if ts.is_empty() {
        return Err(fail(format!(
            "::error::ci test-cache: the Cargo graph has no package in the workspace `{workspace}`. Next step: name a workspace folder such as `crates`."
        )));
    }
    let world = crate::repo_model::build_world(root, &history, &graph, None).map_err(fail)?;
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
    let hashes = crate::repo_model::hash::node_hashes(&crate::input_hash::Real, &ctx, root, &world, &tree, &nodes)?;
    ts.into_iter()
        .map(|t| {
            let h = hashes.get(&t.node).ok_or_else(|| {
                fail(format!(
                    "::error::ci test-cache: no input hash for {}. Next step: run `./dev plan --hash {}`.",
                    t.node, t.node
                ))
            })?;
            Ok(Candidate {
                target: t,
                node_hash: h.hash.clone(),
                dirty: h.dirty,
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
                return forget(&cfg, &cands, store);
            }
            if a.action == "record" {
                return record(a, &root, &cfg, &cands, store, &class);
            }
            run(a, &root, &cfg, &cands, store, &class)
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

fn run(
    a: &TestCacheArgs,
    root: &Path,
    cfg: &Config,
    cands: &[Candidate],
    store: &dyn Store,
    class: &RunnerClass,
) -> Result<Report, CliError> {
    let started = std::time::Instant::now();
    let mut text: Vec<String> = Vec::new();
    // Whether this run may read at all, and what the lookup said.
    let (decisions, notice) = match read_allowed(&env_get) {
        Err(why) => (
            cands.iter().map(|_| Decision::Run(super::Why::Off(why.clone()))).collect(),
            Some(why),
        ),
        Ok(()) => decide(cands, cfg, class, store, now(), &trust_check()),
    };
    if let Some(n) = notice {
        text.push(format!("::notice::test-cache: reuse is off for this run: {n}. Every target runs."));
    }
    let nightly = super::is_nightly(&env_get);
    let skip = cfg.mode() == Mode::On || a.reuse;
    let mut reused = 0;
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
                text.push(format!("test-cache: run target={} ({})", c.target.id, w.text()));
                left.push(c);
            }
        }
    }
    if a.action == "lookup" {
        text.push(format!(
            "test-cache: {} targets, {reused} would be reused, {} would run",
            cands.len(),
            left.len()
        ));
        return Ok(Report::new(
            serde_json::json!({"targets": cands.len(), "reused": reused}),
            text.join("\n"),
        ));
    }
    let mut findings = 0;
    let mut ran_secs = 0.0;
    if !left.is_empty() {
        let asked: Vec<String> = left.iter().map(|c| c.target.package.clone()).collect();
        let t0 = std::time::Instant::now();
        let out = nextest(a, root, cfg, &asked, left.len() == cands.len() && a.only.is_none())?;
        ran_secs = t0.elapsed().as_secs_f64();
        let judged = super::result::judge(&out.0, &asked);
        let mut put: Vec<Record> = Vec::new();
        let writes = write_allowed(&env_get);
        for c in &left {
            let p = &c.target.package;
            if judged.passed.contains(p) {
                if !c.dirty && writes.is_ok() {
                    put.push(Record {
                        key: super::key_of(c, cfg, class),
                        target: c.target.id.clone(),
                        class: class.clone(),
                        run: format!("local-{}", std::process::id()),
                        job: String::new(),
                        git_ref: env_get("GITHUB_REF").unwrap_or_default(),
                        commit: git_text(root, &["rev-parse", "HEAD"]).unwrap_or_default().trim().to_string(),
                        tests: judged.tests.get(p).copied().unwrap_or(0),
                        secs: ran_secs,
                        at: now(),
                        os_build: os_build(),
                    });
                }
            } else {
                // The nightly is the net (§6): a target that fails loses the cached pass under its current key, and the run names it.
                if nightly {
                    let ask = Lookup {
                        target: c.target.id.clone(),
                        class: class.clone(),
                        key: super::key_of(c, cfg, class),
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
                let why = judged
                    .not_passed
                    .iter()
                    .find(|(q, _)| q == p)
                    .map_or("no verdict", |(_, w)| w.as_str());
                if matched.iter().any(|m| m.target.id == c.target.id) {
                    findings += 1;
                    text.push(format!(
                        "::error::test-cache soundness finding: {} matched a valid record and then failed ({why}). Next step: find the input its key misses (ADR 0116 §8); the switch stays off.",
                        c.target.id
                    ));
                } else {
                    text.push(format!("test-cache: not stored target={} ({why})", c.target.id));
                }
            }
        }
        if let Err(w) = &writes {
            text.push(format!("test-cache: nothing stored ({w})"));
        } else if let Err(e) = store.put_many(&put) {
            text.push(format!("::warning::test-cache: records not stored: {e}"));
        } else {
            text.push(format!("test-cache: stored {} pass(es)", put.len()));
        }
        if !out.1 {
            text.push(format!(
                "{}\n::error::ci test-cache: nextest failed. Next step: fix the test it names above.",
                tail(&out.0)
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

fn tail(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

/// The runner's OS build as observed (`uname -sr`): reported in each record, never keyed (decision D5).
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
fn nextest(a: &TestCacheArgs, root: &Path, cfg: &Config, asked: &[String], whole: bool) -> Result<(String, bool), CliError> {
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
        "--no-tests=warn",
        "--workspace",
    ]);
    if a.workspace == "crates" {
        cmd.arg("--all-features");
    }
    if !whole {
        let filter = asked.iter().map(|p| format!("package(={p})")).collect::<Vec<_>>().join(" | ");
        cmd.args(["-E", &filter]);
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
fn forget(cfg: &Config, cands: &[Candidate], store: &dyn Store) -> Result<Report, CliError> {
    let class = RunnerClass::here();
    let asks: Vec<Lookup> = cands
        .iter()
        .map(|c| Lookup {
            target: c.target.id.clone(),
            class: class.clone(),
            key: super::key_of(c, cfg, &class),
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
type Trust = Box<dyn Fn(&Record) -> Result<(), String>>;

/// The reader-side trust check (§7): in CI every record's run is asked of the Gitea API; a local run reads its own store as is. With no
/// token the check refuses every record (never trusts what it cannot verify).
fn trust_check() -> Trust {
    let ci = env_get("GITHUB_ACTIONS").is_some_and(|v| !v.is_empty()) || env_get("GITEA_ACTIONS").is_some_and(|v| !v.is_empty());
    if !ci {
        return Box::new(|_| Ok(()));
    }
    let token = env_get("PLAN_TOKEN").or_else(|| env_get("GITEA_TOKEN")).unwrap_or_default();
    let server = env_get("GITHUB_SERVER_URL").unwrap_or_else(|| "https://dev.decryptology.net".into());
    let repo = env_get("GITHUB_REPOSITORY").unwrap_or_else(|| "decryp7/paddock".into());
    let p = super::trust::Provenance::new(move |id| {
        if token.is_empty() {
            return Err("no API token (PLAN_TOKEN or GITEA_TOKEN)".into());
        }
        let mut http = crate::wait::CurlHttp;
        let mut client = crate::ci_runs::client::Client::new(&mut http, &server, &repo, &token);
        super::trust::facts(&mut client, id)
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
    let asked: Vec<String> = cands.iter().map(|c| c.target.package.clone()).collect();
    let judged = super::result::judge(&log, &asked);
    let commit = git_text(root, &["rev-parse", "HEAD"]).unwrap_or_default().trim().to_string();
    let job = env_get("GITHUB_JOB_ID").unwrap_or_default();
    let run_id = env_get("GITHUB_RUN_ID").unwrap_or_else(|| format!("local-{}", std::process::id()));
    let put: Vec<Record> = cands
        .iter()
        .filter(|c| !c.dirty && judged.passed.contains(&c.target.package))
        .map(|c| Record {
            key: super::key_of(c, cfg, class),
            target: c.target.id.clone(),
            class: class.clone(),
            run: run_id.clone(),
            job: job.clone(),
            git_ref: env_get("GITHUB_REF").unwrap_or_default(),
            commit: commit.clone(),
            tests: judged.tests.get(&c.target.package).copied().unwrap_or(0),
            secs: 0.0,
            at: now(),
            os_build: os_build(),
        })
        .collect();
    let line = match store.put_many(&put) {
        Ok(()) => format!("test-cache: stored {} pass(es) of {} package(s) in the log", put.len(), asked.len()),
        Err(e) => format!("::warning::test-cache: records not stored: {e}"),
    };
    Ok(Report::new(serde_json::json!({"stored": put.len()}), line))
}
