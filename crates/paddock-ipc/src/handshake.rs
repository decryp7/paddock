//! The handshake (ADR 0001 "private endpoint + token + peer-pid handshake"): the host listens and checks `Hello` (frame
//! version, protocol version, token in constant time, the claimed pid against what the OS reports); the client connects
//! and waits for `Welcome`. A refusal is a `Fail` with id 0 and a code from [`codes`], so the client can say why.
//!
//! Every limit runs on the injected [`Clock`]: the accept wait, and each handshake, which a watchdog ends by closing the
//! stream when its time is up (so a silent client holds only its own handshake, never the listener). An
//! [`Authenticator`] is cheap to clone, so a server authenticates each client on a thread of its own.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::endpoint::{Access, Listener, Stream};
use crate::error::{IpcError, RemoteError, codes};
use crate::frame::{self, Frame, FrameError, FrameLimit, ProtocolVersion};
use crate::token::Token;
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline, SystemClock};
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How long a client has for its `Hello` and a host for its `Welcome`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long one OS accept waits before the accept loop looks at its clock and its `alive` check again.
const ACCEPT_SLICE: Duration = Duration::from_millis(50);

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

    /// The raw stream, for tests and diagnostics that must write bytes a [`Peer`](crate::Peer) never would.
    pub fn stream_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
}

/// Runs `f` on `stream` with a watchdog on `clock`: when `limit` passes first, the stream is closed (the blocked read
/// ends) and the result is [`IpcError::Timeout`].
fn bounded<T>(
    stream: &mut Stream,
    clock: &Arc<dyn Clock>,
    limit: Duration,
    f: impl FnOnce(&mut Stream) -> Result<T, IpcError>,
) -> Result<T, IpcError> {
    let closer = stream.closer()?;
    let done = CancelToken::new();
    let fired = Arc::new(AtomicBool::new(false));
    let (clock, d, flag) = (clock.clone(), done.clone(), fired.clone());
    let watchdog = std::thread::Builder::new()
        .name("paddock-ipc-handshake-watchdog".into())
        .spawn(move || {
            if clock.wait(limit, &d).is_ok() {
                flag.store(true, Ordering::SeqCst);
                closer.close();
            }
        })
        .map_err(|e| IpcError::Spawn(format!("cannot start a handshake watchdog: {e}")))?;
    let result = f(stream);
    done.cancel();
    paddock_proc::cleanup::join("the handshake watchdog", watchdog);
    if fired.load(Ordering::SeqCst) {
        return Err(IpcError::Timeout);
    }
    result
}

/// The refusal a client gets for `e`, decided after [`bounded`] returned, so nothing is written to a stream its watchdog
/// closed (that is [`IpcError::Timeout`]): `busy` when this side could not start the handshake ([`bounded`] fails before the
/// hello is read only when it cannot copy the stream or start its watchdog: no descriptor or thread left), `invalid` for a
/// hello that could not be read. `None` for the checks that answered the client themselves (version, auth) and for a stream
/// that is gone. `invalid` is a stopgap of ADR 0072 (I7) that M3 maps to `protocol`.
fn refusal(e: &IpcError) -> Option<(&'static str, String)> {
    match e {
        IpcError::Spawn(why) | IpcError::Io { message: why, .. } => Some((
            codes::BUSY,
            format!("the host cannot take a connection now ({why}); try again later"),
        )),
        IpcError::Protocol(why) => Some((codes::INVALID, why.clone())),
        _ => None,
    }
}

/// Where the user id of the other end of a connection comes from (IP1 item 5): the OS for a [`Stream`], a fake in tests.
pub trait PeerCredentials {
    /// The user id of the process on the other end where the OS tells (Unix); `None` otherwise.
    fn peer_uid(&self) -> Option<u32>;
}

impl PeerCredentials for Stream {
    fn peer_uid(&self) -> Option<u32> {
        Stream::peer_uid(self)
    }
}

/// This process's own user id (Unix); `None` elsewhere, where the endpoint's access list decides who may connect.
fn own_uid() -> Option<u32> {
    #[cfg(unix)]
    {
        // SAFETY: `geteuid` has no arguments and cannot fail.
        #[allow(unsafe_code)]
        Some(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// What a host checks in a `Hello`: cheap to clone, so each client is authenticated on its own thread.
#[derive(Clone)]
pub struct Authenticator {
    token: Token,
    limit: FrameLimit,
    protocol: ProtocolVersion,
    name: String,
    clock: Arc<dyn Clock>,
    timeout: Duration,
    /// Refuse a client that runs as another user (Unix); off for a service that other accounts connect to.
    same_account: bool,
}

impl std::fmt::Debug for Authenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authenticator")
            .field("protocol", &self.protocol)
            .field("name", &self.name)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Authenticator {
    /// Answers a client that cannot be served now (too many connections) with a `busy` refusal and closes it.
    pub fn refuse_busy(&self, mut stream: Stream, message: &str) {
        self.refuse(&mut stream, codes::BUSY, message);
    }

    /// Authenticates a client accepted on the listener (its pid as the OS reported it) within this authenticator's
    /// handshake limit on its clock; with `pid`, only that process.
    pub fn authenticate(
        &self,
        stream: Stream,
        os_pid: Option<u32>,
        pid: Option<u32>,
    ) -> Result<Connection, IpcError> {
        let uid = Stream::peer_uid(&stream);
        self.authenticate_with_uid(stream, os_pid, pid, uid)
    }

    /// [`Authenticator::authenticate`] with the user id taken from `credentials`. **Test support**: it lets the caller say
    /// whose uid the client has, so it is hidden from the documentation and must not be used outside tests.
    #[doc(hidden)]
    pub fn authenticate_as(
        &self,
        stream: Stream,
        os_pid: Option<u32>,
        pid: Option<u32>,
        credentials: &dyn PeerCredentials,
    ) -> Result<Connection, IpcError> {
        self.authenticate_with_uid(stream, os_pid, pid, credentials.peer_uid())
    }

    fn authenticate_with_uid(
        &self,
        mut stream: Stream,
        os_pid: Option<u32>,
        pid: Option<u32>,
        peer_uid: Option<u32>,
    ) -> Result<Connection, IpcError> {
        // Before anything is read: a client of another account never gets to present a token.
        if let Err(e) = self.check_account(peer_uid) {
            self.refuse(
                &mut stream,
                codes::AUTH,
                "this host serves its own account only",
            );
            return Err(e);
        }
        let checked = bounded(&mut stream, &self.clock, self.timeout, |s| {
            self.check_hello(s, os_pid, pid)
        });
        let (protocol, claimed) = match checked {
            Ok(checked) => checked,
            Err(e) => {
                // Every refusal says why (CI run 7763): `check_hello` answers version and auth itself; the rest is
                // answered here, else the client only sees the stream end and cannot tell a busy host from a crashed one.
                if let Some((code, why)) = refusal(&e) {
                    self.refuse(&mut stream, code, &why);
                }
                return Err(e);
            }
        };
        Ok(Connection {
            stream,
            limit: self.limit,
            peer_pid: os_pid.or(Some(claimed)),
            peer_uid,
            protocol,
        })
    }

    fn check_account(&self, peer: Option<u32>) -> Result<(), IpcError> {
        let Some(me) = own_uid().filter(|_| self.same_account) else {
            return Ok(());
        };
        match peer {
            Some(uid) if uid == me => Ok(()),
            Some(uid) => Err(IpcError::Auth(format!(
                "the client runs as another user (uid {uid}; this side runs as {me})"
            ))),
            None => Err(IpcError::Auth(
                "the user id of the client could not be read".into(),
            )),
        }
    }

    fn refuse(&self, stream: &mut Stream, code: &str, message: &str) {
        let fail = Frame::Fail {
            id: 0,
            error: RemoteError::new(code, message),
        };
        paddock_proc::cleanup::logged(
            "telling the peer why the handshake was refused",
            frame::write_frame(stream, &fail, self.limit),
        );
    }

    fn check_hello(
        &self,
        stream: &mut Stream,
        os_pid: Option<u32>,
        pid: Option<u32>,
    ) -> Result<(ProtocolVersion, u32), IpcError> {
        let body = match frame::read_body(stream, self.limit) {
            Ok(b) => b,
            Err(FrameError::Closed) => {
                return Err(IpcError::Closed("the client left before its hello".into()));
            }
            Err(e) => return Err(IpcError::Protocol(e.to_string())),
        };
        let Some((version, protocol)) = frame::handshake_versions(&body) else {
            return Err(IpcError::Protocol("the first frame was not a hello".into()));
        };
        if version != frame::VERSION {
            let m = format!(
                "the other side uses frame version {version}, this side {}; update the older one",
                frame::VERSION
            );
            self.refuse(stream, codes::VERSION, &m);
            return Err(IpcError::Version(m));
        }
        let protocol = protocol.unwrap_or_default();
        if let Some(m) = self
            .protocol
            .mismatch(protocol, "the other side", &self.name)
        {
            self.refuse(stream, codes::VERSION, &m);
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
            self.refuse(stream, codes::AUTH, "wrong token");
            return Err(IpcError::Auth("the client presented a wrong token".into()));
        }
        if let Some(os) = os_pid
            && os != claimed
        {
            self.refuse(stream, codes::AUTH, "wrong process");
            return Err(IpcError::Auth(format!(
                "the client claims pid {claimed} but the OS reports {os}"
            )));
        }
        if let Some(expected) = pid
            && os_pid.unwrap_or(claimed) != expected
        {
            self.refuse(stream, codes::AUTH, "wrong process");
            return Err(IpcError::Auth(format!(
                "expected process {expected}, got {claimed}"
            )));
        }
        let welcome = Frame::Welcome {
            version: frame::VERSION,
            protocol: self.protocol,
        };
        frame::write_frame(stream, &welcome, self.limit)
            .map_err(|e| IpcError::Closed(e.to_string()))?;
        Ok((protocol, claimed))
    }
}

/// The listening side: a private endpoint, the token a client must present and the protocol this side serves.
pub struct Host {
    listener: Listener,
    auth: Authenticator,
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
            auth: Authenticator {
                token,
                limit,
                protocol,
                name: "app".into(),
                clock: Arc::new(SystemClock),
                timeout: HANDSHAKE_TIMEOUT,
                same_account: false,
            },
        })
    }

    /// Names this side in version refusals ("this Debrief speaks 2.0"); "app" by default.
    pub fn named(mut self, name: impl Into<String>) -> Host {
        self.auth.name = name.into();
        self
    }

    /// Refuses a client that does not run as the account of this process (Unix; IP1 item 5). For a host whose clients
    /// are its own children or the user's own helpers; a service that other accounts connect to leaves it off.
    ///
    /// **On Windows this checks nothing**: the OS gives no user id for a named pipe's client here, and the pipe's access
    /// list (`endpoint::Access::owner_only()`, a DACL) is the protection. Use it together with that list, not instead.
    pub fn same_account_only(mut self) -> Host {
        self.auth.same_account = true;
        self
    }

    /// The clock the accept wait and each handshake's limit run on, and that limit ([`HANDSHAKE_TIMEOUT`] by default).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>, handshake: Duration) -> Host {
        self.auth.clock = clock;
        self.auth.timeout = handshake;
        self
    }

    /// The endpoint name.
    pub fn endpoint(&self) -> &str {
        self.listener.name()
    }

    /// The token.
    pub fn token(&self) -> &Token {
        &self.auth.token
    }

    /// The frame limit.
    pub fn limit(&self) -> FrameLimit {
        self.auth.limit
    }

    /// The protocol this side serves.
    pub fn protocol(&self) -> ProtocolVersion {
        self.auth.protocol
    }

    /// The checks of a `Hello`, to authenticate accepted clients elsewhere (a thread per client).
    pub fn authenticator(&self) -> Authenticator {
        self.auth.clone()
    }

    /// After an accept failed with `e`: when no file descriptor was left, the waiting client is accepted on the listener's
    /// reserve one and told `busy` with the cause, instead of waiting in the queue for its own limit while this side fails
    /// the same accept again (Linux; CI run 7763). A stopgap of ADR 0072 for I7, to be absorbed by M3/M4.
    pub(crate) fn turn_away(&mut self, e: &io::Error) {
        if crate::endpoint::out_of_descriptors(e)
            && let Some(turned_away) = self.listener.accept_on_spare()
        {
            self.auth.refuse_busy(
                turned_away,
                &format!("the host cannot take a connection now ({e}); try again later"),
            );
        }
    }

    /// Waits at most `slice` (OS time) for one client, without authenticating it. A timeout is `ErrorKind::TimedOut`.
    pub fn accept_raw(&mut self, slice: Duration) -> io::Result<(Stream, Option<u32>)> {
        self.listener.accept(slice)
    }

    /// Waits up to `timeout` (on the clock) for a client and authenticates it.
    pub fn accept(&mut self, timeout: Duration) -> Result<Connection, IpcError> {
        self.accept_while(None, timeout, &mut || Ok(()))
    }

    /// Accepts within `timeout` on the clock, calling `alive` between short waits so the caller can stop early (the
    /// child exited), and, with `pid`, only the process `pid` (the child this host started).
    pub fn accept_while(
        &mut self,
        pid: Option<u32>,
        timeout: Duration,
        alive: &mut dyn FnMut() -> Result<(), IpcError>,
    ) -> Result<Connection, IpcError> {
        let deadline = Deadline::after(self.auth.clock.clone(), timeout);
        let (stream, os_pid) = loop {
            alive()?;
            if deadline.passed() {
                return Err(IpcError::Timeout);
            }
            match self.listener.accept(ACCEPT_SLICE) {
                Ok(accepted) => break accepted,
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => {
                    self.turn_away(&e);
                    return Err(e.into());
                }
            }
        };
        self.auth.authenticate(stream, os_pid, pid)
    }
}

/// The connecting side.
pub struct Client;

/// The listener must run as `me`; an unreadable uid is a refusal too.
#[cfg(unix)]
fn check_same_account(listener_uid: Option<u32>, me: u32) -> Result<(), IpcError> {
    match listener_uid {
        Some(uid) if uid == me => Ok(()),
        Some(_) => Err(IpcError::Auth(
            "the endpoint is served by another account, so the token was not sent".into(),
        )),
        None => Err(IpcError::Auth(
            "the account serving the endpoint could not be read, so the token was not sent".into(),
        )),
    }
}

impl Client {
    /// Connects to `endpoint` and authenticates with `token`, speaking `protocol`, within [`HANDSHAKE_TIMEOUT`].
    pub fn connect(
        endpoint: &str,
        token: &Token,
        limit: FrameLimit,
        protocol: ProtocolVersion,
    ) -> Result<Connection, IpcError> {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        Client::connect_on(endpoint, token, limit, protocol, &clock, HANDSHAKE_TIMEOUT)
    }

    /// [`Client::connect`] with the handshake limited to `timeout` on `clock`. A host that refuses with a code other
    /// than `version` or `auth` (e.g. `busy`) is [`IpcError::Remote`].
    pub fn connect_on(
        endpoint: &str,
        token: &Token,
        limit: FrameLimit,
        protocol: ProtocolVersion,
        clock: &Arc<dyn Clock>,
        timeout: Duration,
    ) -> Result<Connection, IpcError> {
        let mut stream = Stream::connect(endpoint, HANDSHAKE_TIMEOUT)?;
        // Before the token leaves this process: the one answering must run as this account (rule 53: a discovery file only
        // hints; a listener planted by another account must not receive the token). On Windows the server process's token says whose
        // it is (`GetNamedPipeServerProcessId`), not the pipe's access list.
        #[cfg(unix)]
        check_same_account(stream.peer_uid(), paddock_env::current_uid())?;
        #[cfg(windows)]
        stream.check_server_is_me().map_err(|e| {
            IpcError::Auth(format!(
                "the pipe is not served by your account, so the token was not sent: {e}. Quit the program and start it again"
            ))
        })?;
        let hello = Frame::Hello {
            version: frame::VERSION,
            protocol,
            token: *token.as_bytes(),
            pid: std::process::id(),
        };
        let answer = bounded(&mut stream, clock, timeout, |s| {
            frame::write_frame(s, &hello, limit).map_err(|e| IpcError::Closed(e.to_string()))?;
            match frame::read_frame(s, limit) {
                Ok(Frame::Welcome { protocol, .. }) => Ok(protocol),
                Ok(Frame::Fail { error, .. }) if error.is(codes::VERSION) => {
                    Err(IpcError::Version(error.message))
                }
                Ok(Frame::Fail { error, .. }) if error.is(codes::AUTH) => {
                    Err(IpcError::Auth(error.message))
                }
                Ok(Frame::Fail { error, .. }) => Err(IpcError::Remote(error)),
                Ok(other) => Err(IpcError::Protocol(format!(
                    "expected a welcome, got {other:?}"
                ))),
                Err(FrameError::Closed) => Err(IpcError::Closed(
                    "the host closed the connection during the handshake".into(),
                )),
                Err(FrameError::Io(m)) => Err(IpcError::Closed(m)),
                Err(e) => Err(IpcError::Protocol(e.to_string())),
            }
        })?;
        Ok(Connection {
            stream,
            limit,
            peer_pid: None,
            peer_uid: None,
            protocol: answer,
        })
    }
}

#[cfg(all(test, unix))]
mod same_account_tests {
    use super::*;

    #[test]
    fn only_a_listener_of_the_same_account_may_receive_the_token() {
        assert!(check_same_account(Some(501), 501).is_ok());
        let other = check_same_account(Some(0), 501).unwrap_err();
        assert!(
            matches!(&other, IpcError::Auth(m) if m.contains("another account")),
            "{other:?}"
        );
        let unknown = check_same_account(None, 501).unwrap_err();
        assert!(
            matches!(&unknown, IpcError::Auth(m) if m.contains("could not be read")),
            "{unknown:?}"
        );
    }
}
