//! Running the gates: each one decided by an [`Exec`] (real processes, or a fake in the tests), several at a time, cheapest first,
//! inside a total time budget, and stopped when the person stops the push.

use super::gates::{Gate, Need, Planned};
use crate::ci_tool::{Output, ToolCall, tail};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The push gate's total budget (ADR 0038 amendment A2.5): what has not run by then is "not measured".
pub const BUDGET: Duration = Duration::from_secs(15 * 60);

/// How one run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// Exit status 0.
    pub success: bool,
    /// `exit 101`, `timed out after 2400 s`, `cancelled`.
    pub how: String,
    /// The last lines of the output, for the failure line.
    pub tail: String,
    /// The file that holds the whole output, when there is one.
    pub log: Option<String>,
}

impl Done {
    /// It did not end by itself: the budget or the gate's own limit stopped it, or the person did. Nothing is proven either way.
    pub fn cut_short(&self) -> bool {
        self.how.starts_with("timed out") || self.how == "cancelled" || self.how.starts_with("stopped at the output cap")
    }
}

/// What the gate needs from the machine. The real one starts processes; the tests' one answers from a table (ER-14).
pub trait Exec: Sync {
    /// Is `program` on PATH?
    fn has_tool(&self, program: &str) -> bool;
    /// Does a Docker daemon answer? Never starts one.
    fn docker_up(&self) -> bool;
    /// Runs the gate for at most `left`. `Err`: it could not be started (the program is missing).
    fn run(&self, gate: &Gate, left: Duration) -> Result<Done, String>;
    /// Why a heavy gate has no machine-wide slot to go through (no `heavy` on PATH and the build of tools/heavy failed), or
    /// `None`: it would otherwise compete with every other push (ER-24), so it is not measured instead.
    fn heavy_unavailable(&self) -> Option<String> {
        None
    }
}

/// The outcome of one table row.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// It ran and passed.
    Passed {
        /// Seconds.
        secs: f64,
    },
    /// It ran and failed.
    Failed {
        /// What failed, in one line.
        cause: String,
        /// The tail of its output.
        tail: String,
        /// What to do and how to re-run.
        next: String,
        /// Seconds.
        secs: f64,
    },
    /// The change does not reach it.
    Skipped {
        /// Why.
        reason: String,
    },
    /// It could not look: nothing is proven (ER-33). Blocks the push (exit 6) unless named in an override.
    NotMeasured {
        /// Why.
        reason: String,
        /// What to do.
        next: String,
    },
    /// Not measured, and a person named this gate in the override: logged, and the push may go.
    Accepted {
        /// Why it was not measured.
        reason: String,
    },
    /// The check itself cannot be trusted.
    Broken {
        /// What is wrong.
        cause: String,
        /// What to do.
        next: String,
    },
}

fn first_word(id: &str) -> &str {
    id.split(' ').next().unwrap_or(id)
}

/// The verdict of one gate: its needs first, then the run.
pub fn decide(gate: &Gate, exec: &dyn Exec, left: Duration) -> Verdict {
    match &gate.need {
        Need::Tool(t) if !exec.has_tool(t) => {
            return Verdict::NotMeasured {
                reason: format!("{t} is not installed"),
                next: format!(
                    "install it (`cargo install {t} --locked`), then re-run `./dev check --pre-push`; an agent reports this instead (ER-33)"
                ),
            };
        }
        Need::Docker if !exec.docker_up() => {
            return Verdict::NotMeasured {
                reason: "Docker (OrbStack) is not running".into(),
                next: "start it yourself and re-run; an agent reports this to the coordinator and never starts or restarts it (ER-14)"
                    .into(),
            };
        }
        _ => {}
    }
    if gate.heavy
        && let Some(why) = exec.heavy_unavailable()
    {
        return Verdict::NotMeasured {
            reason: format!("no heavy slot tool, so a compile would run beside every other push's: {why}"),
            next: "fix the build error above and re-run `./dev check --pre-push`; an agent reports this (ER-24, ER-33)".into(),
        };
    }
    let started = Instant::now();
    match exec.run(gate, left) {
        Err(e) => Verdict::NotMeasured {
            reason: e,
            next: "install the program, then re-run `./dev check --pre-push`".into(),
        },
        Ok(done) if done.success => Verdict::Passed {
            secs: started.elapsed().as_secs_f64(),
        },
        Ok(done) if done.cut_short() => Verdict::NotMeasured {
            reason: format!(
                "{} ({}) before it finished{}",
                gate.rerun(),
                done.how,
                done.log.as_ref().map_or(String::new(), |l| format!("; output so far in {l}"))
            ),
            next: format!(
                "re-run it alone: {} (the whole gate has a budget of {} min)",
                gate.rerun(),
                BUDGET.as_secs() / 60
            ),
        },
        Ok(done) => {
            let hint = match (gate.id.starts_with("clippy-linux"), done.how.as_str()) {
                (true, "exit 125") => {
                    "docker could not start the container (usually the image cannot be pulled): `docker login dev.decryptology.net`"
                        .to_string()
                }
                (true, "exit 137") => {
                    "the container was killed (out of memory at 12g): raise PADDOCK_LINUX_MEMORY (for example 16g) and re-run".to_string()
                }
                _ => gate.next.clone(),
            };
            let full = done.log.as_ref().map_or(String::new(), |l| format!(" Full output: {l}."));
            Verdict::Failed {
                cause: format!("{} ({})", gate.rerun(), done.how),
                tail: done.tail.trim_end().to_string(),
                next: format!("{hint}. Re-run alone: {}.{full}", gate.rerun()),
                secs: started.elapsed().as_secs_f64(),
            }
        }
    }
}

/// How a run is bounded.
pub struct RunOpts<'a> {
    /// Gates at a time.
    pub workers: usize,
    /// The total budget: a gate that would start later is not measured, one that runs over is stopped.
    pub budget: Duration,
    /// Set by SIGINT or SIGTERM: nothing more starts.
    pub stop: &'a AtomicBool,
}

/// Every planned row decided, `workers` at a time, lowest rank first; the result keeps the planned order. The default budget, never
/// stopped.
pub fn run_all(planned: &[Planned], exec: &dyn Exec, workers: usize) -> Vec<(String, Verdict)> {
    run_all_with(
        planned,
        exec,
        &RunOpts {
            workers,
            budget: BUDGET,
            stop: &AtomicBool::new(false),
        },
    )
}

fn row_id(p: &Planned) -> &str {
    match p {
        Planned::Run(g) => &g.id,
        Planned::Skip { id, .. } | Planned::Broken { id, .. } => id,
    }
}

/// [`run_all`] with its bounds. A gate that panics, one that never ran and one the budget or a signal cut off are "not measured",
/// never a pass.
pub fn run_all_with(planned: &[Planned], exec: &dyn Exec, opts: &RunOpts) -> Vec<(String, Verdict)> {
    let mut order: Vec<usize> = (0..planned.len()).collect();
    order.sort_by_key(|&i| match &planned[i] {
        Planned::Run(g) => g.rank,
        _ => 0,
    });
    let deadline = Instant::now() + opts.budget;
    let queue = Mutex::new(order.into_iter().collect::<VecDeque<_>>());
    let results: Vec<Mutex<Option<Verdict>>> = planned.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..opts.workers.max(1) {
            s.spawn(|| {
                loop {
                    let next = queue.lock().map(|mut q| q.pop_front()).unwrap_or(None);
                    let Some(i) = next else { break };
                    let v = match &planned[i] {
                        Planned::Run(g) => {
                            let left = deadline.saturating_duration_since(Instant::now());
                            if opts.stop.load(Ordering::SeqCst) {
                                Verdict::NotMeasured {
                                    reason: "the push was interrupted before it started".into(),
                                    next: format!("re-run `./dev check --pre-push --only {}`", first_word(&g.id)),
                                }
                            } else if left.is_zero() {
                                Verdict::NotMeasured {
                                    reason: format!(
                                        "the {} min budget of the push gate was spent before it started",
                                        opts.budget.as_secs() / 60
                                    ),
                                    next: format!("re-run `./dev check --pre-push --only {}`", first_word(&g.id)),
                                }
                            } else {
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decide(g, exec, left))).unwrap_or_else(|_| {
                                    Verdict::NotMeasured {
                                        reason: "the gate's runner crashed".into(),
                                        next: "report this with the command that ran; re-run it alone".into(),
                                    }
                                })
                            }
                        }
                        Planned::Skip { reason, .. } => Verdict::Skipped { reason: reason.clone() },
                        Planned::Broken { cause, next, .. } => Verdict::Broken {
                            cause: cause.clone(),
                            next: next.clone(),
                        },
                    };
                    if let Ok(mut slot) = results[i].lock() {
                        *slot = Some(v);
                    }
                }
            });
        }
    });
    planned
        .iter()
        .zip(results)
        .map(|(p, r)| {
            let v = r.into_inner().ok().flatten().unwrap_or_else(|| Verdict::NotMeasured {
                reason: "the gate never ran".into(),
                next: "re-run `./dev check --pre-push`; if it happens again, report it".into(),
            });
            (row_id(p).to_string(), v)
        })
        .collect()
}

/// How many gates run at once: one while the machine is loaded (ER-24: 20 or more on the 1-minute average), else three; `heavy`
/// still lets one compile at a time through, so the extra workers run the cheap gates meanwhile.
pub fn workers(load: Option<f64>, cores: usize) -> usize {
    match load {
        Some(l) if l >= 20.0 => 1,
        _ => 3.min(cores.max(1)),
    }
}

/// The 1-minute load average from `uptime`'s text (`load averages: 21.92 22.50 17.34` or `load average: 1.0, 2.0, 3.0`).
pub fn parse_load(uptime: &str) -> Option<f64> {
    let rest = uptime.split("load average").nth(1)?;
    let rest = rest.trim_start_matches('s').trim_start_matches(':').trim();
    rest.split(|c: char| c.is_whitespace() || c == ',')
        .next()?
        .replace(',', ".")
        .parse()
        .ok()
}

/// The command that puts a heavy step through the machine-wide slot (ER-24): `PADDOCK_HEAVY` as given, else `heavy.sh` or `heavy`
/// on PATH. Empty when there is none (the caller says so).
pub fn heavy_prefix(env: &HashMap<String, String>, has: &dyn Fn(&str) -> bool) -> Vec<String> {
    if let Some(h) = env.get("PADDOCK_HEAVY").filter(|h| !h.trim().is_empty()) {
        if h.trim() == "off" {
            return Vec::new();
        }
        return h.split_whitespace().map(str::to_string).collect();
    }
    if has("heavy.sh") {
        return vec!["heavy.sh".into()];
    }
    if has("heavy") {
        return ["heavy", "run", "pre-push", "--"].map(String::from).to_vec();
    }
    Vec::new()
}

/// The command that puts a heavy step through the machine-wide slot: [`heavy_prefix`], else the `heavy` built from the tree at
/// `root` (once, kept in `built`; `left` bounds that build). The one resolver of the push gate and of `./dev gen check`, so
/// both queue for the same slot. `Err` is the whole explanation; `Ok(empty)` only for the test-only `PADDOCK_HEAVY=off`.
pub fn heavy_command_for(
    root: &Path,
    env: &HashMap<String, String>,
    has_tool: &dyn Fn(&str) -> bool,
    built: &std::sync::OnceLock<Result<PathBuf, String>>,
    left: Duration,
) -> Result<Vec<String>, String> {
    if env.get("PADDOCK_HEAVY").map(|v| v.trim()) == Some("off") {
        // Only a test may run without the slot: anything else is an override that makes the gate not measured.
        return if env.contains_key(TEST_HEAVY_OFF) {
            Ok(Vec::new())
        } else {
            Err("PADDOCK_HEAVY=off is set without the test-only variable PADDOCK_TEST_HEAVY_OFF, so this is an override of the machine-wide slot (OVERRIDE SET); unset it".into())
        };
    }
    let found = heavy_prefix(env, has_tool);
    if !found.is_empty() {
        return Ok(found);
    }
    match built.get_or_init(|| super::heavy_tool::ensure(root, env, left)) {
        Ok(bin) => Ok(vec![bin.display().to_string(), "run".into(), "pre-push".into(), "--".into()]),
        Err(e) => Err(e.clone()),
    }
}

/// Whether `program` is a file in a folder of `env`'s `PATH`.
pub fn on_path(env: &HashMap<String, String>, program: &str) -> bool {
    let path = env.get("PATH").cloned().unwrap_or_default();
    std::env::split_paths(&path).any(|d| exists(&d, program))
}

/// Whether the output of a gate run through `heavy` shows that it never got the slot: it printed who holds the lock
/// ("the lock is held by <name> (pid <n>) ...") and never "<name> holds the lock". Returns the phrase for the verdict.
pub fn slot_wait(log: &str) -> Option<String> {
    if log.lines().any(|l| l.contains(" holds the lock (pid ")) {
        return None;
    }
    let held = log.lines().rev().find_map(|l| l.split("the lock is held by ").nth(1))?;
    let who = held.split(" for ").next().unwrap_or(held).trim();
    Some(format!("waiting for the heavy slot, held by {who}"))
}

/// The real machine: processes through [`ToolCall`] (bounded, environment listed, output in a file per gate).
pub struct RealExec {
    /// The repository root.
    pub root: PathBuf,
    /// The caller's environment.
    pub env: HashMap<String, String>,
    /// Where each gate's whole output goes (one `.log` per gate).
    pub logs: PathBuf,
    /// Set by SIGINT or SIGTERM: the running gates' process trees are ended.
    pub stop: Arc<AtomicBool>,
    /// The `heavy` built from tools/heavy when none is on PATH, built once per run (or the reason it could not be).
    pub built_heavy: std::sync::OnceLock<Result<PathBuf, String>>,
    /// When the push gate's budget ends: the build of heavy is cut off there.
    pub budget_end: Instant,
}

/// Variables every gate may see besides the base list: the toolchain, Docker, caches and Paddock's own settings.
pub(crate) const PASS: &[&str] = &[
    "CARGO_*",
    "RUSTUP_*",
    "RUSTFLAGS",
    "RUSTC_WRAPPER",
    "DOCKER*",
    "PADDOCK_*",
    "HEAVY_*",
    "XWIN_*",
    "SCCACHE_*",
    "NODE_*",
    "CC",
    "CXX",
    "CLANG*",
    "LIBRARY_PATH",
    "SDKROOT",
    "TMPDIR",
    "NPM_CONFIG_*",
    "npm_config_*",
];

/// The variable that, with `PADDOCK_HEAVY=off`, lets a test run a heavy gate without the slot.
pub const TEST_HEAVY_OFF: &str = "PADDOCK_TEST_HEAVY_OFF";

/// The label that marks the container of one run, so only it is ever stopped (ER-14).
const RUN_LABEL: &str = "paddock.run";

fn slug(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

impl RealExec {
    /// The command that puts a heavy gate through the slot: `PADDOCK_HEAVY`, `heavy.sh` or `heavy` on PATH, else the `heavy`
    /// built from this tree ([`super::heavy_tool`]). Empty for `PADDOCK_HEAVY=off` (hermetic tests only); `Err` when the build failed.
    fn heavy_command(&self) -> Result<Vec<String>, String> {
        let left = self.budget_end.saturating_duration_since(Instant::now());
        heavy_command_for(&self.root, &self.env, &|p| self.has_tool(p), &self.built_heavy, left)
    }

    fn call(&self, gate: &Gate, left: Duration, log: &Path, label: Option<&str>) -> ToolCall {
        let heavy = if gate.heavy {
            self.heavy_command().unwrap_or_default()
        } else {
            Vec::new()
        };
        let (program, args): (String, Vec<String>) = match heavy.split_first() {
            Some((first, rest)) => (
                first.clone(),
                rest.iter()
                    .cloned()
                    .chain(std::iter::once(gate.program.clone()))
                    .chain(gate.args.iter().cloned())
                    .collect(),
            ),
            None => (gate.program.clone(), gate.args.clone()),
        };
        let own = Duration::from_secs(if gate.heavy { 3000 } else { 900 });
        let mut call = ToolCall::new(program)
            .args(args)
            .pass(PASS.iter().copied())
            .cwd(self.root.join(&gate.cwd))
            .timeout(own.min(left))
            .cancel_on(Arc::clone(&self.stop));
        if gate.heavy && !self.env.contains_key("CARGO_BUILD_JOBS") {
            call = call.env("CARGO_BUILD_JOBS", "2");
        }
        for (k, v) in &gate.env {
            call = call.env(k, v);
        }
        if let Some(l) = label {
            call = call.env("PADDOCK_LINUX_RUN_LABEL", format!("{RUN_LABEL}={l}"));
        }
        call.output(Output::File(log.to_path_buf()))
    }

    fn docker(&self) -> String {
        self.env.get("DOCKER").cloned().unwrap_or_else(|| "docker".into())
    }

    /// Stops the container this run started (by its own label) and nothing else.
    fn stop_container(&self, label: &str) {
        let list = ToolCall::new(self.docker())
            .args(["ps", "-q", "--filter", &format!("label={RUN_LABEL}={label}")])
            .pass(PASS.iter().copied())
            .timeout(Duration::from_secs(30))
            .run(&self.env, &mut std::io::sink());
        let Ok(list) = list else { return };
        let ids: Vec<String> = list.stdout.split_whitespace().map(str::to_string).collect();
        if ids.is_empty() {
            return;
        }
        let _ = ToolCall::new(self.docker())
            .args(["stop", "--time", "10"])
            .args(ids)
            .pass(PASS.iter().copied())
            .timeout(Duration::from_secs(60))
            .run(&self.env, &mut std::io::sink());
    }
}

/// A token no other run has: process id and clock.
fn run_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{:x}-{nanos:x}", std::process::id())
}

impl Exec for RealExec {
    fn has_tool(&self, program: &str) -> bool {
        on_path(&self.env, program)
    }

    fn docker_up(&self) -> bool {
        docker_answers(&self.env)
    }

    fn heavy_unavailable(&self) -> Option<String> {
        self.heavy_command().err()
    }

    fn run(&self, gate: &Gate, left: Duration) -> Result<Done, String> {
        let _ = std::fs::create_dir_all(&self.logs);
        let log = self.logs.join(format!("{}.log", slug(&gate.id)));
        let label = (gate.need == Need::Docker).then(run_token);
        let mut ran = self.call(gate, left, &log, label.as_deref()).run(&self.env, &mut std::io::sink())?;
        // Cut short while queued for the heavy slot: say so, the gate itself never ran (ER-33: not measured, with that reason).
        if gate.heavy && !ran.success && ran.how.starts_with("timed out") {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            if let Some(w) = slot_wait(&text) {
                ran.how = format!("{}: {w}", ran.how);
            }
        }
        if let Some(l) = &label
            && !ran.success
            && matches!(ran.how.as_str(), h if h == "cancelled" || h.starts_with("timed out"))
        {
            self.stop_container(l);
        }
        Ok(Done {
            success: ran.success,
            how: ran.how,
            tail: tail(&ran.stderr, 12),
            log: Some(log.display().to_string()),
        })
    }
}

fn exists(dir: &Path, program: &str) -> bool {
    dir.join(program).is_file() || (cfg!(windows) && dir.join(format!("{program}.exe")).is_file())
}

/// Whether a Docker daemon is listening, asked without waking one: OrbStack's `docker` starts its VM on any command (ER-14: a
/// start of shared infrastructure by an agent), so on Unix the socket is connected to instead. Windows has no such launcher.
#[cfg(unix)]
fn docker_answers(env: &HashMap<String, String>) -> bool {
    docker_sockets(env)
        .iter()
        .any(|p| std::os::unix::net::UnixStream::connect(p).is_ok())
}

/// The sockets a Docker client would use: `DOCKER_HOST`, then OrbStack's, Docker Desktop's and the system's.
#[cfg(unix)]
pub fn docker_sockets(env: &HashMap<String, String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(host) = env.get("DOCKER_HOST") {
        if let Some(p) = host.strip_prefix("unix://") {
            out.push(PathBuf::from(p));
        }
        return out;
    }
    if let Some(home) = env.get("HOME") {
        out.push(Path::new(home).join(".orbstack/run/docker.sock"));
        out.push(Path::new(home).join(".docker/run/docker.sock"));
    }
    out.push(PathBuf::from("/var/run/docker.sock"));
    out
}

#[cfg(not(unix))]
fn docker_answers(env: &HashMap<String, String>) -> bool {
    let docker = env.get("DOCKER").cloned().unwrap_or_else(|| "docker".into());
    ToolCall::new(docker)
        .args(["info"])
        .pass(PASS.iter().copied())
        .timeout(Duration::from_secs(30))
        .run(env, &mut std::io::sink())
        .is_ok_and(|r| r.success)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_comes_out_of_both_uptime_formats() {
        assert_eq!(parse_load("16:09  up 17:24, 1 user, load averages: 21.92 22.50 17.34"), Some(21.92));
        assert_eq!(parse_load("up 3 days, load average: 0.52, 0.58, 0.59"), Some(0.52));
        assert_eq!(parse_load("no numbers"), None);
    }

    #[test]
    fn a_loaded_machine_runs_one_gate_at_a_time() {
        assert_eq!(workers(Some(35.0), 14), 1);
        assert_eq!(workers(Some(5.0), 14), 3);
        assert_eq!(workers(None, 2), 2);
        assert_eq!(workers(Some(1.0), 0), 1);
    }

    #[test]
    fn heavy_is_found_when_the_variable_is_not_set() {
        let env = HashMap::new();
        assert!(heavy_prefix(&env, &|_| false).is_empty());
        assert_eq!(heavy_prefix(&env, &|p| p == "heavy.sh"), ["heavy.sh"]);
        assert_eq!(heavy_prefix(&env, &|p| p == "heavy"), ["heavy", "run", "pre-push", "--"]);
        let set = HashMap::from([("PADDOCK_HEAVY".to_string(), "my-heavy --x".to_string())]);
        assert_eq!(heavy_prefix(&set, &|_| true), ["my-heavy", "--x"]);
    }

    #[test]
    fn a_log_that_only_waited_names_the_holder() {
        let waited =
            "heavy: the lock is held by Bob (pid 12) for 3m, running: cargo clippy\nheavy: the lock is held by Bob (pid 12) for 4m\n";
        assert_eq!(
            slot_wait(waited).as_deref(),
            Some("waiting for the heavy slot, held by Bob (pid 12)")
        );
        let ran = format!("{waited}heavy: pre-push holds the lock (pid 9) at 10:00:00Z, waited 5s\n");
        assert_eq!(slot_wait(&ran), None);
        assert_eq!(slot_wait("error: boom\n"), None);
    }

    #[test]
    fn heavy_off_runs_without_the_slot_and_is_the_only_way_to() {
        let off = HashMap::from([("PADDOCK_HEAVY".to_string(), "off".to_string())]);
        assert!(heavy_prefix(&off, &|_| true).is_empty());
    }

    /// A stand-in for `heavy`: a directory is the lock (`mkdir`), a waiter prints who holds it as heavy does, the holder runs
    /// its command with the lock held. Fake commands only (ER-14): no real build runs.
    #[cfg(unix)]
    mod slot {
        use super::*;
        use crate::pre_push::gates::Gate;

        const FAKE_HEAVY: &str = r#"
lock="$1"; shift
while ! mkdir "$lock" 2>/dev/null; do
  echo "heavy: the lock is held by $(cat "$lock/owner" 2>/dev/null || echo other) (pid 1) for 0m" >&2
  sleep 0.1
done
echo "pre-push holds the lock (pid $$) at now, waited 0s" >&2
echo "$$" > "$lock/owner"
"$@"; rc=$?
rm -rf "$lock"
exit $rc
"#;

        fn real(dir: &Path, lock: &Path) -> RealExec {
            let script = dir.join("fake-heavy.sh");
            std::fs::write(&script, FAKE_HEAVY).unwrap();
            RealExec {
                root: dir.to_path_buf(),
                env: HashMap::from([
                    ("PADDOCK_HEAVY".to_string(), format!("sh {} {}", script.display(), lock.display())),
                    ("PATH".to_string(), std::env::var("PATH").unwrap_or_default()),
                ]),
                logs: dir.join("logs"),
                stop: Arc::new(AtomicBool::new(false)),
                built_heavy: Default::default(),
                budget_end: Instant::now() + Duration::from_secs(60),
            }
        }

        fn gate(id: &str, command: &str) -> Gate {
            Gate {
                id: id.into(),
                cwd: String::new(),
                program: "sh".into(),
                args: vec!["-c".into(), command.into()],
                env: Vec::new(),
                heavy: true,
                need: Need::Nothing,
                rank: 2,
                next: "fix it".into(),
            }
        }

        #[test]
        fn two_pushes_at_once_take_the_slot_in_turn() {
            let d = tempfile::tempdir().unwrap();
            let lock = d.path().join("slot.lock");
            let ex = real(d.path(), &lock);
            let trace = d.path().join("trace");
            let cmd = |n: &str| format!("echo start-{n} >> {t}; sleep 1; echo end-{n} >> {t}", t = trace.display());
            std::thread::scope(|s| {
                for n in ["a", "b"] {
                    let (ex, g) = (&ex, gate(&format!("clippy {n}"), &cmd(n)));
                    s.spawn(move || assert!(matches!(decide(&g, ex, Duration::from_secs(60)), Verdict::Passed { .. })));
                }
            });
            let lines: Vec<String> = std::fs::read_to_string(&trace).unwrap().lines().map(String::from).collect();
            assert_eq!(lines.len(), 4, "{lines:?}");
            // Never two starts in a row: the second waited for the first to end.
            assert!(lines[0].starts_with("start-") && lines[1].starts_with("end-"), "{lines:?}");
            assert_eq!(
                lines[0].trim_start_matches("start-"),
                lines[1].trim_start_matches("end-"),
                "{lines:?}"
            );
        }

        #[test]
        fn a_slot_held_past_the_budget_is_not_measured_and_says_who_holds_it() {
            let d = tempfile::tempdir().unwrap();
            let lock = d.path().join("slot.lock");
            std::fs::create_dir(&lock).unwrap();
            std::fs::write(lock.join("owner"), "Bob").unwrap();
            let ex = real(d.path(), &lock);
            let g = gate("clippy a", "echo never runs");
            match decide(&g, &ex, Duration::from_secs(2)) {
                Verdict::NotMeasured { reason, .. } => {
                    assert!(reason.contains("waiting for the heavy slot, held by Bob"), "{reason}");
                }
                v => panic!("a slot that was never taken is not a pass or a failure: {v:?}"),
            }
        }

        /// A repository with the heavy tool's inputs committed, and a `cargo` that "builds" a fake heavy (a script that records
        /// its arguments and runs the gate's command), found first on the PATH of the run (ER-14: no real build).
        fn repo_with_fake_cargo(d: &Path, cargo_ok: bool) -> RealExec {
            let git = |args: &[&str]| {
                let ok = paddock_proc::command("git")
                    .args(args)
                    .current_dir(d)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .unwrap();
                assert!(ok.status.success(), "{args:?}: {}", String::from_utf8_lossy(&ok.stderr));
            };
            std::fs::create_dir_all(d.join("tools/heavy")).unwrap();
            std::fs::write(d.join("tools/heavy/inputs.txt"), "tools/heavy\n").unwrap();
            std::fs::write(d.join("tools/heavy/Cargo.toml"), "# fake\n").unwrap();
            git(&["init", "-q"]);
            git(&["add", "."]);
            git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "x"]);
            let bin = d.join("fakebin");
            std::fs::create_dir_all(&bin).unwrap();
            let body = if cargo_ok {
                format!(
                    "mkdir -p \"$CARGO_TARGET_DIR/release\"\ncat > \"$CARGO_TARGET_DIR/release/heavy\" <<'EOS'\n#!/bin/sh\n[ \"$1\" = --version ] && {{ echo heavy-fake; exit 0; }}\necho \"$@\" >> {m}\nshift 3\nexec \"$@\"\nEOS\nchmod +x \"$CARGO_TARGET_DIR/release/heavy\"\n",
                    m = d.join("heavy-calls").display()
                )
            } else {
                "echo 'error[E0432]: unresolved import' >&2\nexit 101\n".to_string()
            };
            let cargo = bin.join("cargo");
            std::fs::write(&cargo, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&cargo, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
            RealExec {
                root: d.to_path_buf(),
                env: HashMap::from([
                    ("PATH".to_string(), path),
                    ("PADDOCK_DEV_HOME".to_string(), d.join("home").display().to_string()),
                    ("HOME".to_string(), d.display().to_string()),
                    ("PADDOCK_DEV_TARGET_DIR".to_string(), d.join("tgt").display().to_string()),
                ]),
                logs: d.join("logs"),
                stop: Arc::new(AtomicBool::new(false)),
                built_heavy: Default::default(),
                budget_end: Instant::now() + Duration::from_secs(60),
            }
        }

        #[test]
        fn a_heavy_built_from_the_tree_carries_the_gate_and_is_cached_under_its_hash() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), true);
            let g = gate("clippy a", "echo ran >> out.txt");
            assert!(matches!(decide(&g, &ex, Duration::from_secs(60)), Verdict::Passed { .. }));
            let calls = std::fs::read_to_string(d.path().join("heavy-calls")).unwrap();
            assert!(calls.contains("run pre-push --"), "{calls}");
            assert!(d.path().join("out.txt").is_file(), "the gate's command ran under the built heavy");
            let cached: Vec<_> = std::fs::read_dir(d.path().join("home/bin")).unwrap().flatten().collect();
            assert!(
                cached.len() == 1 && cached[0].file_name().to_string_lossy().starts_with("heavy-"),
                "{cached:?}"
            );
            // The per-build folder is gone (not under the cache folder: the build folder is `PADDOCK_DEV_TARGET_DIR`).
            let left: Vec<_> = std::fs::read_dir(d.path().join("tgt"))
                .map(|r| r.flatten().collect())
                .unwrap_or_default();
            assert!(left.is_empty(), "the build folder was not removed: {left:?}");
            assert!(!d.path().join("home").join("target-heavy-x").exists());
            // A second run finds it in the cache: cargo is not needed.
            std::fs::remove_file(d.path().join("fakebin/cargo")).unwrap();
            let ex2 = RealExec {
                built_heavy: Default::default(),
                budget_end: Instant::now() + Duration::from_secs(60),
                env: ex.env.clone(),
                root: ex.root.clone(),
                logs: ex.logs.clone(),
                stop: ex.stop.clone(),
            };
            assert!(matches!(decide(&g, &ex2, Duration::from_secs(60)), Verdict::Passed { .. }));
        }

        #[test]
        fn a_heavy_that_will_not_build_is_not_measured_and_names_the_error() {
            let d = tempfile::tempdir().unwrap();
            let ex = repo_with_fake_cargo(d.path(), false);
            match decide(&gate("clippy a", "true"), &ex, Duration::from_secs(60)) {
                Verdict::NotMeasured { reason, next } => {
                    assert!(
                        reason.contains("unresolved import") && reason.contains("no heavy slot tool"),
                        "{reason}"
                    );
                    assert!(next.contains("re-run"), "{next}");
                }
                v => panic!("{v:?}"),
            }
        }

        #[test]
        fn heavy_off_without_the_test_variable_is_an_override_and_not_measured() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.env.insert("PADDOCK_HEAVY".into(), "off".into());
            match decide(&gate("clippy a", "true"), &ex, Duration::from_secs(5)) {
                Verdict::NotMeasured { reason, .. } => assert!(reason.contains("OVERRIDE SET"), "{reason}"),
                v => panic!("{v:?}"),
            }
        }

        #[test]
        fn a_build_after_the_budget_is_not_started() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = repo_with_fake_cargo(d.path(), true);
            ex.budget_end = Instant::now();
            match decide(&gate("clippy a", "true"), &ex, Duration::from_secs(5)) {
                Verdict::NotMeasured { reason, .. } => assert!(reason.contains("budget was spent"), "{reason}"),
                v => panic!("{v:?}"),
            }
        }

        #[test]
        fn heavy_off_still_runs_without_the_slot_for_hermetic_tests() {
            let d = tempfile::tempdir().unwrap();
            let mut ex = real(d.path(), &d.path().join("l"));
            ex.env.insert("PADDOCK_HEAVY".into(), "off".into());
            ex.env.insert(TEST_HEAVY_OFF.into(), "1".into());
            ex.env.insert("PATH".into(), "/usr/bin:/bin".into());
            assert!(matches!(
                decide(&gate("clippy a", "true"), &ex, Duration::from_secs(5)),
                Verdict::Passed { .. }
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn docker_is_looked_for_at_its_sockets_and_never_started() {
        let env = HashMap::from([("HOME".to_string(), "/h".to_string())]);
        let s = docker_sockets(&env);
        assert!(s.contains(&PathBuf::from("/h/.orbstack/run/docker.sock")) && s.contains(&PathBuf::from("/var/run/docker.sock")));
        let host = HashMap::from([("DOCKER_HOST".to_string(), "unix:///x/d.sock".to_string())]);
        assert_eq!(docker_sockets(&host), [PathBuf::from("/x/d.sock")]);
        // No daemon behind a socket path: not up, and nothing was run to find out.
        let none = HashMap::from([("DOCKER_HOST".to_string(), "unix:///nonexistent/paddock-docker.sock".to_string())]);
        assert!(!docker_answers(&none));
    }
}
