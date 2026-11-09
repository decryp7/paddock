//! The scenarios for endpoints that speak `paddock-ipc` frames: a black-box suite any such endpoint can be given
//! ([`IpcTarget`]: what a client needs to connect), and the in-process suite for the crate's own host and peer, which
//! needs the server's inside (a fake clock, a refusal before the hello, a listener dropped with a client in its backlog).
//!
//! Every scenario returns what a raw client saw below the library ([`Observed::raw`]), decoded as frames.

use super::raw::{READ_GUARD, RawEndKind, RawStream, read_to_end, write_junk};
use super::{Decoded, Expect, Observed, Scenario, Typed};
use crate::endpoint::{self, Stream};
use crate::error::{IpcError, codes};
use crate::frame::{self, Frame, FrameLimit, ProtocolVersion};
use crate::handshake::{Client, Connection, Host, PeerCredentials};
use crate::peer::{CallCtx, Handler, Peer, PeerOptions};
use crate::token::Token;
use paddock_jobs::time::FakeClock;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The wait for something that must already have happened.
const GUARD: Duration = Duration::from_secs(30);

/// What a client needs to reach an endpoint that speaks `paddock-ipc`.
#[derive(Clone)]
pub struct IpcTarget {
    /// The endpoint name (a socket path or a pipe name).
    pub endpoint: String,
    /// The token to present.
    pub token: Token,
    /// The frame limit.
    pub limit: FrameLimit,
    /// The protocol the endpoint serves.
    pub protocol: ProtocolVersion,
}

/// Decodes `bytes` as frames: every `Fail { id: 0 }` is a reason.
pub fn decode_frames(bytes: &[u8], limit: FrameLimit) -> Decoded {
    let mut d = Decoded::default();
    let mut at = 0;
    while at < bytes.len() {
        match frame::decode(&bytes[at..], limit) {
            Ok(Some((f, used))) => {
                at += used;
                match f {
                    Frame::Fail { id: 0, error } => {
                        d.reasons.push(error.code);
                        d.after_last_reason = 0;
                    }
                    _ if !d.reasons.is_empty() => d.after_last_reason += 1,
                    _ => {}
                }
            }
            Ok(None) | Err(_) => {
                d.partial_tail = bytes.len() - at;
                break;
            }
        }
    }
    d
}

fn observed(raw: super::raw::RawEnd, limit: FrameLimit) -> Observed {
    let decoded = decode_frames(&raw.bytes, limit);
    Observed {
        raw: Some(raw),
        decoded,
        ..Observed::default()
    }
}

fn hello(target: &IpcTarget, token: &Token, version: u16) -> Frame {
    Frame::Hello {
        version,
        protocol: target.protocol,
        token: *token.as_bytes(),
        pid: std::process::id(),
    }
}

fn raw_client(target: &IpcTarget) -> Result<Stream, &'static str> {
    Stream::connect(&target.endpoint, GUARD).map_err(|_| "the endpoint could not be reached")
}

/// A client presents `hello` and reads to the end of the stream.
fn refused_by(
    target: &IpcTarget,
    first: &[u8],
    keep_writing: bool,
) -> Result<Observed, &'static str> {
    use std::io::Write;
    let mut c = raw_client(target)?;
    c.write_all(first)
        .map_err(|_| "the first frame could not be written")?;
    if keep_writing {
        write_junk(&mut c, super::KEEP_WRITING_BYTES);
    }
    let raw = read_to_end(&mut c, READ_GUARD);
    Ok(observed(raw, target.limit))
}

/// A frame with a valid length prefix and a body that is nothing the protocol knows.
fn garbage_frame() -> Vec<u8> {
    let body = [0xffu8; 8];
    let mut v = (body.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(&body);
    v
}

/// The black-box scenarios of a client against `target`: a wrong token, another version, a first frame that is not a hello,
/// and a refused client that keeps writing. `None` for a scenario this suite does not cover.
pub fn black_box(
    target: &IpcTarget,
    scenario: Scenario,
) -> Option<Result<(Observed, Expect), &'static str>> {
    let wrong = Token::from_bytes([0x11; crate::token::TOKEN_BYTES]);
    let encode = |f: &Frame| frame::encode(f, target.limit).map_err(|_| "cannot encode");
    let run = || -> Result<(Observed, Expect), &'static str> {
        match scenario {
            Scenario::WrongToken => {
                let first = encode(&hello(target, &wrong, frame::VERSION))?;
                Ok((
                    refused_by(target, &first, false)?,
                    Expect::reason(&[codes::AUTH]),
                ))
            }
            Scenario::WrongVersion => {
                let first = encode(&hello(target, &target.token, frame::VERSION + 1))?;
                Ok((
                    refused_by(target, &first, false)?,
                    Expect::reason(&[codes::VERSION]),
                ))
            }
            Scenario::MalformedFirstFrame => {
                // `invalid` is the stopgap code of ADR 0072 I7; M3 maps it to `protocol`.
                Ok((
                    refused_by(target, &garbage_frame(), false)?,
                    Expect::reason(&["protocol", codes::INVALID]),
                ))
            }
            Scenario::ClientKeepsWriting => {
                let first = encode(&hello(target, &wrong, frame::VERSION))?;
                Ok((
                    refused_by(target, &first, true)?,
                    Expect::reason(&[codes::AUTH]).with_unread_input(),
                ))
            }
            _ => Err("not part of the black-box suite"),
        }
    };
    matches!(
        scenario,
        Scenario::WrongToken
            | Scenario::WrongVersion
            | Scenario::MalformedFirstFrame
            | Scenario::ClientKeepsWriting
    )
    .then(run)
}

/// The scenario "the endpoint stops with a client connected": connects with the real client, calls `stop`, and reads the
/// raw stream of the connection to its end.
pub fn stop_with_open_connection(
    target: &IpcTarget,
    stop: impl FnOnce(),
) -> Result<(Observed, Expect), &'static str> {
    let mut conn = Client::connect(
        &target.endpoint,
        &target.token,
        target.limit,
        target.protocol,
    )
    .map_err(|_| "the client could not connect")?;
    stop();
    let raw = read_to_end(conn.stream_mut(), READ_GUARD);
    Ok((observed(raw, target.limit), Expect::reason(&["stopping"])))
}

/// Runs `serve` (the endpoint's accept and authenticate for one client) on a thread and returns its result when joined.
fn serve_one<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> std::thread::JoinHandle<T> {
    std::thread::spawn(f)
}

fn hello_bytes(host: &Host, token: &Token, version: u16) -> Vec<u8> {
    let f = Frame::Hello {
        version,
        protocol: host.protocol(),
        token: *token.as_bytes(),
        pid: std::process::id(),
    };
    frame::encode(&f, host.limit()).unwrap_or_default()
}

struct Uid(u32);
impl PeerCredentials for Uid {
    fn peer_uid(&self) -> Option<u32> {
        Some(self.0)
    }
}

/// Echo that finishes a call at once when it is cancelled: the handler of bug #1 (CI run 7761).
fn cancel_at_once(started: mpsc::Sender<()>) -> Arc<dyn Handler> {
    let started = Mutex::new(started);
    Arc::new(
        move |ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, crate::RemoteError> {
            started
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .send(())
                .ok();
            ctx.cancel_probe().wait();
            Err(CallCtx::cancelled_error())
        },
    )
}

fn typed_of(result: &Result<Vec<u8>, IpcError>) -> Typed {
    match result {
        Err(IpcError::Remote(e)) if e.is(codes::CANCELLED) => Typed {
            transport_only: false,
            cancelled: true,
        },
        Err(IpcError::Closed(_)) => Typed {
            transport_only: true,
            cancelled: false,
        },
        _ => Typed::default(),
    }
}

/// The in-process suite for `paddock-ipc`'s own host (accept, authenticate) and peer (the ordered close): the endpoint
/// register's `ipc-host` and `ipc-peer` entries.
pub fn in_process(scenario: Scenario) -> Result<(Observed, Expect), &'static str> {
    let limit = FrameLimit::default();
    match scenario {
        Scenario::WrongToken
        | Scenario::WrongVersion
        | Scenario::MalformedFirstFrame
        | Scenario::ClientKeepsWriting => {
            let mut host =
                Host::listen(limit, ProtocolVersion::UNVERSIONED).map_err(|_| "no endpoint")?;
            let target = IpcTarget {
                endpoint: host.endpoint().to_string(),
                token: host.token().clone(),
                limit,
                protocol: ProtocolVersion::UNVERSIONED,
            };
            let server = serve_one(move || drop(host.accept(GUARD)));
            let result = black_box(&target, scenario).ok_or("not covered")?;
            drop(server.join());
            result
        }
        Scenario::WrongAccount => account_scenario(true),
        Scenario::Busy => account_scenario(false),
        Scenario::SilentClient => {
            let clock = FakeClock::new();
            let host =
                Host::listen(limit, ProtocolVersion::UNVERSIONED).map_err(|_| "no endpoint")?;
            let mut host = host.with_clock(Arc::new(clock.clone()), Duration::from_secs(10));
            let mut client =
                Stream::connect(host.endpoint(), GUARD).map_err(|_| "cannot connect")?;
            let server = serve_one(move || drop(host.accept(Duration::from_secs(300))));
            // The handshake's watchdog is now waiting on the clock; the client has said nothing.
            if !clock.wait_for_pending(1, GUARD) {
                return Err("the handshake never started its limit");
            }
            clock.advance(Duration::from_secs(10));
            let raw = read_to_end(&mut client, READ_GUARD);
            drop(server.join());
            Ok((observed(raw, limit), Expect::reason(&["timeout"])))
        }
        Scenario::ListenerStopWithClientInBacklog => {
            let host =
                Host::listen(limit, ProtocolVersion::UNVERSIONED).map_err(|_| "no endpoint")?;
            let first = hello_bytes(&host, host.token(), frame::VERSION);
            let mut client =
                Stream::connect(host.endpoint(), GUARD).map_err(|_| "cannot connect")?;
            // The client is connected and waiting to be accepted; the listener stops.
            drop(host);
            let wrote = std::io::Write::write_all(&mut client, &first);
            let raw = read_to_end(&mut client, READ_GUARD);
            let mut o = observed(raw, limit);
            if wrote.is_err()
                && o.raw
                    .as_ref()
                    .is_some_and(|r| r.bytes.is_empty() && r.kind == RawEndKind::Eof)
            {
                // The write itself failed because the listener was already gone: still no reason.
                o.decoded = Decoded::default();
            }
            Ok((o, Expect::reason(&["stopping"])))
        }
        Scenario::OwnerCloseWithCallsInFlight
        | Scenario::OwnerCloseWhilePeerNotReading
        | Scenario::Stall => peer_scenario(scenario),
        Scenario::StopWithOpenConnection => {
            let (x, mut y) = endpoint::pair().map_err(|_| "no pair")?;
            let peer = Peer::start(
                Connection::raw(x, limit),
                Arc::new(crate::NoCalls),
                PeerOptions::new("owner"),
            )
            .map_err(|_| "no peer")?;
            peer.close();
            let raw = read_to_end(&mut y, READ_GUARD);
            Ok((
                observed(raw, limit),
                Expect::reason(&["stopping", "closed"]),
            ))
        }
        Scenario::MixedVersionPeerIgnoresFail0 => {
            // A peer of today's code (it ignores a `Fail { id: 0 }` after the handshake) with a call in flight must see the
            // owner's close as the end of its call, not wait for it.
            let (started_tx, started) = mpsc::channel();
            let (x, y) = endpoint::pair().map_err(|_| "no pair")?;
            let owner = Peer::start(
                Connection::raw(x, limit),
                cancel_at_once(started_tx),
                PeerOptions::new("owner"),
            )
            .map_err(|_| "no peer")?;
            let old = Peer::start(
                Connection::raw(y, limit),
                Arc::new(crate::NoCalls),
                PeerOptions::new("old peer"),
            )
            .map_err(|_| "no peer")?;
            let caller = old.clone();
            let call = std::thread::spawn(move || caller.call(b"x".to_vec(), Some(GUARD)));
            let hung = started.recv_timeout(GUARD).is_err();
            owner.close();
            let ended = !hung && old.wait_closed(Some(GUARD));
            drop(call.join());
            Ok((
                Observed {
                    hung: !ended,
                    ..Observed::default()
                },
                Expect {
                    reason: super::ReasonRule::Optional,
                    end_line: false,
                    unread_input: false,
                },
            ))
        }
    }
}

/// `wrong_account` (true) refuses a client of another account before its hello is read; otherwise the host is busy. Both
/// leave the client's hello unread.
fn account_scenario(wrong_account: bool) -> Result<(Observed, Expect), &'static str> {
    // Not run on Windows: the register declares it not applicable there (the OS gives no user id for a pipe client).
    let limit = FrameLimit::default();
    let host = Host::listen(limit, ProtocolVersion::UNVERSIONED).map_err(|_| "no endpoint")?;
    let mut host = if wrong_account {
        host.same_account_only()
    } else {
        host
    };
    let first = hello_bytes(&host, host.token(), frame::VERSION);
    let mut client = Stream::connect(host.endpoint(), GUARD).map_err(|_| "cannot connect")?;
    std::io::Write::write_all(&mut client, &first).map_err(|_| "cannot write")?;
    let auth = host.authenticator();
    let server = serve_one(move || {
        if let Ok((stream, pid)) = host.accept_raw(GUARD) {
            if wrong_account {
                drop(auth.authenticate_as(stream, pid, None, &Uid(u32::MAX - 1)));
            } else {
                auth.refuse_busy(stream, "too many connections; try again");
            }
        }
    });
    drop(server.join());
    let raw = read_to_end(&mut client, READ_GUARD);
    let code = if wrong_account {
        codes::AUTH
    } else {
        codes::BUSY
    };
    Ok((
        observed(raw, limit),
        Expect::reason(&[code]).with_unread_input(),
    ))
}

fn peer_scenario(scenario: Scenario) -> Result<(Observed, Expect), &'static str> {
    let limit = FrameLimit::default();
    match scenario {
        Scenario::OwnerCloseWithCallsInFlight => {
            // Raw: the owner closes with a call in flight; the stream must carry the reason last.
            let (started_tx, started) = mpsc::channel();
            let (x, mut y) = endpoint::pair().map_err(|_| "no pair")?;
            let owner = Peer::start(
                Connection::raw(x, limit),
                cancel_at_once(started_tx),
                PeerOptions::new("owner"),
            )
            .map_err(|_| "no peer")?;
            let call = frame::encode(
                &Frame::Call {
                    id: 1,
                    credit: 0,
                    body: b"x".to_vec(),
                },
                limit,
            )
            .map_err(|_| "encode")?;
            std::io::Write::write_all(&mut y, &call).map_err(|_| "write")?;
            if started.recv_timeout(GUARD).is_err() {
                return Err("the call never started");
            }
            owner.close();
            let raw = read_to_end(&mut y, READ_GUARD);
            let mut o = observed(raw, limit);
            // Typed: the same close, seen by a real peer whose call is in flight.
            let (started_tx, started) = mpsc::channel();
            let (x, y) = endpoint::pair().map_err(|_| "no pair")?;
            let owner = Peer::start(
                Connection::raw(x, limit),
                cancel_at_once(started_tx),
                PeerOptions::new("owner"),
            )
            .map_err(|_| "no peer")?;
            let caller = Peer::start(
                Connection::raw(y, limit),
                Arc::new(crate::NoCalls),
                PeerOptions::new("caller"),
            )
            .map_err(|_| "no peer")?;
            let c2 = caller.clone();
            let call = std::thread::spawn(move || c2.call(b"x".to_vec(), Some(GUARD)));
            if started.recv_timeout(GUARD).is_err() {
                return Err("the call never started");
            }
            owner.close();
            o.typed = call.join().ok().map(|r| typed_of(&r));
            Ok((o, Expect::reason(&["stopping", "closed"])))
        }
        Scenario::OwnerCloseWhilePeerNotReading => {
            let (x, mut y) = endpoint::pair().map_err(|_| "no pair")?;
            let owner = Peer::start(
                Connection::raw(x, limit),
                Arc::new(crate::NoCalls),
                PeerOptions::new("owner"),
            )
            .map_err(|_| "no peer")?;
            // 8 MiB of pushes to a peer that reads nothing: the socket's buffers fill and the writer blocks.
            for _ in 0..32 {
                if owner.push(vec![7u8; 256 << 10]).is_err() {
                    break;
                }
            }
            // The close must return at once (its caller never waits for a flush), and release the writer.
            let (done_tx, done) = mpsc::channel();
            let o2 = owner.clone();
            std::thread::spawn(move || {
                o2.close();
                done_tx.send(()).ok();
            });
            let hung = done.recv_timeout(GUARD).is_err();
            let raw = read_to_end(&mut y, READ_GUARD);
            let mut o = observed(raw, limit);
            o.hung = hung;
            Ok((o, Expect::optional_reason()))
        }
        Scenario::Stall => {
            let clock = FakeClock::new();
            let (x, mut y) = endpoint::pair().map_err(|_| "no pair")?;
            let mut opts = PeerOptions::new("owner");
            opts.clock = Arc::new(clock.clone());
            opts.write_queue = 2;
            opts.write_stall = Duration::from_secs(30);
            let owner = Peer::start(Connection::raw(x, limit), Arc::new(crate::NoCalls), opts)
                .map_err(|_| "no peer")?;
            let o2 = owner.clone();
            // Pushes to a peer that reads nothing until the queue is full and a send waits for room on the clock.
            let sender = std::thread::spawn(move || {
                let mut last = Ok(());
                for _ in 0..4096 {
                    last = o2.push(vec![7u8; 256 << 10]);
                    if last.is_err() {
                        break;
                    }
                }
                last
            });
            if !clock.wait_for_pending(1, GUARD) {
                return Err("the send never stalled");
            }
            clock.advance(Duration::from_secs(30));
            let result = sender.join().ok();
            let raw = read_to_end(&mut y, READ_GUARD);
            let mut o = observed(raw, limit);
            o.hung = result.is_none();
            // The reason may be unsent (the peer was not reading); the end must still be orderly.
            Ok((o, Expect::optional_reason()))
        }
        _ => Err("not a peer scenario"),
    }
}

/// Reads the raw stream of `conn` to its end (for a caller that holds a connection).
pub fn read_connection(conn: &mut Connection) -> super::raw::RawEnd {
    let stream: &mut dyn RawStream = conn.stream_mut();
    read_to_end(stream, READ_GUARD)
}
