//! The one launcher (ADR 0001, Debrief ADR 0017 §5): creates a private endpoint and token, starts the child through
//! `paddock-proc` (no console window, its own process group, a kill-on-close job object on Windows, the whole tree ended on
//! drop, an optional memory cap),
//! hands it the [`Bootstrap`] as one JSON line on stdin, keeps stdin open as its lifeline, and accepts its authenticated
//! connection (token, protocol, and the pid of the process it started). The child side is [`crate::child`].
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::error::IpcError;
use crate::frame::{FrameLimit, ProtocolVersion};
use crate::handshake::Host;
use crate::peer::{Handler, Peer, PeerOptions};
use crate::wire::Json;
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline};
use paddock_proc::{KEPT_ENV, MemoryCap, Supervised};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// What a child reads first: one JSON line on stdin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    /// The host's endpoint.
    pub endpoint: String,
    /// The per-launch token, 64 hex digits.
    pub token: String,
    /// Largest frame on the connection.
    pub max_frame: usize,
    /// Free-form role the host asks for (e.g. a plugin's `elevated` role); empty for the default.
    #[serde(default)]
    pub role: String,
    /// What the app passes on top (Debrief: the child's sandbox), opaque here.
    #[serde(default)]
    pub extra: Option<Json>,
}

/// How to start one child.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    /// The executable.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Environment variables added for it.
    pub envs: Vec<(OsString, OsString)>,
    /// Start it with an empty environment plus [`KEPT_ENV`] and `envs` (default), or inherit ours.
    pub clean_env: bool,
    /// Start it at lower priority.
    pub lower_priority: bool,
    /// Its working directory (default: inherited).
    pub current_dir: Option<PathBuf>,
    /// End its process tree when it uses more than this many bytes (proportional memory, `paddock-proc`).
    pub memory_limit: Option<u64>,
    /// Windows: also make `memory_limit` a hard commit limit of the job object (see `MemoryCap::hard_commit`); ignored elsewhere.
    pub memory_hard: bool,
    /// Pass its stderr through to ours (default: discarded; an app that keeps child logs captures it itself).
    pub inherit_stderr: bool,
    /// The frame limit of the connection.
    pub limit: FrameLimit,
    /// The protocol this host serves; a child speaking another one is refused with what to update.
    pub protocol: ProtocolVersion,
    /// Names this host in version refusals ("Debrief").
    pub host_name: String,
    /// How long the child has to connect.
    pub start_timeout: Duration,
    /// Passed in the bootstrap.
    pub role: String,
    /// Passed in the bootstrap.
    pub extra: Option<Json>,
    /// How the connection runs (label, concurrency, pushes).
    pub peer: PeerOptions,
}

impl ChildSpec {
    /// `program` with no arguments, a clean environment, the default frame limit, an unversioned protocol and 30 s to
    /// connect.
    pub fn new(program: impl Into<PathBuf>, label: impl Into<String>) -> ChildSpec {
        ChildSpec {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
            clean_env: true,
            lower_priority: false,
            current_dir: None,
            memory_limit: None,
            memory_hard: false,
            inherit_stderr: false,
            limit: FrameLimit::default(),
            protocol: ProtocolVersion::UNVERSIONED,
            host_name: "app".into(),
            start_timeout: Duration::from_secs(30),
            role: String::new(),
            extra: None,
            peer: PeerOptions::new(label),
        }
    }

    /// Adds an argument.
    pub fn arg(mut self, a: impl Into<OsString>) -> ChildSpec {
        self.args.push(a.into());
        self
    }

    /// Adds an environment variable.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> ChildSpec {
        self.envs.push((key.into(), value.into()));
        self
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A started child with its authenticated connection. Its whole process tree ends on drop.
pub struct Child {
    process: Mutex<Supervised>,
    stdin: Mutex<Option<ChildStdin>>,
    status: Mutex<Option<ExitStatus>>,
    /// The clock `exit_status` waits on (the spec's peer clock).
    clock: Arc<dyn Clock>,
    pid: u32,
    peer: Peer,
    /// Kept so the private socket folder lives as long as the connection.
    _host: Host,
}

impl Child {
    /// The connection.
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Whether it runs in a kill-on-close job object (Windows: it and every helper it starts end when this app's handle
    /// closes, also on a crash; elsewhere its process group plays that part).
    pub fn in_job(&self) -> bool {
        lock(&self.process).in_job()
    }

    /// Whether the process has exited.
    pub fn exited(&self) -> bool {
        !lock(&self.process).is_running()
    }

    /// How it ended: waits up to `wait` for the OS to report the exit, then returns the status (`None` while it still runs,
    /// or when its tree was already ended by [`Child::kill`] / [`Child::stop`], which reap it).
    pub fn exit_status(&self, wait: Duration) -> Option<ExitStatus> {
        let deadline = Deadline::after(self.clock.clone(), wait);
        // The process lock is held for one look at a time, never across a wait, so `kill` and `stop` are not held up.
        while lock(&self.process).is_running() {
            if deadline.passed() {
                return None;
            }
            // silent: Err(Cancelled) is the wake-up; the loop re-reads the process and the deadline right after
            #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
            let _ = self
                .clock
                .wait(Duration::from_millis(10), &CancelToken::new());
        }
        let mut status = lock(&self.status);
        if status.is_none() {
            *status = lock(&self.process).wait();
        }
        *status
    }

    /// The most memory its watcher saw, in bytes (0 without a memory limit).
    pub fn memory_peak(&self) -> u64 {
        lock(&self.process).memory_peak()
    }

    /// The memory limit it was ended over, if it was.
    pub fn stopped_over_memory(&self) -> Option<u64> {
        lock(&self.process).stopped_over_memory()
    }

    /// Closes its lifeline (stdin): a well-behaved child exits.
    pub fn close_stdin(&self) {
        lock(&self.stdin).take();
    }

    /// Closes the connection and the lifeline, gives it `grace` to exit, then ends its tree. Idempotent.
    pub fn stop(&self, grace: Duration) {
        self.peer.close();
        self.close_stdin();
        let end = Instant::now().checked_add(grace);
        let mut process = lock(&self.process);
        while process.is_running() && end.is_some_and(|e| Instant::now() < e) {
            std::thread::sleep(Duration::from_millis(10));
        }
        process.kill();
    }

    /// Ends its tree now.
    pub fn kill(&self) {
        self.peer.close();
        lock(&self.process).kill();
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The launch error for a bootstrap line that could not be written. A closed pipe means the child is already gone (a child
/// that exits at once may do so before the line is written, or after it): that is the same outcome as one that exits
/// before it connects, so both read alike and the OS error stays in the message.
fn bootstrap_failure(label: &str, e: &io::Error) -> IpcError {
    if e.kind() == io::ErrorKind::BrokenPipe {
        IpcError::Spawn(format!("{label} exited before it connected ({e})"))
    } else {
        IpcError::Spawn(format!("{label} did not take its bootstrap ({e})"))
    }
}

/// Starts the child of `spec`, writes its bootstrap, and waits up to `spec.start_timeout` for it to connect (giving up
/// at once if it exits). On any failure the child's tree is ended.
pub fn launch(spec: &ChildSpec, handler: Arc<dyn Handler>) -> Result<Child, IpcError> {
    let mut host = Host::listen(spec.limit, spec.protocol)?
        .named(spec.host_name.clone())
        .same_account_only();
    let mut cmd = paddock_proc::command(&spec.program);
    if spec.clean_env {
        paddock_proc::clean_env(&mut cmd, KEPT_ENV);
    }
    if spec.lower_priority {
        paddock_proc::lower_priority(&mut cmd);
    }
    cmd.args(&spec.args)
        .envs(spec.envs.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(if spec.inherit_stderr {
            Stdio::inherit()
        } else {
            Stdio::null()
        });
    if let Some(dir) = &spec.current_dir {
        cmd.current_dir(dir);
    }
    let spawned = match spec.memory_limit {
        Some(limit) => {
            let cap = MemoryCap::new(limit, spec.peer.label.clone());
            Supervised::spawn_capped(
                cmd,
                if spec.memory_hard {
                    cap.hard_commit()
                } else {
                    cap
                },
            )
        }
        None => Supervised::spawn(cmd),
    };
    let mut process = spawned
        .map_err(|e| IpcError::Spawn(format!("{} could not be started ({e})", spec.peer.label)))?;
    if spec.lower_priority {
        process.lower_priority();
    }
    let pid = process
        .id()
        .ok_or_else(|| IpcError::Spawn(format!("{} exited at once", spec.peer.label)))?;
    let mut stdin = process.take_stdin();
    let bootstrap = Bootstrap {
        endpoint: host.endpoint().to_string(),
        token: host.token().to_hex(),
        max_frame: host.limit().get(),
        role: spec.role.clone(),
        extra: spec.extra.clone(),
    };
    let mut line = serde_json::to_string(&bootstrap).map_err(|e| IpcError::Spawn(e.to_string()))?;
    line.push('\n');
    let label = spec.peer.label.clone();
    let no_stdin = || Err(io::Error::from(io::ErrorKind::BrokenPipe));
    let written = stdin.as_mut().map_or_else(no_stdin, |s| {
        s.write_all(line.as_bytes()).and_then(|()| s.flush())
    });
    // On failure `process` is dropped, which ends its tree.
    written.map_err(|e| bootstrap_failure(&label, &e))?;
    let conn = host.accept_while(Some(pid), spec.start_timeout, &mut || {
        if process.is_running() {
            Ok(())
        } else {
            Err(IpcError::Spawn(format!(
                "{label} exited before it connected"
            )))
        }
    })?;
    let peer = Peer::start(conn, handler, spec.peer.clone())?;
    Ok(Child {
        process: Mutex::new(process),
        stdin: Mutex::new(stdin),
        status: Mutex::new(None),
        clock: spec.peer.clock.clone(),
        pid,
        peer,
        _host: host,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(e: IpcError) -> String {
        match e {
            IpcError::Spawn(m) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_child_that_exits_before_the_bootstrap_reads_like_one_that_exits_before_it_connects() {
        let pipe = io::Error::from(io::ErrorKind::BrokenPipe);
        let m = message(bootstrap_failure("plugin \"exit\"", &pipe));
        assert!(m.contains("exited before it connected"), "{m}");
        assert!(
            m.contains(&pipe.to_string()),
            "the OS error stays in the message: {m}"
        );
    }

    #[test]
    fn another_write_error_is_not_called_an_exit() {
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let m = message(bootstrap_failure("p", &denied));
        assert!(m.contains("did not take its bootstrap"), "{m}");
        assert!(!m.contains("exited"), "{m}");
    }
}
