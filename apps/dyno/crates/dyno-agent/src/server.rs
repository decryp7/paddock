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

use crate::agent::{lock, Shared, Sub};
use crate::discovery::Discovery;
use crate::handlers::{handle, Conn};
use dyno_core::protocol::{AgentError, ErrorCode, Message, PROTOCOL_VERSION};
use dyno_ipc::{Host, IpcError, Peer, PeerOptions, RemoteError};
use std::sync::atomic::AtomicU8;
use std::sync::mpsc::sync_channel;
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
        elevated_for: shared.session.for_user.clone().filter(|_| shared.session.elevated),
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
            Ok(conn) if !shared.session.caller_allowed(conn.peer_uid()) => {
                // Someone else's connection (another user on this Mac or Linux box): refused without reading anything.
                eprintln!("dyno-agent: refused a connection from another user");
            }
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
    lock(&shared.subs).push(Sub { id, detail, tx });
    // One thread per connection drains its queue; it ends when the app goes away or the Sub is dropped.
    let p = peer.clone();
    let s = shared.clone();
    std::thread::Builder::new()
        .name(format!("dyno-push-{id}"))
        .spawn(move || {
            while let Ok(m) = rx.recv() {
                if p.push(m).is_err() {
                    break;
                }
            }
            lock(&s.subs).retain(|x| x.id != id);
        })
        .map_err(|e| IpcError::io(e.kind(), format!("could not start the push thread: {e}")))?;
    Ok((id, peer))
}

/// The error a request gets when it can't be read at all.
pub fn unreadable() -> AgentError {
    AgentError::new(ErrorCode::BadRequest, "Dyno could not read that request.")
}
