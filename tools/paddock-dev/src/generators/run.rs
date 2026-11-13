//! Running the checks of the due generators: bounded, each command and duration logged, fast ones side by side and heavy ones
//! one after another. A check that cannot run is NOT MEASURED, never a pass (ER-33, ADR 0089): a missing tool, a file the
//! entry requires, a timeout.

use super::registry::{Cost, Generator};
use paddock_cli::{CheckId, Finding, Outcome, Reason, Subject, ToolVersion, UnixSeconds, Unmeasured, measure};
use paddock_proc::{Limits, command, run_bounded};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The longest a check may run, by its cost class. These are hang guards, not measurements: a fast check is a script over files
/// (seconds), a heavy one may build a workspace on a loaded machine (a cold `cargo test --workspace` is the longest known).
pub fn limit(cost: Cost) -> Duration {
    match cost {
        Cost::Fast => Duration::from_secs(300),
        Cost::Heavy => Duration::from_secs(1800),
    }
}
/// Fast checks that run at once: scripts over files, not builds.
const PARALLEL: usize = 4;

/// What a started command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// It ran to the end.
    Exited {
        /// Its exit status (`None`: ended by a signal).
        code: Option<i32>,
        /// The last lines it printed (stderr, then stdout), on one line.
        tail: String,
        /// Everything it printed (capped), for the drift markers.
        text: String,
    },
    /// It could not be started (a program that is not installed): the text says which.
    Unavailable(String),
    /// It did not finish within the limit and its tree was ended.
    TimedOut,
    /// It never got the machine-wide heavy slot within the limit (the text says who held it).
    SlotWait(String),
    /// It wrote more than the output cap and its tree was ended.
    OutputCap,
}

impl Ran {
    /// A finished run with this status and output (the whole output is the tail).
    pub fn exit(code: i32, output: &str) -> Ran {
        Ran::Exited {
            code: Some(code),
            tail: output.to_string(),
            text: output.to_string(),
        }
    }
}

/// How one check is to be started beyond its command: through the machine-wide heavy slot or not, and for which workspace (its
/// folder), whose kept build folder it gets.
#[derive(Debug, Clone, Copy)]
pub struct Spec<'a> {
    /// A heavy entry queues for the slot the push gate uses (ER-24), so it never builds beside another heavy step.
    pub heavy: bool,
    /// The entry's `cwd`: one kept build folder per workspace.
    pub workspace: &'a str,
}

/// Starts programs; faked in tests.
pub trait Runner: Sync {
    /// Runs `argv` in `dir`, ending it after `limit` (the wait for the slot counts).
    fn run(&self, dir: &Path, argv: &[String], limit: Duration, spec: &Spec) -> Ran;
    /// Whether `path` exists (the `requires` of an entry).
    fn exists(&self, path: &Path) -> bool;
    /// What a check of the tree at `root` is about: its head and a digest of the working tree the checks read.
    fn subject(&self, root: &Path) -> Subject;
}

/// The program as the OS can start it: on Windows `npm` is `npm.cmd` and is found on the `PATH` with its extension.
fn resolve(program: &str) -> String {
    if !cfg!(windows) || program.contains(['/', '\\']) {
        return program.to_string();
    }
    let path = std::env::var("PATH").or_else(|_| std::env::var("Path")).unwrap_or_default();
    let exts = crate::pathfind::extensions(std::env::var("PATHEXT").ok().as_deref());
    crate::pathfind::find_program(&path, program, true, &exts).map_or_else(|| program.to_string(), |p| p.to_string_lossy().into_owned())
}

/// The real runner: bounded and started through `paddock-proc`. Every check runs with `CARGO_BUILD_JOBS` (the caller's, else 2)
/// and its workspace's kept `CARGO_TARGET_DIR` under the build-folder root; a heavy one is queued through `heavy`.
pub struct Real {
    root: PathBuf,
    env: HashMap<String, String>,
    /// Build-folder roots tried when `PADDOCK_DEV_TARGET_DIR` is not set (`./dev disk`'s configured roots), first that exists.
    default_roots: Vec<PathBuf>,
    built_heavy: OnceLock<Result<PathBuf, String>>,
}

impl Real {
    /// The runner for the checkout at `root`, with the process's environment.
    pub fn new(root: &Path) -> Real {
        let env: HashMap<String, String> = std::env::vars().collect();
        // The roots `./dev disk` tracks (PADDOCK_DISK_ROOTS, else its defaults): no machine path is named here.
        let roots = crate::disk::config::Settings::resolve(&crate::disk::config::CommonArgs::default(), &|k| env.get(k).cloned()).roots;
        Real::with(root, env, roots)
    }

    /// The same with an explicit environment and default roots (tests).
    pub fn with(root: &Path, env: HashMap<String, String>, default_roots: Vec<PathBuf>) -> Real {
        Real {
            root: root.to_path_buf(),
            env,
            default_roots,
            built_heavy: OnceLock::new(),
        }
    }

    /// The kept build folder of a workspace: `<root>/gen-check/<workspace>`, where the root is `PADDOCK_DEV_TARGET_DIR`, else the
    /// first of `./dev disk`'s configured roots that exists. Never inside the checkout and never under `$HOME` (ER-14: the
    /// internal disk), and never on d8 unless that is what is configured.
    pub fn target_dir(&self, workspace: &str) -> Result<PathBuf, String> {
        let configured = self.env.get("PADDOCK_DEV_TARGET_DIR").filter(|d| !d.is_empty()).map(PathBuf::from);
        let base = configured
            .or_else(|| self.default_roots.iter().find(|r| r.is_dir()).cloned())
            .ok_or("there is no build-folder root: set PADDOCK_DEV_TARGET_DIR (generator builds never go inside the worktree or under ~, and not onto d8 unless configured)")?;
        if let Some(home) = self.env.get("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
            && base.starts_with(&home)
        {
            return Err(format!(
                "the build-folder root {} is under your home folder ({}), the internal disk; set PADDOCK_DEV_TARGET_DIR to a folder on a build disk",
                base.display(),
                home.display()
            ));
        }
        let slug: String = workspace.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let dir = base.join("gen-check").join(if slug == "-" { "root".to_string() } else { slug });
        if dir.starts_with(&self.root) {
            return Err(format!(
                "the build folder {} is inside the checkout; point PADDOCK_DEV_TARGET_DIR outside it",
                dir.display()
            ));
        }
        if !dir.exists() {
            std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make the build folder {}: {e}", dir.display()))?;
            // The owner record lets `./dev disk` and `safe_remove` know the folder is a build folder of ours.
            let _ = crate::disk::record::write(
                &dir,
                &crate::disk::record::Record::new(crate::disk::record::Kind::Build, "dev:gen-check"),
            );
        }
        Ok(dir)
    }
}

impl Runner for Real {
    fn run(&self, dir: &Path, argv: &[String], limit: Duration, spec: &Spec) -> Ran {
        let target = match self.target_dir(spec.workspace) {
            Ok(t) => t,
            Err(why) => return Ran::Unavailable(why),
        };
        let mut full: Vec<String> = Vec::new();
        if spec.heavy {
            // The same resolver as the push gate; a heavy built from the tree goes into the configured root, not the checkout.
            let mut env = self.env.clone();
            if let Some(root) = target.parent().and_then(Path::parent) {
                env.entry("PADDOCK_DEV_TARGET_DIR".into())
                    .or_insert_with(|| root.display().to_string());
            }
            match crate::pre_push::exec::heavy_command_for(
                &self.root,
                &env,
                &|p| crate::pre_push::exec::on_path(&self.env, p),
                &self.built_heavy,
                limit,
            ) {
                Ok(prefix) => full.extend(prefix),
                Err(why) => return Ran::Unavailable(format!("no machine-wide heavy slot: {why}")),
            }
        }
        full.extend(argv.iter().cloned());
        let Some((program, args)) = full.split_first() else {
            return Ran::Unavailable("the entry names no command".into());
        };
        let jobs = self
            .env
            .get("CARGO_BUILD_JOBS")
            .filter(|j| !j.is_empty())
            .cloned()
            .unwrap_or_else(|| "2".into());
        let mut cmd = command(resolve(program));
        // The whole environment as this runner has it (the process's, in production): the program is looked up on its PATH.
        cmd.env_clear()
            .envs(&self.env)
            .current_dir(dir)
            .args(args)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_BUILD_JOBS", jobs);
        match run_bounded(cmd, Limits::new(limit).max_output(1 << 22).stop_on_cap()) {
            Ok(o) => match o.outcome {
                paddock_proc::Outcome::Exited(status) => {
                    let text = format!("{}\n{}", o.stderr_text(), o.stdout_text());
                    // heavy's own refusals (it could not take the lock, or refused the folder) are not the check's verdict.
                    if spec.heavy && matches!(status.code(), Some(75 | 78)) && text.contains("heavy ") {
                        return Ran::Unavailable(format!(
                            "heavy gave no slot (status {}): {}",
                            status.code().unwrap_or(0),
                            tail(&text)
                        ));
                    }
                    Ran::Exited {
                        code: status.code(),
                        tail: tail(&text),
                        text,
                    }
                }
                paddock_proc::Outcome::TimedOut => {
                    let text = format!("{}\n{}", o.stderr_text(), o.stdout_text());
                    match crate::pre_push::exec::slot_wait(&text).filter(|_| spec.heavy) {
                        Some(w) => Ran::SlotWait(w),
                        None => Ran::TimedOut,
                    }
                }
                paddock_proc::Outcome::OutputCap => Ran::OutputCap,
            },
            Err(e) => Ran::Unavailable(e.to_string()),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn subject(&self, root: &Path) -> Subject {
        working_tree(root)
    }
}

/// The head and a SHA-256 of the working tree's state (`git diff HEAD` and the untracked files' names): what a generator check
/// that reads files is a check of. A git that cannot answer gives `unknown`, which no consumer matches to a commit.
pub fn working_tree(root: &Path) -> Subject {
    use sha2::{Digest, Sha256};
    let git = crate::merge_check::git::Git::new(root);
    let head = git
        .text(&["rev-parse", "HEAD"])
        .map(|t| t.trim().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let mut h = Sha256::new();
    for args in [&["diff", "HEAD"][..], &["ls-files", "--others", "--exclude-standard"][..]] {
        h.update(git.text(args).unwrap_or_default());
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Subject::Files {
        head: paddock_cli::outcome::Sha::new(head),
        digest: paddock_cli::outcome::Sha256::new(hex),
    }
}

/// The last non-empty lines of a command's output, on one line.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(4)..].join(" | ")
}

/// What a tool says when it could not run at all (a missing module, command, subcommand or test target, a build that does not
/// compile), which is not drift: the check did not look.
pub fn broken_tool(text: &str) -> Option<&'static str> {
    const SIGNS: [(&str, &str); 11] = [
        ("ERR_MODULE_NOT_FOUND", "a Node module is missing; run `npm ci`"),
        ("Cannot find module", "a Node module is missing; run `npm ci`"),
        ("command not found", "a program is not installed"),
        ("no such command", "a cargo subcommand is not installed"),
        ("no such subcommand", "a cargo subcommand is not installed"),
        ("could not find `Cargo.toml`", "there is no Cargo.toml in the folder"),
        ("could not compile", "the code does not compile"),
        ("no test target named", "the test target does not exist"),
        ("did not match any packages", "the package does not exist"),
        (
            "library 'duckdb' not found",
            "libduckdb is not set up; run `cargo xtask libduckdb` in apps/debrief",
        ),
        ("error: unexpected argument", "the command line is wrong"),
    ];
    SIGNS.iter().find(|(sign, _)| text.contains(sign)).map(|(_, why)| *why)
}

/// The exit statuses that say "could not look" whatever the tool: 126 and 127 (the shell could not start the program) and 6
/// (ADR 0089's not measured, which a check script uses for a missing precondition).
fn status_says_not_measured(code: i32) -> bool {
    matches!(code, 6 | 126 | 127)
}

/// One generator's result.
#[derive(Debug, Clone)]
pub struct Checked {
    /// The generator id.
    pub id: String,
    /// The check as typed.
    pub line: String,
    /// The regenerate command as typed.
    pub regenerate: String,
    /// Fast or heavy.
    pub cost: Cost,
    /// What it found: an [`Outcome`] made by `measure` (the check's exit 0 is the probe inside it), or not measured.
    pub outcome: Outcome,
    /// How long it took (zero when it did not run).
    pub took: Duration,
}

/// Runs the checks of `due` in `root`: the fast ones up to [`PARALLEL`] at a time, then the heavy ones one by one. Results are
/// in the order of `due`. `log` gets one line when a check starts and one when it ends.
pub fn execute(root: &Path, due: &[&Generator], runner: &dyn Runner, log: &(dyn Fn(&str) + Sync)) -> Vec<Checked> {
    let subject = runner.subject(root);
    let slots: Vec<Mutex<Option<Checked>>> = due.iter().map(|_| Mutex::new(None)).collect();
    let fast: Vec<usize> = (0..due.len()).filter(|&i| due[i].cost == Cost::Fast).collect();
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..PARALLEL.min(fast.len()) {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::SeqCst);
                    let Some(&i) = fast.get(k) else { break };
                    store(&slots[i], one(root, due[i], runner, &subject, log));
                }
            });
        }
    });
    for (i, g) in due.iter().enumerate().filter(|(_, g)| g.cost == Cost::Heavy) {
        store(&slots[i], one(root, g, runner, &subject, log));
    }
    slots
        .into_iter()
        .zip(due)
        .map(|(m, g)| {
            m.into_inner().ok().flatten().unwrap_or_else(|| Checked {
                id: g.id.clone(),
                line: g.check_line(),
                regenerate: g.regenerate_line(),
                cost: g.cost,
                outcome: Outcome::NotMeasured(Reason::new("the check did not run (a worker ended early)")),
                took: Duration::ZERO,
            })
        })
        .collect()
}

/// A check that failed: drift only when it says so (a failing finding). A crash, a missing tool, a build that does not compile, a
/// status that means "could not look", or (when the entry names `drift_marker`s) a failure that carries none of them is NOT
/// MEASURED.
pub fn classify_failure(g: &Generator, code: Option<i32>, tail: &str, text: &str) -> Result<Vec<Finding>, Unmeasured> {
    let not = |why: String| Err(Unmeasured::NotMeasured(Reason::new(why)));
    if let Some(c) = code.filter(|c| status_says_not_measured(*c)) {
        return not(format!("it ended with status {c} (could not look): {tail}"));
    }
    if code.is_none() {
        return not(format!("it was ended by a signal: {tail}"));
    }
    if let Some(why) = broken_tool(text) {
        return not(format!("the tool did not run to a verdict ({why}): {tail}"));
    }
    if !g.drift_marker.is_empty() && !g.drift_marker.iter().any(|m| text.contains(m.as_str())) {
        return not(format!(
            "it failed without saying the files are stale (expected one of {:?}), so it did not reach a verdict: {tail}",
            g.drift_marker
        ));
    }
    Ok(vec![Finding::failing(tail)])
}

fn store(slot: &Mutex<Option<Checked>>, c: Checked) {
    if let Ok(mut s) = slot.lock() {
        *s = Some(c);
    }
}

fn one(root: &Path, g: &Generator, runner: &dyn Runner, subject: &Subject, log: &(dyn Fn(&str) + Sync)) -> Checked {
    let done = |outcome: Outcome, took: Duration| Checked {
        id: g.id.clone(),
        line: g.check_line(),
        regenerate: g.regenerate_line(),
        cost: g.cost,
        outcome,
        took,
    };
    if let Some(missing) = g.requires.iter().find(|r| !runner.exists(&root.join(r))) {
        let c = done(
            Outcome::NotMeasured(Reason::new(format!(
                "not on this tree: {missing} is missing (the check needs it: install or build what provides it, or merge the branch that adds it)"
            ))),
            Duration::ZERO,
        );
        log(&format!("gen {}: NOT MEASURED, not on this tree ({missing} is missing)", g.id));
        return c;
    }
    let dir = root.join(&g.cwd);
    if !runner.exists(&dir) {
        let why = format!("the folder {} does not exist on this tree", g.cwd);
        log(&format!("gen {}: NOT MEASURED, {why}", g.id));
        return done(Outcome::NotMeasured(Reason::new(why)), Duration::ZERO);
    }
    log(&format!("gen {}: running [{}] {}", g.id, g.cost.name(), g.check_line()));
    let start = Instant::now();
    let ran = runner.run(
        &dir,
        &g.check,
        limit(g.cost),
        &Spec {
            heavy: g.cost == Cost::Heavy,
            workspace: &g.cwd,
        },
    );
    let took = start.elapsed();
    let tool = ToolVersion::new(concat!("paddock-dev ", env!("CARGO_PKG_VERSION")));
    let outcome = measure(
        CheckId::new(format!("gen:{}", g.id)),
        subject.clone(),
        tool,
        UnixSeconds::now(),
        || {
            let not = |why: String| Err(Unmeasured::NotMeasured(Reason::new(why)));
            match ran {
                // The check's own exit 0 is the probe: only `measure` turns it into a pass.
                Ran::Exited { code: Some(0), .. } => Ok(Vec::new()),
                Ran::Exited { code, tail, text } => classify_failure(g, code, &tail, &text),
                Ran::Unavailable(why) => not(why),
                Ran::SlotWait(who) => not(format!(
                    "it waited {} s, the hang guard, for the machine-wide heavy slot and never started ({who}); run it again when the machine is quieter",
                    limit(g.cost).as_secs()
                )),
                Ran::TimedOut => not(format!("it did not finish within {} s and was ended", limit(g.cost).as_secs())),
                Ran::OutputCap => not("it wrote more output than the cap (4 MiB) and was ended".into()),
            }
        },
    );
    let word = match &outcome {
        Outcome::Clean(_) => "current",
        Outcome::Findings(_) => "STALE",
        _ => "NOT MEASURED",
    };
    log(&format!("gen {}: {word} in {:.1}s", g.id, took.as_secs_f64()));
    done(outcome, took)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_that_could_not_run_is_not_drift() {
        assert!(broken_tool("Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'x'").is_some());
        assert!(broken_tool("sh: cargo-hakari: command not found").is_some());
        assert!(broken_tool("error: could not compile `x`").is_some());
        assert!(broken_tool("error: no test target named `shell`").is_some());
        assert!(broken_tool("error: no such command: `hakari`").is_some());
        assert_eq!(broken_tool("messages.xlf is stale"), None);
    }

    #[test]
    fn the_tail_is_the_last_four_lines_on_one_line() {
        assert_eq!(tail("a\n\n b \nc\nd\ne\n"), "b | c | d | e");
    }
}
