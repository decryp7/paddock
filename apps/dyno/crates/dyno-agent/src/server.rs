//! The IPC server: accepts apps on the agent's private endpoint, answers their requests and feeds each subscriber
//! its own bounded queue (ADR 0002). Writes `agent.json` so the app can find the endpoint and token.
//!
//! # Local API versioning (design note, no behaviour here)
//!
//! The app protocol is [`PROTOCOL_VERSION`] plus the append-only rule in `dyno_core::protocol`: new `Request`, `Reply`
//! and `Message` variants go at the END of their enums, so an older peer reading a newer variant fails that one frame
//! (reported as a protocol error) instead of misreading others. Anything else (reordering, removing or changing a
//! variant, changing a field's type) bumps `PROTOCOL_VERSION`, which `connect` checks in the handshake.
//!
//! A command-line client (`dyno status`, `dyno capture`, `dyno alarms`...) is meant to be a second kind of peer on the
//! SAME endpoint: it reads the same discovery file (`agent.json`: endpoint, token, protocol version), connects with
//! the same token and speaks the same `Request`/`Reply` frames, so a request added for the app is available to the CLI
//! without new server code, and every handler keeps validating its arguments because it cannot know who is asking.
//! The CLI must therefore check `protocolVersion` in `agent.json` before connecting and say "update Dyno" when it is
//! newer than the CLI knows. The optional local web API (`Settings.webApi`) is a separate, opt-in adapter that maps
//! HTTP onto these same requests; it is not a second protocol.

use crate::agent::{lock, ScanSlot, Shared, Sub};
use crate::discovery::Discovery;
use crate::handlers::{handle, Conn};
use dyno_core::protocol::{AgentError, ErrorCode, Message, PROTOCOL_VERSION};
use dyno_ipc::{Host, IpcError, Peer, PeerOptions, RemoteError};
use std::sync::atomic::AtomicU8;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// Frames waiting for a slow app, per connection.
const QUEUE: usize = 4;

/// Binds the endpoint and writes the discovery file. Returns the host and the file's content.
pub fn bind(shared: &Shared) -> std::io::Result<(Host, Discovery)> {
    let host = Host::listen()?;
    let d = Discovery {
        endpoint: host.name().to_string(),
        token: host.token().to_hex(),
        pid: std::process::id(),
        started: crate::agent::now().millis(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        protocol: PROTOCOL_VERSION,
    };
    d.write(&shared.dirs.discovery())?;
    Ok((host, d))
}

/// Accepts connections until the agent stops. Each connection gets a [`Peer`] whose handler answers requests.
pub fn serve(shared: &Arc<Shared>, mut host: Host) {
    let mut peers: Vec<(u64, Arc<Peer<Message>>)> = Vec::new();
    while !shared.stopped() {
        // A closed connection loses its subscription; dropping the sender ends its push thread.
        let gone: Vec<u64> = peers.iter().filter(|(_, p)| p.is_closed()).map(|(id, _)| *id).collect();
        if !gone.is_empty() {
            peers.retain(|(id, _)| !gone.contains(id));
            lock(&shared.subs).retain(|s| !gone.contains(&s.id));
            shared.wake_sampler();
        }
        match host.accept(Duration::from_millis(400)) {
            Ok(conn) => match connect(shared, conn) {
                Ok(entry) => peers.push(entry),
                Err(e) => eprintln!("dyno-agent: could not start a connection: {e}"),
            },
            Err(IpcError::Timeout(_)) => {}
            Err(e) => {
                // A client with the wrong token or version: refused, logged without content.
                eprintln!("dyno-agent: refused a connection: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    for (_, p) in &peers {
        p.close();
    }
    Discovery::remove_if_mine(&shared.dirs.discovery());
}

fn connect(shared: &Arc<Shared>, conn: dyno_ipc::Connection) -> Result<(u64, Arc<Peer<Message>>), IpcError> {
    let id = shared.new_conn_id();
    let detail = Arc::new(AtomicU8::new(0));
    let (tx, rx) = sync_channel::<Message>(QUEUE);
    let ctx = Conn { detail: detail.clone() };
    let s = shared.clone();
    let handler = move |m: Message| -> Result<Message, RemoteError> {
        match m {
            Message::Request(r) => Ok(Message::Reply(Box::new(handle(&s, &ctx, *r)))),
            _ => Err(RemoteError::new(
                dyno_ipc::wire::codes::UNSUPPORTED,
                "The agent only answers requests.",
            )),
        }
    };
    let peer = Arc::new(Peer::start(conn, PeerOptions::new().serve(handler).workers(4))?);
    let scans = Arc::new(ScanSlot::default());
    lock(&shared.subs).push(Sub {
        id,
        detail,
        tx,
        scans: scans.clone(),
    });
    // One thread per connection drains its queue; it ends when the app goes away or the Sub is dropped.
    let p = peer.clone();
    let s = shared.clone();
    std::thread::Builder::new()
        .name(format!("dyno-push-{id}"))
        .spawn(move || {
            pump(&rx, &scans, |m| p.push(m).is_ok());
            lock(&s.subs).retain(|x| x.id != id);
        })
        .map_err(|e| IpcError::io(e.kind(), format!("could not start the push thread: {e}")))?;
    Ok((id, peer))
}

/// Sends a connection's frames until the app goes away (`send` returns false) or the queue is closed. Disk scan statuses do
/// not travel through the queue: a `DiskScan` message there only wakes this loop, and after every message the newest status
/// of each scan is taken from the slot and sent.
pub(crate) fn pump(rx: &Receiver<Message>, scans: &ScanSlot, mut send: impl FnMut(Message) -> bool) {
    while let Ok(m) = rx.recv() {
        if !matches!(m, Message::DiskScan(_)) && !send(m) {
            return;
        }
        for status in scans.take() {
            if !send(Message::DiskScan(Box::new(status))) {
                return;
            }
        }
    }
}

/// The error a request gets when it can't be read at all.
pub fn unreadable() -> AgentError {
    AgentError::new(ErrorCode::BadRequest, "Dyno could not read that request.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::disk::scan::*;
    use std::sync::mpsc::sync_channel;

    fn status(scan: u64, seq: u64, state: ScanState) -> DiskScanStatus {
        DiskScanStatus {
            scan,
            seq,
            name: "x".into(),
            target: DiskTarget::Folder { path: "/x".into() },
            volume: None,
            state,
            phase: ScanPhase::ReadingFolders,
            progress: ScanProgress::default(),
            truncated: false,
            unreadable_first: vec![],
            needs_full_disk_access: false,
            items_per_second: None,
            eta_seconds: None,
            hidden_bytes: None,
            scanned_at: None,
        }
    }

    /// A slow app: its queue is full of other frames while a scan runs, ends and runs again. The final state must still arrive.
    #[test]
    fn a_final_scan_state_survives_a_full_queue() {
        let (tx, rx) = sync_channel::<Message>(QUEUE);
        let slot = ScanSlot::default();
        let alert = || Message::Request(Box::new(dyno_core::protocol::Request::Health));
        for _ in 0..QUEUE {
            tx.try_send(alert()).unwrap();
        }
        // Statuses arrive while the queue is full: their wake-ups are refused, as the real broadcast does.
        for (seq, state) in [
            (1, ScanState::Running),
            (2, ScanState::Running),
            (3, ScanState::Done),
            (4, ScanState::Running),
        ] {
            let s = status(7, seq, state);
            slot.put(s.clone());
            assert!(tx.try_send(Message::DiskScan(Box::new(s))).is_err(), "the queue is full");
        }
        slot.put(status(8, 1, ScanState::Cancelled));
        slot.put(status(7, 2, ScanState::Running)); // an older event arriving late changes nothing
        drop(tx);
        let mut got = Vec::new();
        pump(&rx, &slot, |m| {
            got.push(m);
            true
        });
        let scans: Vec<_> = got
            .iter()
            .filter_map(|m| {
                if let Message::DiskScan(s) = m {
                    Some((s.scan, s.seq))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(scans, vec![(7, 4), (8, 1)], "the newest event of each scan, once");
        assert_eq!(got.len(), QUEUE + 2, "the other frames are all there too");
    }

    #[test]
    fn the_final_state_wins_over_everything_before_it() {
        let slot = ScanSlot::default();
        slot.put(status(1, 5, ScanState::Done));
        slot.put(status(1, 4, ScanState::Running));
        let taken = slot.take();
        assert_eq!((taken.len(), taken[0].state.clone()), (1, ScanState::Done));
        assert!(slot.take().is_empty());
    }

    #[test]
    fn the_pump_stops_when_the_app_is_gone() {
        let (tx, rx) = sync_channel::<Message>(QUEUE);
        let slot = ScanSlot::default();
        slot.put(status(1, 1, ScanState::Done));
        tx.try_send(Message::DiskScan(Box::new(status(1, 1, ScanState::Done)))).unwrap();
        tx.try_send(Message::Request(Box::new(dyno_core::protocol::Request::Health)))
            .unwrap();
        pump(&rx, &slot, |_| false);
        slot.put(status(2, 1, ScanState::Done));
        tx.try_send(Message::Request(Box::new(dyno_core::protocol::Request::Health)))
            .unwrap();
        let mut sent = 0;
        pump(&rx, &slot, |_| {
            sent += 1;
            false
        });
        assert_eq!(sent, 1);
    }
}
