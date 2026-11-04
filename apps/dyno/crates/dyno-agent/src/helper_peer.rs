//! The real [`HelperTransport`]: a connection kept between calls (a status poll is one connect, not one per call; every connect is
//! one log line in the helper) through `dyno-helper-service`'s trusted connect (the pipe's owner must
//! be SYSTEM, the socket's folder root's, before a byte is sent). A helper that is not installed is `Absent`; one that is
//! installed but does not answer is `Silent`; a squatted endpoint is `Foreign` and is never sent anything.

use crate::helper_link::{HelperTransport, TransportError};

use dyno_core::helper_wire::{HelperMessage, HelperReply, HelperRequest};
use dyno_helper_service::client::{connect, ConnectError};
use paddock_ipc::{BodyPeer, IpcError};
use paddock_jobs::time::{Clock, SystemClock};
use std::sync::{Arc, Mutex};
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
    /// The connection kept for the next call. A call takes it, so two calls at once use two connections.
    kept: Mutex<Option<BodyPeer<HelperMessage>>>,
    /// Times a live deadline across the connect and the retry.
    clock: Arc<dyn Clock>,
}

impl PeerTransport {
    /// A transport to `endpoint`. `installed` says whether the helper's protected copy exists, which tells "never turned on"
    /// (`Absent`) from "turned on but not running" (`Silent`) when nothing listens.
    pub fn new(endpoint: impl Into<String>, installed: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        PeerTransport {
            endpoint: endpoint.into(),
            installed: Box::new(installed),
            open: Box::new(connect),
            kept: Mutex::new(None),
            clock: Arc::new(SystemClock),
        }
    }

    /// The same transport timing its deadlines on `clock` (tests).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// [`PeerTransport::new`] over another way to open the connection (tests).
    pub fn with_opener(mut self, open: Opener) -> Self {
        self.open = open;
        self
    }
}

/// How long one call may wait: the kind's own wait, shortened to `max` for a live reading, except that a slow kind (SMART, an
/// install) ignores a live deadline: it takes as long as the tool takes.
fn effective_wait(request: &HelperRequest, max: Option<Duration>) -> Duration {
    let own = wait_for(request);
    match max {
        Some(m) if own < SLOW => m.min(own),
        _ => own,
    }
}

fn wait_for(request: &HelperRequest) -> Duration {
    match request {
        HelperRequest::Smart(_) | HelperRequest::InstallTool { .. } | HelperRequest::LoginItems | HelperRequest::Firewall => SLOW,
        _ => QUICK,
    }
}

impl PeerTransport {
    fn connect(&self) -> Result<BodyPeer<HelperMessage>, TransportError> {
        (self.open)(&self.endpoint).map_err(|e| match e {
            ConnectError::NotRunning(why) if !(self.installed)() => {
                log::debug!(why = why.as_str(); "the helper is not installed");
                TransportError::Absent
            }
            ConnectError::NotRunning(why) => TransportError::Silent(why),
            ConnectError::Untrusted(owner) => TransportError::Foreign(owner),
            ConnectError::Peer(e) => TransportError::Silent(e.to_string()),
        })
    }

    fn take(&self) -> Option<BodyPeer<HelperMessage>> {
        self.kept.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    /// Keeps `peer` for the next call; a connection another call already kept is closed instead.
    fn keep(&self, peer: BodyPeer<HelperMessage>) {
        let mut slot = self.kept.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            *slot = Some(peer);
        } else {
            peer.close();
        }
    }
}

impl HelperTransport for PeerTransport {
    fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
        self.call_within(request, None)
    }

    fn call_within(&self, request: HelperRequest, max: Option<Duration>) -> Result<HelperReply, TransportError> {
        // One total for the whole call: the connect and the retry on a stale connection spend it too.
        let began = self.clock.monotonic();
        let mut reused = true;
        loop {
            let left = || max.map(|m| m.saturating_sub(self.clock.monotonic().saturating_sub(began)));
            let spent = || TransportError::Silent("the deadline passed before the helper could be asked".into());
            if left() == Some(Duration::ZERO) {
                return Err(spent());
            }
            let peer = match self.take() {
                Some(p) => p,
                None => {
                    reused = false;
                    self.connect()?
                }
            };
            // The connect may have taken the whole deadline.
            if left() == Some(Duration::ZERO) {
                self.keep(peer);
                return Err(spent());
            }
            let wait = effective_wait(&request, left());
            let answer = peer.call(HelperMessage::Request(request.clone()), wait);
            match answer {
                Ok(HelperMessage::Reply(r)) => {
                    self.keep(peer);
                    return Ok(r);
                }
                Ok(HelperMessage::Request(_)) => {
                    peer.close();
                    return Err(TransportError::Silent("it answered with a request".into()));
                }
                // The helper refused or failed the request but the connection is fine.
                Err(IpcError::Remote(r)) => {
                    self.keep(peer);
                    return Err(TransportError::Remote {
                        code: r.code.to_string(),
                        message: r.message.to_string(),
                    });
                }
                // A kept connection may have gone stale (the helper restarted): once, try again on a fresh one.
                Err(IpcError::Closed(_) | IpcError::Io { .. } | IpcError::Protocol(_)) if reused => {
                    peer.close();
                    reused = false;
                    continue;
                }
                Err(IpcError::Timeout) => {
                    peer.close();
                    return Err(TransportError::Silent(format!("no answer within {} ms", wait.as_millis())));
                }
                Err(e) => {
                    peer.close();
                    return Err(TransportError::Silent(e.to_string()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper_requests::SmartQuery;
    use paddock_ipc::endpoint::pair;
    use paddock_ipc::{BodyHandler, BodyOptions, Connection, FrameLimit, RemoteError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

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

    /// A server that answers `Bye` and counts the connections it was given.
    fn counting() -> (PeerTransport, Arc<AtomicUsize>) {
        let opens = Arc::new(AtomicUsize::new(0));
        let count = opens.clone();
        let t = PeerTransport::new("x", || true).with_opener(Box::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            let (ours, theirs) = pair().expect("a stream pair");
            let server = BodyPeer::<HelperMessage>::start(
                Connection::raw(theirs, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)),
                BodyOptions::new("test helper").serve(Server(Ok(HelperMessage::Reply(HelperReply::Bye)))),
            )
            .expect("the test server starts");
            std::mem::forget(server);
            BodyPeer::start(
                Connection::raw(ours, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)),
                BodyOptions::new("the helper"),
            )
            .map_err(ConnectError::Peer)
        }));
        (t, opens)
    }

    #[test]
    fn a_poll_of_many_calls_is_one_connection() {
        let (t, opens) = counting();
        for _ in 0..20 {
            assert_eq!(t.call(HelperRequest::Info), Ok(HelperReply::Bye));
        }
        assert_eq!(opens.load(Ordering::SeqCst), 1, "one connect for twenty calls");
    }

    #[test]
    fn a_stale_kept_connection_is_replaced_once_and_the_call_still_answers() {
        let (t, opens) = counting();
        assert_eq!(t.call(HelperRequest::Info), Ok(HelperReply::Bye));
        // The helper went away: close the kept connection under the transport.
        t.kept.lock().unwrap().as_ref().expect("kept").close();
        assert_eq!(t.call(HelperRequest::Info), Ok(HelperReply::Bye));
        assert_eq!(opens.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn slow_kinds_ignore_a_live_deadline_and_quick_ones_obey_it() {
        let live = Some(Duration::from_secs(2));
        assert_eq!(effective_wait(&HelperRequest::Smart(SmartQuery::default()), live), SLOW);
        assert_eq!(effective_wait(&HelperRequest::Info, live), Duration::from_secs(2));
        assert_eq!(effective_wait(&HelperRequest::Info, None), QUICK);
        assert_eq!(
            effective_wait(&HelperRequest::Info, Some(Duration::from_secs(90))),
            QUICK,
            "a deadline never lengthens a wait"
        );
    }

    #[test]
    fn the_deadline_is_one_total_that_the_connect_uses_up_first() {
        use paddock_jobs::time::FakeClock;
        let clock = FakeClock::new();
        let seen = Arc::new(AtomicUsize::new(0));
        let (spent, counted) = (clock.clone(), seen.clone());
        let t = PeerTransport::new("x", || true)
            .with_clock(Arc::new(clock))
            .with_opener(Box::new(move |_| {
                // A slow connect: it takes longer than the whole deadline.
                spent.advance(Duration::from_secs(3));
                let (ours, theirs) = pair().expect("a stream pair");
                struct Count(Arc<AtomicUsize>);
                impl BodyHandler<HelperMessage> for Count {
                    fn handle(&self, _: HelperMessage) -> Result<HelperMessage, RemoteError> {
                        self.0.fetch_add(1, Ordering::SeqCst);
                        Ok(HelperMessage::Reply(HelperReply::Bye))
                    }
                }
                let server = BodyPeer::<HelperMessage>::start(
                    Connection::raw(theirs, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)),
                    BodyOptions::new("test helper").serve(Count(counted.clone())),
                )
                .expect("the test server starts");
                std::mem::forget(server);
                BodyPeer::start(
                    Connection::raw(ours, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)),
                    BodyOptions::new("the helper"),
                )
                .map_err(ConnectError::Peer)
            }));
        let out = t.call_within(HelperRequest::Info, Some(Duration::from_secs(2)));
        assert!(
            matches!(out, Err(TransportError::Silent(ref why)) if why.contains("deadline")),
            "{out:?}"
        );
        assert_eq!(seen.load(Ordering::SeqCst), 0, "nothing was sent once the time was spent");
    }

    #[test]
    fn slow_requests_wait_longer_than_quick_ones() {
        assert!(wait_for(&HelperRequest::Smart(SmartQuery::default())) > wait_for(&HelperRequest::Info));
        assert_eq!(wait_for(&HelperRequest::Audit(1)), QUICK);
    }
}
