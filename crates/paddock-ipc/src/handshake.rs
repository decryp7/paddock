//! The handshake (ADR 0001 "private endpoint + token + peer-pid handshake"): the host listens and checks `Hello` (frame
//! version, protocol version, token in constant time, the claimed pid against what the OS reports); the client connects
//! and waits for `Welcome`. A refusal is a `Fail` with id 0 and a code from [`codes`], so the client can say why.

use crate::endpoint::{Access, Listener, Stream};
use crate::error::{IpcError, RemoteError, codes};
use crate::frame::{self, Frame, FrameError, FrameLimit, ProtocolVersion};
use crate::token::Token;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a client waits for a busy endpoint and for the `Welcome`, and a host for the `Hello` after a connect.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// An authenticated connection, ready for a [`Peer`](crate::Peer).
pub struct Connection {
    pub(crate) stream: Stream,
    pub(crate) limit: FrameLimit,
    peer_pid: Option<u32>,
    peer_uid: Option<u32>,
    protocol: ProtocolVersion,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("peer_pid", &self.peer_pid)
            .field("protocol", &self.protocol)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

impl Connection {
    /// A connection over `stream` without a handshake (both ends in one process, or a test that plays the other side).
    pub fn raw(stream: Stream, limit: FrameLimit) -> Connection {
        Connection {
            stream,
            limit,
            peer_pid: None,
            peer_uid: None,
            protocol: ProtocolVersion::UNVERSIONED,
        }
    }

    /// The other side's process id: what the OS reports, else what it claimed (host side); `None` on the client side.
    pub fn peer_pid(&self) -> Option<u32> {
        self.peer_pid
    }

    /// The other side's user id where the OS tells (Unix); `None` otherwise.
    pub fn peer_uid(&self) -> Option<u32> {
        self.peer_uid
    }

    /// The protocol the other side speaks (from its `Hello` or `Welcome`).
    pub fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    /// The frame limit of this connection.
    pub fn limit(&self) -> FrameLimit {
        self.limit
    }
}

/// The listening side: a private endpoint, the token a client must present and the protocol this side serves.
pub struct Host {
    listener: Listener,
    token: Token,
    limit: FrameLimit,
    protocol: ProtocolVersion,
    name: String,
}

impl Host {
    /// A fresh endpoint and token. Give [`Host::endpoint`] and [`Host::token`] to the child.
    pub fn listen(limit: FrameLimit, protocol: ProtocolVersion) -> io::Result<Host> {
        Host::with(Listener::bind()?, Token::random()?, limit, protocol)
    }

    /// [`Host::listen`] with the socket folder under `dir` (the app's runtime folder).
    pub fn listen_in(dir: &Path, limit: FrameLimit, protocol: ProtocolVersion) -> io::Result<Host> {
        Host::with(Listener::bind_in(dir)?, Token::random()?, limit, protocol)
    }

    /// A host on a fixed endpoint with a known token (a service whose clients find it through a discovery file).
    pub fn listen_at(
        name: &str,
        access: &Access,
        token: Token,
        limit: FrameLimit,
        protocol: ProtocolVersion,
    ) -> io::Result<Host> {
        Host::with(Listener::bind_fixed(name, access)?, token, limit, protocol)
    }

    fn with(
        listener: Listener,
        token: Token,
        limit: FrameLimit,
        protocol: ProtocolVersion,
    ) -> io::Result<Host> {
        Ok(Host {
            listener,
            token,
            limit,
            protocol,
            name: "app".into(),
        })
    }

    /// Names this side in version refusals ("this Debrief speaks 2.0"); "app" by default.
    pub fn named(mut self, name: impl Into<String>) -> Host {
        self.name = name.into();
        self
    }

    /// The endpoint name.
    pub fn endpoint(&self) -> &str {
        self.listener.name()
    }

    /// The token.
    pub fn token(&self) -> &Token {
        &self.token
    }

    /// The frame limit.
    pub fn limit(&self) -> FrameLimit {
        self.limit
    }

    /// The protocol this side serves.
    pub fn protocol(&self) -> ProtocolVersion {
        self.protocol
    }

    /// Waits up to `timeout` for a client and authenticates it.
    pub fn accept(&mut self, timeout: Duration) -> Result<Connection, IpcError> {
        self.accept_while(None, timeout, &mut || Ok(()))
    }

    /// Accepts within `timeout`, calling `alive` between short waits so the caller can stop early (the child exited),
    /// and, with `pid`, only the process `pid` (the child this host started).
    pub fn accept_while(
        &mut self,
        pid: Option<u32>,
        timeout: Duration,
        alive: &mut dyn FnMut() -> Result<(), IpcError>,
    ) -> Result<Connection, IpcError> {
        const SLICE: Duration = Duration::from_millis(50);
        let deadline = Instant::now().checked_add(timeout);
        let left = || {
            deadline.map_or(Duration::MAX, |d| {
                d.saturating_duration_since(Instant::now())
            })
        };
        let (mut stream, os_pid) = loop {
            alive()?;
            if left().is_zero() {
                return Err(IpcError::Timeout);
            }
            match self.listener.accept(left().min(SLICE)) {
                Ok(accepted) => break accepted,
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.into()),
            }
        };
        // macOS refuses socket options once the other side has gone: that client left.
        stream
            .set_read_timeout(Some(
                left().clamp(Duration::from_millis(1), HANDSHAKE_TIMEOUT),
            ))
            .map_err(|e| IpcError::Closed(format!("the client left before its hello ({e})")))?;
        let body = match frame::read_body(&mut stream, self.limit) {
            Ok(b) => b,
            Err(FrameError::TimedOut) => return Err(IpcError::Timeout),
            Err(FrameError::Closed) => {
                return Err(IpcError::Closed("the client left before its hello".into()));
            }
            Err(e) => return Err(IpcError::Protocol(e.to_string())),
        };
        let limit = self.limit;
        let refuse = |stream: &mut Stream, code: &str, message: &str| {
            let fail = Frame::Fail {
                id: 0,
                error: RemoteError::new(code, message),
            };
            let _ = frame::write_frame(stream, &fail, limit);
        };
        let Some((version, protocol)) = frame::handshake_versions(&body) else {
            return Err(IpcError::Protocol("the first frame was not a hello".into()));
        };
        if version != frame::VERSION {
            let m = format!(
                "the other side uses frame version {version}, this side {}; update the older one",
                frame::VERSION
            );
            refuse(&mut stream, codes::VERSION, &m);
            return Err(IpcError::Version(m));
        }
        let protocol = protocol.unwrap_or_default();
        if let Some(m) = self
            .protocol
            .mismatch(protocol, "the other side", &self.name)
        {
            refuse(&mut stream, codes::VERSION, &m);
            return Err(IpcError::Version(m));
        }
        let Ok(Frame::Hello {
            token,
            pid: claimed,
            ..
        }) = frame::decode_body(&body)
        else {
            return Err(IpcError::Protocol("the hello is not valid".into()));
        };
        if !self.token.matches(&token) {
            refuse(&mut stream, codes::AUTH, "wrong token");
            return Err(IpcError::Auth("the client presented a wrong token".into()));
        }
        if let Some(os) = os_pid
            && os != claimed
        {
            refuse(&mut stream, codes::AUTH, "wrong process");
            return Err(IpcError::Auth(format!(
                "the client claims pid {claimed} but the OS reports {os}"
            )));
        }
        if let Some(expected) = pid
            && os_pid.unwrap_or(claimed) != expected
        {
            refuse(&mut stream, codes::AUTH, "wrong process");
            return Err(IpcError::Auth(format!(
                "expected process {expected}, got {claimed}"
            )));
        }
        stream
            .set_read_timeout(None)
            .map_err(|e| IpcError::Closed(format!("the client left during the handshake ({e})")))?;
        let welcome = Frame::Welcome {
            version: frame::VERSION,
            protocol: self.protocol,
        };
        frame::write_frame(&mut stream, &welcome, self.limit)
            .map_err(|e| IpcError::Closed(e.to_string()))?;
        let peer_uid = stream.peer_uid();
        Ok(Connection {
            stream,
            limit: self.limit,
            peer_pid: os_pid.or(Some(claimed)),
            peer_uid,
            protocol,
        })
    }
}

/// The connecting side.
pub struct Client;

impl Client {
    /// Connects to `endpoint` and authenticates with `token`, speaking `protocol`.
    pub fn connect(
        endpoint: &str,
        token: &Token,
        limit: FrameLimit,
        protocol: ProtocolVersion,
    ) -> Result<Connection, IpcError> {
        let mut stream = Stream::connect(endpoint, HANDSHAKE_TIMEOUT)?;
        let hello = Frame::Hello {
            version: frame::VERSION,
            protocol,
            token: *token.as_bytes(),
            pid: std::process::id(),
        };
        frame::write_frame(&mut stream, &hello, limit)
            .map_err(|e| IpcError::Closed(e.to_string()))?;
        stream
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| IpcError::Closed(format!("the host left during the handshake ({e})")))?;
        match frame::read_frame(&mut stream, limit) {
            Ok(Frame::Welcome { protocol, .. }) => Ok(Connection {
                stream: {
                    stream.set_read_timeout(None).map_err(|e| {
                        IpcError::Closed(format!("the host left during the handshake ({e})"))
                    })?;
                    stream
                },
                limit,
                peer_pid: None,
                peer_uid: None,
                protocol,
            }),
            Ok(Frame::Fail { error, .. }) if error.is(codes::VERSION) => {
                Err(IpcError::Version(error.message))
            }
            Ok(Frame::Fail { error, .. }) if error.is(codes::AUTH) => {
                Err(IpcError::Auth(error.message))
            }
            Ok(other) => Err(IpcError::Protocol(format!(
                "expected a welcome, got {other:?}"
            ))),
            Err(FrameError::Closed) => Err(IpcError::Closed(
                "the host closed the connection during the handshake".into(),
            )),
            Err(FrameError::TimedOut) => Err(IpcError::Timeout),
            Err(FrameError::Io(m)) => Err(IpcError::Closed(m)),
            Err(e) => Err(IpcError::Protocol(e.to_string())),
        }
    }
}
