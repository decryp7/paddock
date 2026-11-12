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
}

/// Variables every gate may see besides the base list: the toolchain, Docker, caches and Paddock's own settings.
const PASS: &[&str] = &[
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

/// The label that marks the container of one run, so only it is ever stopped (ER-14).
const RUN_LABEL: &str = "paddock.run";

fn slug(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

impl RealExec {
    fn call(&self, gate: &Gate, left: Duration, log: &Path, label: Option<&str>) -> ToolCall {
        let heavy = if gate.heavy {
            heavy_prefix(&self.env, &|p| self.has_tool(p))
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
        let path = self.env.get("PATH").cloned().unwrap_or_default();
        std::env::split_paths(&path).any(|d| exists(&d, program))
    }

    fn docker_up(&self) -> bool {
        docker_answers(&self.env)
    }

    fn run(&self, gate: &Gate, left: Duration) -> Result<Done, String> {
        let _ = std::fs::create_dir_all(&self.logs);
        let log = self.logs.join(format!("{}.log", slug(&gate.id)));
        let label = (gate.need == Need::Docker).then(run_token);
        let ran = self.call(gate, left, &log, label.as_deref()).run(&self.env, &mut std::io::sink())?;
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
