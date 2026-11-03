//! The accept loop of the system endpoint (ADR 0027 §2). Every connection: the OS says who the caller is
//! ([`paddock_ipc::caller_of`], from the kernel's credentials or the caller's own token, never from a process id), an
//! unreadable identity is refused and audited, and then each request is checked ([`crate::auth::authorize`]) and takes a
//! slot of its account's allowance ([`AccountLimits`]: more is refused as busy at once) before the [`Dispatcher`] runs
//! it. The request handlers are the dispatcher's; this file owns the connection, the identity and the limits.

use crate::auth::{authorize, scope, weight};
use crate::dispatch::{Ctx, Dispatcher};
use crate::policy::HelperPolicy;
use dyno_core::helper_wire::{codes, CallerView, HelperMessage, HelperUser, MAX_FRAME};
use paddock_ipc::endpoint::Listener;
use paddock_ipc::{
    caller_of, AccountId, AccountLimits, AdminRule, BodyHandler, BodyOptions, BodyPeer, Caller, Connection, FrameLimit, RemoteError,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long one connection may stay open.
pub const SESSION_MAX: Duration = Duration::from_secs(120);
/// Connections served at once, in all (the per-account allowance is separate and smaller).
pub const MAX_SESSIONS: usize = 16;
/// Consecutive accept failures after which the daemon gives up (the service manager restarts it).
const MAX_ACCEPT_FAILURES: u32 = 50;

/// The listening daemon.
pub struct Daemon {
    /// The system endpoint.
    pub listener: Listener,
    /// The account that turned the helper on.
    pub owner: HelperUser,
    /// Which Unix groups administer the machine.
    pub admin_rule: AdminRule,
    /// What an account may run at once.
    pub limits: AccountLimits,
    /// What to do with requests.
    pub dispatcher: Arc<Dispatcher>,
    /// The policy in force (replaced when the policy file changes).
    pub policy: Arc<Mutex<HelperPolicy>>,
    /// Set to end the loop (service stop, uninstall).
    pub stop: Arc<AtomicBool>,
}

struct Session {
    dispatcher: Arc<Dispatcher>,
    caller: Caller,
    owner: HelperUser,
    limits: AccountLimits,
    policy: Arc<Mutex<HelperPolicy>>,
}

impl BodyHandler<HelperMessage> for Session {
    fn handle(&self, body: HelperMessage) -> Result<HelperMessage, RemoteError> {
        let HelperMessage::Request(request) = body else {
            return Err(RemoteError::new(codes::REFUSED, "The helper only answers requests."));
        };
        let pid = self.caller.audit().pid_for_log();
        let access = match authorize(&self.caller, &self.owner, &request) {
            Ok(access) => access,
            Err(refusal) => {
                log::warn!(account = self.caller.account().to_string().as_str(), reason = refusal.to_string().as_str(); "Helper: a request was refused");
                self.dispatcher.record_refused_caller(pid, &refusal.to_string());
                return Err(RemoteError::new(
                    codes::REFUSED,
                    "This request is for the owner of the helper or an administrator.",
                ));
            }
        };
        let _slot = self.limits.try_acquire(self.caller.account(), weight(&request)).map_err(|busy| {
            log::warn!(account = self.caller.account().to_string().as_str(), reason = busy.to_string().as_str(); "Helper: a request was turned away as busy");
            RemoteError::new(paddock_ipc::codes::BUSY, busy.to_string())
        })?;
        let policy = *self.policy.lock().unwrap_or_else(|p| p.into_inner());
        let uid = match self.caller.account() {
            AccountId::Uid(uid) => Some(*uid),
            AccountId::Sid(_) => None,
        };
        let ctx = Ctx {
            pid,
            uid,
            access,
            view: CallerView {
                scope: scope(&self.caller, &self.owner, &policy),
                tools: vec![],
            },
        };
        self.dispatcher.handle_for(&ctx, request).map(HelperMessage::Reply)
    }
}

impl Daemon {
    /// Serves until `stop` is set. Returns an error only when the endpoint itself keeps failing.
    ///
    /// # Errors
    /// The accept failures, in words.
    pub fn run(mut self) -> Result<(), String> {
        let active = Arc::new(AtomicUsize::new(0));
        let mut failures = 0u32;
        while !self.stop.load(Ordering::SeqCst) {
            match self.listener.accept(Duration::from_millis(500)) {
                Ok((stream, _pid)) => {
                    failures = 0;
                    // The identity is read from the OS here; the pid the accept reported is for the log line only.
                    let caller = match caller_of(&stream, &self.admin_rule) {
                        Ok(c) => c,
                        Err(e) => {
                            log::warn!(error = e.to_string().as_str(); "Helper: a caller could not be identified and was refused");
                            self.dispatcher.record_refused_caller(None, &e.to_string());
                            continue;
                        }
                    };
                    log::info!(
                        account = caller.account().to_string().as_str(),
                        admin = caller.is_admin(),
                        pid = caller.audit().pid_for_log().unwrap_or(0);
                        "Helper: a connection was accepted"
                    );
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_SESSIONS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        log::warn!("Helper: too many connections, one was dropped");
                        continue;
                    }
                    let session = Session {
                        dispatcher: self.dispatcher.clone(),
                        caller,
                        owner: self.owner.clone(),
                        limits: self.limits.clone(),
                        policy: self.policy.clone(),
                    };
                    let count = active.clone();
                    let spawned = std::thread::Builder::new().name("helper-session".into()).spawn(move || {
                        let conn = Connection::raw(stream, FrameLimit::new(MAX_FRAME));
                        if let Ok(peer) = BodyPeer::start(conn, BodyOptions::new("a caller").serve(session)) {
                            peer.wait_closed(Some(SESSION_MAX));
                            peer.close();
                        }
                        count.fetch_sub(1, Ordering::SeqCst);
                    });
                    if spawned.is_err() {
                        active.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock => failures = 0,
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

// Unix only: they drive the endpoint as the test's own account; the Windows identity is exercised on the VM matrix.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::audit::AuditLog;
    use crate::dispatch::Parts;
    use dyno_core::helper::HelperHost;
    use dyno_core::helper_wire::{HelperInfo, HelperReply, HelperRequest};
    use dyno_core::ports::{PlatformError, ProcessTraffic};
    use paddock_ipc::endpoint::Stream;
    use paddock_ipc::IpcError;
    use paddock_jobs::time::SystemClock;

    struct NoTraffic;
    impl ProcessTraffic for NoTraffic {
        fn counters(&self) -> Result<Vec<dyno_core::network_traffic::TrafficCounter>, PlatformError> {
            Err(PlatformError::Unsupported("none".into()))
        }
    }

    struct NoItems;
    impl dyno_core::btm::LoginItemsReader for NoItems {
        fn read(&self, _: u32) -> Result<dyno_core::btm::LoginItemsReply, PlatformError> {
            Err(PlatformError::Unsupported("none".into()))
        }
    }

    struct Rig {
        name: String,
        dispatcher: Arc<Dispatcher>,
        stop: Arc<AtomicBool>,
        daemon: Option<std::thread::JoinHandle<Result<(), String>>>,
        _dir: tempfile::TempDir,
    }

    impl Rig {
        fn start(owner: HelperUser, max_total: usize) -> Rig {
            let dir = tempfile::tempdir().unwrap();
            let listener = Listener::bind().unwrap();
            let name = listener.name().to_string();
            let dispatcher = Arc::new(Dispatcher::new(Parts {
                info: HelperInfo {
                    version: "1.0.0".into(),
                    api: 1,
                    host: HelperHost::Windows,
                    agent_exe: String::new(),
                },
                traffic: Box::new(NoTraffic),
                audit: AuditLog::new(dir.path().join("audit.jsonl")),
                clock: Box::new(|| 42),
                on_uninstall: Box::new(|| {}),
                login_items: Box::new(NoItems),
                smart: Box::new(crate::tools::NotHere("none")),
                tools: Box::new(crate::tools::NotHere("none")),
                log_tail: Box::new(|_| vec![]),
            }));
            let stop = Arc::new(AtomicBool::new(false));
            let daemon = Daemon {
                listener,
                owner,
                admin_rule: AdminRule::default(),
                limits: AccountLimits::with_max_total(max_total, Arc::new(SystemClock)),
                dispatcher: dispatcher.clone(),
                policy: Arc::new(Mutex::new(HelperPolicy::default())),
                stop: stop.clone(),
            };
            Rig {
                name,
                dispatcher,
                stop,
                daemon: Some(std::thread::spawn(move || daemon.run())),
                _dir: dir,
            }
        }

        fn call(&self, request: HelperRequest) -> Result<HelperReply, IpcError> {
            let stream = Stream::connect(&self.name, Duration::from_secs(5)).map_err(|e| IpcError::Closed(e.to_string()))?;
            let conn = Connection::raw(stream, FrameLimit::new(MAX_FRAME));
            let peer = BodyPeer::<HelperMessage>::start(conn, BodyOptions::new("the helper"))?;
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

    /// The account running the tests, as the helper's owner (a Unix uid; on Windows the SID is read by the caller's token).
    #[cfg(unix)]
    fn own_uid() -> u32 {
        use std::os::unix::fs::MetadataExt;
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::metadata(f.path()).unwrap().uid()
    }

    #[cfg(unix)]
    #[test]
    fn the_owner_is_answered_by_the_os_identity_not_by_a_secret() {
        let rig = Rig::start(HelperUser::Uid(own_uid()), 4);
        assert!(
            matches!(rig.call(HelperRequest::Info).unwrap(), HelperReply::About(i, v) if v.scope == dyno_core::helper_policy::Visibility::Full && i.version == "1.0.0")
        );
        assert!(matches!(rig.call(HelperRequest::Traffic).unwrap_err(), IpcError::Remote(e) if e.code == codes::UNSUPPORTED));
        rig.stop();
    }

    #[cfg(unix)]
    #[test]
    fn another_account_gets_who_the_helper_is_and_a_recorded_refusal_for_the_rest() {
        let rig = Rig::start(HelperUser::Uid(own_uid().wrapping_add(1)), 4);
        // The test runs as a normal user here; as root it would be an administrator and everything would be allowed.
        if own_uid() == 0 {
            rig.stop();
            return;
        }
        assert!(
            matches!(rig.call(HelperRequest::Info).unwrap(), HelperReply::About(_, v) if v.scope == dyno_core::helper_policy::Visibility::Own(dyno_core::helper_policy::OthersPolicy::Total))
        );
        // Readings are for everyone (scoped); the audit log and removal are not.
        let e = rig.call(HelperRequest::Traffic).unwrap_err();
        assert!(matches!(&e, IpcError::Remote(r) if r.code == codes::UNSUPPORTED), "{e:?}");
        let e = rig.call(HelperRequest::Audit(5)).unwrap_err();
        assert!(matches!(&e, IpcError::Remote(r) if r.code == codes::REFUSED), "{e:?}");
        let e = rig.call(HelperRequest::Uninstall).unwrap_err();
        assert!(matches!(&e, IpcError::Remote(r) if r.code == codes::REFUSED), "{e:?}");
        let log = match rig.dispatcher.handle(None, HelperRequest::Audit(5)).unwrap() {
            HelperReply::Entries(e) => e,
            other => panic!("{other:?}"),
        };
        assert!(log[0].detail.contains("owner or an administrator"), "{log:?}");
        rig.stop();
    }

    #[cfg(unix)]
    #[test]
    fn a_reply_sent_as_a_request_is_refused() {
        let rig = Rig::start(HelperUser::Uid(own_uid()), 4);
        let stream = Stream::connect(&rig.name, Duration::from_secs(5)).unwrap();
        let peer = BodyPeer::<HelperMessage>::start(Connection::raw(stream, FrameLimit::new(MAX_FRAME)), BodyOptions::new("x")).unwrap();
        let e = peer
            .call(HelperMessage::Reply(HelperReply::Bye), Duration::from_secs(5))
            .unwrap_err();
        assert!(matches!(e, IpcError::Remote(r) if r.code == codes::REFUSED));
        peer.close();
        rig.stop();
    }

    #[cfg(unix)]
    #[test]
    fn an_account_over_its_allowance_is_turned_away_as_busy_and_the_next_request_is_served() {
        // Zero slots in all: every request is busy, which proves the limit sits in front of the handlers.
        let rig = Rig::start(HelperUser::Uid(own_uid()), 0);
        let e = rig.call(HelperRequest::Info).unwrap_err();
        assert!(matches!(&e, IpcError::Remote(r) if r.code == paddock_ipc::codes::BUSY), "{e:?}");
        rig.stop();
    }
}
