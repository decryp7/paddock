//! The life of an sccache server that `./dev` starts: the agents' local server ([`crate::local_sccache`]). CI's `ci setup-rust` and
//! `ci sccache-stats` use the same [`probe`] (rule 55); they still start the server with `sccache --start-server`, as the shell steps
//! of a release run do (the parity tests hold the two to the same calls), so a CI server is not held by its pid.
//!
//! - **Start, held** ([`start`]): the server runs in the foreground of a process this tool spawns detached (`SCCACHE_START_SERVER=1`,
//!   `SCCACHE_NO_DAEMON=1`), so its pid is the spawned child's, read with its start time (rule 53) and written to a record beside the
//!   server's folder ([`record_path`]). `sccache --start-server` forks a daemon whose pid nobody is told, so it is not used.
//! - **Probe** ([`probe`]): the server's statistics within [`PROBE_LIMIT`]. A hung server takes the connection and never answers
//!   (measured: `--show-stats` against a stopped server waits for ever, and every compile client waits at 0% CPU).
//! - **Stop by the record only** ([`stop_recorded`]): the recorded pid must still be the process that started at the recorded time,
//!   and after it ends the port must be free. A server this tool did not start (no record, another start time, or another process on
//!   the port) is never touched.
//! - **Restart once** ([`answer_or_restart`]): no answer, then stop and start, then one more probe; still none, the caller builds
//!   without the cache and says so.
//!
//! What a waiting client does when its server is stopped (measured 2026-11-25 with sccache 0.19.0, docs/benchmarks.md "sccache server
//! hang: client behaviour and probe time"): it fails the compile with "Failed to read response header ... Connection reset by peer"
//! (exit 2), `SCCACHE_IGNORE_SERVER_IO_ERROR=1` or not; it never compiles locally. So a build whose compiles already waited on a hung
//! server cannot pass after a restart; such a build is not detected here (no mid-build watchdog: user decision 2026-11-25).

use crate::ci_tool::{ToolCall, read_tail};
use paddock_proc::{Presence, ProcessId, StartTime};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a server may take to answer its statistics before it counts as hung.
///
/// Measured cause (2026-11-25, docs/benchmarks.md "sccache server hang: client behaviour and probe time"): 300 probes of the machine's
/// shared server (`--show-adv-stats`, the client's own start included) at a 1-minute load of 12 to 16 on 14 cores: p50 20 ms, p99 68 ms,
/// max 86 ms. 10 s is over 100 times the largest. A hang detector, not a speed limit (ER-12): a hung server never answers.
pub const PROBE_LIMIT: Duration = Duration::from_secs(10);

/// How long a server may take from its start until it listens: the limit `--start-server` had (60 s); a local-disk server listens
/// after about 35 ms (measured 2026-11-25, 5 starts), and one with the S3 level first checks the store.
pub const START_LIMIT: Duration = Duration::from_secs(60);

/// How long a stopped server may take to end and free its port.
const STOP_LIMIT: Duration = Duration::from_secs(10);

/// How long a server is asked to end (SIGTERM) before it is killed; a hung server rarely ends on the request.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// The most one caller waits for another's restart of the same server: a stop, a start and two probes, with room.
const RESTART_LOCK_WAIT: Duration = Duration::from_secs(120);

/// The limits of one use, so a test can pass short ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// [`PROBE_LIMIT`].
    pub probe: Duration,
    /// [`START_LIMIT`].
    pub start: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            probe: PROBE_LIMIT,
            start: START_LIMIT,
        }
    }
}

/// Whether something listens on `127.0.0.1:port` (sccache listens there). A connect has no side effect, unlike `--show-stats`, which
/// starts a server when none runs (ER-45).
pub fn listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)).is_ok()
}

/// The record of the server this tool started on `port`, in the server's own folder.
pub fn record_path(dir: &Path, port: u16) -> PathBuf {
    dir.join(format!("server-{port}.json"))
}

/// What a record holds: the server's pid, its start time and its port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    /// The server process.
    pub id: ProcessId,
    /// The port it was started on.
    pub port: u16,
}

fn write_record(path: &Path, rec: Record) -> std::io::Result<()> {
    let body = serde_json::json!({
        "pid": rec.id.pid(),
        "started_us": rec.id.started().as_micros(),
        "port": rec.port,
    });
    paddock_proc::hint::write_private_atomic(path, format!("{body}\n").as_bytes())
}

/// The record at `path`, or `None` when there is none or it cannot be read as one.
pub fn read_record(path: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let pid = u32::try_from(v.get("pid")?.as_u64()?).ok()?;
    let started = v.get("started_us")?.as_u64()?;
    let port = u16::try_from(v.get("port")?.as_u64()?).ok()?;
    Some(Record {
        id: ProcessId::new(pid, StartTime::from_micros(started)),
        port,
    })
}

/// Starts the server `call` names (the sccache binary with the server's environment, no arguments) on `port`, held: the spawned
/// process is the server, and its pid and start time go to the record in `record_dir`. Its output goes to `start_log`. `Ok` once it
/// listens; `Err` with the cause when it exits first (the log's end: "Address already in use" when another server has the port), when
/// its start time cannot be read, or when it does not listen within `limit` (it is then ended by its identity).
pub fn start(
    call: &ToolCall,
    env: &HashMap<String, String>,
    port: u16,
    record_dir: &Path,
    start_log: &Path,
    limit: Duration,
) -> Result<ProcessId, String> {
    let began = Instant::now();
    // A port that already listens is another server's: a second one would only fail to bind, and until it did, the listener would
    // look like it. (A server that takes the port between this look and the bind makes ours exit; its record then names a process
    // that has ended, which `stop_recorded` never acts on.)
    if listening(port) {
        return Err(format!("Address already in use: another server listens on port {port}"));
    }
    let log = std::fs::File::create(start_log).map_err(|e| format!("cannot write its start log {}: {e}", start_log.display()))?;
    let log2 = log
        .try_clone()
        .map_err(|e| format!("cannot write its start log {}: {e}", start_log.display()))?;
    let mut cmd = call
        .clone()
        .env("SCCACHE_START_SERVER", "1")
        .env("SCCACHE_NO_DAEMON", "1")
        .command(env);
    cmd.stdin(std::process::Stdio::null()).stdout(log).stderr(log2);
    // Detached (its own process group), so a Ctrl+C of the caller does not reach it; safe from a program with several threads.
    let mut child = paddock_proc::spawn_detached(&mut cmd).map_err(|e| format!("it could not be started: {e}"))?;
    // A child not yet reaped keeps its pid, so its identity read now is its own (rule 53).
    let Some(id) = ProcessId::of(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("its start time could not be read, so it could not be held; it was ended".to_string());
    };
    let deadline = began + limit;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let said = read_tail(start_log, 4096);
            return Err(format!(
                "it exited at once ({}): {}",
                crate::ci_tool::describe_status(status),
                crate::ci_tool::tail(said.trim(), 4).trim()
            ));
        }
        if listening(port) {
            break;
        }
        if Instant::now() >= deadline {
            let _ = paddock_proc::end(id);
            let _ = child.wait();
            return Err(format!(
                "it did not listen on port {port} within {} s; it was ended",
                limit.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Err(e) = write_record(&record_path(record_dir, port), Record { id, port }) {
        // Without the record a later hang could not be repaired: end it, so nothing runs that nobody can stop.
        let _ = paddock_proc::end(id);
        let _ = child.wait();
        return Err(format!(
            "its record could not be written in {}: {e}; it was ended",
            record_dir.display()
        ));
    }
    eprintln!(
        "sccache-server: started {id} on port {port}, listening after {:.2} s",
        began.elapsed().as_secs_f64()
    );
    // The server outlives this program; it is never waited for here (dropping a Child neither kills nor waits).
    drop(child);
    Ok(id)
}

/// Whether "another server has the port" is what a failed start said.
pub fn address_in_use(why: &str) -> bool {
    why.contains("Address already in use") || why.contains("Address in use")
}

/// Asks the server for the statistics `call` names (`--show-stats` or `--show-adv-stats`) within `limit`. `Ok` with the output;
/// `Err` with why it gave none. Ask only a port that listens: the client starts a server when none does (ER-45).
pub fn probe(call: &ToolCall, env: &HashMap<String, String>, limit: Duration) -> Result<String, String> {
    let began = Instant::now();
    let ran = call.clone().timeout(limit).run(env, &mut std::io::sink());
    let took = began.elapsed().as_secs_f64();
    match ran {
        Ok(r) if r.success => Ok(r.stdout),
        Ok(r) => {
            let why = if r.how.starts_with("timed out") {
                format!("it did not answer its statistics within {:.1} s", limit.as_secs_f64())
            } else {
                format!(
                    "its statistics failed ({}): {}",
                    r.how,
                    crate::ci_tool::tail(r.stderr.trim(), 3).trim()
                )
            };
            Err(format!("{why} ({took:.1} s)"))
        }
        Err(e) => Err(format!("its statistics could not be asked for: {e}")),
    }
}

/// What [`stop_recorded`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// The recorded server was ours and has ended; the port is free.
    Ended,
    /// The recorded server had already ended, and nothing listens on the port.
    Gone,
    /// Not touched: why it is not ours (no record, another start time, another process on the port) or could not be judged.
    NotOurs(String),
}

/// Stops the server recorded for `port` in `record_dir`, only when the process now under the recorded pid started at the recorded
/// time (rule 53), and waits (at most a few seconds) until the port is free. Anything else is left alone.
pub fn stop_recorded(record_dir: &Path, port: u16) -> Stopped {
    let path = record_path(record_dir, port);
    let Some(rec) = read_record(&path).filter(|r| r.port == port) else {
        return Stopped::NotOurs(format!(
            "there is no record of a server this tool started on port {port} ({})",
            path.display()
        ));
    };
    let began = Instant::now();
    match rec.id.presence() {
        Presence::Unknown => {
            return Stopped::NotOurs(format!(
                "the recorded server {} cannot be read in the process table, so it is not ended",
                rec.id
            ));
        }
        Presence::Gone => {
            let _ = std::fs::remove_file(&path);
            return if listening(port) {
                Stopped::NotOurs(format!(
                    "the recorded server {} has ended (or its pid now names another process) and something else listens on port {port}",
                    rec.id
                ))
            } else {
                Stopped::Gone
            };
        }
        Presence::Running => {}
    }
    let ended = paddock_proc::terminate(rec.id, STOP_GRACE);
    let deadline = Instant::now() + STOP_LIMIT;
    while (!rec.id.has_ended() || listening(port)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if listening(port) {
        return Stopped::NotOurs(format!(
            "the recorded server {} was told to end ({ended:?}) but port {port} still listens after {} s",
            rec.id,
            STOP_LIMIT.as_secs()
        ));
    }
    let _ = std::fs::remove_file(&path);
    eprintln!(
        "sccache-server: stopped {} on port {port} ({ended:?}) in {:.1} s",
        rec.id,
        began.elapsed().as_secs_f64()
    );
    Stopped::Ended
}

/// What [`answer_or_restart`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// The server answered: its statistics, and whether it was restarted first.
    Answered {
        /// The statistics.
        stats: String,
        /// It did not answer at first and was restarted.
        restarted: bool,
    },
    /// No answer, even after a restart (or it could not be restarted): why, for the caller's one warning line.
    Unanswered(String),
}

/// The probe, and on no answer one restart and one more probe (the user's decision of 2026-11-25). `restart` stops the server by its
/// record and starts a fresh one; its `Err` names why it could not. Pure over the two closures.
pub fn answer_or_restart(probe: &mut dyn FnMut() -> Result<String, String>, restart: &mut dyn FnMut() -> Result<(), String>) -> Health {
    let first = match probe() {
        Ok(stats) => return Health::Answered { stats, restarted: false },
        Err(why) => why,
    };
    if let Err(why) = restart() {
        return Health::Unanswered(format!("{first}, and it was not restarted: {why}"));
    }
    match probe() {
        Ok(stats) => Health::Answered { stats, restarted: true },
        Err(why) => Health::Unanswered(format!("{first}; restarted, it still gave no answer: {why}")),
    }
}

/// Runs `f` while holding the restart lock of `port` in `dir` (a kernel file lock, freed with this process), so two callers never
/// restart one server at once; the second finds the first's fresh server. `Err` when the lock was not had within a bounded wait.
pub fn with_restart_lock<T>(dir: &Path, port: u16, f: impl FnOnce() -> T) -> Result<T, String> {
    let path = dir.join(format!("restart-{port}.lock"));
    let _ = std::fs::create_dir_all(dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("cannot open the restart lock {}: {e}", path.display()))?;
    let deadline = Instant::now() + RESTART_LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "another restart of the server on port {port} held {} for over {} s",
                    path.display(),
                    RESTART_LOCK_WAIT.as_secs()
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(format!("cannot lock {}: {e}", path.display())),
        }
    }
    let out = f();
    let _ = file.unlock();
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests;
