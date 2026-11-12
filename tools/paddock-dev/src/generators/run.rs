//! Running the checks of the due generators: bounded, each command and duration logged, fast ones side by side and heavy ones
//! one after another. A check that cannot run is NOT MEASURED, never a pass (ER-33, ADR 0089): a missing tool, a file the
//! entry requires, a timeout.

use super::registry::{Cost, Generator};
use paddock_proc::{Limits, Outcome, command, run_bounded};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// Starts programs; faked in tests.
pub trait Runner: Sync {
    /// Runs `argv` in `dir`, ending it after `limit`.
    fn run(&self, dir: &Path, argv: &[String], limit: Duration) -> Ran;
    /// Whether `path` exists (the `requires` of an entry).
    fn exists(&self, path: &Path) -> bool;
}

/// The real runner: bounded and started through `paddock-proc`.
pub struct Real;

/// The program as the OS can start it: on Windows `npm` is `npm.cmd` and is found on the `PATH` with its extension.
fn resolve(program: &str) -> String {
    if !cfg!(windows) || program.contains(['/', '\\']) {
        return program.to_string();
    }
    let path = std::env::var("PATH").or_else(|_| std::env::var("Path")).unwrap_or_default();
    let exts = crate::pathfind::extensions(std::env::var("PATHEXT").ok().as_deref());
    crate::pathfind::find_program(&path, program, true, &exts).map_or_else(|| program.to_string(), |p| p.to_string_lossy().into_owned())
}

impl Runner for Real {
    fn run(&self, dir: &Path, argv: &[String], limit: Duration) -> Ran {
        let Some((program, args)) = argv.split_first() else {
            return Ran::Unavailable("the entry names no command".into());
        };
        let mut cmd = command(resolve(program));
        cmd.current_dir(dir).args(args);
        match run_bounded(cmd, Limits::new(limit).max_output(1 << 22).stop_on_cap()) {
            Ok(o) => match o.outcome {
                Outcome::Exited(status) => {
                    let text = format!("{}\n{}", o.stderr_text(), o.stdout_text());
                    Ran::Exited {
                        code: status.code(),
                        tail: tail(&text),
                        text,
                    }
                }
                Outcome::TimedOut => Ran::TimedOut,
                Outcome::OutputCap => Ran::OutputCap,
            },
            Err(e) => Ran::Unavailable(e.to_string()),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
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

/// What one check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The check passed: the files are current.
    Current,
    /// The check failed: the files are stale (or the check itself is broken; the tail says).
    Stale(String),
    /// The check did not look; the text says why.
    NotMeasured(String),
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
    /// What it found.
    pub verdict: Verdict,
    /// How long it took (zero when it did not run).
    pub took: Duration,
}

/// Runs the checks of `due` in `root`: the fast ones up to [`PARALLEL`] at a time, then the heavy ones one by one. Results are
/// in the order of `due`. `log` gets one line when a check starts and one when it ends.
pub fn execute(root: &Path, due: &[&Generator], runner: &dyn Runner, log: &(dyn Fn(&str) + Sync)) -> Vec<Checked> {
    let slots: Vec<Mutex<Option<Checked>>> = due.iter().map(|_| Mutex::new(None)).collect();
    let fast: Vec<usize> = (0..due.len()).filter(|&i| due[i].cost == Cost::Fast).collect();
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..PARALLEL.min(fast.len()) {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::SeqCst);
                    let Some(&i) = fast.get(k) else { break };
                    store(&slots[i], one(root, due[i], runner, log));
                }
            });
        }
    });
    for (i, g) in due.iter().enumerate().filter(|(_, g)| g.cost == Cost::Heavy) {
        store(&slots[i], one(root, g, runner, log));
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
                verdict: Verdict::NotMeasured("the check did not run (a worker ended early)".into()),
                took: Duration::ZERO,
            })
        })
        .collect()
}

/// A check that failed: drift only when it says so. A crash, a missing tool, a build that does not compile, a status that means
/// "could not look", or (when the entry names `drift_marker`s) a failure that carries none of them is NOT MEASURED.
pub fn classify_failure(g: &Generator, code: Option<i32>, tail: &str, text: &str) -> Verdict {
    if let Some(c) = code.filter(|c| status_says_not_measured(*c)) {
        return Verdict::NotMeasured(format!("it ended with status {c} (could not look): {tail}"));
    }
    if code.is_none() {
        return Verdict::NotMeasured(format!("it was ended by a signal: {tail}"));
    }
    if let Some(why) = broken_tool(text) {
        return Verdict::NotMeasured(format!("the tool did not run to a verdict ({why}): {tail}"));
    }
    if !g.drift_marker.is_empty() && !g.drift_marker.iter().any(|m| text.contains(m.as_str())) {
        return Verdict::NotMeasured(format!(
            "it failed without saying the files are stale (expected one of {:?}), so it did not reach a verdict: {tail}",
            g.drift_marker
        ));
    }
    Verdict::Stale(tail.to_string())
}

fn store(slot: &Mutex<Option<Checked>>, c: Checked) {
    if let Ok(mut s) = slot.lock() {
        *s = Some(c);
    }
}

fn one(root: &Path, g: &Generator, runner: &dyn Runner, log: &(dyn Fn(&str) + Sync)) -> Checked {
    let done = |verdict: Verdict, took: Duration| Checked {
        id: g.id.clone(),
        line: g.check_line(),
        regenerate: g.regenerate_line(),
        cost: g.cost,
        verdict,
        took,
    };
    if let Some(missing) = g.requires.iter().find(|r| !runner.exists(&root.join(r))) {
        let c = done(
            Verdict::NotMeasured(format!(
                "not on this tree: {missing} is missing (the check needs it: install or build what provides it, or merge the branch that adds it)"
            )),
            Duration::ZERO,
        );
        log(&format!("gen {}: NOT MEASURED, not on this tree ({missing} is missing)", g.id));
        return c;
    }
    let dir = root.join(&g.cwd);
    if !runner.exists(&dir) {
        let why = format!("the folder {} does not exist on this tree", g.cwd);
        log(&format!("gen {}: NOT MEASURED, {why}", g.id));
        return done(Verdict::NotMeasured(why), Duration::ZERO);
    }
    log(&format!("gen {}: running [{}] {}", g.id, g.cost.name(), g.check_line()));
    let start = Instant::now();
    let ran = runner.run(&dir, &g.check, limit(g.cost));
    let took = start.elapsed();
    let verdict = match ran {
        Ran::Exited { code: Some(0), .. } => Verdict::Current,
        Ran::Exited { code, tail, text } => classify_failure(g, code, &tail, &text),
        Ran::Unavailable(why) => Verdict::NotMeasured(why),
        Ran::TimedOut => Verdict::NotMeasured(format!("it did not finish within {} s and was ended", limit(g.cost).as_secs())),
        Ran::OutputCap => Verdict::NotMeasured("it wrote more output than the cap (4 MiB) and was ended".into()),
    };
    let word = match &verdict {
        Verdict::Current => "current",
        Verdict::Stale(_) => "STALE",
        Verdict::NotMeasured(_) => "NOT MEASURED",
    };
    log(&format!("gen {}: {word} in {:.1}s", g.id, took.as_secs_f64()));
    done(verdict, took)
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
