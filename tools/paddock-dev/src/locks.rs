//! `locks check` and `locks sync`: every lockfile of the repository is current, and one command makes them so.
//!
//! What goes stale after a shared change is always the same three things: a `Cargo.lock` that `--locked` no longer resolves,
//! the root `package-lock.json`, and the workspace hack that cargo-hakari generates. `check` runs the same commands CI does
//! (`cargo metadata --locked` per workspace, `npm ci --dry-run`, `cargo hakari generate --diff`, `manage-deps --dry-run` and
//! `verify`) and prints one `::error::` per stale file with the fix. `sync` runs only cargo, cargo-hakari and npm to regenerate
//! them; a lockfile is never edited by hand (ER-40). The commands run through a [`Runner`], so the plan is tested without them.

use paddock_cli::{CliError, Exit, Report};
use paddock_proc::{Limits, command, run_bounded};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// The longest any one command may run (a cold `cargo metadata` fetches the registry index).
const LIMIT: Duration = Duration::from_secs(600);
/// Checks that run at once: they read the network and the cargo package cache, not the CPU.
const PARALLEL: usize = 4;

/// What a lockfile is, and so which tool regenerates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A `Cargo.lock` next to its `Cargo.toml`.
    Cargo,
    /// The npm workspace's `package-lock.json`.
    Npm,
    /// The workspace hack of a workspace with `.config/hakari.toml`.
    Hakari,
}

/// One thing to keep current: a command that must succeed, the file it speaks for and the command that fixes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Which tool owns it.
    pub kind: Kind,
    /// The folder the commands run in, relative to the repository root.
    pub dir: PathBuf,
    /// The file named in the finding (relative to the root).
    pub file: String,
    /// The check: program and arguments.
    pub check: Vec<Vec<String>>,
    /// The fix: program and arguments, run in order.
    pub sync: Vec<Vec<String>>,
}

impl Target {
    /// The fix as a person types it.
    pub fn fix_text(&self) -> String {
        let steps: Vec<String> = self.sync.iter().map(|s| s.join(" ")).collect();
        if self.dir.as_os_str().is_empty() {
            steps.join(" && ")
        } else {
            format!("cd {} && {}", self.dir.display(), steps.join(" && "))
        }
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

/// The golden repository of the planner's tests: its lockfiles are test data, not workspaces anyone builds. Only this one folder is
/// skipped; any other lockfile is checked, so a new workspace under a folder named `tests` is not skipped by accident.
const TEST_DATA: &str = "tools/paddock-dev/tests/golden/fixture/";

fn is_test_data(file: &str) -> bool {
    file.starts_with(TEST_DATA)
}

/// The tracked lockfiles and hack configs that [`plan`] leaves out as test data, so a run can say what it did not look at.
pub fn skipped(tracked: &[String]) -> Vec<String> {
    let mut v: Vec<String> = tracked
        .iter()
        .filter(|f| is_test_data(f) && (f.ends_with("Cargo.lock") || f.ends_with("package-lock.json")))
        .cloned()
        .collect();
    v.sort();
    v
}

/// The targets for a list of tracked files (paths relative to the root, `/` separated).
pub fn plan(tracked: &[String]) -> Vec<Target> {
    let has = |p: &str| tracked.iter().any(|t| t == p);
    let mut out = Vec::new();
    let mut files: Vec<&String> = tracked.iter().collect();
    files.sort();
    for f in files {
        if is_test_data(f) {
            continue;
        }
        let dir = Path::new(f).parent().map(Path::to_path_buf).unwrap_or_default();
        let at = |name: &str| {
            if dir.as_os_str().is_empty() {
                name.to_string()
            } else {
                format!("{}/{name}", dir.display())
            }
        };
        if f.rsplit('/').next() == Some("Cargo.lock") && has(&at("Cargo.toml")) {
            out.push(Target {
                kind: Kind::Cargo,
                dir: dir.clone(),
                file: f.clone(),
                check: vec![argv(&["cargo", "metadata", "--locked", "--format-version", "1"])],
                sync: vec![argv(&["cargo", "update", "--workspace"])],
            });
        } else if f.rsplit('/').next() == Some("package-lock.json") {
            out.push(Target {
                kind: Kind::Npm,
                dir: dir.clone(),
                file: f.clone(),
                check: vec![argv(&["npm", "ci", "--dry-run", "--ignore-scripts"])],
                sync: vec![argv(&["npm", "install", "--package-lock-only", "--ignore-scripts"])],
            });
        } else if f.ends_with("/.config/hakari.toml") {
            let ws = dir.parent().map(Path::to_path_buf).unwrap_or_default();
            out.push(Target {
                kind: Kind::Hakari,
                dir: ws.clone(),
                file: f.clone(),
                check: vec![
                    argv(&["cargo", "hakari", "generate", "--diff"]),
                    argv(&["cargo", "hakari", "manage-deps", "--dry-run"]),
                    argv(&["cargo", "hakari", "verify"]),
                ],
                sync: vec![
                    argv(&["cargo", "hakari", "manage-deps", "--yes"]),
                    argv(&["cargo", "hakari", "generate"]),
                ],
            });
        }
    }
    out
}

/// What running one command said.
#[derive(Clone, Debug)]
pub struct Ran {
    /// It exited with status 0.
    pub ok: bool,
    /// The last lines it wrote, or why it could not run.
    pub tail: String,
}

/// Starts programs; faked in tests.
pub trait Runner: Sync {
    /// Runs `argv` in `dir`.
    fn run(&self, dir: &Path, argv: &[String]) -> Ran;
}

/// The file to start for a command's program. npm is `npm.cmd` on Windows, and Rust finds only `.exe` programs there, so a bare
/// `npm` was "not available on this computer" on every Windows machine that has it.
fn program_for(program: &str, windows: bool) -> &str {
    if windows && program == "npm" { "npm.cmd" } else { program }
}

/// The real runner: bounded and started through `paddock-proc`.
pub struct Real;

impl Runner for Real {
    fn run(&self, dir: &Path, argv: &[String]) -> Ran {
        let Some((program, args)) = argv.split_first() else {
            return Ran {
                ok: false,
                tail: "no command".into(),
            };
        };
        let mut cmd = command(program_for(program, cfg!(windows)));
        cmd.current_dir(dir).args(args);
        match run_bounded(cmd, Limits::new(LIMIT).max_output(1 << 22)) {
            Ok(o) => Ran {
                ok: o.success(),
                tail: tail(&format!("{}\n{}", o.stderr_text(), o.stdout_text())),
            },
            Err(e) => Ran {
                ok: false,
                tail: format!("{e}"),
            },
        }
    }
}

/// The last non-empty lines of a command's output, one line.
fn tail(text: &str) -> String {
    // npm prints the cause first, then its usage text and the path of its debug log; neither says why it refused.
    let cause = text.split("Clean install a project").next().unwrap_or(text);
    let mut lines: Vec<&str> = cause
        .lines()
        .map(|l| l.trim().trim_start_matches("npm error").trim())
        .filter(|l| !l.is_empty() && !l.contains("A complete log of this run can be found"))
        .collect();
    lines.dedup();
    lines[lines.len().saturating_sub(3)..].join(" | ")
}

/// One stale target and what said so.
#[derive(Clone, Debug)]
pub struct Stale {
    /// The target.
    pub target: Target,
    /// The command that failed.
    pub command: String,
    /// What it printed last.
    pub tail: String,
}

fn check_one(runner: &dyn Runner, root: &Path, t: &Target) -> Option<Stale> {
    for step in &t.check {
        let r = runner.run(&root.join(&t.dir), step);
        if !r.ok {
            return Some(Stale {
                target: t.clone(),
                command: step.join(" "),
                tail: r.tail,
            });
        }
    }
    None
}

/// Checks every target, a few at a time; the result is in the order of `targets`.
pub fn check_all(runner: &dyn Runner, root: &Path, targets: &[Target]) -> Vec<Stale> {
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<(usize, Stale)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..PARALLEL.min(targets.len().max(1)) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(t) = targets.get(i) else { break };
                    if let Some(stale) = check_one(runner, root, t) {
                        found.lock().unwrap_or_else(|p| p.into_inner()).push((i, stale));
                    }
                }
            });
        }
    });
    let mut v = found.into_inner().unwrap_or_else(|p| p.into_inner());
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, s)| s).collect()
}

/// Regenerates every target in the order that settles: hakari first (it edits manifests), then cargo, then npm. Returns the
/// failures (a command that did not exit 0).
pub fn sync_all(runner: &dyn Runner, root: &Path, targets: &[Target]) -> Vec<Stale> {
    let mut order: Vec<&Target> = targets.iter().collect();
    order.sort_by_key(|t| match t.kind {
        Kind::Hakari => 0,
        Kind::Cargo => 1,
        Kind::Npm => 2,
    });
    let mut failed = Vec::new();
    for t in order {
        for step in &t.sync {
            let r = runner.run(&root.join(&t.dir), step);
            if !r.ok {
                failed.push(Stale {
                    target: t.clone(),
                    command: step.join(" "),
                    tail: r.tail,
                });
                break;
            }
        }
    }
    failed
}

/// The tracked files that name lockfiles and hack configs, from git.
fn tracked(root: &Path) -> Result<Vec<String>, CliError> {
    let mut cmd = command("git");
    cmd.arg("-C")
        .arg(root)
        .args(["ls-files", "--", "*Cargo.lock", "*Cargo.toml", "*package-lock.json", "*hakari.toml"]);
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(60)).max_output(1 << 24))
        .map_err(|e| CliError::failure(format!("git could not list the tracked files: {e}.")))?;
    if !out.success() {
        return Err(
            CliError::failure(format!("git ls-files failed in {}: {}", root.display(), tail(&out.stderr_text())))
                .with_hint("Run it inside the repository, or pass --root."),
        );
    }
    Ok(out.stdout_text().lines().map(str::to_string).collect())
}

fn annotation(s: &Stale) -> String {
    format!(
        "::error file={}::{} is stale: `{}` failed ({}). Fix: ./dev locks sync (or `{}`), then commit the result.",
        s.target.file,
        s.target.file.rsplit('/').next().unwrap_or(&s.target.file),
        s.command,
        s.tail,
        s.target.fix_text()
    )
}

/// `locks check`.
pub fn check(root: Option<PathBuf>) -> Result<Report, CliError> {
    run_check(&Real, &root_of(root)?)
}

/// `locks sync`.
pub fn sync(root: Option<PathBuf>) -> Result<Report, CliError> {
    run_sync(&Real, &root_of(root)?)
}

fn root_of(given: Option<PathBuf>) -> Result<PathBuf, CliError> {
    crate::gates::command::repo_root(given.as_deref())
}

/// [`check`] with the runner and root given.
pub fn run_check(runner: &dyn Runner, root: &Path) -> Result<Report, CliError> {
    let files = tracked(root)?;
    let targets = plan(&files);
    let skipped = skipped(&files);
    if targets.is_empty() {
        return Err(CliError::failure(format!("No lockfile found under {}.", root.display()))
            .with_hint("Run it in the repository root, or pass --root."));
    }
    let started = std::time::Instant::now();
    let stale = check_all(runner, root, &targets);
    let mut text: String = stale.iter().map(|s| annotation(s) + "\n").collect();
    text.push_str(&format!(
        "locks: {} checked, {} stale, {} skipped as test data ({} ms)\n",
        targets.len(),
        stale.len(),
        skipped.len(),
        started.elapsed().as_millis()
    ));
    for f in &skipped {
        text.push_str(&format!("locks: skipped {f} (test data of the planner's golden repository)\n"));
    }
    let data = json!({
        "checked": targets.len(),
        "skipped": skipped,
        "annotations": stale.iter().map(annotation).collect::<Vec<String>>(),
        "stale": stale.iter().map(|s| json!({"file": s.target.file, "command": s.command, "fix": s.target.fix_text()})).collect::<Vec<Value>>(),
    });
    let report = Report::new(data, text);
    Ok(if stale.is_empty() {
        report
    } else {
        report.with_exit(Exit::Findings)
    })
}

/// [`sync`] with the runner and root given.
pub fn run_sync(runner: &dyn Runner, root: &Path) -> Result<Report, CliError> {
    let targets = plan(&tracked(root)?);
    if targets.is_empty() {
        return Err(CliError::failure(format!("No lockfile found under {}.", root.display()))
            .with_hint("Run it in the repository root, or pass --root."));
    }
    let failed = sync_all(runner, root, &targets);
    let mut text: String = failed
        .iter()
        .map(|s| {
            format!(
                "::error file={}::`{}` failed ({}); regenerate it by hand with that command and read its message.\n",
                s.target.file, s.command, s.tail
            )
        })
        .collect();
    text.push_str(&format!(
        "locks: {} synced, {} failed. Review the diff and commit it (never edit a lockfile by hand, ER-40); run `./dev locks check` to confirm.\n",
        targets.len() - failed.len(),
        failed.len()
    ));
    let data = json!({"synced": targets.len() - failed.len(), "failed": failed.iter().map(|s| json!({"file": s.target.file, "command": s.command})).collect::<Vec<Value>>()});
    let report = Report::new(data, text);
    Ok(if failed.is_empty() {
        report
    } else {
        report.with_exit(Exit::Findings)
    })
}

#[cfg(test)]
mod tests {
    use super::program_for;

    #[test]
    fn npm_is_started_as_its_cmd_file_on_windows_and_nothing_else_changes() {
        assert_eq!(program_for("npm", true), "npm.cmd");
        assert_eq!(program_for("npm", false), "npm");
        assert_eq!(program_for("cargo", true), "cargo");
    }
}
