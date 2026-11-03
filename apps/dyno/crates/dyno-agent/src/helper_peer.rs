//! The real [`HelperTransport`]: one connection per call through `dyno-helper-service`'s trusted connect (the pipe's owner must
//! be SYSTEM, the socket's folder root's, before a byte is sent). A helper that is not installed is `Absent`; one that is
//! installed but does not answer is `Silent`; a squatted endpoint is `Foreign` and is never sent anything.

use crate::helper_link::{HelperTransport, TransportError};

use dyno_core::helper_wire::{HelperMessage, HelperReply, HelperRequest};
use dyno_helper_service::client::{connect, ConnectError};
use paddock_ipc::{BodyPeer, IpcError};
use std::time::Duration;

/// How long an ordinary request may take before the agent stops waiting for the helper (the helper's own tool timeouts are
/// shorter). Slow kinds get their own, longer wait; the UI never times out the job that waits for them (ER-19).
const QUICK: Duration = Duration::from_secs(30);
const SLOW: Duration = Duration::from_secs(12 * 60);

/// How the connection is opened (the real one, or a test's).
pub type Opener = Box<dyn Fn(&str) -> Result<BodyPeer<HelperMessage>, ConnectError> + Send + Sync>;

/// Calls the helper at its system endpoint.
pub struct PeerTransport {
    endpoint: String,
    installed: Box<dyn Fn() -> bool + Send + Sync>,
    open: Opener,
}

impl PeerTransport {
    /// A transport to `endpoint`. `installed` says whether the helper's protected copy exists, which tells "never turned on"
    /// (`Absent`) from "turned on but not running" (`Silent`) when nothing listens.
    pub fn new(endpoint: impl Into<String>, installed: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        PeerTransport {
            endpoint: endpoint.into(),
            installed: Box::new(installed),
            open: Box::new(connect),
        }
    }

    /// [`PeerTransport::new`] over another way to open the connection (tests).
    pub fn with_opener(mut self, open: Opener) -> Self {
        self.open = open;
        self
    }
}

fn wait_for(request: &HelperRequest) -> Duration {
    match request {
        HelperRequest::Smart(_) | HelperRequest::InstallTool { .. } | HelperRequest::LoginItems | HelperRequest::Firewall => SLOW,
        _ => QUICK,
    }
}

impl HelperTransport for PeerTransport {
    fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
        let peer = (self.open)(&self.endpoint).map_err(|e| match e {
            ConnectError::NotRunning(why) if !(self.installed)() => {
                log::debug!(why = why.as_str(); "the helper is not installed");
                TransportError::Absent
            }
            ConnectError::NotRunning(why) => TransportError::Silent(why),
            ConnectError::Untrusted(owner) => TransportError::Foreign(owner),
            ConnectError::Peer(e) => TransportError::Silent(e.to_string()),
        })?;
        let wait = wait_for(&request);
        let answer = peer.call(HelperMessage::Request(request), wait);
        peer.close();
        match answer {
            Ok(HelperMessage::Reply(r)) => Ok(r),
            Ok(HelperMessage::Request(_)) => Err(TransportError::Silent("it answered with a request".into())),
            Err(IpcError::Remote(r)) => Err(TransportError::Remote {
                code: r.code.to_string(),
                message: r.message.to_string(),
            }),
            Err(IpcError::Timeout) => Err(TransportError::Silent(format!("no answer within {} s", wait.as_secs()))),
            Err(e) => Err(TransportError::Silent(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper_requests::SmartQuery;
    use paddock_ipc::endpoint::pair;
    use paddock_ipc::{BodyHandler, BodyOptions, Connection, FrameLimit, RemoteError};
    use std::sync::Mutex;

    struct Server(Result<HelperMessage, RemoteError>);
    impl BodyHandler<HelperMessage> for Server {
        fn handle(&self, _: HelperMessage) -> Result<HelperMessage, RemoteError> {
            self.0.clone()
        }
    }

    fn served(answer: Result<HelperMessage, RemoteError>) -> PeerTransport {
        let (ours, theirs) = pair().expect("a stream pair");
        let server = BodyPeer::<HelperMessage>::start(
            Connection::raw(theirs, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)),
            BodyOptions::new("test helper").serve(Server(answer)),
        )
        .expect("the test server starts");
        std::mem::forget(server);
        let slot = Mutex::new(Some(ours));
        PeerTransport::new("x", || true).with_opener(Box::new(move |_| {
            let s = slot.lock().unwrap().take().expect("one call");
            BodyPeer::start(
                Connection::raw(s, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)),
                BodyOptions::new("the helper"),
            )
            .map_err(ConnectError::Peer)
        }))
    }

    #[test]
    fn a_reply_comes_back_and_a_remote_error_keeps_its_code() {
        let t = served(Ok(HelperMessage::Reply(HelperReply::Bye)));
        assert_eq!(t.call(HelperRequest::Uninstall), Ok(HelperReply::Bye));
        let t = served(Err(RemoteError::new("refused", "owner only")));
        assert_eq!(
            t.call(HelperRequest::Audit(5)),
            Err(TransportError::Remote {
                code: "refused".into(),
                message: "owner only".into()
            })
        );
    }

    fn refusing(err: impl Fn() -> ConnectError + Send + Sync + 'static, installed: bool) -> PeerTransport {
        PeerTransport::new("x", move || installed).with_opener(Box::new(move |_| Err(err())))
    }

    #[test]
    fn connection_failures_are_the_helper_states() {
        let down = || ConnectError::NotRunning("refused".into());
        assert_eq!(refusing(down, false).call(HelperRequest::Info), Err(TransportError::Absent));
        assert_eq!(
            refusing(down, true).call(HelperRequest::Info),
            Err(TransportError::Silent("refused".into()))
        );
        let squat = || ConnectError::Untrusted("owner S-1-5-21-9".into());
        assert_eq!(
            refusing(squat, true).call(HelperRequest::Info),
            Err(TransportError::Foreign("owner S-1-5-21-9".into()))
        );
        let peer = || ConnectError::Peer(IpcError::Timeout);
        assert!(matches!(
            refusing(peer, true).call(HelperRequest::Info),
            Err(TransportError::Silent(_))
        ));
    }

    #[test]
    fn slow_requests_wait_longer_than_quick_ones() {
        assert!(wait_for(&HelperRequest::Smart(SmartQuery::default())) > wait_for(&HelperRequest::Info));
        assert_eq!(wait_for(&HelperRequest::Audit(1)), QUICK);
    }
}
