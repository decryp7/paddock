//! Starting a child process that connects back, and the child's side of that.
//!
//! The host passes the endpoint name and token on the child's **stdin** as one JSON line ([`Bootstrap`]) and keeps
//! stdin open: it is the child's lifeline, and the child exits when it closes ([`ChildLink::lifeline`]). The child
//! never gets a console window on Windows and runs in a job object with kill-on-close and an optional memory cap.
//! On Unix there is no per-process memory cap (macOS does not enforce `RLIMIT_AS`); the lifeline still holds.

use crate::error::IpcError;
use crate::frame::FrameLimit;
use crate::host::{Client, Connection, Host};
use crate::peer::{Peer, PeerOptions};
use crate::token::Token;
use crate::wire::PROTOCOL_VERSION;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// The largest bootstrap line a child reads.
const MAX_BOOTSTRAP: u64 = 64 * 1024;

/// What a child needs to connect back; one JSON line on its stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bootstrap {
    /// The endpoint name.
    pub endpoint: String,
    /// The token, as 64 hex digits.
    pub token: String,
    /// The frame limit in bytes.
    pub max_frame: usize,
}

/// How to start a child.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    /// The executable.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Extra environment variables (the rest is inherited).
    pub envs: Vec<(OsString, OsString)>,
    /// Its working directory (default: inherited).
    pub current_dir: Option<PathBuf>,
    /// A cap on its committed memory in bytes (Windows; ignored elsewhere).
    pub memory_limit: Option<u64>,
    /// Pass its stderr through to ours (default: discarded).
    pub inherit_stderr: bool,
    /// The frame limit of the connection.
    pub limit: FrameLimit,
}

impl ChildSpec {
    /// Runs `program` with no arguments, no memory cap, stderr discarded and the default frame limit.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        ChildSpec {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
            current_dir: None,
            memory_limit: None,
            inherit_stderr: false,
            limit: FrameLimit::default(),
        }
    }
}

/// A running child. Dropping it kills the child if it is still running.
pub struct ChildProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    #[cfg(windows)]
    _job: crate::job::Job,
}

impl ChildProcess {
    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Its exit status, if it has exited.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Waits up to `timeout` for it to exit.
    pub fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Closes its stdin, the lifeline: a well-behaved child exits.
    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }

    /// Kills it and reaps it. Fine if it already exited.
    pub fn kill(&mut self) -> io::Result<ExitStatus> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(status);
        }
        // It may exit between the check and the kill; the wait below settles it either way.
        let _ = self.child.kill();
        self.child.wait()
    }

    /// Closes the lifeline, gives it `grace` to exit, then kills it.
    pub fn stop(&mut self, grace: Duration) -> io::Result<ExitStatus> {
        self.close_stdin();
        match self.wait_timeout(grace)? {
            Some(status) => Ok(status),
            None => self.kill(),
        }
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

/// Starts the child described by `spec` and hands it `host`'s endpoint and token on stdin.
pub fn spawn(spec: &ChildSpec, host: &Host) -> io::Result<ChildProcess> {
    let mut process = start_process(spec)?;
    // On failure `process` is dropped, which kills the child.
    send_bootstrap(&mut process, host).map(|_| process)
}

/// Starts the child without telling it anything yet.
fn start_process(spec: &ChildSpec) -> io::Result<ChildProcess> {
    // dyno-ipc sits below dyno-platform and applies its rules itself: no console window (below), and the child is
    // bounded by the caller's start timeout, the lifeline and (Windows) the job.
    #[allow(clippy::disallowed_methods)]
    let mut cmd = std::process::Command::new(&spec.program);
    cmd.args(&spec.args)
        .envs(spec.envs.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(if spec.inherit_stderr { Stdio::inherit() } else { Stdio::null() });
    if let Some(dir) = &spec.current_dir {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    #[cfg(windows)]
    let job = match crate::job::Job::assign(&child, spec.memory_limit) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(e.kind(), format!("cannot put the child in a job object: {e}")));
        }
    };
    let stdin = child.stdin.take();
    Ok(ChildProcess {
        child,
        stdin,
        #[cfg(windows)]
        _job: job,
    })
}

/// Writes the bootstrap line to the child's stdin.
fn send_bootstrap(process: &mut ChildProcess, host: &Host) -> io::Result<()> {
    let bootstrap = Bootstrap {
        endpoint: host.name().to_string(),
        token: host.token().to_hex(),
        max_frame: host.limit().get(),
    };
    let mut line = serde_json::to_string(&bootstrap).map_err(io::Error::other)?;
    line.push('\n');
    match process.stdin.as_mut() {
        Some(stdin) => stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()),
        None => Err(io::Error::other("the child has no stdin")),
    }
}

/// Listens, starts the child, waits up to `start_timeout` for it to connect (giving up early if it exits) and starts
/// a [`Peer`] on the connection. On any failure the child is killed.
pub fn launch<B>(spec: &ChildSpec, opts: PeerOptions<B>, start_timeout: Duration) -> Result<(ChildProcess, Peer<B>), IpcError>
where
    B: Serialize + DeserializeOwned + Send + 'static,
{
    launch_after(spec, opts, start_timeout, |_| {})
}

/// [`launch`], calling `after_spawn` once the child runs and before it is told how to connect (tests use it to let a
/// child exit first, which is the race a fast-failing plugin runs into).
fn launch_after<B>(
    spec: &ChildSpec,
    opts: PeerOptions<B>,
    start_timeout: Duration,
    after_spawn: impl FnOnce(&mut ChildProcess),
) -> Result<(ChildProcess, Peer<B>), IpcError>
where
    B: Serialize + DeserializeOwned + Send + 'static,
{
    let mut host = Host::listen_with(spec.limit)?;
    let mut child = start_process(spec)?;
    after_spawn(&mut child);
    match send_bootstrap(&mut child, &host) {
        Ok(()) => {}
        // A child that has already exited (or closed stdin) cannot be told anything: not an IPC failure of its own.
        // The wait below reports it as "exited before it connected" with its status, the same as a child that exits a
        // moment later, instead of racing the write against the exit.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {}
        Err(e) => return Err(e.into()),
    }
    let pid = child.pid();
    let conn = host.accept_inner(Some(pid), Instant::now() + start_timeout, &mut || match child.try_wait() {
        Ok(Some(status)) => Err(IpcError::io(
            io::ErrorKind::BrokenPipe,
            format!("the process exited ({status}) before it connected"),
        )),
        _ => Ok(()),
    })?;
    let peer = Peer::start(conn, opts)?;
    Ok((child, peer))
}

/// The child's side: read the bootstrap, connect, keep the lifeline.
pub struct ChildLink;

impl ChildLink {
    /// Reads the bootstrap line from `input` (at most 64 KiB).
    pub fn read_bootstrap(input: impl BufRead) -> io::Result<Bootstrap> {
        let mut line = String::new();
        input.take(MAX_BOOTSTRAP).read_line(&mut line)?;
        serde_json::from_str(line.trim()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad bootstrap line: {e}")))
    }

    /// Connects to the host named in `bootstrap`.
    pub fn connect(bootstrap: &Bootstrap) -> Result<Connection, IpcError> {
        let token = Token::from_hex(&bootstrap.token).ok_or_else(|| IpcError::Auth("the bootstrap token is not 64 hex digits".into()))?;
        Client::connect_with(&bootstrap.endpoint, &token, FrameLimit::new(bootstrap.max_frame), PROTOCOL_VERSION)
    }

    /// The usual child start: the bootstrap from stdin, connect, then [`ChildLink::lifeline`].
    pub fn from_stdin() -> Result<Connection, IpcError> {
        let bootstrap = ChildLink::read_bootstrap(io::stdin().lock())?;
        let conn = ChildLink::connect(&bootstrap)?;
        ChildLink::lifeline();
        Ok(conn)
    }

    /// Exits the process as soon as stdin closes (the host stopped us or died).
    pub fn lifeline() {
        std::thread::spawn(|| {
            let mut sink = [0u8; 256];
            while matches!(io::stdin().read(&mut sink), Ok(n) if n > 0) {}
            std::process::exit(0);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bootstrap_line_roundtrips_and_bad_lines_are_refused() {
        let b = Bootstrap {
            endpoint: "e".into(),
            token: "t".into(),
            max_frame: 1024,
        };
        let text = format!("{}\nrest", serde_json::to_string(&b).unwrap());
        assert_eq!(ChildLink::read_bootstrap(text.as_bytes()).unwrap(), b);
        assert_eq!(
            ChildLink::read_bootstrap(&b"nope\n"[..]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let long = format!("{{\"endpoint\":\"{}\"}}\n", "a".repeat(70_000));
        assert!(ChildLink::read_bootstrap(long.as_bytes()).is_err());
        assert!(matches!(ChildLink::connect(&b), Err(IpcError::Auth(_))));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_exited_before_the_bootstrap_is_reported_as_exited_before_it_connected() {
        let mut spec = ChildSpec::new("/bin/sh");
        spec.args = vec!["-c".into(), "exit 7".into()];
        let opts: PeerOptions<String> = PeerOptions::new();
        let err = launch_after(&spec, opts, Duration::from_secs(20), |child| {
            // Wait on the condition (the child is gone), so the bootstrap write meets a closed pipe every time.
            assert!(child.wait_timeout(Duration::from_secs(20)).unwrap().is_some());
        })
        .map(|_| ())
        .unwrap_err();
        assert!(err.to_string().contains("before it connected"), "{err}");
    }

    #[test]
    fn a_program_that_does_not_exist_cannot_be_started() {
        let host = Host::listen().unwrap();
        assert!(spawn(&ChildSpec::new("/no/such/dyno-program"), &host).is_err());
        let opts: PeerOptions<String> = PeerOptions::new();
        assert!(launch(&ChildSpec::new("/no/such/dyno-program"), opts, Duration::from_secs(1)).is_err());
    }
}
