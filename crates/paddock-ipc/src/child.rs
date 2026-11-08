//! The one child loop (ADR 0001, Debrief ADR 0017 §2): a child process' `main` calls [`serve`], which
//!
//! 1. reads its [`Bootstrap`] (one JSON line on stdin, at most 64 KiB);
//! 2. connects to the host and authenticates with the token, speaking its protocol version;
//! 3. answers the host's calls with its handler until the connection closes, then returns (the process exits).
//!
//! It also exits as soon as its stdin closes ([`lifeline`]): when the host stops it or dies, nothing is left behind.
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
use std::sync::Arc;

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

/// The lifeline: when the host closes stdin (or dies), exit at once.
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
    lifeline();
    peer.wait_closed(None);
    Ok(())
}
