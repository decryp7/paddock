//! Runs benchmarks on the user's click, one kind at a time, each in its own `dyno-bench` process with a timeout, and
//! keeps the run history. Nothing here starts on its own: only [`BenchCentre::start`] (from the `StartBenchmark`
//! request) begins a run. The scratch folder of the disk test is created here, so it is removed here whatever the
//! child did, including when it was killed.

use crate::agent::{lock, now};
use crate::files;
use dyno_core::bench::{
    BenchHistory, BenchKind, BenchOverview, BenchPhase, BenchProgress, BenchRun, BenchStatus, KindResult, LoadAtRun, RunPlan,
    HISTORY_VERSION,
};
use dyno_core::protocol::{AgentError, ErrorCode};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long one kind may run before it is killed. Disk gets more: a slow USB stick writing the capped file.
pub fn timeout(kind: BenchKind) -> Duration {
    match kind {
        BenchKind::Disk => Duration::from_secs(180),
        _ => Duration::from_secs(60),
    }
}

/// Runs one kind somewhere else. A port so tests need no real process.
pub trait BenchLauncher: Send + Sync {
    /// Measures `kind`. `scratch` is a folder the caller created and will remove. Never panics; every failure is a
    /// [`BenchStatus`].
    fn run(&self, kind: BenchKind, scratch: &Path, disk_mib: u32) -> BenchStatus;
}

/// Starts the `dyno-bench` executable that sits next to the agent.
pub struct ProcessLauncher {
    exe: PathBuf,
}

impl ProcessLauncher {
    /// The launcher for the executable beside the running agent.
    pub fn beside_agent() -> ProcessLauncher {
        let name = if cfg!(windows) { "dyno-bench.exe" } else { "dyno-bench" };
        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        ProcessLauncher { exe: dir.join(name) }
    }

    /// A launcher for this executable.
    pub fn at(exe: impl Into<PathBuf>) -> ProcessLauncher {
        ProcessLauncher { exe: exe.into() }
    }
}

impl BenchLauncher for ProcessLauncher {
    fn run(&self, kind: BenchKind, scratch: &Path, disk_mib: u32) -> BenchStatus {
        if !self.exe.is_file() {
            return BenchStatus::Unavailable {
                reason: "The benchmark runner (dyno-bench) is missing from the installation; reinstall Dyno.".into(),
            };
        }
        let mut cmd = dyno_platform::process::command(&self.exe);
        cmd.arg(kind.name());
        if kind == BenchKind::Disk {
            cmd.arg("--dir").arg(scratch).arg("--disk-mib").arg(disk_mib.to_string());
        }
        match dyno_platform::process::run_with_timeout(cmd, timeout(kind)) {
            Ok(out) if out.status.success() => serde_json::from_slice::<BenchStatus>(&out.stdout).unwrap_or_else(|_| BenchStatus::Failed {
                reason: "The benchmark runner answered with something Dyno could not read.".into(),
            }),
            Ok(out) => BenchStatus::Failed {
                reason: format!(
                    "The benchmark runner failed ({}).",
                    out.status.code().map_or("it was stopped".to_string(), |c| format!("exit code {c}"))
                ),
            },
            Err(dyno_core::ports::PlatformError::Timeout(_)) => BenchStatus::Stopped {
                reason: format!("It took longer than {} seconds and was stopped.", timeout(kind).as_secs()),
            },
            Err(e) => BenchStatus::Failed { reason: e.to_string() },
        }
    }
}

struct State {
    history: BenchHistory,
    running: Option<BenchProgress>,
    counter: u64,
}

struct Inner {
    state: Mutex<State>,
    cancel: AtomicBool,
    launcher: Box<dyn BenchLauncher>,
    file: PathBuf,
    scratch_root: PathBuf,
}

/// The benchmark runs: history on disk, at most one run at a time.
#[derive(Clone)]
pub struct BenchCentre {
    inner: Arc<Inner>,
}

impl BenchCentre {
    /// Loads the history from `file` (damage means an empty history) and runs through `launcher`; scratch folders
    /// are made inside `scratch_root`.
    pub fn load(file: PathBuf, scratch_root: PathBuf, launcher: Box<dyn BenchLauncher>) -> (BenchCentre, Option<String>) {
        let (history, warning) = files::load(&file, BenchHistory::parse);
        let centre = BenchCentre {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    history: history.unwrap_or_default(),
                    running: None,
                    counter: 0,
                }),
                cancel: AtomicBool::new(false),
                launcher,
                file,
                scratch_root,
            }),
        };
        (centre, warning)
    }

    /// The history and the run in progress.
    pub fn overview(&self) -> BenchOverview {
        let s = lock(&self.inner.state);
        BenchOverview {
            running: s.running.clone(),
            runs: s.history.runs.clone(),
        }
    }

    /// One saved run.
    pub fn run(&self, id: &str) -> Option<BenchRun> {
        lock(&self.inner.state).history.get(id).cloned()
    }

    /// Deletes a saved run.
    pub fn delete(&self, id: &str) -> Result<(), AgentError> {
        let mut s = lock(&self.inner.state);
        if !s.history.remove(id) {
            return Err(AgentError::new(ErrorCode::NotFound, "That benchmark run no longer exists."));
        }
        self.save(&s.history);
        Ok(())
    }

    /// Asks the run in progress to stop after the kind it is measuring.
    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
    }

    fn save(&self, h: &BenchHistory) {
        if let Err(e) = files::save(&self.inner.file, h) {
            log::error!("dyno-agent: could not save the benchmark history: {e}");
        }
    }

    /// Starts a run in the background and returns at once. Refused with `Busy` while another is running.
    pub fn start(&self, plan: RunPlan, phase: Option<BenchPhase>, load: LoadAtRun, machine: String) -> Result<BenchOverview, AgentError> {
        let started = now();
        let run_id = {
            let mut s = lock(&self.inner.state);
            if s.running.is_some() {
                return Err(AgentError::new(
                    ErrorCode::Busy,
                    "A benchmark is already running; wait for it or cancel it.",
                ));
            }
            s.counter += 1;
            let id = format!("{}-{}", started.millis(), s.counter);
            s.running = Some(BenchProgress {
                run_id: id.clone(),
                started_at: started,
                kinds: plan.kinds.clone(),
                current: None,
                done: Vec::new(),
            });
            id
        };
        self.inner.cancel.store(false, Ordering::SeqCst);
        let me = self.clone();
        // Not lowered (rule 52): it measures how fast this machine is, which a lowered thread would understate.
        let spawned = std::thread::Builder::new().name("dyno-bench".into()).spawn({
            let run_id = run_id.clone();
            move || me.execute(&run_id, plan, phase, load, machine)
        });
        if let Err(e) = spawned {
            lock(&self.inner.state).running = None;
            return Err(AgentError::new(ErrorCode::Internal, format!("Could not start the benchmark: {e}")));
        }
        Ok(self.overview())
    }

    fn execute(&self, run_id: &str, plan: RunPlan, phase: Option<BenchPhase>, load: LoadAtRun, machine: String) {
        let started = lock(&self.inner.state).running.as_ref().map_or_else(now, |r| r.started_at);
        let mut results = Vec::new();
        for kind in &plan.kinds {
            if self.inner.cancel.load(Ordering::SeqCst) {
                results.push(KindResult {
                    kind: *kind,
                    millis: 0,
                    status: BenchStatus::Stopped {
                        reason: "Cancelled before it started.".into(),
                    },
                });
                continue;
            }
            if let Some(r) = lock(&self.inner.state).running.as_mut() {
                r.current = Some(*kind);
            }
            let t = Instant::now();
            let status = self.measure(run_id, *kind, plan.disk_mib);
            results.push(KindResult {
                kind: *kind,
                millis: u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX),
                status,
            });
            if let Some(r) = lock(&self.inner.state).running.as_mut() {
                r.current = None;
                r.done.push(*kind);
            }
        }
        let run = BenchRun {
            id: run_id.to_string(),
            started_at: started,
            label: plan.label,
            group: plan.group,
            phase,
            load,
            machine,
            results,
        };
        let mut s = lock(&self.inner.state);
        s.history.push(run);
        s.running = None;
        self.save(&s.history);
    }

    /// One kind, with its scratch folder made and removed here.
    fn measure(&self, run_id: &str, kind: BenchKind, disk_mib: u32) -> BenchStatus {
        let scratch = self.inner.scratch_root.join(format!("dyno-bench-{run_id}"));
        if kind == BenchKind::Disk {
            if let Err(e) = std::fs::create_dir_all(&scratch) {
                return BenchStatus::Failed {
                    reason: format!("Dyno could not make a scratch folder for the disk test ({e})."),
                };
            }
        }
        let launcher = &self.inner.launcher;
        let status =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| launcher.run(kind, &scratch, disk_mib))).unwrap_or_else(|_| {
                BenchStatus::Failed {
                    reason: "The benchmark runner crashed.".into(),
                }
            });
        if kind == BenchKind::Disk {
            let _ = std::fs::remove_dir_all(&scratch);
        }
        status
    }
}

/// The file version written next to the history (kept for the schema test).
pub const FILE_VERSION: u32 = HISTORY_VERSION;

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::bench::{plan, BenchMetric};
    use std::sync::atomic::AtomicUsize;

    struct Fake {
        calls: AtomicUsize,
        gate: Option<Arc<(Mutex<bool>, std::sync::Condvar)>>,
        panic_on: Option<BenchKind>,
    }

    impl BenchLauncher for Fake {
        fn run(&self, kind: BenchKind, scratch: &Path, _mib: u32) -> BenchStatus {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(g) = &self.gate {
                let mut open = lock(&g.0);
                while !*open {
                    open = g.1.wait(open).unwrap();
                }
            }
            if Some(kind) == self.panic_on {
                panic!("boom");
            }
            if kind == BenchKind::Disk {
                assert!(scratch.is_dir(), "the disk test gets its scratch folder");
                std::fs::write(scratch.join("left-behind"), b"x").unwrap();
            }
            BenchStatus::Done {
                metrics: vec![BenchMetric::higher("m", "M", 1.0, "u")],
            }
        }
    }

    fn centre(dir: &Path, gate: Option<Arc<(Mutex<bool>, std::sync::Condvar)>>, panic_on: Option<BenchKind>) -> BenchCentre {
        BenchCentre::load(
            dir.join("bench.json"),
            dir.to_path_buf(),
            Box::new(Fake {
                calls: AtomicUsize::new(0),
                gate,
                panic_on,
            }),
        )
        .0
    }

    fn wait_idle(c: &BenchCentre) -> BenchOverview {
        for _ in 0..500 {
            let o = c.overview();
            if o.running.is_none() {
                return o;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the run did not finish");
    }

    fn start(c: &BenchCentre, kinds: &[BenchKind]) -> Result<BenchOverview, AgentError> {
        c.start(plan(kinds, None, "test run", None).unwrap(), None, LoadAtRun::default(), "m".into())
    }

    #[test]
    fn a_run_measures_each_kind_saves_and_removes_the_scratch_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, None);
        // Whether the run is still going when start returns is a race with the fake; the gated test below pins the running state.
        start(&c, &[BenchKind::Cpu, BenchKind::Disk]).unwrap();
        let o = wait_idle(&c);
        assert_eq!(o.runs.len(), 1);
        assert_eq!(o.runs[0].results.len(), 2);
        assert!(o.runs[0].results.iter().all(|r| matches!(r.status, BenchStatus::Done { .. })));
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(
            leftovers.iter().all(|n| !n.to_string_lossy().starts_with("dyno-bench-")),
            "{leftovers:?}"
        );
        // The history survives a restart.
        let (again, warn) = BenchCentre::load(
            tmp.path().join("bench.json"),
            tmp.path().to_path_buf(),
            Box::new(ProcessLauncher::at("nope")),
        );
        assert!(warn.is_none());
        assert_eq!(again.overview().runs.len(), 1);
        assert!(again.run(&o.runs[0].id).is_some());
    }

    #[test]
    fn a_second_run_is_refused_while_one_runs_and_cancel_stops_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let c = centre(tmp.path(), Some(gate.clone()), None);
        start(&c, &[BenchKind::Cpu, BenchKind::Memory]).unwrap();
        assert_eq!(start(&c, &[BenchKind::Cpu]).unwrap_err().code, ErrorCode::Busy);
        while c.overview().running.and_then(|r| r.current).is_none() {
            std::thread::sleep(Duration::from_millis(5));
        }
        c.cancel();
        *lock(&gate.0) = true;
        gate.1.notify_all();
        let o = wait_idle(&c);
        assert!(matches!(o.runs[0].results[0].status, BenchStatus::Done { .. }));
        assert!(matches!(o.runs[0].results[1].status, BenchStatus::Stopped { .. }));
    }

    #[test]
    fn a_crashing_launcher_is_one_failed_kind_not_a_dead_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, Some(BenchKind::Memory));
        start(&c, &[BenchKind::Cpu, BenchKind::Memory]).unwrap();
        let o = wait_idle(&c);
        assert!(matches!(o.runs[0].results[1].status, BenchStatus::Failed { .. }));
        assert!(matches!(o.runs[0].results[0].status, BenchStatus::Done { .. }));
        // And it can run again.
        start(&c, &[BenchKind::Cpu]).unwrap();
        assert_eq!(wait_idle(&c).runs.len(), 2);
    }

    #[test]
    fn deleting_a_run_and_a_damaged_history() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, None);
        start(&c, &[BenchKind::Cpu]).unwrap();
        let id = wait_idle(&c).runs[0].id.clone();
        c.delete(&id).unwrap();
        assert_eq!(c.delete(&id).unwrap_err().code, ErrorCode::NotFound);
        std::fs::write(tmp.path().join("bench.json"), "{broken").unwrap();
        std::fs::write(tmp.path().join("bench.json.bak"), "{broken too").unwrap();
        let (c2, warn) = BenchCentre::load(
            tmp.path().join("bench.json"),
            tmp.path().to_path_buf(),
            Box::new(ProcessLauncher::at("nope")),
        );
        assert!(warn.is_some());
        assert!(c2.overview().runs.is_empty());
    }

    #[test]
    fn a_missing_runner_is_unavailable_with_a_next_step() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProcessLauncher::at(tmp.path().join("absent")).run(BenchKind::Cpu, tmp.path(), 16);
        assert!(matches!(s, BenchStatus::Unavailable { reason } if reason.contains("reinstall")));
    }

    #[test]
    fn timeouts_are_bounded() {
        assert!(timeout(BenchKind::Disk) > timeout(BenchKind::Cpu));
        assert_eq!(FILE_VERSION, 1);
    }
}
