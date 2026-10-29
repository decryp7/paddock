//! The app's side of the worker: a pool of worker processes behind the index's [`SymbolSource`].
//!
//! A call never has a deadline (ADR 0031). It ends when the worker answers, when the worker dies, when the stall judge says the
//! worker is stuck, or when the run is cancelled; in the last three the worker is ended and the file is quarantined (or the
//! run cancelled) while the run goes on with a fresh worker.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use pacenotes_core::{Language, RepoPath};
use pacenotes_index::{CallFact, ChunkFact, FileFacts, ImportFact, SourceError, SymbolFact, SymbolSource};
use paddock_ipc::{Child, ChildSpec, IpcError, NoCalls, launch};
use paddock_jobs::CancelToken;

use crate::probe::ProcessProbe;
use crate::protocol::{PROTOCOL, ParseReply, ParseRequest, WireFacts};
use crate::stall::{StallJudge, StallLimits, Tick, Verdict};

/// How the pool starts and watches workers.
#[derive(Debug, Clone)]
pub struct WorkerSettings {
    /// The worker program.
    pub program: PathBuf,
    /// A worker's memory cap in bytes (proportional memory); its tree is ended above it.
    pub memory_limit: Option<u64>,
    /// The stall limits.
    pub stall: StallLimits,
    /// The monitor's beat: how often it wakes to look at a worker. A beat is a unit of ticks, not a deadline.
    pub beat: Duration,
    /// The node limit sent with every request.
    pub max_nodes: u32,
}

impl WorkerSettings {
    /// Defaults for `program`: 1 GiB per worker, the default stall limits, a beat of one second.
    pub fn new(program: impl Into<PathBuf>) -> WorkerSettings {
        WorkerSettings {
            program: program.into(),
            memory_limit: Some(1 << 30),
            stall: StallLimits::default(),
            beat: Duration::from_secs(1),
            max_nodes: pacenotes_parse::DEFAULT_MAX_NODES as u32,
        }
    }
}

/// The pool. Cheap to share: a thread takes an idle worker or starts one, and gives it back after a good answer.
pub struct ParseWorkers {
    settings: WorkerSettings,
    probe: Arc<dyn ProcessProbe>,
    idle: Mutex<Vec<Child>>,
    started: Mutex<u64>,
    paused: Mutex<u64>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ParseWorkers {
    /// A pool that reads worker processes with `probe`.
    pub fn new(settings: WorkerSettings, probe: Arc<dyn ProcessProbe>) -> ParseWorkers {
        ParseWorkers {
            settings,
            probe,
            idle: Mutex::new(Vec::new()),
            started: Mutex::new(0),
            paused: Mutex::new(0),
        }
    }

    /// How many worker processes were started so far (a restart after a crash counts).
    pub fn started(&self) -> u64 {
        *lock(&self.started)
    }

    /// How many ticks a worker was seen paused by the system (for the job's "Paused by macOS or another app").
    pub fn paused_ticks(&self) -> u64 {
        *lock(&self.paused)
    }

    /// Ends every idle worker.
    pub fn shutdown(&self) {
        for child in lock(&self.idle).drain(..) {
            child.stop(Duration::from_millis(200));
        }
    }

    fn take(&self) -> Result<Child, SourceError> {
        if let Some(child) = lock(&self.idle).pop()
            && !child.exited()
            && !child.peer().is_closed()
        {
            return Ok(child);
        }
        let mut spec = ChildSpec::new(&self.settings.program, "pacenotes-parse-worker");
        spec.lower_priority = true;
        spec.memory_limit = self.settings.memory_limit;
        spec.protocol = PROTOCOL;
        spec.host_name = "Pacenotes".into();
        // a cap, not a liveness bound: the launcher gives up at once when the process exits before it connects
        spec.start_timeout = Duration::from_secs(600);
        let child =
            launch(&spec, Arc::new(NoCalls)).map_err(|e| SourceError::Failed(format!("the parser worker could not be started: {e}")))?;
        *lock(&self.started) += 1;
        Ok(child)
    }
}

impl Drop for ParseWorkers {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn into_facts(wire: WireFacts) -> Result<FileFacts, String> {
    Ok(FileFacts {
        language: Language::parse(&wire.language).map_err(|e| e.to_string())?,
        symbols: wire
            .symbols
            .into_iter()
            .map(|s| SymbolFact {
                kind: s.kind,
                name: s.name,
                qualified_name: s.qualified_name,
                span: s.span,
                depth: s.depth,
            })
            .collect(),
        imports: wire
            .imports
            .into_iter()
            .map(|i| ImportFact {
                target: i.target,
                span: i.span,
            })
            .collect(),
        calls: wire
            .calls
            .into_iter()
            .map(|c| CallFact {
                caller: c.caller,
                callee: c.callee,
                span: c.span,
            })
            .collect(),
        chunks: wire
            .chunks
            .into_iter()
            .map(|c| ChunkFact {
                span: c.span,
                symbol: c.symbol,
                ordinal: c.ordinal,
            })
            .collect(),
        partial: wire.partial,
        has_errors: wire.has_errors,
    })
}

/// What the monitor decided about a call.
#[derive(Default)]
struct Watch {
    done: bool,
    stuck: Option<String>,
}

impl SymbolSource for ParseWorkers {
    fn extract(&self, path: &RepoPath, bytes: &[u8], cancel: &CancelToken) -> Result<Option<FileFacts>, SourceError> {
        if cancel.is_cancelled() {
            return Err(SourceError::Cancelled);
        }
        let child = self.take()?;
        let pid = child.pid();
        let request = ParseRequest {
            path: path.to_string(),
            bytes: bytes.to_vec(),
            max_nodes: self.settings.max_nodes,
        };
        let watch = (Mutex::new(Watch::default()), Condvar::new());
        let result = std::thread::scope(|scope| {
            scope.spawn(|| self.monitor(&child, pid, cancel, &watch));
            let result = child.peer().request::<ParseRequest, ParseReply>(&request, None);
            lock(&watch.0).done = true;
            watch.1.notify_all();
            result
        });
        match result {
            Ok(reply) => {
                lock(&self.idle).push(child);
                match reply.facts {
                    None => Ok(None),
                    Some(wire) => into_facts(wire)
                        .map(Some)
                        .map_err(|why| SourceError::Failed(format!("the worker's reply is not valid: {why}"))),
                }
            }
            // a typed answer: the worker is fine, this file is not
            Err(IpcError::Remote(e)) => {
                lock(&self.idle).push(child);
                Err(SourceError::Failed(e.message))
            }
            Err(IpcError::TooLarge { len, max }) => {
                lock(&self.idle).push(child);
                Err(SourceError::Failed(format!(
                    "the file does not fit in one message ({len} bytes, at most {max})"
                )))
            }
            // the worker is gone, ended by the monitor, or ended by cancel
            Err(other) => {
                child.kill();
                if cancel.is_cancelled() {
                    return Err(SourceError::Cancelled);
                }
                let stuck = lock(&watch.0).stuck.clone();
                Err(SourceError::Crashed(stuck.unwrap_or_else(|| {
                    format!("the parser worker ended while reading this file ({other})")
                })))
            }
        }
    }
}

impl ParseWorkers {
    /// Watches one call: every beat, looks at the worker and the run's cancel token. Ends the worker when it is stuck or the run
    /// is cancelled; the blocked call then returns a closed-connection error.
    fn monitor(&self, child: &Child, pid: u32, cancel: &CancelToken, watch: &(Mutex<Watch>, Condvar)) {
        let mut judge = StallJudge::new(self.settings.stall);
        // the first sample only sets the baseline: a call that ends before its first beat never asks the probe (on macOS a sample
        // is a `ps` process)
        let mut last_cpu: Option<u64> = None;
        let (state, wake) = watch;
        let mut guard = lock(state);
        loop {
            // one wake-up is one tick, however late it comes; a call that already ended is not waited for (the notify may have come
            // before this thread first took the lock)
            if !guard.done {
                guard = wake
                    .wait_timeout(guard, self.settings.beat)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0;
            }
            if guard.done {
                return;
            }
            if cancel.is_cancelled() {
                child.kill();
                return;
            }
            // the lock is not held while the probe runs (a `ps` process on macOS), so a call that ends meanwhile is not delayed by it
            drop(guard);
            let sampled = self.probe.sample(pid);
            guard = lock(state);
            let Some(sample) = sampled else { continue };
            let grew = last_cpu.map_or(0, |last| sample.cpu_micros.saturating_sub(last));
            last_cpu = Some(sample.cpu_micros);
            match judge.observe(Tick {
                state: sample.state,
                cpu_grew_micros: grew,
                progress: false,
                owes_host: false,
            }) {
                Verdict::Fine => {}
                Verdict::Paused => *lock(&self.paused) += 1,
                Verdict::Stuck(why) => {
                    guard.stuck = Some(format!("the parser worker was judged stuck ({why:?}) and ended"));
                    child.kill();
                    return;
                }
            }
        }
    }
}
