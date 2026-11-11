//! The positive controls (ER-33, ADR 0072 §5): a tiny line-based endpoint that ends every connection **right**
//! ([`MiniEndpoint::reference`]) and five copies that each end it wrong in one deliberate way ([`MiniFault`]). The harness
//! must pass the reference and fail each broken endpoint in the scenario that can see that fault; a test asserts both, so a
//! harness that stopped seeing a fault turns red.
//!
//! The wire is TCP on loopback, lines of text: the client sends `HELLO <token>`; the endpoint answers `WELCOME` or
//! `REFUSE <code>`; the client may send `CALL <n>` (a call in flight); the endpoint's last line is `CLOSING <code>`.
//! The reference model is the shape of ADR 0072 §2 in small: seal, flush under a cap on the injected clock, the reason
//! under its deadline, half-close, drain, release, one end line. A peer that "does not read" is a [`Sink`] whose budget runs
//! out (deterministic; no buffer is filled and nothing sleeps).

use super::log::LogCapture;
use super::raw::{RawEnd, RawStream, read_to_end, write_junk};
use super::{Decoded, Expect, Observed, Scenario, Typed};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline, FakeClock};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// The flush cap of an owner close.
pub const FLUSH_CAP: Duration = Duration::from_secs(2);
/// The reason deadline.
pub const REASON_DEADLINE: Duration = Duration::from_millis(250);
/// The bytes a refused client may send after its refusal before it is cut off.
pub const DRAIN_BYTE_CAP: usize = 64 << 10;
/// How long the harness waits for something that **will** happen (an event, not a pause): a hang guard, long enough for a
/// machine that is compiling, short enough that a broken endpoint fails the test.
const GUARD: Duration = Duration::from_secs(30);
/// How long the harness waits to learn that something will **not** happen (a broken endpoint that never waits on the
/// clock). Only the positive control of the missing flush cap uses it, because only there is the wait's answer "never": a
/// short guard on a wait that does happen turns a busy machine into a false "hang" (found when the reference endpoint failed
/// while two builds ran).
const NEGATIVE_GUARD: Duration = Duration::from_secs(3);
/// One queued push: written whole or not at all.
const CHUNK: usize = 1024;
/// The token a client must present.
const TOKEN: &str = "good";

/// One deliberate fault of a broken endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniFault {
    /// A refusal or a close drops the stream without a reason.
    DropWithoutReason,
    /// An owner close lets the served calls answer their cancel before the reason.
    CancelBeforeSeal,
    /// An owner close flushes everything queued and waits for it, with no cap.
    FlushWithoutCap,
    /// A refusal is released without reading what the client sent.
    ReleaseWithoutDrain,
    /// A stopping listener drops the clients waiting in its backlog.
    DropBacklog,
}

impl MiniFault {
    /// Every fault.
    pub const ALL: [MiniFault; 5] = [
        MiniFault::DropWithoutReason,
        MiniFault::CancelBeforeSeal,
        MiniFault::FlushWithoutCap,
        MiniFault::ReleaseWithoutDrain,
        MiniFault::DropBacklog,
    ];

    /// The scenario that can see this fault.
    pub fn seen_in(self) -> Scenario {
        match self {
            MiniFault::DropWithoutReason => Scenario::WrongToken,
            MiniFault::CancelBeforeSeal => Scenario::OwnerCloseWithCallsInFlight,
            MiniFault::FlushWithoutCap => Scenario::OwnerCloseWhilePeerNotReading,
            MiniFault::ReleaseWithoutDrain => Scenario::ClientKeepsWriting,
            MiniFault::DropBacklog => Scenario::ListenerStopWithClientInBacklog,
        }
    }
}

/// The peer's receive side as the endpoint writes to it: a socket that takes `budget` more bytes and then answers
/// `WouldBlock`, as a full buffer does ("the peer is not reading").
struct Sink {
    inner: TcpStream,
    budget: Arc<Mutex<usize>>,
}

impl Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut b = self.budget.lock().unwrap_or_else(|p| p.into_inner());
        if *b == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = buf.len().min(*b);
        let written = self.inner.write(&buf[..n])?;
        *b -= written;
        Ok(written)
    }
}

enum Ev {
    Bytes(Vec<u8>),
    Eof,
    Close,
    Push(usize),
}

struct Conn {
    events: Sender<Ev>,
    thread: JoinHandle<()>,
    budget: Arc<Mutex<usize>>,
}

struct Shared {
    fault: Option<MiniFault>,
    clock: FakeClock,
    log: LogCapture,
}

/// The endpoint under test: a loopback listener whose accepting the harness drives ([`MiniEndpoint::accept_exactly`]),
/// so that a client can wait in the backlog.
pub struct MiniEndpoint {
    listener: Option<TcpListener>,
    shared: Arc<Shared>,
    conns: Vec<Conn>,
}

impl MiniEndpoint {
    /// The endpoint that ends connections right.
    ///
    /// # Errors
    /// The listener could not be bound.
    pub fn reference(clock: FakeClock, log: LogCapture) -> io::Result<MiniEndpoint> {
        MiniEndpoint::with(None, clock, log)
    }

    /// An endpoint with one deliberate fault.
    ///
    /// # Errors
    /// The listener could not be bound.
    pub fn broken(fault: MiniFault, clock: FakeClock, log: LogCapture) -> io::Result<MiniEndpoint> {
        MiniEndpoint::with(Some(fault), clock, log)
    }

    fn with(
        fault: Option<MiniFault>,
        clock: FakeClock,
        log: LogCapture,
    ) -> io::Result<MiniEndpoint> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        Ok(MiniEndpoint {
            listener: Some(listener),
            shared: Arc::new(Shared { fault, clock, log }),
            conns: Vec::new(),
        })
    }

    /// The address clients connect to.
    ///
    /// # Errors
    /// The endpoint was stopped.
    pub fn addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener
            .as_ref()
            .ok_or_else(|| io::Error::other("stopped"))?
            .local_addr()
    }

    /// Accepts `n` clients and serves each on a thread of its own. A client whose `connect` returned may not be in the
    /// accept queue yet (the kernel moves it there when it has processed the handshake's last packet, which a loaded
    /// machine can delay), so this waits for the connections themselves (a bounded poll of a condition) instead of
    /// accepting "what is there".
    ///
    /// # Errors
    /// The listener failed, or the clients did not arrive within the hang guard.
    pub fn accept_exactly(&mut self, n: usize) -> io::Result<()> {
        let Some(listener) = self.listener.as_ref() else {
            return Ok(());
        };
        listener.set_nonblocking(true)?;
        let end = std::time::Instant::now() + GUARD;
        let mut accepted = 0;
        while accepted < n {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false)?;
                    let conn = self.serve(stream)?;
                    self.conns.push(conn);
                    accepted += 1;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= end {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the clients never reached the accept queue",
                        ));
                    }
                    std::thread::yield_now();
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn serve(&self, stream: TcpStream) -> io::Result<Conn> {
        let (tx, rx) = mpsc::channel::<Ev>();
        let mut reader = stream.try_clone()?;
        let rtx = tx.clone();
        std::thread::Builder::new()
            .name("mini-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            drop(rtx.send(Ev::Eof));
                            return;
                        }
                        Ok(n) => {
                            if rtx.send(Ev::Bytes(buf[..n].to_vec())).is_err() {
                                return;
                            }
                        }
                    }
                }
            })?;
        let budget = Arc::new(Mutex::new(usize::MAX));
        let sink = Sink {
            inner: stream,
            budget: budget.clone(),
        };
        let shared = self.shared.clone();
        let thread = std::thread::Builder::new()
            .name("mini-conn".into())
            .spawn(move || run_conn(&shared, sink, &rx))?;
        Ok(Conn {
            events: tx,
            thread,
            budget,
        })
    }

    /// The owner closes connection `index`.
    pub fn close_owner(&self, index: usize) {
        if let Some(c) = self.conns.get(index) {
            drop(c.events.send(Ev::Close));
        }
    }

    /// Lets connection `index` take only `bytes` more before its sink answers `WouldBlock` (the peer is not reading).
    pub fn limit_peer(&self, index: usize, bytes: usize) {
        if let Some(c) = self.conns.get(index) {
            *c.budget.lock().unwrap_or_else(|p| p.into_inner()) = bytes;
        }
    }

    /// Queues `bytes` of pushes on connection `index`.
    pub fn push(&self, index: usize, bytes: usize) {
        if let Some(c) = self.conns.get(index) {
            drop(c.events.send(Ev::Push(bytes)));
        }
    }

    /// Stops the endpoint: the open connections are closed by the owner, and the `backlog` clients still waiting to be
    /// accepted are accepted and refused with `stopping`, before the listener goes (a broken endpoint drops them).
    pub fn stop(&mut self, backlog: usize) {
        if self.shared.fault != Some(MiniFault::DropBacklog) {
            drop(self.accept_exactly(backlog));
        }
        self.listener = None;
        for i in 0..self.conns.len() {
            self.close_owner(i);
        }
    }

    /// Waits for every connection thread to end (after the clients left); a thread blocked on a broken endpoint's flush is
    /// not waited for.
    pub fn finish(self) {
        for c in self.conns {
            drop(c.events);
            if c.thread.is_finished() {
                drop(c.thread.join());
            }
        }
    }
}

/// Writes `bytes` to the sink under `limit` on the clock; false when the deadline passed first (the rest is dropped).
fn write_under(sink: &mut Sink, mut bytes: &[u8], clock: &FakeClock, limit: Duration) -> bool {
    let deadline = Deadline::after(Arc::new(clock.clone()), limit);
    while !bytes.is_empty() {
        match sink.write(bytes) {
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if deadline.passed()
                    || clock
                        .wait(
                            deadline.remaining().unwrap_or_default(),
                            &CancelToken::new(),
                        )
                        .is_err()
                {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

fn end_line(shared: &Shared, code: &str, flush: &str, reason: &str, drain: &str, bytes: usize) {
    shared.log.push(format!(
        "connection ended code={code} flush={flush} reason={reason} drain={drain} bytes={bytes}"
    ));
}

/// Reads and discards until the client's end or `cap` bytes; returns the outcome and the bytes read.
fn drain(rx: &Receiver<Ev>, mut seen: usize, cap: usize) -> (&'static str, usize) {
    loop {
        if seen >= cap {
            return ("byteCap", seen);
        }
        match rx.recv() {
            Ok(Ev::Bytes(b)) => seen += b.len(),
            Ok(Ev::Eof) | Err(_) => return ("eof", seen),
            Ok(_) => {}
        }
    }
}

fn refuse(shared: &Shared, sink: &mut Sink, rx: &Receiver<Ev>, code: &str, mut unread: usize) {
    if shared.fault == Some(MiniFault::DropWithoutReason) {
        drop(sink.inner.shutdown(Shutdown::Both));
        end_line(shared, code, "none", "none", "skipped", 0);
        return;
    }
    let sent = write_under(
        sink,
        format!("REFUSE {code}\n").as_bytes(),
        &shared.clock,
        REASON_DEADLINE,
    );
    drop(sink.inner.shutdown(Shutdown::Write));
    if shared.fault == Some(MiniFault::ReleaseWithoutDrain) {
        drop(sink.inner.shutdown(Shutdown::Both));
        end_line(
            shared,
            code,
            "none",
            if sent { "sent" } else { "reasonUnsent" },
            "skipped",
            0,
        );
        return;
    }
    unread += 0;
    let (outcome, bytes) = drain(rx, unread, DRAIN_BYTE_CAP);
    drop(sink.inner.shutdown(Shutdown::Both));
    end_line(
        shared,
        code,
        "none",
        if sent { "sent" } else { "reasonUnsent" },
        outcome,
        bytes,
    );
}

fn run_conn(shared: &Shared, mut sink: Sink, rx: &Receiver<Ev>) {
    let mut buf: Vec<u8> = Vec::new();
    let take_line = |buf: &mut Vec<u8>| -> Option<String> {
        let at = buf.iter().position(|&b| b == b'\n')?;
        let line: Vec<u8> = buf.drain(..=at).collect();
        Some(String::from_utf8_lossy(&line[..line.len() - 1]).to_string())
    };
    let hello = loop {
        if let Some(l) = take_line(&mut buf) {
            break l;
        }
        match rx.recv() {
            Ok(Ev::Bytes(b)) => buf.extend_from_slice(&b),
            Ok(Ev::Close) => {
                // The listener stopped before the hello: refuse the client waiting in the backlog.
                refuse(shared, &mut sink, rx, "stopping", buf.len());
                return;
            }
            _ => {
                end_line(shared, "none", "none", "none", "none", 0);
                return;
            }
        }
    };
    if hello != format!("HELLO {TOKEN}") {
        refuse(shared, &mut sink, rx, "auth", buf.len());
        return;
    }
    if !write_under(&mut sink, b"WELCOME\n", &shared.clock, REASON_DEADLINE) {
        return;
    }
    let mut in_flight: Vec<String> = Vec::new();
    let mut queue: VecDeque<Vec<u8>> = VecDeque::new();
    loop {
        while let Some(line) = take_line(&mut buf) {
            if let Some(n) = line.strip_prefix("CALL ") {
                in_flight.push(n.to_string());
                shared.log.push(format!("call {n} in flight"));
            }
        }
        match rx.recv() {
            Ok(Ev::Bytes(b)) => buf.extend_from_slice(&b),
            Ok(Ev::Push(n)) => {
                for _ in 0..n.div_ceil(CHUNK) {
                    let mut chunk = vec![b'p'; CHUNK - 1];
                    chunk.push(b'\n');
                    queue.push_back(chunk);
                }
            }
            Ok(Ev::Eof) | Err(_) => {
                end_line(shared, "none", "none", "none", "none", 0);
                return;
            }
            Ok(Ev::Close) => {
                owner_close(shared, &mut sink, rx, &in_flight, queue);
                return;
            }
        }
    }
}

fn owner_close(
    shared: &Shared,
    sink: &mut Sink,
    rx: &Receiver<Ev>,
    in_flight: &[String],
    mut queue: VecDeque<Vec<u8>>,
) {
    let fault = shared.fault;
    if fault == Some(MiniFault::CancelBeforeSeal) {
        // The bug of CI run 7761: the served calls answer their cancel before the queue is sealed.
        for n in in_flight {
            write_under(
                sink,
                format!("CANCELLED {n}\n").as_bytes(),
                &shared.clock,
                REASON_DEADLINE,
            );
        }
    }
    if fault == Some(MiniFault::DropWithoutReason) {
        drop(sink.inner.shutdown(Shutdown::Both));
        end_line(shared, "closed", "none", "none", "skipped", 0);
        return;
    }
    // Seal: the served calls' answers find the queue shut, so nothing but the queued frames and the reason go out.
    let (mut flushed, mut dropped) = ("flushed", 0usize);
    if fault == Some(MiniFault::FlushWithoutCap) {
        // No cap: the flush waits for the peer for as long as it takes. Only the client's end of the stream ends the wait (so
        // a test that drops its client leaves no thread behind); nothing else wakes it, which is the fault.
        while let Some(chunk) = queue.pop_front() {
            let mut rest = &chunk[..];
            while !rest.is_empty() {
                match sink.write(rest) {
                    Ok(n) => rest = &rest[n..],
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        if matches!(rx.recv(), Ok(Ev::Eof) | Err(_)) {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        }
    } else {
        let deadline = Deadline::after(Arc::new(shared.clock.clone()), FLUSH_CAP);
        'flush: while let Some(chunk) = queue.front() {
            let mut rest = &chunk[..];
            while !rest.is_empty() {
                match sink.write(rest) {
                    Ok(n) => rest = &rest[n..],
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        if deadline.passed()
                            || shared
                                .clock
                                .wait(
                                    deadline.remaining().unwrap_or_default(),
                                    &CancelToken::new(),
                                )
                                .is_err()
                        {
                            flushed = "flushCap";
                            dropped = queue.len();
                            break 'flush;
                        }
                    }
                    Err(_) => return,
                }
            }
            queue.pop_front();
        }
    }
    let flush = if dropped > 0 {
        format!("{flushed}:dropped{dropped}")
    } else {
        flushed.to_string()
    };
    let sent = write_under(sink, b"CLOSING stopping\n", &shared.clock, REASON_DEADLINE);
    if !sent {
        // The peer is not reading: the closer closes the stream.
        drop(sink.inner.shutdown(Shutdown::Both));
        end_line(shared, "stopping", &flush, "reasonUnsent", "none", 0);
        return;
    }
    drop(sink.inner.shutdown(Shutdown::Write));
    let (outcome, bytes) = drain(rx, 0, DRAIN_BYTE_CAP);
    drop(sink.inner.shutdown(Shutdown::Both));
    end_line(shared, "stopping", &flush, "sent", outcome, bytes);
}

/// Decodes the endpoint's lines: `REFUSE <code>` and `CLOSING <code>` are reasons.
pub fn decode_lines(bytes: &[u8]) -> Decoded {
    let text = String::from_utf8_lossy(bytes);
    let mut d = Decoded::default();
    let complete = text.rfind('\n').map_or(0, |i| i + 1);
    d.partial_tail = text.len() - complete;
    for line in text[..complete].lines() {
        if let Some(code) = line
            .strip_prefix("REFUSE ")
            .or_else(|| line.strip_prefix("CLOSING "))
        {
            d.reasons.push(code.to_string());
            d.after_last_reason = 0;
        } else if !d.reasons.is_empty() {
            d.after_last_reason += 1;
        }
    }
    d
}

fn read_line(s: &mut dyn RawStream) -> io::Result<String> {
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    s.set_read_timeout(Some(GUARD))?;
    loop {
        if s.read(&mut b)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if b[0] == b'\n' {
            return Ok(String::from_utf8_lossy(&line).to_string());
        }
        line.push(b[0]);
    }
}

fn observe(raw: RawEnd, log: &LogCapture, typed: bool) -> Observed {
    let decoded = decode_lines(&raw.bytes);
    let text = String::from_utf8_lossy(&raw.bytes).to_string();
    let typed = typed.then(|| Typed {
        transport_only: decoded.reasons.is_empty(),
        cancelled: text.contains("CANCELLED"),
    });
    Observed {
        raw: Some(raw),
        decoded,
        typed,
        log: log.lines(),
        hung: false,
    }
}

/// Whether the mini protocol has a counterpart for `scenario`.
pub fn applies(scenario: Scenario) -> bool {
    matches!(
        scenario,
        Scenario::WrongToken
            | Scenario::ClientKeepsWriting
            | Scenario::OwnerCloseWithCallsInFlight
            | Scenario::OwnerCloseWhilePeerNotReading
            | Scenario::ListenerStopWithClientInBacklog
            | Scenario::StopWithOpenConnection
    )
}

/// Runs `scenario` against a fresh endpoint (`None`: the reference). `Err` names why a scenario does not apply to the
/// mini protocol.
///
/// # Errors
/// The scenario has no counterpart in the mini protocol.
pub fn run(
    fault: Option<MiniFault>,
    scenario: Scenario,
) -> Result<(Observed, Expect), &'static str> {
    if !applies(scenario) {
        return Err("the mini protocol has no counterpart for this scenario");
    }
    let (clock, log) = (FakeClock::new(), LogCapture::new());
    let mut ep =
        MiniEndpoint::with(fault, clock.clone(), log.clone()).map_err(|_| "no loopback port")?;
    let addr = ep.addr().map_err(|_| "no address")?;
    let connect = || TcpStream::connect(addr).map_err(|_| "cannot connect");
    let hello = |s: &mut TcpStream, token: &str| {
        s.write_all(format!("HELLO {token}\n").as_bytes())
            .map_err(|_| "write failed")
    };
    let mut hung = false;
    let (raw, expect, typed) = match scenario {
        Scenario::WrongToken | Scenario::ClientKeepsWriting => {
            let mut c = connect()?;
            hello(&mut c, "bad")?;
            if scenario == Scenario::ClientKeepsWriting {
                write_junk(&mut c, super::KEEP_WRITING_BYTES);
                drop(c.shutdown_write());
            }
            ep.accept_exactly(1).map_err(|_| "accept failed")?;
            let raw = read_to_end(&mut c, GUARD);
            drop(c);
            (raw, Expect::reason(&["auth"]), false)
        }
        Scenario::ListenerStopWithClientInBacklog => {
            let mut c = connect()?;
            hello(&mut c, TOKEN)?;
            ep.stop(1);
            let raw = read_to_end(&mut c, GUARD);
            drop(c);
            (raw, Expect::reason(&["stopping"]), false)
        }
        Scenario::StopWithOpenConnection
        | Scenario::OwnerCloseWithCallsInFlight
        | Scenario::OwnerCloseWhilePeerNotReading => {
            let mut c = connect()?;
            hello(&mut c, TOKEN)?;
            ep.accept_exactly(1).map_err(|_| "accept failed")?;
            if read_line(&mut c).map_err(|_| "no welcome")? != "WELCOME" {
                return Err("no welcome");
            }
            match scenario {
                Scenario::StopWithOpenConnection => ep.stop(0),
                Scenario::OwnerCloseWithCallsInFlight => {
                    c.write_all(b"CALL 1\n").map_err(|_| "write failed")?;
                    if !log.wait_for_len(1, GUARD) {
                        return Err("the call never started");
                    }
                    ep.close_owner(0);
                }
                _ => {
                    // The peer takes 4 chunks and then stops reading; 64 are queued; the owner closes.
                    ep.limit_peer(0, 4 * CHUNK);
                    ep.push(0, 64 * CHUNK);
                    ep.close_owner(0);
                    // The reference waits on the clock (the guard is a hang guard); the endpoint with no flush cap never does.
                    let wait = if fault == Some(MiniFault::FlushWithoutCap) {
                        NEGATIVE_GUARD
                    } else {
                        GUARD
                    };
                    hung = !clock.wait_for_pending(1, wait);
                    if !hung {
                        clock.advance(FLUSH_CAP);
                        hung = !clock.wait_for_pending(1, wait);
                    }
                    if !hung {
                        clock.advance(REASON_DEADLINE);
                    }
                    if hung {
                        // Unblock the broken endpoint's thread so the test leaves nothing behind.
                        drop(c);
                        let o = Observed {
                            hung: true,
                            log: log.lines(),
                            ..Observed::default()
                        };
                        return Ok((o, Expect::optional_reason().with_drain_eof()));
                    }
                    // Now the peer reads again.
                    ep.limit_peer(0, usize::MAX);
                }
            }
            let raw = read_to_end(&mut c, GUARD);
            drop(c);
            let expect = if scenario == Scenario::OwnerCloseWhilePeerNotReading {
                Expect::optional_reason()
            } else {
                Expect::reason(&["stopping"])
            };
            (
                raw,
                expect,
                scenario == Scenario::OwnerCloseWithCallsInFlight,
            )
        }
        _ => return Err("the mini protocol has no counterpart for this scenario"),
    };
    // The end line is written when the endpoint has released the connection, which follows the client's end.
    if !ep.conns.is_empty() {
        log.wait_for_end_line(GUARD);
    }
    let mut observed = observe(raw, &log, typed);
    observed.hung = hung;
    ep.finish();
    Ok((observed, expect.with_drain_eof()))
}

#[allow(dead_code)]
fn _assert_send(_: &dyn Fn(&mut dyn RawStream) -> RawEnd) {}
