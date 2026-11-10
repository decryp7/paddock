//! The one child loop (ADR 0001, Debrief ADR 0017 §2): a child process' `main` calls [`serve`], which
//!
//! 1. reads its [`Bootstrap`] (one JSON line on stdin, at most 64 KiB);
//! 2. connects to the host and authenticates with the token, speaking its protocol version;
//! 3. answers the host's calls with its handler until the connection closes, then returns (the process exits).
//!
//! It also ends as soon as its stdin closes ([`lifeline_closing`]): when the host stops it or dies, nothing is left behind.
//! It ends through **one** exit: the lifeline closes the connection, `main` returns and is the only `exit` (Rust guards two
//! `exit`s only on Linux; on macOS two threads ran the `atexit` handlers and a database's static destructors together).
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
use crate::handshake::{Client, Connection};
use crate::launch::Bootstrap;
use crate::peer::{Handler, Peer, PeerOptions};
use crate::token::Token;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// What a child's exit phase may take once its host is gone or stops it: the child's own stop budget. A parent that stops
/// a child waits at least this plus [`STOP_MARGIN`] before it kills it, so a child that stops within its budget is never
/// killed in the middle of it.
pub const STOP_BUDGET: Duration = Duration::from_millis(1250);

/// Covers the parent's kill and reap, and a child's last log lines, on top of [`STOP_BUDGET`].
pub const STOP_MARGIN: Duration = Duration::from_millis(250);

/// Who ends the process: the child's `main` (past the connection) or, after a bound, the lifeline. One claim wins, under
/// one lock, so the two never both run an exit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Exit {
    Serving,
    MainExiting,
    LifelineExiting,
}

struct Phase {
    state: Mutex<Exit>,
    wake: Condvar,
}

static PHASE: Phase = Phase {
    state: Mutex::new(Exit::Serving),
    wake: Condvar::new(),
};

impl Phase {
    #[cfg(test)]
    fn new() -> Phase {
        Phase {
            state: Mutex::new(Exit::Serving),
            wake: Condvar::new(),
        }
    }

    /// `main` claims the exit; false when the lifeline claimed it first (this `main` must then park: the lifeline ends the
    /// process).
    fn claim_main(&self) -> bool {
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if *st == Exit::LifelineExiting {
            return false;
        }
        *st = Exit::MainExiting;
        self.wake.notify_all();
        true
    }

    /// Waits at most `max` for `main` to claim the exit; if it has not by then, the lifeline claims it and this says true.
    fn claim_fallback(&self, max: Duration) -> bool {
        let g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut g = self
            .wake
            .wait_timeout_while(g, max, |st| *st == Exit::Serving)
            .map(|(g, _)| g)
            .unwrap_or_else(|p| p.into_inner().0);
        if *g == Exit::Serving {
            *g = Exit::LifelineExiting;
            return true;
        }
        false
    }
}

/// Says that this child's exit phase has begun: its `main` is past the connection and ends the process itself. A child
/// whose `main` calls [`serve`] never calls this (it does); one that runs its own loop with [`lifeline_closing`] calls it
/// when its own `wait_closed` returns. If the lifeline's fallback already took the exit (a `main` that was too late), this
/// waits at most [`STOP_MARGIN`] for the lifeline to end the process, then ends it itself.
pub fn exit_phase_begun() {
    begin_exit(&PHASE, STOP_MARGIN, || end_without_destructors(0));
}

/// `main`'s claim on `phase`; when the lifeline's fallback holds it, see [`main_lost_claim`].
fn begin_exit(phase: &Phase, wait: Duration, end: impl FnOnce()) {
    if !phase.claim_main() {
        main_lost_claim(wait, end);
    }
}

/// A `main` that finds the lifeline's fallback already took the exit: waits at most `wait` for the lifeline to end the
/// process, then calls `end` itself (never parks for good: the lifeline may have died, and this `main` holds what the child
/// holds).
fn main_lost_claim(wait: Duration, end: impl FnOnce()) {
    std::thread::park_timeout(wait);
    end();
}

/// Ends this process at once without running `atexit` handlers or static destructors (`_exit`, `TerminateProcess` on
/// itself): for a child whose exit phase overran or was taken by the lifeline's fallback. Stdio is not flushed.
#[cfg(unix)]
#[rustfmt::skip]
pub fn end_without_destructors(code: i32) -> ! { /* SAFETY: `_exit` takes no pointers and never returns. */ unsafe { libc::_exit(code) } }

/// [`end_without_destructors`] on Windows (`TerminateProcess` on this process).
#[cfg(windows)]
pub fn end_without_destructors(code: i32) -> ! {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    // SAFETY: the pseudo-handle of this process; `TerminateProcess` notifies no DLL.
    unsafe {
        // conformance: pid-kill — this process itself (`GetCurrentProcess`), not a pid read earlier
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    std::process::abort()
}

/// Largest bootstrap line read from stdin.
pub const MAX_BOOTSTRAP: u64 = 64 * 1024;

/// Reads the bootstrap line from `input` (at most [`MAX_BOOTSTRAP`] bytes).
pub fn read_bootstrap(input: impl Read) -> io::Result<Bootstrap> {
    let mut line = String::new();
    BufReader::new(input.take(MAX_BOOTSTRAP)).read_line(&mut line)?;
    serde_json::from_str(line.trim())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad bootstrap: {e}")))
}

/// Connects to the host named in `bootstrap`, speaking `protocol`.
pub fn connect(bootstrap: &Bootstrap, protocol: ProtocolVersion) -> Result<Connection, IpcError> {
    let token = Token::from_hex(&bootstrap.token)
        .ok_or_else(|| IpcError::Auth("the bootstrap token is not 64 hex digits".into()))?;
    Client::connect(
        &bootstrap.endpoint,
        &token,
        FrameLimit::new(bootstrap.max_frame),
        protocol,
    )
}

/// The lifeline of a child that has no connection to close: when the host closes stdin (or dies), exit at once. Once the
/// peer is up use [`lifeline_closing`]: two threads in `exit` crashed the store worker.
pub fn lifeline() {
    paddock_proc::cleanup::logged(
        "starting the lifeline thread (the child will not exit when its host dies)",
        std::thread::Builder::new()
            .name("paddock-ipc-lifeline".into())
            .spawn(|| {
                let mut sink = [0u8; 256];
                while matches!(io::stdin().read(&mut sink), Ok(n) if n > 0) {}
                std::process::exit(0);
            }),
    );
}

/// The lifeline of a child whose connection is up: when the host closes stdin (or dies) it **closes the connection**, so
/// the child's `main` (past `wait_closed`) is the only exit. If the exit phase has not begun within [`STOP_BUDGET`] plus
/// [`STOP_MARGIN`] (a `main` that hangs would keep what the child holds, a database lock for one), it ends the process
/// itself. Call [`exit_phase_begun`] from a `main` that does not go through [`serve`].
pub fn lifeline_closing(peer: &Peer) {
    let peer = peer.clone();
    paddock_proc::cleanup::logged(
        "starting the lifeline thread (the child will not exit when its host dies)",
        std::thread::Builder::new()
            .name("paddock-ipc-lifeline".into())
            .spawn(move || {
                let mut sink = [0u8; 256];
                while matches!(io::stdin().read(&mut sink), Ok(n) if n > 0) {}
                let end = || end_without_destructors(0);
                lifeline_end(&peer, &PHASE, LIFELINE_WAIT, &mut io::stderr(), end);
            }),
    );
}

/// What the lifeline does once stdin is closed: close `peer`, wait for `main` to claim the exit, and if it does not within
/// the child's stop budget plus margin, say so on `err` and `end` the process.
fn lifeline_end(
    peer: &Peer,
    phase: &Phase,
    wait: Duration,
    err: &mut dyn Write,
    end: impl FnOnce(),
) {
    close_then_end(peer, phase, wait, || end_after_fallback(err, end));
}

/// The last steps of the lifeline's fallback: say why, then `end`. Nothing before `end` may stop it: a broken stderr (the
/// host is gone) or a writer that panics must not keep the process alive, holding what the child holds.
fn end_after_fallback(err: &mut dyn Write, end: impl FnOnce()) {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        writeln!(
            err,
            "the exit phase did not begin within {} ms of the host going away; ending the process (exit code 0)",
            (STOP_BUDGET + STOP_MARGIN).as_millis()
        )
        .ok();
    }))
    .ok();
    end();
}

/// How long the lifeline waits for `main` to claim the exit after the host went away.
const LIFELINE_WAIT: Duration = STOP_BUDGET.saturating_add(STOP_MARGIN);

/// Closes `peer`, then waits at most `wait` for `phase` to begin; `fallback` runs when it does not.
fn close_then_end(peer: &Peer, phase: &Phase, wait: Duration, fallback: impl FnOnce()) {
    peer.close();
    if phase.claim_fallback(wait) {
        fallback();
    }
}

/// A child process' `main`; see the module docs. `make` builds the handler once the bootstrap is known (it may look at
/// [`Bootstrap::role`] or [`Bootstrap::extra`]); `connected` runs once the connection is up (e.g. to start pushing).
pub fn serve(
    label: &str,
    protocol: ProtocolVersion,
    make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>,
    connected: impl FnOnce(&Bootstrap, &Peer),
) -> Result<(), IpcError> {
    let bootstrap = read_bootstrap(io::stdin().lock())?;
    let conn = connect(&bootstrap, protocol)?;
    let peer = Peer::start(conn, make(&bootstrap), PeerOptions::new(label))?;
    connected(&bootstrap, &peer);
    lifeline_closing(&peer);
    peer.wait_closed(None);
    exit_phase_begun();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint;
    use crate::peer::NoCalls;
    use crate::peer::PeerOptions;

    fn a_peer() -> (Peer, endpoint::Stream) {
        let (ours, theirs) = endpoint::pair().unwrap();
        let peer = Peer::start(
            crate::handshake::Connection::raw(ours, FrameLimit::default()),
            Arc::new(NoCalls),
            PeerOptions::new("test"),
        )
        .unwrap();
        // The other end stays open for the test's length.
        (peer, theirs)
    }

    #[test]
    fn the_lifeline_closes_the_peer_and_leaves_the_exit_to_main_once_the_phase_begins() {
        let (peer, _other) = a_peer();
        let phase = Arc::new(Phase::new());
        let t = {
            let (peer, phase) = (peer.clone(), phase.clone());
            std::thread::spawn(move || {
                let mut said = Vec::new();
                lifeline_end(&peer, &phase, LIFELINE_WAIT, &mut said, || panic!("took"));
                said
            })
        };
        // `main` sees the connection close and begins the phase.
        assert!(peer.wait_closed(Some(Duration::from_secs(30))));
        assert!(phase.claim_main());
        assert!(
            t.join().unwrap().is_empty(),
            "the lifeline left the exit to main and said nothing"
        );
    }

    #[test]
    fn the_lifeline_says_why_and_ends_when_main_never_claims_the_exit() {
        let (peer, _other) = a_peer();
        let (mut said, mut ended) = (Vec::new(), false);
        let wait = Duration::from_millis(30);
        lifeline_end(&peer, &Phase::new(), wait, &mut said, || ended = true);
        assert!(ended && String::from_utf8(said).unwrap().contains("did not begin"));
    }

    #[test]
    fn a_main_that_lost_the_claim_goes_through_begin_exit() {
        let phase = Phase::new();
        assert!(phase.claim_fallback(Duration::ZERO));
        let mut ended = false;
        begin_exit(&phase, Duration::from_millis(30), || ended = true);
        assert!(ended, "a main that lost the claim ends the process itself");
        let mut kept = false;
        begin_exit(&Phase::new(), Duration::from_millis(30), || kept = true);
        assert!(!kept, "a main that wins the claim does not");
    }

    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Panicking;
    impl Write for Panicking {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            panic!("a writer that panics")
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_fallback_ends_the_process_whatever_its_message_does() {
        let mut said = Vec::new();
        let mut ended = 0;
        end_after_fallback(&mut said, || ended += 1);
        assert_eq!(
            (
                ended,
                String::from_utf8(said).unwrap().contains("did not begin")
            ),
            (1, true)
        );
        // The host is gone: a broken pipe, and a writer that panics, still end it.
        end_after_fallback(&mut Broken, || ended += 1);
        end_after_fallback(&mut Panicking, || ended += 1);
        assert_eq!(ended, 3);
        Broken.flush().unwrap();
        Panicking.flush().unwrap();
    }

    #[test]
    fn a_main_that_lost_the_claim_ends_the_process_within_the_bound() {
        // On a thread, so a `main` that parked for good fails this test instead of hanging it.
        let (done, ended) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            main_lost_claim(Duration::from_millis(30), || done.send(()).unwrap())
        });
        assert!(
            ended.recv_timeout(Duration::from_secs(30)).is_ok(),
            "the late main did not end the process"
        );
    }

    #[test]
    fn the_lifeline_ends_the_process_itself_when_the_phase_never_begins() {
        let (peer, _other) = a_peer();
        let phase = Phase::new();
        let mut ended = false;
        close_then_end(&peer, &phase, Duration::from_millis(50), || ended = true);
        assert!(peer.closed().is_some(), "it closed the peer first");
        assert!(ended, "after the bound it falls back to ending the process");
        // A `main` that comes late finds the exit taken: it must park, never run a second exit.
        assert!(!phase.claim_main(), "the lifeline owns the exit now");
    }
}
