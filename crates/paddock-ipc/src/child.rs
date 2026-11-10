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
use std::io::{self, BufRead, BufReader, Read};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// What a child's exit phase may take once its host is gone or stops it: the child's own stop budget. A parent that stops
/// a child waits at least this plus [`STOP_MARGIN`] before it kills it, so a child that stops within its budget is never
/// killed in the middle of it.
pub const STOP_BUDGET: Duration = Duration::from_millis(1250);

/// Covers the parent's kill and reap, and a child's last log lines, on top of [`STOP_BUDGET`].
pub const STOP_MARGIN: Duration = Duration::from_millis(250);

/// Whether the child's exit phase has begun (its `main` is past the connection), and the wait for it.
struct Phase {
    begun: Mutex<bool>,
    wake: Condvar,
}

static PHASE: Phase = Phase {
    begun: Mutex::new(false),
    wake: Condvar::new(),
};

impl Phase {
    fn begin(&self) {
        *self.begun.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.wake.notify_all();
    }

    /// Waits at most `max` for the phase to begin; whether it has.
    fn wait_begun(&self, max: Duration) -> bool {
        let g = self.begun.lock().unwrap_or_else(|p| p.into_inner());
        self.wake
            .wait_timeout_while(g, max, |begun| !*begun)
            .map(|(g, _)| *g)
            .unwrap_or_else(|p| *p.into_inner().0)
    }
}

/// Says that this child's exit phase has begun: its `main` is past the connection and ends the process itself. [`serve`]
/// calls it when its `wait_closed` returns; a child that runs its own loop with [`lifeline_closing`] calls it too.
pub fn exit_phase_begun() {
    PHASE.begin();
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
                close_then_end(&peer, &PHASE, STOP_BUDGET + STOP_MARGIN, || {
                    eprintln!(
                        "the exit phase did not begin within {} ms of the host going away; ending the process (exit code 0)",
                        (STOP_BUDGET + STOP_MARGIN).as_millis()
                    );
                    std::process::exit(0);
                });
            }),
    );
}

/// Closes `peer`, then waits at most `wait` for `phase` to begin; `fallback` runs when it does not.
fn close_then_end(peer: &Peer, phase: &Phase, wait: Duration, fallback: impl FnOnce()) {
    peer.close();
    if !phase.wait_begun(wait) {
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
    use std::sync::atomic::{AtomicBool, Ordering};

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
        let phase = Arc::new(Phase {
            begun: Mutex::new(false),
            wake: Condvar::new(),
        });
        let ended = Arc::new(AtomicBool::new(false));
        let t = {
            let (peer, phase, ended) = (peer.clone(), phase.clone(), ended.clone());
            std::thread::spawn(move || {
                close_then_end(&peer, &phase, Duration::from_secs(30), || {
                    ended.store(true, Ordering::SeqCst);
                });
            })
        };
        // `main` sees the connection close and begins the phase.
        assert!(peer.wait_closed(Some(Duration::from_secs(30))));
        phase.begin();
        t.join().unwrap();
        assert!(
            !ended.load(Ordering::SeqCst),
            "the lifeline left the exit to main"
        );
    }

    #[test]
    fn the_lifeline_ends_the_process_itself_when_the_phase_never_begins() {
        let (peer, _other) = a_peer();
        let phase = Phase {
            begun: Mutex::new(false),
            wake: Condvar::new(),
        };
        let mut ended = false;
        close_then_end(&peer, &phase, Duration::from_millis(50), || ended = true);
        assert!(peer.closed().is_some(), "it closed the peer first");
        assert!(ended, "after the bound it falls back to ending the process");
    }
}
