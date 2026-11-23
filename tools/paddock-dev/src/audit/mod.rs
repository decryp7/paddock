//! `dev audit nightly|compare`: the advisory audits of every lockfile (ER-41; docs/ci-schedules.md "Dependency audits").
//!
//! External advisories turn a tree red with no code change (RUSTSEC-2026-0543 on chrono, run 8214; GHSA-fc5q-39xc-vjpf on picomatch,
//! run 8225), on main and every train alike. As Dependabot, Renovate and a scheduled `cargo deny` do, two questions are kept apart:
//!
//! - `nightly`: does main have a finding? Every one is an `::error::` that names the advisory, the dependency path and the decision
//!   step (an exception with a reason and a review date, which the user approves, or a fix).
//! - `compare`: does this change add one? The merge base's lockfiles are audited with the same tools ([`GitBase`]), and a finding
//!   main has too is a `::warning::` (decided there). Only a finding the change adds fails. No list of accepted findings exists
//!   (ER-46): main's own result is the reference, measured again each time.
//!
//! A tool that did not look (down, offline, no advisory database, unparseable answer) is NOT MEASURED, and never a pass (ER-33).
//! Report-only until `--enforce` is given (ER-41): the workflows pass it when the repository variable `PADDOCK_AUDIT_ENFORCE`
//! is `1`, which is set after the first green run of each job. The tools are `cargo deny` (every Cargo.lock that has a Cargo.toml
//! and a deny.toml beside it) and `npm audit` (every package-lock.json).

mod allow;
pub mod deny_step;
mod judge;
mod parse;
#[cfg(test)]
mod tests;

use crate::locks::program_for;
pub use allow::{ALLOW_FILE, Exception, parse_allow};
use clap::{Args, Subcommand};
pub use judge::{Mode, Verdict, lines};
use paddock_cli::{CliError, Exit, Report};
use paddock_proc::{Limits, command as proc_command, run_bounded};
pub use parse::{Finding, Measure, NPM_LEVELS, parse_deny, parse_npm};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The longest any one audit command may run (a cold run fetches the advisory database and the registry index).
const LIMIT: Duration = Duration::from_secs(900);
/// Where the merge base's manifests are extracted, under the tool's own ignored build folder.
const BASE_DIR: &str = "tools/paddock-dev/target/audit-base";

/// `dev audit`.
#[derive(Subcommand)]
pub enum AuditCommand {
    /// Audit every lockfile on this tree (main, nightly): one `::error::` per advisory, with the dependency path and the decision step.
    Nightly(AuditArgs),
    /// Audit this tree and the merge base with `--base`: only an advisory the change adds fails, one main has too warns.
    Compare(CompareArgs),
    /// The one `cargo deny` step of a workflow: licences, bans and sources block; advisories block only when the change touches this
    /// workspace's dependencies, on an unknown base, and on a tag, release or scheduled run, else they warn (ER-41). Run it in the
    /// workspace folder; arguments after `--` go before `check` (`-- --all-features --workspace`).
    Deny(DenyArgs),
}

/// Arguments of `deny`.
#[derive(Args)]
pub struct DenyArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// A release: every advisory blocks (a tag ref or a scheduled event counts without it).
    #[arg(long)]
    pub strict: bool,
    /// The ref the change is measured from (default: PADDOCK_BASE_REF; none or unresolvable: every advisory blocks).
    #[arg(long)]
    pub base: Option<String>,
    /// Arguments for `cargo deny` before `check`.
    #[arg(last = true)]
    pub extra: Vec<String>,
}

/// Arguments of both commands.
#[derive(Args)]
pub struct AuditArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Fail on a finding or a missing measurement. Without it the run is report-only: the same lines as warnings, exit 0 (ER-41).
    #[arg(long)]
    pub enforce: bool,
    /// The lowest npm severity that counts (the plain audit step of debrief.yml uses `high`).
    #[arg(long, default_value = "high", value_parser = NPM_LEVELS)]
    pub npm_level: String,
    /// Leave devDependencies out of the npm audit, as the apps' plain `npm audit --omit=dev` steps do: what ships (`--npm-omit-dev false` audits all).
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub npm_omit_dev: bool,
}

/// The npm audit's scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Npm {
    /// The lowest severity that counts.
    pub level: String,
    /// Dev dependencies are left out.
    pub omit_dev: bool,
}

impl AuditArgs {
    fn npm(&self) -> Npm {
        Npm {
            level: self.npm_level.clone(),
            omit_dev: self.npm_omit_dev,
        }
    }
}

/// Arguments of `compare`.
#[derive(Args)]
pub struct CompareArgs {
    /// The arguments both commands take.
    #[command(flatten)]
    pub common: AuditArgs,
    /// The branch whose merge base with HEAD is the reference (needs history: fetch-depth 0 in CI).
    #[arg(long, default_value = "origin/main")]
    pub base: String,
}

/// What an audit target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A Cargo workspace with a deny.toml: `cargo deny check advisories`.
    Cargo,
    /// An npm lockfile: `npm audit`.
    Npm,
}

/// One lockfile to audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The tool.
    pub kind: Kind,
    /// The folder the tool runs in, relative to the root.
    pub dir: PathBuf,
    /// The lockfile, relative to the root, `/` separated.
    pub file: String,
}

impl Target {
    /// The name in findings: `cargo crates/Cargo.lock`.
    pub fn source(&self) -> String {
        format!("{} {}", if self.kind == Kind::Cargo { "cargo" } else { "npm" }, self.file)
    }
}

/// The targets of a list of tracked files (paths relative to the root, `/` separated).
pub fn plan(tracked: &[String]) -> Vec<Target> {
    let has = |p: &str| tracked.iter().any(|t| t == p);
    let mut files: Vec<&String> = tracked.iter().collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        if crate::locks::is_test_data(f) {
            continue;
        }
        let dir = Path::new(f).parent().map(Path::to_path_buf).unwrap_or_default();
        let beside = |name: &str| {
            if dir.as_os_str().is_empty() {
                name.to_string()
            } else {
                format!("{}/{name}", dir.display())
            }
        };
        match f.rsplit('/').next() {
            Some("Cargo.lock") if has(&beside("Cargo.toml")) && has(&beside("deny.toml")) => {
                out.push(Target {
                    kind: Kind::Cargo,
                    dir: dir.clone(),
                    file: f.clone(),
                });
            }
            Some("package-lock.json") => out.push(Target {
                kind: Kind::Npm,
                dir: dir.clone(),
                file: f.clone(),
            }),
            _ => {}
        }
    }
    out
}

/// What running one command said; `code` is `None` when it could not run or was ended.
#[derive(Clone, Debug)]
pub struct Ran {
    /// The exit code.
    pub code: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error (cargo deny's JSON lines).
    pub stderr: String,
}

/// Starts programs; faked in tests.
pub trait Runner: Sync {
    /// Runs `argv` in `dir`.
    fn run(&self, dir: &Path, argv: &[String]) -> Ran;
}

/// The merge base's manifests, extracted on demand; faked in tests.
pub trait Base {
    /// Makes the folder holding the merge base's manifests and returns it (repeated calls may extract again), or says why not.
    fn prepare(&self) -> Result<PathBuf, String>;
    /// Removes what [`Base::prepare`] made.
    fn release(&self);
}

/// The real runner: bounded, through `paddock-proc`, `RUSTUP_AUTO_INSTALL=0` (ER-45: a probe never installs a toolchain).
pub struct Real;

impl Runner for Real {
    fn run(&self, dir: &Path, argv: &[String]) -> Ran {
        let Some((program, args)) = argv.split_first() else {
            return Ran {
                code: None,
                stdout: String::new(),
                stderr: "no command".into(),
            };
        };
        let mut cmd = proc_command(program_for(
            program,
            &std::env::var("PATH").unwrap_or_default(),
            std::env::var("PATHEXT").ok().as_deref(),
            cfg!(windows),
        ));
        cmd.current_dir(dir).args(args).env("RUSTUP_AUTO_INSTALL", "0");
        match run_bounded(cmd, Limits::new(LIMIT).max_output(1 << 25)) {
            Ok(o) => Ran {
                code: exit_code(&o),
                stdout: o.stdout_text(),
                stderr: o.stderr_text(),
            },
            Err(e) => Ran {
                code: None,
                stdout: String::new(),
                stderr: format!("{e}"),
            },
        }
    }
}

fn exit_code(o: &paddock_proc::Output) -> Option<i32> {
    match o.outcome {
        paddock_proc::Outcome::Exited(s) => s.code(),
        _ => None,
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

/// Audits one target in the tree at `tree` (the checkout, or the merge base's folder).
pub fn audit_target(runner: &dyn Runner, tree: &Path, t: &Target, npm: &Npm) -> Measure {
    let dir = tree.join(&t.dir);
    match t.kind {
        Kind::Cargo => {
            let r = runner.run(
                &dir,
                &argv(&["cargo", "deny", "--locked", "--format", "json", "check", "advisories"]),
            );
            if r.code.is_none() {
                return Measure::NotMeasured(format!("cargo deny could not run ({})", r.stderr.trim()));
            }
            parse_deny(&t.source(), &format!("{}\n{}", r.stderr, r.stdout))
        }
        Kind::Npm => {
            let mut args = argv(&["npm", "audit", "--json", "--package-lock-only"]);
            if npm.omit_dev {
                args.push("--omit=dev".into());
            }
            let r = runner.run(&dir, &args);
            if r.code.is_none() {
                return Measure::NotMeasured(format!("npm could not run ({})", r.stderr.trim()));
            }
            parse_npm(&t.source(), &r.stdout, &npm.level)
        }
    }
}

/// Runs the audit of every target and judges it.
pub fn judge_all(runner: &dyn Runner, root: &Path, targets: &[Target], npm: &Npm, base: Option<&dyn Base>) -> Verdict {
    let mut verdict = Verdict::default();
    let mut prepared: Option<Result<PathBuf, String>> = None;
    for t in targets {
        let source = t.source();
        let head = audit_target(runner, root, t, npm);
        let v = match base {
            None => Verdict::nightly(&source, &head),
            Some(b) => Verdict::compare(&source, &head, &mut || {
                let tree = prepared.get_or_insert_with(|| b.prepare());
                match tree {
                    // A lockfile the change adds is not on the merge base: main has none of its findings, so each one is new.
                    Ok(tree) if !tree.join(&t.file).exists() => Measure::Found(Vec::new()),
                    Ok(tree) => audit_target(runner, tree, t, npm),
                    Err(why) => Measure::NotMeasured(why.clone()),
                }
            }),
        };
        verdict.merge(v);
    }
    if let Some(b) = base
        && prepared.is_some()
    {
        b.release();
    }
    verdict
}

fn report(verdict: &Verdict, mode: Mode, enforce: bool, targets: usize) -> Report {
    let mut text: String = lines(verdict, mode, enforce).iter().map(|l| format!("{l}\n")).collect();
    text.push_str(&format!(
        "audit: {targets} lockfile(s), {} new, {} already on main, {} allowed by an exception, {} exception(s) expired, {} not measured; {}\n",
        verdict.new.len(),
        verdict.existing.len(),
        verdict.allowed.len(),
        verdict.expired.len(),
        verdict.not_measured.len(),
        if enforce {
            "enforced"
        } else {
            "report-only (PADDOCK_AUDIT_ENFORCE is not 1)"
        }
    ));
    let f = |v: &[Finding]| {
        v.iter()
            .map(|f| json!({"source": f.source, "id": f.id, "package": f.package, "path": f.path}))
            .collect::<Vec<_>>()
    };
    let data = json!({
        "enforced": enforce,
        "new": f(&verdict.new),
        "existing": f(&verdict.existing),
        "allowed": verdict.allowed.iter().map(|(f, e)| json!({"source": f.source, "id": f.id, "package": f.package, "path": f.path, "reviewBy": e.review_by})).collect::<Vec<_>>(),
        "expired": verdict.expired.iter().map(|e| json!({"id": e.id, "reviewBy": e.review_by})).collect::<Vec<_>>(),
        "not_measured": verdict.not_measured.iter().map(|(s, w)| json!({"source": s, "why": w})).collect::<Vec<_>>(),
    });
    let r = Report::new(data, text);
    if !enforce || verdict.clean() {
        r
    } else if verdict.new.is_empty() && verdict.expired.is_empty() {
        r.with_exit(Exit::NotMeasured)
    } else {
        r.with_exit(Exit::Findings)
    }
}

/// The tracked files that name audit targets.
fn tracked(root: &Path) -> Result<Vec<String>, CliError> {
    let mut cmd = crate::git_env::git();
    cmd.arg("-C")
        .arg(root)
        .args(["ls-files", "--", "*Cargo.lock", "*Cargo.toml", "*deny.toml", "*package-lock.json"]);
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(60)).max_output(1 << 24))
        .map_err(|e| CliError::failure(format!("git could not list the tracked files: {e}.")))?;
    if !out.success() {
        return Err(
            CliError::failure(format!("git ls-files failed in {}: {}", root.display(), out.stderr_text().trim()))
                .with_hint("Run it inside the repository, or pass --root."),
        );
    }
    Ok(out.stdout_text().lines().map(str::to_string).collect())
}

/// The merge base of HEAD and a branch, extracted with `git archive` into [`BASE_DIR`]: the manifests, lockfiles, deny.toml, cargo config and
/// Rust sources (cargo needs a target per package), not the whole tree.
pub struct GitBase {
    root: PathBuf,
    base: String,
}

impl GitBase {
    fn git(&self, args: &[&str]) -> Result<String, String> {
        let mut cmd = crate::git_env::git();
        cmd.arg("-C").arg(&self.root).args(args);
        let out =
            run_bounded(cmd, Limits::new(Duration::from_secs(300)).max_output(1 << 24)).map_err(|e| format!("git {}: {e}", args[0]))?;
        if out.success() {
            Ok(out.stdout_text())
        } else {
            Err(format!("git {} failed: {}", args[0], out.stderr_text().trim()))
        }
    }

    fn work(&self) -> PathBuf {
        self.root.join(BASE_DIR)
    }
}

impl Base for GitBase {
    fn prepare(&self) -> Result<PathBuf, String> {
        let sha = self
            .git(&["merge-base", "HEAD", &self.base])
            .map_err(|e| format!("{e}; fetch the history (fetch-depth 0) and the branch {}", self.base))?;
        let sha = sha.trim();
        self.release();
        let work = self.work();
        std::fs::create_dir_all(&work).map_err(|e| format!("{} cannot be made: {e}", work.display()))?;
        let tar = work.join("base.tar");
        self.git(&[
            "archive",
            "--format=tar",
            "-o",
            &tar.display().to_string(),
            sha,
            "--",
            "*.toml",
            "*.lock",
            "*.rs",
            "*package.json",
            "*package-lock.json",
        ])?;
        let mut cmd = proc_command("tar");
        cmd.arg("-xf").arg(&tar).arg("-C").arg(&work);
        let out = run_bounded(cmd, Limits::new(Duration::from_secs(300))).map_err(|e| format!("tar: {e}"))?;
        if !out.success() {
            return Err(format!("tar could not extract the merge base {sha}: {}", out.stderr_text().trim()));
        }
        Ok(work)
    }

    fn release(&self) {
        let work = self.work();
        if let Some(parent) = work.parent() {
            // The folder is ours and under the ignored build folder; a failure to remove it only leaves a stale copy for the next run.
            let _ = crate::safe_remove::remove_under(parent, &work, None);
        }
    }
}

fn root_of(given: Option<PathBuf>) -> Result<PathBuf, CliError> {
    crate::gates::command::repo_root(given.as_deref())
}

/// `audit nightly` and `audit compare`.
pub fn command(c: AuditCommand) -> Result<Report, CliError> {
    match c {
        AuditCommand::Deny(a) => deny_step::command(a.root, a.strict, a.base, a.extra),
        AuditCommand::Nightly(a) => {
            let npm = a.npm();
            let root = root_of(a.root)?;
            run(&Real, &root, &npm, &allow::today_utc(), a.enforce, None)
        }
        AuditCommand::Compare(a) => {
            let npm = a.common.npm();
            let root = root_of(a.common.root)?;
            let base = GitBase {
                root: root.clone(),
                base: a.base,
            };
            run(&Real, &root, &npm, &allow::today_utc(), a.common.enforce, Some(&base))
        }
    }
}

/// What stops a run before any audit: in report-only mode (ER-41) a `::warning::` and exit 0, in enforce mode the error. A gate that
/// cannot start never turns a report-only job red, and never passes an enforced one.
fn cannot_start(e: CliError, enforce: bool) -> Result<Report, CliError> {
    if enforce {
        return Err(e);
    }
    let next = if e.hint.is_empty() {
        String::new()
    } else {
        format!(" Next step: {}", e.hint)
    };
    let text = format!(
        "::warning::report-only, would fail: audit did not run: {}.{next}\naudit: NOT MEASURED; report-only (PADDOCK_AUDIT_ENFORCE is not 1)\n",
        e.message.trim_end_matches('.')
    );
    Ok(Report::new(
        json!({"enforced": false, "not_run": e.message, "new": [], "existing": [], "allowed": [], "expired": [], "not_measured": []}),
        text,
    ))
}

/// Both commands, with the runner and the merge base given.
pub fn run(runner: &dyn Runner, root: &Path, npm: &Npm, today: &str, enforce: bool, base: Option<&dyn Base>) -> Result<Report, CliError> {
    let targets = match tracked(root) {
        Ok(t) => plan(&t),
        Err(e) => return cannot_start(e, enforce),
    };
    if targets.is_empty() {
        return cannot_start(
            CliError::failure(format!("No lockfile with an audit found under {}.", root.display()))
                .with_hint("Run it in the repository root, or pass --root."),
            enforce,
        );
    }
    let allow = match allow::load(root) {
        Ok(a) => a,
        Err(e) => {
            return cannot_start(
                CliError::failure(e).with_hint("Fix scripts/npm-audit-allow.json (id, reason, approvedBy, approved, reviewBy): it is the family's one list of npm audit exceptions."),
                enforce,
            );
        }
    };
    let mut verdict = judge_all(runner, root, &targets, npm, base);
    verdict.apply_exceptions(&allow, today);
    Ok(report(
        &verdict,
        if base.is_some() { Mode::Compare } else { Mode::Nightly },
        enforce,
        targets.len(),
    ))
}
