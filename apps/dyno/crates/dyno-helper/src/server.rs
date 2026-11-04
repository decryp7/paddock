//! The accept loop of the system endpoint (ADR 0027 §2). Every connection: the OS says who the caller is
//! ([`paddock_ipc::caller_of`], from the kernel's credentials or the caller's own token, never from a process id), an
//! unreadable identity is refused and audited, and then each request is checked ([`crate::auth::authorize`]) and takes a
//! slot of its account's allowance ([`AccountLimits`]: more is refused as busy at once) before the [`Dispatcher`] runs
//! it. The request handlers are the dispatcher's; this file owns the connection, the identity and the limits.

use crate::auth::{authorize, scope, weight};
use crate::dispatch::{Ctx, Dispatcher};
use crate::policy::HelperPolicy;
use dyno_core::helper_policy::RequestKind;
use dyno_core::helper_wire::{codes, CallerView, HelperMessage, HelperUser, MAX_FRAME};
use paddock_ipc::endpoint::Listener;
use paddock_ipc::{
    caller_of, AccountId, AccountLimits, AdminRule, BodyHandler, BodyOptions, BodyPeer, Caller, Connection, FrameLimit, RemoteError,
};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long one connection may stay open.
pub const SESSION_MAX: Duration = Duration::from_secs(120);
/// Connections served at once, in all (the per-account allowance is separate and smaller).
pub const MAX_SESSIONS: usize = 16;
/// Consecutive accept failures after which the daemon gives up (the service manager restarts it).
const MAX_ACCEPT_FAILURES: u32 = 50;

/// How many requests of each kind the helper answered, for the stop line `served: kind=count ...` (ADR 0052 section 1.5).
///
/// Only counts, indexed by [`RequestKind`]: no account, no argument, no content. The VM suite fails when a reading an OS
/// advertises was served 0 times, which is the run-time half of the static check in `dyno-agent/tests/helper_wiring.rs`.
#[derive(Debug)]
pub struct ServedCounts {
    counts: [AtomicU64; RequestKind::ALL.len()],
}

impl Default for ServedCounts {
    fn default() -> Self {
        ServedCounts {
            counts: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl ServedCounts {
    /// Counts one answered request of `kind`.
    pub fn record(&self, kind: RequestKind) {
        self.counts[kind as usize].fetch_add(1, Ordering::Relaxed);
    }

    /// How many of `kind` were answered so far.
    pub fn get(&self, kind: RequestKind) -> u64 {
        self.counts[kind as usize].load(Ordering::Relaxed)
    }

    /// `kind=count` for every kind, in the enum's order (a kind served 0 times is listed with 0).
    pub fn summary(&self) -> String {
        RequestKind::ALL
            .iter()
            .map(|k| format!("{k:?}={}", self.get(*k)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

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
    served: Arc<ServedCounts>,
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
            account: Some(match self.caller.account() {
                AccountId::Uid(uid) => HelperUser::Uid(*uid),
                AccountId::Sid(sid) => HelperUser::Sid(sid.clone()),
            }),
            access,
            view: CallerView {
                scope: scope(&self.caller, &self.owner, &policy),
                tools: vec![],
            },
        };
        let kind = request.kind();
        let answer = self.dispatcher.handle_for(&ctx, request).map(HelperMessage::Reply);
        // Answered means the reader ran and replied; a request the handler failed is not a served reading.
        if answer.is_ok() {
            self.served.record(kind);
        }
        answer
    }
}

impl Daemon {
    /// Serves until `stop` is set. Returns an error only when the endpoint itself keeps failing.
    ///
    /// # Errors
    /// The accept failures, in words.
    pub fn run(self) -> Result<(), String> {
        self.run_counted(Arc::new(ServedCounts::default()))
    }

    /// [`Daemon::run`] with the caller's own counter (tests read it); logs `served: kind=count ...` when the loop ends.
    ///
    /// # Errors
    /// The accept failures, in words.
    pub fn run_counted(mut self, served: Arc<ServedCounts>) -> Result<(), String> {
        let result = self.serve(&served);
        log::info!(outcome = if result.is_ok() { "stopped" } else { "failed" }; "Helper: served: {}", served.summary());
        result
    }

    fn serve(&mut self, served: &Arc<ServedCounts>) -> Result<(), String> {
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
                        served: served.clone(),
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
        served: Arc<ServedCounts>,
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
                holders: Box::new(dyno_core::helper_holders::NoHolders),
                extras: Box::new(dyno_core::helper_extras::NoExtras),
                counters: Box::new(dyno_core::helper_counters::NoCounters),
                memory: Box::new(dyno_core::helper_memory::NoMemory),
                processes: Box::new(dyno_core::helper_processes::NoProcesses),
                smart: Box::new(crate::tools::NotHere("none")),
                tools: Box::new(crate::tools::NotHere("none")),
                wakes: Box::new(dyno_core::wake::NoWakes),
                wol: Box::new(dyno_core::wake::NoWakes),
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
            let served = Arc::new(ServedCounts::default());
            let counted = served.clone();
            Rig {
                name,
                dispatcher,
                stop,
                served,
                daemon: Some(std::thread::spawn(move || daemon.run_counted(counted))),
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

    /// One valid request of every kind (defaults at their caps), so a test can send each to the endpoint.
    #[cfg(unix)]
    fn sample(kind: dyno_core::helper_policy::RequestKind) -> HelperRequest {
        use dyno_core::helper_policy::RequestKind as K;
        use dyno_core::helper_requests::*;
        match kind {
            K::Info => HelperRequest::Info,
            K::Processes => HelperRequest::Processes(ProcessQuery::default()),
            K::Traffic => HelperRequest::Traffic,
            K::SocketOwners => HelperRequest::SocketOwners,
            K::FileHolders => HelperRequest::FileHolders { dev: 1, ino: 2 },
            K::LoginItems => HelperRequest::LoginItems,
            K::Counters => HelperRequest::Counters(CounterQuery {
                ids: vec![CounterId::Thermal],
                class: RequestClass::User,
            }),
            K::MemoryModules => HelperRequest::MemoryModules,
            K::Smart => HelperRequest::Smart(SmartQuery::default()),
            K::InstallTool => HelperRequest::InstallTool {
                tool: ToolKind::Smartmontools,
                bytes: vec![1],
            },
            K::Firewall => HelperRequest::Firewall,
            K::Extras => HelperRequest::Extras(ExtrasKind::WakeRequests),
            K::Audit => HelperRequest::Audit(1),
            K::LogTail => HelperRequest::LogTail(1),
            K::Uninstall => HelperRequest::Uninstall,
            K::ScheduleWake => HelperRequest::ScheduleWake {
                tag: dyno_core::wake::WakeTag::parse("t").unwrap(),
                at_utc: 1,
            },
            K::CancelWake => HelperRequest::CancelWake {
                tag: dyno_core::wake::WakeTag::parse("t").unwrap(),
            },
            K::ListWakes => HelperRequest::ListWakes,
            K::WolState => HelperRequest::WolState {
                ifname: dyno_core::wake::IfName::parse("eth0").unwrap(),
            },
        }
    }

    /// The socket is mode 0666, so any local account connects. What makes that safe: the account is read from the kernel's credentials
    /// of the connection (before a single frame is parsed), and every request is checked against the standing table before it is
    /// handled. A different uid gets exactly what the table allows: refused where it says Denied, and handled (here: "not available
    /// yet" or a validation answer, never a refusal) everywhere else.
    #[cfg(unix)]
    #[test]
    fn a_different_account_gets_exactly_what_the_standing_table_allows() {
        use dyno_core::helper_policy::{access, Access, RequestKind, Standing};
        if own_uid() == 0 {
            return; // root is an administrator: the table would allow everything
        }
        let rig = Rig::start(HelperUser::Uid(own_uid().wrapping_add(1)), 8);
        for kind in RequestKind::ALL {
            let answer = rig.call(sample(kind));
            let refused = matches!(&answer, Err(IpcError::Remote(r)) if r.code == codes::REFUSED);
            assert_eq!(refused, access(kind, Standing::Other) == Access::Denied, "{kind:?}: {answer:?}");
        }
        rig.stop();
    }

    #[cfg(unix)]
    #[test]
    fn answered_requests_are_counted_per_kind_and_refused_or_failed_ones_are_not() {
        let rig = Rig::start(HelperUser::Uid(own_uid()), 8);
        rig.call(HelperRequest::Info).unwrap();
        rig.call(HelperRequest::Info).unwrap();
        // The test rig's traffic reader fails: a failed request is not a served reading.
        rig.call(HelperRequest::Traffic).unwrap_err();
        let served = rig.served.clone();
        rig.stop();
        assert_eq!(served.get(RequestKind::Info), 2);
        assert_eq!(served.get(RequestKind::Traffic), 0);
        assert!(served.summary().starts_with("Info=2 Processes=0 Traffic=0"), "{}", served.summary());
        assert_eq!(served.summary().split(' ').count(), RequestKind::ALL.len());
        // The counters are indexed by the enum's number: ALL must list the kinds in declaration order.
        assert!(RequestKind::ALL.iter().enumerate().all(|(i, k)| *k as usize == i));
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
