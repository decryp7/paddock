//! The handshake: the host listens and checks `Hello`, the client connects and waits for `Welcome`.

use crate::endpoint::{Listener, Stream};
use crate::error::IpcError;
use crate::frame::{read_frame, write_frame, FrameLimit};
use crate::token::Token;
use crate::wire::{codes, Frame, RemoteError, PROTOCOL_VERSION};
use std::io;
use std::time::{Duration, Instant};

/// How long a client waits for a busy endpoint and for the `Welcome`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// An authenticated connection, ready for a [`Peer`](crate::Peer).
pub struct Connection {
    pub(crate) stream: Stream,
    pub(crate) limit: FrameLimit,
    peer_pid: Option<u32>,
    peer_uid: Option<u32>,
}

impl Connection {
    /// The other side's user id where the OS tells (Unix, host side); `None` otherwise.
    pub fn peer_uid(&self) -> Option<u32> {
        self.peer_uid
    }

    /// The other side's process id: what the OS reports (host side) or the host's `Hello` was answered by (`None` on
    /// the client side).
    pub fn peer_pid(&self) -> Option<u32> {
        self.peer_pid
    }

    /// The frame limit of this connection.
    pub fn limit(&self) -> FrameLimit {
        self.limit
    }

    /// The raw stream, for tests and diagnostics that must write bytes a [`Peer`](crate::Peer) never would.
    pub fn stream_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
}

/// The listening side: a private endpoint and the token the child must present.
pub struct Host {
    listener: Listener,
    token: Token,
    limit: FrameLimit,
}

impl Host {
    /// A host on a fixed endpoint with a known `token` (the privileged helper, ADR 0016: its clients are not its children).
    pub fn listen_at(name: &str, access: &crate::endpoint::Access, token: Token, limit: FrameLimit) -> io::Result<Host> {
        Ok(Host {
            listener: Listener::bind_fixed(name, access)?,
            token,
            limit,
        })
    }

    /// A fresh endpoint and token with the default frame limit. Give [`Host::name`] and [`Host::token`] to the child.
    pub fn listen() -> io::Result<Host> {
        Host::listen_with(FrameLimit::default())
    }

    /// [`Host::listen`] with a frame limit.
    pub fn listen_with(limit: FrameLimit) -> io::Result<Host> {
        Ok(Host {
            listener: Listener::bind()?,
            token: Token::random()?,
            limit,
        })
    }

    /// The endpoint name.
    pub fn name(&self) -> &str {
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

    /// Waits up to `timeout` for a client and authenticates it (token, version, and its claimed pid against the OS).
    pub fn accept(&mut self, timeout: Duration) -> Result<Connection, IpcError> {
        self.accept_inner(None, Instant::now() + timeout, &mut || Ok(()))
    }

    /// [`Host::accept`] that also requires the client to be the process `pid` (the child this host started).
    pub fn accept_from(&mut self, pid: u32, timeout: Duration) -> Result<Connection, IpcError> {
        self.accept_inner(Some(pid), Instant::now() + timeout, &mut || Ok(()))
    }

    /// Accepts until `deadline`, calling `alive` between short waits so a caller can stop early (the child exited).
    pub(crate) fn accept_inner(
        &mut self,
        pid: Option<u32>,
        deadline: Instant,
        alive: &mut dyn FnMut() -> Result<(), IpcError>,
    ) -> Result<Connection, IpcError> {
        const SLICE: Duration = Duration::from_millis(50);
        let started = Instant::now();
        let (mut stream, os_pid) = loop {
            alive()?;
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(IpcError::Timeout(deadline.saturating_duration_since(started)));
            }
            match self.listener.accept(left.min(SLICE)) {
                Ok(accepted) => break accepted,
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.into()),
            }
        };
        let left = handshake_timeout(deadline, Instant::now());
        stream.set_read_timeout(Some(left))?;
        let hello = match read_frame::<Frame<()>>(&mut stream, self.limit) {
            Ok(Some(f)) => f,
            Ok(None) => return Err(IpcError::Closed),
            Err(IpcError::Io {
                kind: io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock,
                ..
            }) => return Err(IpcError::Timeout(left)),
            Err(e) => return Err(e),
        };
        let Frame::Hello {
            version,
            token,
            pid: claimed,
        } = hello
        else {
            return Err(IpcError::Protocol("the first frame was not a Hello".into()));
        };
        let refuse = |stream: &mut Stream, code: &str, message: String| {
            let fail: Frame<()> = Frame::Fail {
                id: 0,
                error: RemoteError::new(code, message),
            };
            let _ = write_frame(stream, &fail, self.limit);
        };
        if version != PROTOCOL_VERSION {
            let e = IpcError::Version {
                ours: PROTOCOL_VERSION,
                theirs: version,
            };
            refuse(&mut stream, codes::VERSION, e.to_string());
            return Err(e);
        }
        if !self.token.matches(&token) {
            refuse(&mut stream, codes::AUTH, "wrong token".into());
            return Err(IpcError::Auth("the client presented a wrong token".into()));
        }
        if let Some(os) = os_pid {
            if os != claimed {
                refuse(&mut stream, codes::AUTH, "wrong process".into());
                return Err(IpcError::Auth(format!("the client claims pid {claimed} but the OS reports {os}")));
            }
        }
        if let Some(expected) = pid {
            if os_pid.unwrap_or(claimed) != expected {
                refuse(&mut stream, codes::AUTH, "wrong process".into());
                return Err(IpcError::Auth(format!("expected process {expected}, got {claimed}")));
            }
        }
        stream.set_read_timeout(None)?;
        write_frame::<Frame<()>>(&mut stream, &Frame::Welcome { version: PROTOCOL_VERSION }, self.limit)?;
        let peer_uid = stream.peer_uid();
        Ok(Connection {
            stream,
            limit: self.limit,
            peer_pid: os_pid.or(Some(claimed)),
            peer_uid,
        })
    }
}

/// How long an accepted client has to send its Hello. The wait for a client ends at `deadline`; a client that connected just
/// before it still gets the whole handshake time (as long as a client waits for the answer), instead of what was left of the
/// wait. Otherwise a client that arrived in the last moment of a short accept window was dropped before its Hello was read
/// and saw a reset connection instead of an answer (the helper accepts in 500 ms windows).
fn handshake_timeout(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now).max(CONNECT_TIMEOUT)
}

/// The connecting side.
pub struct Client;

impl Client {
    /// Connects to `name` and authenticates with `token`, with the default frame limit.
    pub fn connect(name: &str, token: &Token) -> Result<Connection, IpcError> {
        Client::connect_with(name, token, FrameLimit::default(), PROTOCOL_VERSION)
    }

    /// [`Client::connect`] with a frame limit and a protocol version (tests use another version to see the refusal).
    pub fn connect_with(name: &str, token: &Token, limit: FrameLimit, version: u32) -> Result<Connection, IpcError> {
        let mut stream = Stream::connect(name, CONNECT_TIMEOUT)?;
        let hello: Frame<()> = Frame::Hello {
            version,
            token: *token.as_bytes(),
            pid: std::process::id(),
        };
        write_frame(&mut stream, &hello, limit)?;
        stream.set_read_timeout(Some(CONNECT_TIMEOUT))?;
        let answer = match read_frame::<Frame<()>>(&mut stream, limit) {
            Ok(Some(f)) => f,
            Ok(None) => return Err(IpcError::Auth("the host closed the connection during the handshake".into())),
            Err(IpcError::Io {
                kind: io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock,
                ..
            }) => return Err(IpcError::Timeout(CONNECT_TIMEOUT)),
            Err(e) => return Err(e),
        };
        match answer {
            // Only now: macOS refuses setsockopt (EINVAL) on a socket whose peer already refused and closed.
            Frame::Welcome { version: v } if v == version => {
                stream.set_read_timeout(None)?;
                Ok(Connection {
                    stream,
                    limit,
                    peer_pid: None,
                    peer_uid: None,
                })
            }
            Frame::Welcome { version: theirs } | Frame::Hello { version: theirs, .. } => Err(IpcError::Version { ours: version, theirs }),
            Frame::Fail { error, .. } if error.code == codes::VERSION => Err(IpcError::Version {
                ours: version,
                theirs: PROTOCOL_VERSION,
            }),
            Frame::Fail { error, .. } => Err(IpcError::Auth(error.message)),
            _ => Err(IpcError::Protocol("the host did not answer the Hello".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn host() -> (Host, String, Token) {
        let h = Host::listen().unwrap();
        let (name, token) = (h.name().to_string(), h.token().clone());
        (h, name, token)
    }

    #[test]
    fn a_client_that_connects_at_the_end_of_the_wait_gets_the_whole_handshake_time() {
        let now = Instant::now();
        assert_eq!(handshake_timeout(now, now), CONNECT_TIMEOUT);
        assert_eq!(
            handshake_timeout(now, now + Duration::from_secs(1)),
            CONNECT_TIMEOUT,
            "a wait already over"
        );
        assert_eq!(
            handshake_timeout(now + Duration::from_secs(60), now),
            Duration::from_secs(60),
            "a longer wait is kept"
        );
    }

    #[test]
    fn the_right_token_is_welcomed() {
        let (mut h, name, token) = host();
        let c = std::thread::spawn(move || Client::connect(&name, &token));
        let conn = h.accept_from(std::process::id(), Duration::from_secs(5)).unwrap();
        assert_eq!(conn.peer_pid(), Some(std::process::id()));
        assert_eq!(conn.limit(), FrameLimit::default());
        let client = c.join().unwrap().unwrap();
        assert_eq!(client.peer_pid(), None);
    }

    /// A fixed endpoint name that cannot clash with another test or a real endpoint.
    fn fixed_name() -> String {
        let id = crate::token::random_hex(6).unwrap();
        if cfg!(windows) {
            format!(r"\\.\pipe\dyno-test-fixed-{id}")
        } else {
            std::env::temp_dir()
                .join(format!("dyno-fx-{id}.sock"))
                .to_string_lossy()
                .into_owned()
        }
    }

    fn open_access() -> crate::endpoint::Access {
        crate::endpoint::Access {
            mode: 0o600,
            sddl: "D:P(A;;GA;;;OW)".into(),
        }
    }

    #[test]
    fn a_fixed_endpoint_with_its_own_token_welcomes_a_client_and_tells_the_users_id() {
        let name = fixed_name();
        let token = Token::random().unwrap();
        let mut h = Host::listen_at(&name, &open_access(), token.clone(), FrameLimit::default()).unwrap();
        assert_eq!(h.name(), name);
        let n = name.clone();
        let c = std::thread::spawn(move || Client::connect(&n, &token));
        let conn = h.accept(Duration::from_secs(5)).unwrap();
        c.join().unwrap().unwrap();
        #[cfg(unix)]
        {
            // The test process connected to itself, so the kernel's answer is the owner of the socket file it created.
            use std::os::unix::fs::MetadataExt;
            assert_eq!(conn.peer_uid(), Some(std::fs::metadata(&name).unwrap().uid()));
        }
        #[cfg(windows)]
        assert_eq!(conn.peer_uid(), None);
        drop(conn);
        drop(h);
        #[cfg(unix)]
        assert!(!std::path::Path::new(&name).exists(), "the socket file goes away with the host");
    }

    #[test]
    fn a_fixed_endpoint_refuses_a_second_host_and_a_wrong_token() {
        let name = fixed_name();
        let token = Token::random().unwrap();
        let mut h = Host::listen_at(&name, &open_access(), token, FrameLimit::default()).unwrap();
        #[cfg(windows)]
        assert!(Host::listen_at(&name, &open_access(), Token::random().unwrap(), FrameLimit::default()).is_err());
        let n = name.clone();
        let c = std::thread::spawn(move || Client::connect(&n, &Token::from_bytes([1; 32])));
        assert!(matches!(h.accept(Duration::from_secs(5)), Err(IpcError::Auth(_))));
        assert!(c.join().unwrap().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_fixed_socket_replaces_a_stale_socket_but_never_another_kind_of_file() {
        let name = fixed_name();
        // A leftover socket (a crashed helper) is replaced.
        let stale = std::os::unix::net::UnixListener::bind(&name).unwrap();
        drop(stale);
        let h = Host::listen_at(&name, &open_access(), Token::random().unwrap(), FrameLimit::default()).unwrap();
        drop(h);
        // A regular file is not touched.
        std::fs::write(&name, b"keep").unwrap();
        let err = Host::listen_at(&name, &open_access(), Token::random().unwrap(), FrameLimit::default())
            .err()
            .unwrap();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&name).unwrap(), b"keep");
        std::fs::remove_file(&name).unwrap();
        // A path that cannot be a socket path.
        let long = format!("{}/{}", std::env::temp_dir().display(), "x".repeat(120));
        assert!(Host::listen_at(&long, &open_access(), Token::random().unwrap(), FrameLimit::default()).is_err());
    }

    #[test]
    fn a_wrong_token_is_refused_on_both_sides() {
        let (mut h, name, _) = host();
        let c = std::thread::spawn(move || Client::connect(&name, &Token::from_bytes([7; 32])));
        assert!(matches!(h.accept(Duration::from_secs(5)), Err(IpcError::Auth(_))));
        assert!(matches!(c.join().unwrap(), Err(IpcError::Auth(_))));
    }

    #[test]
    fn a_wrong_version_is_refused_on_both_sides() {
        let (mut h, name, token) = host();
        let c = std::thread::spawn(move || Client::connect_with(&name, &token, FrameLimit::default(), PROTOCOL_VERSION + 1));
        assert!(matches!(h.accept(Duration::from_secs(5)), Err(IpcError::Version { theirs, .. }) if theirs == PROTOCOL_VERSION + 1));
        assert!(matches!(c.join().unwrap(), Err(IpcError::Version { .. })));
    }

    #[test]
    fn another_process_than_the_child_is_refused() {
        let (mut h, name, token) = host();
        let c = std::thread::spawn(move || Client::connect(&name, &token));
        assert!(matches!(
            h.accept_from(std::process::id().wrapping_add(1), Duration::from_secs(5)),
            Err(IpcError::Auth(_))
        ));
        assert!(c.join().unwrap().is_err());
    }

    #[test]
    fn nobody_connecting_times_out_and_a_client_that_hangs_up_unheard_is_refused() {
        let (mut h, name, _) = host();
        assert!(matches!(h.accept(Duration::from_millis(80)), Err(IpcError::Timeout(_))));
        // Connected and gone before saying anything: an error, never a connection (closed, or on macOS the socket option
        // refused on a socket whose peer left). A silent client that stays is bounded by handshake_timeout.
        drop(Stream::connect(&name, Duration::from_secs(5)).unwrap());
        assert!(h.accept(Duration::from_millis(80)).is_err());
    }

    #[test]
    fn garbage_instead_of_hello_is_a_protocol_error() {
        let (mut h, name, _) = host();
        let c = std::thread::spawn(move || {
            let mut s = Stream::connect(&name, Duration::from_secs(5)).unwrap();
            s.write_all(&[0xff, 0xff, 0xff, 0x7f]).unwrap();
            s
        });
        assert!(matches!(h.accept(Duration::from_secs(5)), Err(IpcError::Protocol(_))));
        drop(c.join());
        let (mut h, name, _) = host();
        let c = std::thread::spawn(move || {
            let mut s = Stream::connect(&name, Duration::from_secs(5)).unwrap();
            write_frame::<Frame<()>>(&mut s, &Frame::Welcome { version: 1 }, FrameLimit::default()).unwrap();
            s
        });
        assert!(matches!(h.accept(Duration::from_secs(5)), Err(IpcError::Protocol(m)) if m.contains("Hello")));
        drop(c.join());
    }

    #[test]
    fn a_client_whose_host_vanishes_fails() {
        let (mut h, name, token) = host();
        let c = std::thread::spawn(move || Client::connect(&name, &token));
        // Accept the raw connection, then drop it without answering.
        let accepted = h.listener.accept(Duration::from_secs(5)).unwrap();
        drop(accepted);
        assert!(c.join().unwrap().is_err());
    }
}
