//! The agent's side of the helper's system endpoint (ADR 0027 §2): the connection goes through
//! `paddock_ipc::endpoint::Stream::connect_system`, which opens the pipe at identification level and refuses an owner
//! other than SYSTEM, or a socket whose folder is not root's, before a byte is sent. A squatted endpoint is therefore
//! "the helper is missing" ([`ConnectError::Untrusted`]), never an answer.

use dyno_core::helper_wire::{HelperMessage, MAX_FRAME};
use paddock_ipc::endpoint::{Stream, SystemConnectError};
use paddock_ipc::{BodyOptions, BodyPeer, Connection, FrameLimit, IpcError};
use std::time::Duration;

/// How long the agent waits for the endpoint to open.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Why there is no connection.
#[derive(Debug)]
pub enum ConnectError {
    /// Nobody listens: the helper is off, stopped or not installed.
    NotRunning(String),
    /// Something listens that is not the helper (a squatted pipe name, a socket in a folder others can change). Nothing was
    /// sent to it.
    Untrusted(String),
    /// The connection opened but the peer could not start.
    Peer(IpcError),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::NotRunning(why) => write!(f, "The helper is not running ({why}). Turn it on in Settings > Helper."),
            ConnectError::Untrusted(why) => write!(
                f,
                "Something else answers where the helper should be, so Dyno did not use it ({why}). Turn the helper off and on again in Settings > Helper."
            ),
            ConnectError::Peer(e) => write!(f, "The helper could not be reached: {e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// Connects to the helper at `endpoint` through the trust check.
///
/// # Errors
/// [`ConnectError`].
pub fn connect(endpoint: &str) -> Result<BodyPeer<HelperMessage>, ConnectError> {
    connect_with(endpoint, &Stream::connect_system)
}

/// [`connect`] over another way to open the stream (tests).
///
/// # Errors
/// [`ConnectError`].
pub fn connect_with(
    endpoint: &str,
    open: &dyn Fn(&str, Duration) -> Result<Stream, SystemConnectError>,
) -> Result<BodyPeer<HelperMessage>, ConnectError> {
    let stream = open(endpoint, CONNECT_TIMEOUT).map_err(|e| match e {
        SystemConnectError::Connect(io) => ConnectError::NotRunning(io.to_string()),
        SystemConnectError::Untrusted(u) => {
            log::warn!(endpoint = endpoint, reason = u.to_string().as_str(); "the helper's endpoint failed the trust check");
            ConnectError::Untrusted(u.to_string())
        }
    })?;
    BodyPeer::start(Connection::raw(stream, FrameLimit::new(MAX_FRAME)), BodyOptions::new("the helper")).map_err(ConnectError::Peer)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use dyno_core::helper_wire::{HelperReply, HelperRequest};
    use paddock_ipc::endpoint::pair;
    use paddock_ipc::{BodyHandler, RemoteError};

    struct Echo;
    impl BodyHandler<HelperMessage> for Echo {
        fn handle(&self, _: HelperMessage) -> Result<HelperMessage, RemoteError> {
            Ok(HelperMessage::Reply(HelperReply::Bye))
        }
    }

    #[test]
    fn a_trusted_endpoint_connects_and_answers() {
        let (ours, theirs) = pair().unwrap();
        let server = BodyPeer::<HelperMessage>::start(
            Connection::raw(theirs, FrameLimit::new(MAX_FRAME)),
            BodyOptions::new("test helper").serve(Echo),
        )
        .unwrap();
        let slot = std::sync::Mutex::new(Some(ours));
        let peer = connect_with("x", &|_, _| Ok(slot.lock().unwrap().take().unwrap())).unwrap();
        let answer = peer
            .call(HelperMessage::Request(HelperRequest::Info), Duration::from_secs(5))
            .unwrap();
        assert!(matches!(answer, HelperMessage::Reply(HelperReply::Bye)));
        peer.close();
        server.close();
    }

    #[test]
    fn a_missing_endpoint_is_not_running_and_says_what_to_do() {
        let err = connect_with("x", &|_, _| Err(SystemConnectError::Connect(std::io::Error::other("no such file"))))
            .err()
            .unwrap();
        assert!(matches!(&err, ConnectError::NotRunning(w) if w.contains("no such file")));
        assert!(err.to_string().contains("Settings > Helper"));
    }

    #[test]
    fn the_real_connect_to_a_name_nobody_owns_is_not_running() {
        let err = connect("/nonexistent-dyno-helper-test/helper.sock").err().unwrap();
        assert!(matches!(err, ConnectError::NotRunning(_) | ConnectError::Untrusted(_)), "{err}");
        assert!(!err.to_string().is_empty());
        assert!(!ConnectError::Peer(IpcError::Timeout).to_string().is_empty());
        assert!(!ConnectError::Untrusted("x".into()).to_string().is_empty());
    }
}
