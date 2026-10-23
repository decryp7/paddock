//! The accept loop: authenticate every connection (token and version in `dyno-ipc`, then the user and the program
//! in [`crate::auth`]), then serve its calls through the [`Dispatcher`]. A connection that fails any check is dropped
//! without an answer and recorded in the audit log.

use crate::auth::{ImageOf, Policy};
use crate::dispatch::Dispatcher;
use dyno_core::helper_wire::{codes, HelperMessage};
use dyno_ipc::{Handler, Host, IpcError, Peer, PeerOptions, RemoteError};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How long one connection may stay open.
pub const SESSION_MAX: Duration = Duration::from_secs(120);
/// Connections served at once (the agent opens one per call).
pub const MAX_SESSIONS: usize = 4;
/// Consecutive accept failures after which the daemon gives up (the service manager restarts it).
const MAX_ACCEPT_FAILURES: u32 = 50;

/// The listening daemon.
pub struct Daemon {
    /// The authenticated endpoint.
    pub host: Host,
    /// Who may call.
    pub policy: Policy,
    /// How to tell which program a process is.
    pub images: Arc<dyn ImageOf>,
    /// What to do with requests.
    pub dispatcher: Arc<Dispatcher>,
    /// Set to end the loop (service stop, uninstall).
    pub stop: Arc<AtomicBool>,
}

struct Session {
    dispatcher: Arc<Dispatcher>,
    peer: Option<u32>,
}

impl Handler<HelperMessage> for Session {
    fn handle(&self, body: HelperMessage) -> Result<HelperMessage, RemoteError> {
        match body {
            HelperMessage::Request(r) => self.dispatcher.handle(self.peer, r).map(HelperMessage::Reply),
            HelperMessage::Reply(_) => Err(RemoteError::new(codes::REFUSED, "The helper only answers requests.")),
        }
    }
}

impl Daemon {
    /// Serves until `stop` is set. Returns an error only when the endpoint itself keeps failing.
    pub fn run(mut self) -> Result<(), String> {
        let active = Arc::new(AtomicUsize::new(0));
        let mut failures = 0u32;
        while !self.stop.load(Ordering::SeqCst) {
            match self.host.accept(Duration::from_millis(500)) {
                Ok(conn) => {
                    failures = 0;
                    let (uid, pid) = (conn.peer_uid(), conn.peer_pid());
                    if let Err(refusal) = self.policy.check(uid, pid, &*self.images) {
                        self.dispatcher.record_refused_caller(pid, &refusal.to_string());
                        continue;
                    }
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_SESSIONS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let (dispatcher, session_count) = (self.dispatcher.clone(), active.clone());
                    let spawned = std::thread::Builder::new().name("helper-session".into()).spawn(move || {
                        if let Ok(peer) = Peer::start(conn, PeerOptions::new().serve(Session { dispatcher, peer: pid }).workers(1)) {
                            peer.wait_closed(Some(SESSION_MAX));
                            peer.close();
                        }
                        session_count.fetch_sub(1, Ordering::SeqCst);
                    });
                    if spawned.is_err() {
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                Err(IpcError::Timeout(_)) => failures = 0,
                // A client that failed the token, the version or the protocol is simply dropped.
                Err(IpcError::Auth(_) | IpcError::Version { .. } | IpcError::Protocol(_) | IpcError::Closed) => failures = 0,
                Err(e) => {
                    failures += 1;
                    if failures >= MAX_ACCEPT_FAILURES {
                        return Err(format!("The helper's endpoint keeps failing: {e}"));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditLog;
    use crate::dispatch::Parts;
    use dyno_core::helper::{HelperAction, HelperOutcome, TuningHost};
    use dyno_core::helper_wire::{HelperInfo, HelperReply, HelperRequest, HelperUser};
    use dyno_core::ports::{PlatformError, ProcessTraffic};
    use dyno_ipc::{Client, Token};
    use dyno_platform::tuning::FakeTuning;
    use std::path::PathBuf;

    struct NoTraffic;
    impl ProcessTraffic for NoTraffic {
        fn counters(&self) -> Result<Vec<dyno_core::network_traffic::TrafficCounter>, PlatformError> {
            Err(PlatformError::Unsupported("none".into()))
        }
    }

    struct Pretend(PathBuf);
    impl ImageOf for Pretend {
        fn image(&self, _: u32) -> Option<PathBuf> {
            Some(self.0.clone())
        }
    }

    struct Rig {
        name: String,
        token: Token,
        fake: FakeTuning,
        dispatcher: Arc<Dispatcher>,
        stop: Arc<AtomicBool>,
        daemon: Option<std::thread::JoinHandle<Result<(), String>>>,
        _dir: tempfile::TempDir,
    }

    impl Rig {
        fn start(user: HelperUser, program: &str) -> Rig {
            let dir = tempfile::tempdir().unwrap();
            let host = Host::listen().unwrap();
            let (name, token) = (host.name().to_string(), host.token().clone());
            let fake = FakeTuning::new(TuningHost::Windows);
            let dispatcher = Arc::new(Dispatcher::new(Parts {
                info: HelperInfo {
                    version: "1.0.0".into(),
                    api: 1,
                    host: TuningHost::Windows,
                    agent_exe: "/x".into(),
                },
                tuning: Box::new(fake.clone()),
                traffic: Box::new(NoTraffic),
                audit: AuditLog::new(dir.path().join("audit.jsonl")),
                clock: Box::new(|| 42),
                on_uninstall: Box::new(|| {}),
            }));
            let stop = Arc::new(AtomicBool::new(false));
            let daemon = Daemon {
                host,
                policy: Policy {
                    user,
                    agent_exe: PathBuf::from("/opt/dyno/dyno-agent"),
                },
                images: Arc::new(Pretend(PathBuf::from(program))),
                dispatcher: dispatcher.clone(),
                stop: stop.clone(),
            };
            Rig {
                name,
                token,
                fake,
                dispatcher,
                stop,
                daemon: Some(std::thread::spawn(move || daemon.run())),
                _dir: dir,
            }
        }

        fn call(&self, request: HelperRequest) -> Result<HelperReply, IpcError> {
            let conn = Client::connect(&self.name, &self.token)?;
            let peer = Peer::<HelperMessage>::start(conn, PeerOptions::new())?;
            let answer = peer.call(HelperMessage::Request(request), Duration::from_secs(5));
            peer.close();
            match answer? {
                HelperMessage::Reply(r) => Ok(r),
                HelperMessage::Request(_) => Err(IpcError::Protocol("a request came back".into())),
            }
        }

        fn stop(mut self) {
            self.stop.store(true, Ordering::SeqCst);
            assert_eq!(self.daemon.take().unwrap().join().unwrap(), Ok(()));
        }
    }

    /// A user that is "us" on Unix (any user on Windows, where the pipe's list decides).
    fn this_user() -> HelperUser {
        #[cfg(unix)]
        {
            HelperUser::Uid(own_uid())
        }
        #[cfg(not(unix))]
        {
            HelperUser::Sid("S-1-5-21-1-2-3-1001".into())
        }
    }

    #[cfg(unix)]
    fn own_uid() -> u32 {
        use std::os::unix::fs::MetadataExt;
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::metadata(f.path()).unwrap().uid()
    }

    #[test]
    fn the_authorised_user_with_the_agent_program_gets_its_requests_answered_in_process() {
        let rig = Rig::start(this_user(), "/opt/dyno/dyno-agent");
        assert!(matches!(rig.call(HelperRequest::Info).unwrap(), HelperReply::About(i) if i.version == "1.0.0"));
        let a = HelperAction::SetHibernate(false);
        assert_eq!(
            rig.call(HelperRequest::Apply(a.clone())).unwrap(),
            HelperReply::Outcome(HelperOutcome::Done)
        );
        assert_eq!(rig.fake.applied(), vec![a]);
        // A refusal is a typed remote failure, not a dropped connection.
        let refused = rig
            .call(HelperRequest::Apply(HelperAction::SetPowerScheme(
                dyno_core::helper::BALANCED_SCHEME.into(),
            )))
            .unwrap_err();
        assert!(matches!(refused, IpcError::Remote(e) if e.code == codes::REFUSED));
        rig.stop();
    }

    #[test]
    fn another_program_is_dropped_and_recorded_without_an_answer() {
        let rig = Rig::start(this_user(), "/tmp/evil/dyno-agent");
        let err = rig.call(HelperRequest::Apply(HelperAction::SetHibernate(false))).unwrap_err();
        assert!(
            matches!(err, IpcError::Closed | IpcError::Io { .. } | IpcError::Timeout(_)),
            "{err:?}"
        );
        assert!(rig.fake.applied().is_empty());
        let log = match rig.dispatcher.handle(None, HelperRequest::Audit(5)).unwrap() {
            HelperReply::Entries(e) => e,
            other => panic!("{other:?}"),
        };
        assert!(log[0].detail.contains("not the Dyno agent"), "{log:?}");
        rig.stop();
    }

    #[cfg(unix)]
    #[test]
    fn another_user_is_dropped() {
        let rig = Rig::start(HelperUser::Uid(own_uid().wrapping_add(1)), "/opt/dyno/dyno-agent");
        assert!(rig.call(HelperRequest::Info).is_err());
        let log = match rig.dispatcher.handle(None, HelperRequest::Audit(5)).unwrap() {
            HelperReply::Entries(e) => e,
            other => panic!("{other:?}"),
        };
        assert!(log[0].detail.contains("is not the user the helper serves"));
        rig.stop();
    }

    #[test]
    fn a_wrong_token_never_reaches_the_dispatcher() {
        let rig = Rig::start(this_user(), "/opt/dyno/dyno-agent");
        assert!(matches!(
            Client::connect(&rig.name, &Token::from_bytes([9; 32])),
            Err(IpcError::Auth(_))
        ));
        // The daemon is still serving.
        assert!(rig.call(HelperRequest::Info).is_ok());
        rig.stop();
    }

    #[test]
    fn a_reply_sent_as_a_request_is_refused() {
        let rig = Rig::start(this_user(), "/opt/dyno/dyno-agent");
        let conn = Client::connect(&rig.name, &rig.token).unwrap();
        let peer = Peer::<HelperMessage>::start(conn, PeerOptions::new()).unwrap();
        let e = peer
            .call(HelperMessage::Reply(HelperReply::Bye), Duration::from_secs(5))
            .unwrap_err();
        assert!(matches!(e, IpcError::Remote(r) if r.code == codes::REFUSED));
        peer.close();
        rig.stop();
    }
}
