//! Running the gates: each one decided by an [`Exec`] (real processes, or a fake in the tests), several at a time, cheapest first.

use super::gates::{Gate, Need, Planned};
use crate::ci_tool::{Output, ToolCall, tail};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How one run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// Exit status 0.
    pub success: bool,
    /// `exit 101`, `timed out after 2400 s`.
    pub how: String,
    /// The last lines of the output, for the failure line.
    pub tail: String,
}

/// What the gate needs from the machine. The real one starts processes; the tests' one answers from a table (ER-14).
pub trait Exec: Sync {
    /// Is `program` on PATH?
    fn has_tool(&self, program: &str) -> bool;
    /// Does a Docker daemon answer? Never starts one.
    fn docker_up(&self) -> bool;
    /// Runs the gate. `Err`: it could not be started (the program is missing).
    fn run(&self, gate: &Gate) -> Result<Done, String>;
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
    /// It could not look: nothing is proven (ER-33).
    NotMeasured {
        /// Why.
        reason: String,
        /// What to do.
        next: String,
    },
    /// The check itself cannot be trusted.
    Broken {
        /// What is wrong.
        cause: String,
        /// What to do.
        next: String,
    },
}

/// The verdict of one gate: its needs first, then the run.
pub fn decide(gate: &Gate, exec: &dyn Exec) -> Verdict {
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
    match exec.run(gate) {
        Err(e) => Verdict::NotMeasured {
            reason: e,
            next: "install the program, then re-run `./dev check --pre-push`".into(),
        },
        Ok(done) if done.success => Verdict::Passed {
            secs: started.elapsed().as_secs_f64(),
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
            Verdict::Failed {
                cause: format!("{} ({})", gate.rerun(), done.how),
                tail: done.tail.trim_end().to_string(),
                next: format!("{hint}. Re-run alone: {}", gate.rerun()),
                secs: started.elapsed().as_secs_f64(),
            }
        }
    }
}

/// Every planned row decided, `workers` at a time, lowest rank first; the result keeps the planned order.
pub fn run_all(planned: &[Planned], exec: &dyn Exec, workers: usize) -> Vec<(String, Verdict)> {
    let mut order: Vec<usize> = (0..planned.len()).collect();
    order.sort_by_key(|&i| match &planned[i] {
        Planned::Run(g) => g.rank,
        _ => 0,
    });
    let queue = Mutex::new(order.into_iter().collect::<VecDeque<_>>());
    let results: Vec<Mutex<Option<Verdict>>> = planned.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..workers.max(1) {
            s.spawn(|| {
                loop {
                    let next = queue.lock().map(|mut q| q.pop_front()).unwrap_or(None);
                    let Some(i) = next else { break };
                    let v = match &planned[i] {
                        Planned::Run(g) => decide(g, exec),
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
            let id = match p {
                Planned::Run(g) => g.id.clone(),
                Planned::Skip { id, .. } | Planned::Broken { id, .. } => id.clone(),
            };
            let v = r.into_inner().ok().flatten().unwrap_or(Verdict::Broken {
                cause: "the gate was never run".into(),
                next: "report this: a worker ended early".into(),
            });
            (id, v)
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

/// The real machine: processes through [`ToolCall`] (bounded, environment listed, output kept short).
pub struct RealExec {
    /// The repository root.
    pub root: PathBuf,
    /// The caller's environment.
    pub env: HashMap<String, String>,
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

impl RealExec {
    fn call(&self, gate: &Gate) -> ToolCall {
        let heavy: Vec<String> = if gate.heavy {
            self.env
                .get("PADDOCK_HEAVY")
                .map(|h| h.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default()
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
        let mut call = ToolCall::new(program)
            .args(args)
            .pass(PASS.iter().copied())
            .cwd(self.root.join(&gate.cwd))
            .timeout(Duration::from_secs(if gate.heavy { 3000 } else { 900 }));
        if gate.heavy && !self.env.contains_key("CARGO_BUILD_JOBS") {
            call = call.env("CARGO_BUILD_JOBS", "2");
        }
        for (k, v) in &gate.env {
            call = call.env(k, v);
        }
        call.output(Output::Capture)
    }
}

impl Exec for RealExec {
    fn has_tool(&self, program: &str) -> bool {
        let path = self.env.get("PATH").cloned().unwrap_or_default();
        std::env::split_paths(&path).any(|d| exists(&d, program))
    }

    fn docker_up(&self) -> bool {
        let docker = self.env.get("DOCKER").cloned().unwrap_or_else(|| "docker".into());
        let call = ToolCall::new(docker)
            .args(["info"])
            .pass(PASS.iter().copied())
            .timeout(Duration::from_secs(30));
        call.run(&self.env, &mut std::io::sink()).is_ok_and(|r| r.success)
    }

    fn run(&self, gate: &Gate) -> Result<Done, String> {
        let ran = self.call(gate).run(&self.env, &mut std::io::sink())?;
        Ok(Done {
            success: ran.success,
            how: ran.how,
            tail: tail(&format!("{}{}", ran.stdout, ran.stderr), 12),
        })
    }
}

fn exists(dir: &Path, program: &str) -> bool {
    dir.join(program).is_file() || (cfg!(windows) && dir.join(format!("{program}.exe")).is_file())
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
}
