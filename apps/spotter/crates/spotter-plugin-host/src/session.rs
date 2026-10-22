//! One guarded conversation with a plugin process: start it, check its handshake, send requests one at a time with
//! a deadline each, watch its memory, and stop it. Anything a plugin can do wrong (not start, crash, hang, flood,
//! speak garbage, use too much memory) ends as a [`Fault`], never as a panic or a hung caller.

use crate::process;
use spotter_plugin_api::frame::{read_message, write_message, FrameError, FRAME_VERSION};
use spotter_plugin_api::protocol::{Message, MAGIC};
use spotter_plugin_api::{Request, Response, PROTOCOL};
use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Limits of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How long the plugin has to start and say hello.
    pub handshake: Duration,
    /// How long one request may take.
    pub request: Duration,
    /// Most resident memory, in bytes.
    pub memory_bytes: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            handshake: Duration::from_secs(10),
            request: Duration::from_secs(60),
            memory_bytes: 512 << 20,
        }
    }
}

/// How a session failed. Every fault is the plugin's (a strike towards quarantine), except [`Fault::Start`] when the
/// file is simply missing, which the caller reports as an installation problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The program could not be started.
    Start(String),
    /// It refused or failed the handshake, or speaks another protocol version.
    Handshake(String),
    /// It exited or closed its output before answering.
    Crashed(String),
    /// It did not answer in time and was stopped.
    TimedOut(Duration),
    /// It used more memory than allowed and was stopped.
    Memory(u64),
    /// It sent something that is not a valid answer.
    Protocol(String),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Start(e) => write!(f, "could not be started: {e}"),
            Fault::Handshake(e) => write!(f, "{e}"),
            Fault::Crashed(e) => write!(f, "crashed: {e}"),
            Fault::TimedOut(d) => write!(f, "did not answer within {} seconds and was stopped", d.as_secs()),
            Fault::Memory(limit) => write!(f, "used more than {} MiB of memory and was stopped", limit >> 20),
            Fault::Protocol(e) => write!(f, "sent an invalid answer: {e}"),
        }
    }
}

/// How much of a plugin's standard error is kept for messages.
const STDERR_TAIL: usize = 4096;
/// How often memory is checked while waiting.
const POLL: Duration = Duration::from_millis(100);

struct Running {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Result<Message, FrameError>>,
    stderr: Receiver<String>,
    system: System,
    pid: Pid,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs `requests` against the plugin at `exe`, returning one response per request.
pub fn run(exe: &Path, name: &str, requests: &[Request], limits: Limits) -> Result<Vec<Response>, Fault> {
    let mut child = process::command(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Fault::Start(e.to_string()))?;
    let (Some(stdin), Some(mut stdout), Some(mut stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Err(Fault::Start("its pipes could not be opened".into()));
    };
    let (msg_tx, messages) = channel();
    std::thread::spawn(move || loop {
        let m = read_message(&mut stdout);
        let end = m.is_err();
        if msg_tx.send(m).is_err() || end {
            break;
        }
    });
    let (err_tx, stderr_rx) = channel();
    std::thread::spawn(move || {
        let mut tail: Vec<u8> = Vec::new();
        let mut buf = [0u8; 1024];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > STDERR_TAIL {
                tail.drain(..tail.len() - STDERR_TAIL);
            }
        }
        let _ = err_tx.send(String::from_utf8_lossy(&tail).trim().to_string());
    });
    let pid = Pid::from_u32(child.id());
    let mut s = Running {
        child,
        stdin,
        messages,
        stderr: stderr_rx,
        system: System::new(),
        pid,
    };
    match s.next(limits.handshake, limits.memory_bytes)? {
        Message::Hello { magic, frame, protocol } if magic == MAGIC && frame == FRAME_VERSION => {
            if let Some(reason) = PROTOCOL.mismatch(protocol, name) {
                let _ = write_message(&mut s.stdin, &Message::Refused { reason: reason.clone() });
                return Err(Fault::Handshake(reason));
            }
        }
        _ => {
            return Err(Fault::Handshake(
                "is not a Spotter plugin (it did not start with the plugin handshake)".into(),
            ))
        }
    }
    s.send(&Message::Welcome { protocol: PROTOCOL })?;
    let mut out = Vec::with_capacity(requests.len());
    for (i, request) in requests.iter().enumerate() {
        let id = i as u32 + 1;
        s.send(&Message::Request {
            id,
            request: request.clone(),
        })?;
        match s.next(limits.request, limits.memory_bytes)? {
            Message::Response { id: got, response } if got == id => out.push(response),
            other => {
                return Err(Fault::Protocol(format!(
                    "expected the answer to request {id}, got {}",
                    describe(&other)
                )))
            }
        }
    }
    // A polite goodbye; the drop kills whatever is left.
    let _ = s.send(&Message::Request {
        id: u32::MAX,
        request: Request::Shutdown,
    });
    let _ = s.next(Duration::from_millis(500), limits.memory_bytes);
    Ok(out)
}

fn describe(m: &Message) -> &'static str {
    match m {
        Message::Hello { .. } => "a hello",
        Message::Welcome { .. } => "a welcome",
        Message::Refused { .. } => "a refusal",
        Message::Request { .. } => "a request",
        Message::Response { .. } => "an answer to another request",
    }
}

impl Running {
    fn send(&mut self, m: &Message) -> Result<(), Fault> {
        write_message(&mut self.stdin, m).map_err(|e| self.crashed(&e.to_string()))
    }

    fn next(&mut self, timeout: Duration, memory_limit: u64) -> Result<Message, Fault> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Fault::TimedOut(timeout));
            }
            match self.messages.recv_timeout(left.min(POLL)) {
                Ok(Ok(m)) => return Ok(m),
                Ok(Err(FrameError::Invalid(e))) => return Err(Fault::Protocol(e)),
                Ok(Err(FrameError::TooLarge(n))) => return Err(Fault::Protocol(format!("a message of {n} bytes is too large"))),
                Ok(Err(e)) => return Err(self.crashed(&e.to_string())),
                Err(RecvTimeoutError::Disconnected) => return Err(self.crashed("its output closed")),
                Err(RecvTimeoutError::Timeout) => {
                    self.system.refresh_processes_specifics(
                        ProcessesToUpdate::Some(&[self.pid]),
                        true,
                        ProcessRefreshKind::nothing().with_memory(),
                    );
                    if self.system.process(self.pid).is_some_and(|p| p.memory() > memory_limit) {
                        return Err(Fault::Memory(memory_limit));
                    }
                }
            }
        }
    }

    /// A crash, with the exit status and the end of standard error when they are available.
    fn crashed(&mut self, what: &str) -> Fault {
        let status = (0..20)
            .find_map(|_| match self.child.try_wait() {
                Ok(Some(s)) => Some(s.to_string()),
                _ => {
                    std::thread::sleep(Duration::from_millis(25));
                    None
                }
            })
            .unwrap_or_else(|| "still running".into());
        let tail = self.stderr.recv_timeout(Duration::from_millis(500)).unwrap_or_default();
        let mut msg = format!("{what} ({status})");
        if !tail.is_empty() {
            msg.push_str(": ");
            msg.push_str(tail.lines().last().unwrap_or(""));
        }
        Fault::Crashed(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_describe_themselves() {
        for f in [
            Fault::Start("x".into()),
            Fault::Handshake("x".into()),
            Fault::Crashed("x".into()),
            Fault::TimedOut(Duration::from_secs(3)),
            Fault::Memory(512 << 20),
            Fault::Protocol("x".into()),
        ] {
            assert!(!f.to_string().is_empty());
        }
        assert!(Fault::Memory(512 << 20).to_string().contains("512 MiB"));
        assert_eq!(Limits::default().memory_bytes, 512 << 20);
        let names = [
            describe(&Message::Hello {
                magic: MAGIC,
                frame: 1,
                protocol: PROTOCOL,
            }),
            describe(&Message::Welcome { protocol: PROTOCOL }),
            describe(&Message::Refused { reason: String::new() }),
            describe(&Message::Request {
                id: 1,
                request: Request::Describe,
            }),
            describe(&Message::Response {
                id: 1,
                response: Response::Bye,
            }),
        ];
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn a_missing_program_cannot_start() {
        let e = run(Path::new("/no/such/plugin"), "x", &[Request::Describe], Limits::default()).unwrap_err();
        assert!(matches!(e, Fault::Start(_)));
    }
}
