//! A long-lived child spoken to over its standard input and output: the bounded form of a request-and-answer program such as
//! `git cat-file --batch`, which answers one request per line for as long as its input stays open. [`crate::run_bounded`] closes
//! stdin and waits for the end; a [`Session`] keeps stdin open, bounds every read by its own deadline and every answer by a size
//! cap, keeps the last of the child's error output for messages (ER-27), and ends the child's whole tree when it is finished or
//! dropped (rule 53: by the identity taken at the start, never a pid read later).
//!
//! ```text
//! # use std::time::Duration;
//! # fn demo() -> Result<(), paddock_proc::SessionError> {
//! let mut cmd = paddock_proc::command("git");
//! cmd.args(["cat-file", "--batch"]);
//! let mut s = paddock_proc::Session::start(cmd, paddock_proc::SessionLimits::new(Duration::from_secs(30)))?;
//! s.send(b"HEAD\n")?;
//! let header = s.read_line()?;
//! # let _ = header;
//! s.finish();
//! # Ok(()) }
//! ```

use crate::identity::ProcessId;
use crate::run::{RunError, program_name};
use crate::tree::kill_tree;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Most bytes of the child's error output kept for messages.
const STDERR_KEPT: usize = 16 * 1024;
/// How many chunks of the child's output (each at most [`CHUNK`]) wait between the thread that reads its pipe and the caller: a
/// child that writes faster than the caller reads is then held by its pipe, never buffered without bound (1 MiB at most here).
pub(crate) const QUEUED_CHUNKS: usize = 16;
/// The most one read of the child's pipe takes.
const CHUNK: usize = 64 * 1024;

/// What the pipe-reading thread did, for the tests: chunks sent and taken, the most queued at once, and whether it found the
/// queue full. Two running totals, never one shared count: the channel hands a chunk to the caller before the thread can count
/// it, so a caller may take a chunk the thread has not counted yet (run 8327: a single count went below zero, overflowed, and its
/// panic closed the channel under the caller).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct QueueProbe {
    sent: std::sync::atomic::AtomicUsize,
    taken: std::sync::atomic::AtomicUsize,
    pub(crate) peak: std::sync::atomic::AtomicUsize,
    pub(crate) blocked: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl QueueProbe {
    /// The reading thread queued a chunk. The peak is exact while the caller does not read (the cap test); while it reads, a
    /// chunk it has taken but not yet counted is still counted as queued, so the peak may be one over.
    fn sent(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        let sent = self.sent.fetch_add(1, SeqCst) + 1;
        let now = sent.saturating_sub(self.taken.load(SeqCst));
        self.peak.fetch_max(now, SeqCst);
    }

    /// The caller took a chunk.
    fn taken(&self) {
        self.taken.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Chunks counted as sent and not yet as taken.
    fn queued(&self) -> usize {
        use std::sync::atomic::Ordering::SeqCst;
        self.sent
            .load(SeqCst)
            .saturating_sub(self.taken.load(SeqCst))
    }
}

/// How long [`Session::finish`] waits for the child to end on its own once its input is closed, before it ends the tree.
pub const FINISH_GRACE: Duration = Duration::from_secs(2);

/// How far a session may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLimits {
    /// The longest one read may wait for the child (a line, or the bytes of one answer).
    pub read_timeout: Duration,
    /// The most bytes one read may ask for or one line may hold.
    pub max_answer: usize,
}

impl SessionLimits {
    /// A read timeout with a 256 MiB answer cap.
    pub fn new(read_timeout: Duration) -> SessionLimits {
        SessionLimits {
            read_timeout,
            max_answer: 256 * 1024 * 1024,
        }
    }

    /// With another answer cap.
    pub fn max_answer(mut self, bytes: usize) -> SessionLimits {
        self.max_answer = bytes;
        self
    }
}

/// Why a session's request or read failed. Each names the program and, when the child said something, its last words.
#[derive(Debug)]
pub enum SessionError {
    /// The program could not be started.
    Start(RunError),
    /// A read did not complete within [`SessionLimits::read_timeout`]; the session is ended.
    TimedOut {
        /// The program's file name.
        program: String,
        /// What was being read.
        what: &'static str,
        /// The last of its error output.
        stderr: String,
    },
    /// The child ended (it closed its output, or its input before a request was written) before the answer was complete.
    Closed {
        /// The program's file name.
        program: String,
        /// The last of its error output.
        stderr: String,
    },
    /// An answer is larger than [`SessionLimits::max_answer`]; the session is ended.
    TooLarge {
        /// The program's file name.
        program: String,
        /// The size asked for or reached.
        bytes: usize,
    },
    /// Writing the request failed for another reason than the child's end (that is [`SessionError::Closed`]).
    Write(String, std::io::Error),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tail = |s: &str| {
            if s.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", s.trim())
            }
        };
        match self {
            SessionError::Start(e) => write!(f, "{e}"),
            SessionError::TimedOut {
                program,
                what,
                stderr,
            } => {
                write!(
                    f,
                    "{program} did not answer ({what}) in time and was ended{}",
                    tail(stderr)
                )
            }
            SessionError::Closed { program, stderr } => write!(
                f,
                "{program} ended before its answer was complete{}",
                tail(stderr)
            ),
            SessionError::TooLarge { program, bytes } => {
                write!(f, "{program}'s answer of {bytes} bytes is over the limit")
            }
            SessionError::Write(p, e) => write!(f, "a request to {p} could not be written: {e}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SessionError::Start(e) => Some(e),
            SessionError::Write(_, e) => Some(e),
            _ => None,
        }
    }
}

/// A started child with its input open (see the module).
pub struct Session {
    name: String,
    child: Child,
    root: Option<ProcessId>,
    stdin: Option<ChildStdin>,
    chunks: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    eof: bool,
    stderr: Arc<Mutex<Vec<u8>>>,
    /// Disconnected once the thread that reads the child's error output has read it to the end.
    stderr_read: Receiver<()>,
    limits: SessionLimits,
    ended: bool,
    #[cfg(test)]
    probe: Arc<QueueProbe>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("program", &self.name)
            .field("ended", &self.ended)
            .finish()
    }
}

impl Session {
    /// Starts `cmd` with stdin, stdout and stderr piped, as the leader of its own process group on Unix.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    pub fn start(mut cmd: Command, limits: SessionLimits) -> Result<Session, SessionError> {
        let name = program_name(&cmd);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::sys::own_group(&mut cmd);
        let mut child = crate::command::spawn_retrying(&mut cmd).map_err(|e| {
            SessionError::Start(match e.kind() {
                std::io::ErrorKind::NotFound => RunError::NotFound(name.clone()),
                std::io::ErrorKind::PermissionDenied => RunError::PermissionDenied(name.clone()),
                _ => RunError::Spawn(name.clone(), e),
            })
        })?;
        let root = ProcessId::of(child.id());
        let stdin = child.stdin.take();
        let (tx, chunks) = mpsc::sync_channel::<Vec<u8>>(QUEUED_CHUNKS);
        #[cfg(test)]
        let probe = Arc::new(QueueProbe::default());
        if let Some(mut out) = child.stdout.take() {
            #[cfg(test)]
            let probe = probe.clone();
            std::thread::spawn(move || {
                let mut chunk = vec![0u8; CHUNK];
                loop {
                    match out.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let sent = match tx.try_send(chunk[..n].to_vec()) {
                                Ok(()) => Ok(()),
                                Err(mpsc::TrySendError::Full(c)) => {
                                    #[cfg(test)]
                                    probe
                                        .blocked
                                        .store(true, std::sync::atomic::Ordering::SeqCst);
                                    // The caller has not taken the queued chunks yet: wait for room, which holds the child at
                                    // its pipe.
                                    tx.send(c).map_err(|_| ())
                                }
                                Err(mpsc::TrySendError::Disconnected(_)) => Err(()),
                            };
                            if sent.is_err() {
                                break;
                            }
                            #[cfg(test)]
                            probe.sent();
                        }
                    }
                }
            });
        }
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let (stderr_end, stderr_read) = mpsc::channel::<()>();
        if let Some(mut err) = child.stderr.take() {
            let kept = stderr.clone();
            std::thread::spawn(move || {
                // Dropped when the thread ends: the caller's `stderr_read` then reads Disconnected.
                let _end = stderr_end;
                let mut chunk = [0u8; 4096];
                while let Ok(n) = err.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    let mut k = kept.lock().unwrap_or_else(|p| p.into_inner());
                    k.extend_from_slice(&chunk[..n]);
                    let over = k.len().saturating_sub(STDERR_KEPT);
                    k.drain(..over);
                }
            });
        }
        Ok(Session {
            name,
            child,
            root,
            stdin,
            chunks,
            buf: Vec::new(),
            eof: false,
            stderr,
            stderr_read,
            limits,
            ended: false,
            #[cfg(test)]
            probe,
        })
    }

    /// The last of the child's error output (lossy UTF-8).
    pub fn stderr_tail(&self) -> String {
        String::from_utf8_lossy(&self.stderr.lock().unwrap_or_else(|p| p.into_inner())).into_owned()
    }

    /// Writes `request` and flushes it.
    ///
    /// # Errors
    ///
    /// [`SessionError::Closed`] with its last words when the child has ended; [`SessionError::Write`] when the write fails otherwise.
    pub fn send(&mut self, request: &[u8]) -> Result<(), SessionError> {
        let Some(input) = self.stdin.as_mut() else {
            return Err(SessionError::Write(
                self.name.clone(),
                std::io::Error::other("the input is closed"),
            ));
        };
        match input.write_all(request).and_then(|()| input.flush()) {
            Ok(()) => Ok(()),
            // The child closed its input because it ended (run 8329: git outside a repository ends before the first request):
            // that is `Closed` with what it said, as a read would have found, never a bare "Broken pipe".
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Err(SessionError::Closed {
                program: self.name.clone(),
                stderr: self.last_words(),
            }),
            Err(e) => Err(SessionError::Write(self.name.clone(), e)),
        }
    }

    /// Ends the child (it has closed its input or output) and returns all of its error output: the thread that reads it is given
    /// up to the read timeout to reach the end, so a message the child wrote just before it ended is not lost to the race.
    fn last_words(&mut self) -> String {
        self.end();
        // silent: Disconnected is the thread's end; Timeout means a process outside the tree still holds the pipe, and the
        // words read so far are what there is
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.stderr_read.recv_timeout(self.limits.read_timeout);
        self.stderr_tail()
    }

    /// Waits until `ready(buf)` says how many buffered bytes make the answer, or the deadline passes.
    fn fill(
        &mut self,
        what: &'static str,
        ready: impl Fn(&[u8]) -> Option<usize>,
    ) -> Result<Vec<u8>, SessionError> {
        let deadline = Instant::now() + self.limits.read_timeout;
        loop {
            if let Some(n) = ready(&self.buf) {
                return Ok(self.buf.drain(..n).collect());
            }
            if self.buf.len() > self.limits.max_answer {
                let bytes = self.buf.len();
                self.end();
                return Err(SessionError::TooLarge {
                    program: self.name.clone(),
                    bytes,
                });
            }
            if self.eof {
                return Err(SessionError::Closed {
                    program: self.name.clone(),
                    stderr: self.last_words(),
                });
            }
            match self
                .chunks
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(c) => {
                    #[cfg(test)]
                    self.probe.taken();
                    self.buf.extend_from_slice(&c);
                }
                Err(RecvTimeoutError::Disconnected) => self.eof = true,
                Err(RecvTimeoutError::Timeout) => {
                    self.end();
                    return Err(SessionError::TimedOut {
                        program: self.name.clone(),
                        what,
                        stderr: self.stderr_tail(),
                    });
                }
            }
        }
    }

    /// The next line of the child's output, without its `\n`.
    ///
    /// # Errors
    ///
    /// When it does not come within the read timeout, the child ended first, or the line is over the cap.
    pub fn read_line(&mut self) -> Result<Vec<u8>, SessionError> {
        let mut line = self.fill("a line", |b| {
            b.iter().position(|c| *c == b'\n').map(|i| i + 1)
        })?;
        line.pop();
        Ok(line)
    }

    /// Exactly `n` bytes of the child's output.
    ///
    /// # Errors
    ///
    /// When `n` is over the cap, they do not come within the read timeout, or the child ended first.
    pub fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, SessionError> {
        if n > self.limits.max_answer {
            self.end();
            return Err(SessionError::TooLarge {
                program: self.name.clone(),
                bytes: n,
            });
        }
        self.fill("an answer", |b| (b.len() >= n).then_some(n))
    }

    /// Ends the child at once: its tree by the identity taken at the start, then the child itself, reaped.
    fn end(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.stdin = None;
        if let Some(r) = self.root {
            kill_tree(r);
        }
        // silent: the child may have ended already; reaping is what matters here
        #[allow(clippy::let_underscore_must_use)]
        let _: std::io::Result<()> = self.child.kill();
        #[allow(clippy::let_underscore_must_use)]
        let _: std::io::Result<std::process::ExitStatus> = self.child.wait();
    }

    /// Closes the input and waits up to [`FINISH_GRACE`] for the child to end on its own, then ends its tree. Returns whether it
    /// ended on its own.
    pub fn finish(mut self) -> bool {
        self.stdin = None;
        let deadline = Instant::now() + FINISH_GRACE;
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                self.ended = true;
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.end();
        false
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testchild::{Mode, child};

    /// The answer to one request: the `CHILD:len <n>` line (libtest's banner may share the stream: lines without the prefix are
    /// skipped), then the bytes.
    fn ask(s: &mut Session, request: &str) -> Result<String, SessionError> {
        s.send(format!("{request}\n").as_bytes())?;
        let n: usize = loop {
            let line = String::from_utf8(s.read_line()?).unwrap();
            if let Some(i) = line.find("CHILD:len ") {
                break line[i + "CHILD:len ".len()..].trim().parse().unwrap();
            }
        };
        let mut body = s.read_exact(n + 1)?;
        assert_eq!(body.pop(), Some(b'\n'));
        Ok(String::from_utf8(body).unwrap())
    }

    fn limits() -> SessionLimits {
        SessionLimits::new(Duration::from_secs(60))
    }

    /// A child that writes without pause while nobody reads is held by its pipe: the thread that reads the pipe queues at most
    /// [`QUEUED_CHUNKS`] chunks and then waits, so the caller's memory stays bounded however much the child writes.
    #[test]
    fn a_child_that_writes_faster_than_it_is_read_is_held_at_the_queue_cap() {
        use std::sync::atomic::Ordering::SeqCst;
        let s = Session::start(child(Mode::Endless), limits()).unwrap();
        let probe = s.probe.clone();
        let deadline = Instant::now() + Duration::from_secs(60);
        // Either the thread finds the queue full (bounded) or it queues past the cap (unbounded); wait for one of the two.
        while !probe.blocked.load(SeqCst) && probe.peak.load(SeqCst) <= QUEUED_CHUNKS {
            assert!(
                Instant::now() < deadline,
                "the reading thread neither queued past the cap nor found the queue full in 60 s"
            );
            std::thread::yield_now();
        }
        assert!(
            probe.peak.load(SeqCst) <= QUEUED_CHUNKS,
            "{} chunks queued, over the cap of {QUEUED_CHUNKS}",
            probe.peak.load(SeqCst)
        );
        assert!(probe.blocked.load(SeqCst));
        drop(s);
    }

    /// The caller can take a chunk before the reading thread has counted it (the channel hands it over first): the count must
    /// survive that order. Run 8327 (Linux and Windows): the thread's count overflowed, it panicked, its end of the channel
    /// closed, and the caller read `Closed` with an empty stderr in the middle of an answer.
    #[test]
    fn the_queue_count_survives_the_caller_taking_a_chunk_before_the_thread_counts_it() {
        use std::sync::atomic::Ordering::SeqCst;
        let probe = QueueProbe::default();
        let counted = std::panic::catch_unwind(|| {
            probe.taken();
            probe.sent();
        });
        assert!(
            counted.is_ok(),
            "the count panicked when the caller took the chunk first"
        );
        assert_eq!(
            probe.queued(),
            0,
            "one chunk sent and taken leaves none queued"
        );
        probe.sent();
        assert!(
            probe.peak.load(SeqCst) <= 1,
            "peak {}",
            probe.peak.load(SeqCst)
        );
    }

    #[test]
    fn many_requests_go_to_one_process_and_it_ends_when_its_input_closes() {
        let mut s = Session::start(child(Mode::Answer), limits()).unwrap();
        for k in 0..50 {
            let req = format!("request {k} {}", "x".repeat(k * 7));
            assert_eq!(ask(&mut s, &req).unwrap(), req);
        }
        let big = "y".repeat(200_000);
        assert_eq!(
            ask(&mut s, &big).unwrap(),
            big,
            "an answer larger than one pipe chunk"
        );
        assert!(
            s.finish(),
            "the child ends by itself once its input is closed"
        );
    }

    /// A request to a child that has already ended is `Closed` with its last words, never a bare write error (run 8329:
    /// `git cat-file` in a folder that is no repository ended before the request was written, and the read said "Broken pipe"
    /// instead of git's "not a git repository").
    #[test]
    fn a_request_to_a_child_that_has_ended_is_closed_with_its_words() {
        let mut s = Session::start(child(Mode::Complain), limits()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !matches!(s.child.try_wait(), Ok(Some(_))) {
            assert!(Instant::now() < deadline, "the child did not end in 60 s");
            std::thread::yield_now();
        }
        let e = s.send(b"HEAD\n").unwrap_err();
        assert!(
            matches!(&e, SessionError::Closed { stderr, .. } if stderr.contains("CHILD:complaint")),
            "{e:?}"
        );
    }

    /// A child that ends before it answers is `Closed` with all it said: its error output is read to the end first.
    #[test]
    fn a_child_that_ends_before_it_answers_says_why() {
        let mut s = Session::start(child(Mode::Complain), limits()).unwrap();
        // libtest's banner may come first on the child's stdout.
        let e = loop {
            match s.read_line() {
                Ok(_) => continue,
                Err(e) => break e,
            }
        };
        assert!(
            matches!(&e, SessionError::Closed { stderr, .. } if stderr.contains("CHILD:complaint")),
            "{e:?}"
        );
    }

    /// A child that does not answer is ended at the read timeout, with its tree, and says what was read.
    #[test]
    fn a_child_that_does_not_answer_is_ended_at_the_deadline() {
        // The child starts under the hang-detector limit: it is a whole test binary, and a loaded machine can take longer than the
        // short deadline under test to start it (CI run 9238, Windows: the warm-up timed out at 300 ms). Only the quiet read below
        // runs under the short deadline, so what the test waits for is the child having answered once, not a time.
        let mut s = Session::start(child(Mode::Answer), limits()).unwrap();
        ask(&mut s, "warm").unwrap();
        s.limits.read_timeout = Duration::from_millis(300);
        let pid = s.child.id();
        s.send(b"quiet\n").unwrap();
        let e = s.read_line().unwrap_err();
        assert!(
            matches!(e, SessionError::TimedOut { what: "a line", .. }),
            "{e}"
        );
        assert!(e.to_string().contains("did not answer"), "{e}");
        assert!(
            crate::testchild::is_gone(pid),
            "the child is ended, not left running"
        );
    }

    #[test]
    fn a_child_that_ends_before_its_answer_is_closed_not_hung() {
        let mut s = Session::start(child(Mode::Answer), limits()).unwrap();
        s.send(b"exit\n").unwrap();
        let e = loop {
            match s.read_line() {
                Ok(_) => continue,
                Err(e) => break e,
            }
        };
        assert!(matches!(e, SessionError::Closed { .. }), "{e}");
    }

    #[test]
    fn an_answer_over_the_cap_ends_the_session() {
        let mut s = Session::start(child(Mode::Answer), limits().max_answer(1024)).unwrap();
        let e = s.read_exact(4096).unwrap_err();
        assert!(
            matches!(e, SessionError::TooLarge { bytes: 4096, .. }),
            "{e}"
        );
        assert!(
            s.send(b"again\n").is_err(),
            "the input is closed after the end"
        );
    }

    /// Dropping a session ends its child (and the tree under it), so a caller's early return leaves nothing behind.
    #[test]
    fn dropping_a_session_ends_its_child() {
        let s = Session::start(child(Mode::Answer), limits()).unwrap();
        let pid = s.child.id();
        drop(s);
        assert!(crate::testchild::is_gone(pid));
    }

    #[test]
    fn a_program_that_is_not_there_is_named() {
        let e = Session::start(
            crate::command("paddock-no-such-program-for-session"),
            limits(),
        )
        .unwrap_err();
        assert!(
            e.to_string()
                .contains("paddock-no-such-program-for-session"),
            "{e}"
        );
    }
}
