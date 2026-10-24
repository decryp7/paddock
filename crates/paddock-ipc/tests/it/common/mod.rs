//! Helpers shared by the integration tests. Waits are on events (channels, the peer's close) with a generous hang
//! guard, never on a stopwatch.
#![allow(dead_code)]

use paddock_ipc::endpoint;
use paddock_ipc::{Connection, FrameLimit, Handler, Peer, PeerOptions};
use std::sync::Arc;
use std::time::Duration;

/// A hang guard: long enough that a loaded machine never trips it, short enough that a broken test fails.
pub const GUARD: Duration = Duration::from_secs(60);
/// The wait for calls in tests that expect an answer.
pub const T: Option<Duration> = Some(GUARD);

/// Two peers connected over a fresh private endpoint.
pub fn pair(a: Arc<dyn Handler>, b: Arc<dyn Handler>) -> (Peer, Peer) {
    pair_with(
        a,
        PeerOptions::new("side a"),
        b,
        PeerOptions::new("side b"),
        FrameLimit::default(),
    )
}

/// [`pair`] with options and a frame limit.
pub fn pair_with(
    a: Arc<dyn Handler>,
    oa: PeerOptions,
    b: Arc<dyn Handler>,
    ob: PeerOptions,
    limit: FrameLimit,
) -> (Peer, Peer) {
    let (x, y) = endpoint::pair().unwrap();
    let pa = Peer::start(Connection::raw(x, limit), a, oa).unwrap();
    let pb = Peer::start(Connection::raw(y, limit), b, ob).unwrap();
    (pa, pb)
}

/// A peer and the raw other end of its stream.
pub fn raw(
    handler: Arc<dyn Handler>,
    opts: PeerOptions,
    limit: FrameLimit,
) -> (Peer, endpoint::Stream) {
    let (x, y) = endpoint::pair().unwrap();
    (
        Peer::start(Connection::raw(x, limit), handler, opts).unwrap(),
        y,
    )
}

/// The test child program.
pub fn child_program() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_paddock-ipc-test-child"))
}
