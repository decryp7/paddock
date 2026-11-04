//! The real [`HelperTransport`]: a connection kept between calls (a status poll is one connect, not one per call; every connect is
//! one log line in the helper) through `dyno-helper-service`'s trusted connect (the pipe's owner must
//! be SYSTEM, the socket's folder root's, before a byte is sent). A helper that is not installed is `Absent`; one that is
//! installed but does not answer is `Silent`; a squatted endpoint is `Foreign` and is never sent anything.

use crate::helper_link::{HelperTransport, TransportError};

use dyno_core::helper_wire::{HelperMessage, HelperReply, HelperRequest};
use dyno_helper_service::client::{connect_for, ConnectError, CONNECT_TIMEOUT};
use paddock_ipc::{BodyPeer, IpcError};
use paddock_jobs::time::{Clock, SystemClock};
use paddock_service::start::{Revival, DEFAULT_BUDGET};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long an ordinary request may take before the agent stops waiting for the helper (the helper's own tool timeouts are
/// shorter). Slow kinds get their own, longer wait; the UI never times out the job that waits for them (ER-19).
const QUICK: Duration = Duration::from_secs(30);
const SLOW: Duration = Duration::from_secs(12 * 60);

/// How the connection is opened (the real one, or a test's).
/// It is given the endpoint and the longest it may wait to connect.
pub type Opener = Box<dyn Fn(&str, Duration) -> Result<BodyPeer<HelperMessage>, ConnectError> + Send + Sync>;

/// Brings the helper's stopped service up without elevation (the SCM lets interactive users start it) and waits for the endpoint.
/// The real one is `helper_os::ServiceReviver`; a test's is a fake service.
pub trait Revive: Send + Sync {
    /// Starts the service if it is stopped cleanly (waits if it is starting) and waits up to `budget` for `ready` to say the
    /// endpoint answers.
    fn revive(&self, budget: Duration, ready: &mut dyn FnMut() -> bool) -> Revival;
}

/// Calls the helper at its system endpoint.
pub struct PeerTransport {
    endpoint: String,
    installed: Box<dyn Fn() -> bool + Send + Sync>,
    open: Opener,
    /// The connection kept for the next call. A call takes it, so two calls at once use two connections.
    kept: Mutex<Option<BodyPeer<HelperMessage>>>,
    /// Times a live deadline across the connect and the retry.
    clock: Arc<dyn Clock>,
    /// What brings a stopped service up when the program is installed and nothing listens (none: report it silent).
    revive: Option<Box<dyn Revive>>,
}

impl PeerTransport {
    /// A transport to `endpoint`. `installed` says whether the helper's protected copy exists, which tells "never turned on"
    /// (`Absent`) from "turned on but not running" (`Silent`) when nothing listens.
    pub fn new(endpoint: impl Into<String>, installed: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        PeerTransport {
            endpoint: endpoint.into(),
            installed: Box::new(installed),
            open: Box::new(connect_for),
            kept: Mutex::new(None),
            clock: Arc::new(SystemClock),
            revive: None,
        }
    }

    /// The same transport starting a stopped helper service itself, through `revive`, before it reports the helper silent.
    pub fn with_revive(mut self, revive: Box<dyn Revive>) -> Self {
        self.revive = Some(revive);
        self
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
    /// Opens a connection within `timeout`. When nothing listens but the program is installed, the helper's service is
    /// brought up first (it may be stopped, or still starting after a boot) and waited for up to `budget`.
    fn connect(&self, timeout: Duration, budget: Duration) -> Result<BodyPeer<HelperMessage>, TransportError> {
        let began = self.clock.monotonic();
        let why = match (self.open)(&self.endpoint, timeout) {
            Ok(peer) => return Ok(peer),
            Err(ConnectError::NotRunning(why)) if !(self.installed)() => {
                log::debug!(why = why.as_str(); "the helper is not installed");
                return Err(TransportError::Absent);
            }
            Err(ConnectError::NotRunning(why)) => why,
            Err(ConnectError::Untrusted(owner)) => return Err(TransportError::Foreign(owner)),
            Err(ConnectError::Peer(e)) => return Err(TransportError::Silent(e.to_string())),
        };
        let Some(revive) = &self.revive else {
            return Err(TransportError::Silent(why));
        };
        // The peer that answers while waiting is the one used: no second connect.
        let mut found: Option<BodyPeer<HelperMessage>> = None;
        let outcome = revive.revive(budget, &mut || match (self.open)(&self.endpoint, timeout) {
            Ok(peer) => {
                found = Some(peer);
                true
            }
            Err(_) => false,
        });
        let ms = self.clock.monotonic().saturating_sub(began).as_millis() as u64;
        match (outcome, found) {
            (Revival::Up, Some(peer)) => {
                log::info!(Ms = ms; "The helper service was not answering and is up now");
                Ok(peer)
            }
            (Revival::Up, None) => Err(TransportError::Silent(why)),
            (Revival::Waiting(reason), _) => {
                log::info!(Ms = ms, Reason = reason.as_str(); "The helper service is starting");
                Err(TransportError::Starting(reason))
            }
            (Revival::Down(reason), _) => {
                log::warn!(Ms = ms, Reason = reason.as_str(); "The helper service is down");
                Err(TransportError::Silent(format!("{reason} ({why})")))
            }
            (Revival::NotInstalled, _) => Err(TransportError::Silent(format!("the helper service is not registered ({why})"))),
            (Revival::NotSupported, _) => Err(TransportError::Silent(why)),
        }
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
        // One total for the whole call: the connect and the retry on a stale connection spend it too. A slow kind has none: it
        // takes as long as the tool takes (and is never refused for a deadline that was meant for a live reading).
        let max = max.filter(|_| wait_for(&request) < SLOW);
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
                    // The connect waits no longer than what is left of the deadline.
                    self.connect(left().map_or(CONNECT_TIMEOUT, |l| l.min(CONNECT_TIMEOUT)), left().map_or(DEFAULT_BUDGET, |l| l.min(DEFAULT_BUDGET)))?
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
        PeerTransport::new("x", || true).with_opener(Box::new(move |_, _| {
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
        PeerTransport::new("x", move || installed).with_opener(Box::new(move |_, _| Err(err())))
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
        let t = PeerTransport::new("x", || true).with_opener(Box::new(move |_, _| {
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
        let clock = paddock_jobs::time::FakeClock::new();
        let (t, seen, _) = slow_connect(&clock, Duration::from_secs(3));
        let out = t.call_within(HelperRequest::Info, Some(Duration::from_secs(2)));
        assert!(
            matches!(out, Err(TransportError::Silent(ref why)) if why.contains("deadline")),
            "{out:?}"
        );
        assert_eq!(seen.load(Ordering::SeqCst), 0, "nothing was sent once the time was spent");
    }

    /// A transport whose connect takes `connect` on `clock` and whose helper counts the requests it gets; `timeouts` records what
    /// the connect was allowed.
    fn slow_connect(
        clock: &paddock_jobs::time::FakeClock,
        connect: Duration,
    ) -> (PeerTransport, Arc<AtomicUsize>, Arc<Mutex<Vec<Duration>>>) {
        let (seen, timeouts) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
        let (spent, counted, allowed) = (clock.clone(), seen.clone(), timeouts.clone());
        let t = PeerTransport::new("x", || true)
            .with_clock(Arc::new(clock.clone()))
            .with_opener(Box::new(move |_, timeout| {
                allowed.lock().unwrap().push(timeout);
                spent.advance(connect);
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
        (t, seen, timeouts)
    }

    #[test]
    fn a_slow_kind_is_sent_even_when_a_live_deadline_has_passed() {
        let clock = paddock_jobs::time::FakeClock::new();
        let (t, seen, _) = slow_connect(&clock, Duration::from_secs(3));
        let out = t.call_within(HelperRequest::Smart(SmartQuery::default()), Some(Duration::from_secs(2)));
        assert_eq!(out, Ok(HelperReply::Bye), "smartctl takes as long as it takes");
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_connect_is_bounded_by_the_deadline_that_is_left() {
        let clock = paddock_jobs::time::FakeClock::new();
        let (t, _, timeouts) = slow_connect(&clock, Duration::ZERO);
        t.call_within(HelperRequest::Info, Some(Duration::from_secs(1))).unwrap();
        assert_eq!(
            timeouts.lock().unwrap()[0],
            Duration::from_secs(1),
            "a live call may not wait the usual 3 s to connect"
        );
        let (t, _, timeouts) = slow_connect(&clock, Duration::ZERO);
        t.call(HelperRequest::Info).unwrap();
        assert_eq!(
            timeouts.lock().unwrap()[0],
            dyno_helper_service::client::CONNECT_TIMEOUT,
            "an ordinary call keeps the usual connect time"
        );
    }

    // The helper service after a restart: the program is installed, nothing listens, and the service is down, starting or failed.
    use paddock_jobs::time::FakeClock;
    use paddock_service::start::{revive, Pace};
    use paddock_service::testing::FakeSystem;
    use paddock_env::Os;
    use paddock_service::Output;

    /// Time on a fake clock: waiting advances it.
    struct Advance(FakeClock);
    impl Pace for Advance {
        fn elapsed(&mut self) -> Duration {
            self.0.monotonic()
        }
        fn pause(&mut self, how_long: Duration) {
            self.0.advance(how_long);
        }
    }

    /// The real start logic over a fake service whose `sc query` answers follow `script` (the last repeats).
    struct FakeService {
        sys: FakeSystem,
        clock: FakeClock,
    }
    impl Revive for FakeService {
        fn revive(&self, budget: Duration, ready: &mut dyn FnMut() -> bool) -> Revival {
            let mut sys = self.sys.clone();
            let mut pace = Advance(self.clock.clone());
            revive(&mut sys, Os::Windows, "DynoHelper", budget, &mut pace, &dyno_core::helper_wire::start_exit_words, ready)
        }
    }

    fn query(state: &str, win32: u32, specific: u32) -> Output {
        Output::ok(&format!(
            "SERVICE_NAME: DynoHelper\r\n        STATE              : {state}\r\n        WIN32_EXIT_CODE    : {win32}  (0x0)\r\n        SERVICE_EXIT_CODE  : {specific}  (0x0)\r\n"
        ))
    }

    /// A transport whose pipe opens `up_at` after the clock's start; the service answers `script` to `sc query`.
    fn after_restart(script: Vec<Output>, up_at: Duration) -> (PeerTransport, FakeSystem, FakeClock, Arc<AtomicUsize>) {
        let clock = FakeClock::new();
        let sys = FakeSystem::new();
        let at = Arc::new(AtomicUsize::new(0));
        sys.on("sc query", move |_, _| Ok(script[at.fetch_add(1, Ordering::SeqCst).min(script.len() - 1)].clone()));
        let starts = Arc::new(AtomicUsize::new(0));
        let n = starts.clone();
        sys.on("sc start", move |_, _| {
            n.fetch_add(1, Ordering::SeqCst);
            Ok(Output::ok("STATE : 2  START_PENDING"))
        });
        let now = clock.clone();
        let t = PeerTransport::new("x", || true)
            .with_clock(Arc::new(clock.clone()))
            .with_revive(Box::new(FakeService { sys: sys.clone(), clock: clock.clone() }))
            .with_opener(Box::new(move |_, _| {
                if now.monotonic() < up_at {
                    return Err(ConnectError::NotRunning("no such pipe".into()));
                }
                let (ours, theirs) = pair().expect("a stream pair");
                let server = BodyPeer::<HelperMessage>::start(
                    Connection::raw(theirs, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)),
                    BodyOptions::new("test helper").serve(Server(Ok(HelperMessage::Reply(HelperReply::Bye)))),
                )
                .expect("the test server starts");
                std::mem::forget(server);
                BodyPeer::start(Connection::raw(ours, FrameLimit::new(dyno_core::helper_wire::MAX_FRAME)), BodyOptions::new("the helper"))
                    .map_err(ConnectError::Peer)
            }));
        (t, sys, clock, starts)
    }

    #[test]
    fn a_stopped_helper_service_is_started_once_without_a_prompt_and_the_call_answers() {
        let (t, _sys, _clock, starts) = after_restart(
            vec![query("1  STOPPED", 0, 0), query("2  START_PENDING", 0, 0), query("4  RUNNING", 0, 0)],
            Duration::from_secs(2),
        );
        assert_eq!(t.call(HelperRequest::Info), Ok(HelperReply::Bye));
        assert_eq!(starts.load(Ordering::SeqCst), 1, "one start, no Repair");
    }

    #[test]
    fn a_helper_service_that_is_still_starting_after_the_boot_is_waited_for_not_started_again() {
        let (t, _sys, _clock, starts) = after_restart(vec![query("2  START_PENDING", 0, 0)], Duration::from_secs(9));
        assert_eq!(t.call(HelperRequest::Info), Ok(HelperReply::Bye));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_helper_service_stopped_with_an_error_is_silent_with_the_services_own_reason_and_not_started() {
        let (t, _sys, _clock, starts) = after_restart(vec![query("1  STOPPED", 1066, 3)], Duration::from_secs(2));
        match t.call(HelperRequest::Info) {
            Err(TransportError::Silent(why)) => {
                assert!(why.contains("exit code 3") && why.contains("not protected") && why.contains("no such pipe"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(starts.load(Ordering::SeqCst), 0, "a service that fails is repaired, not started in a loop");
    }

    #[test]
    fn a_live_reading_does_not_wait_out_a_slow_start_and_is_told_it_is_starting() {
        let (t, _sys, clock, _starts) = after_restart(vec![query("2  START_PENDING", 0, 0)], Duration::from_secs(60));
        let out = t.call_within(HelperRequest::Info, Some(Duration::from_secs(2)));
        assert!(matches!(out, Err(TransportError::Starting(_))), "{out:?}");
        assert!(clock.monotonic() <= Duration::from_secs(4), "waited {:?}, not the whole start", clock.monotonic());
    }

    #[test]
    fn a_helper_that_is_not_installed_is_absent_and_the_service_is_never_asked() {
        let (t, sys, _clock, _starts) = after_restart(vec![query("1  STOPPED", 0, 0)], Duration::from_secs(60));
        let t = PeerTransport { installed: Box::new(|| false), ..t };
        assert_eq!(t.call(HelperRequest::Info), Err(TransportError::Absent));
        assert!(sys.commands().is_empty(), "{:?}", sys.commands());
    }

    #[test]
    fn slow_requests_wait_longer_than_quick_ones() {
        assert!(wait_for(&HelperRequest::Smart(SmartQuery::default())) > wait_for(&HelperRequest::Info));
        assert_eq!(wait_for(&HelperRequest::Audit(1)), QUICK);
    }
}
