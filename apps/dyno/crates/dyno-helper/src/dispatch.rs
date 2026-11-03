//! What the helper does with a request (ADR 0016 section 4): it answers read-only questions (who it is, its audit
//! log, per-program traffic) and removes itself on request. It makes no change to the machine's settings. Pure over
//! its ports: tests give it a fake [`ProcessTraffic`] and a temporary audit log.

use crate::audit::AuditLog;
use dyno_core::btm::LoginItemsReader;
use dyno_core::helper_policy::{Access, OthersPolicy, Visibility};
use dyno_core::helper_wire::{codes, error_code, AuditEntry, AuditVerdict, CallerView, HelperInfo, HelperReply, HelperRequest};
use dyno_core::network_traffic::MAX_COUNTERS;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use paddock_ipc::RemoteError;

/// Everything a [`Dispatcher`] needs from outside.
pub struct Parts {
    /// What the helper tells about itself.
    pub info: HelperInfo,
    /// Per-program traffic counters.
    pub traffic: Box<dyn ProcessTraffic>,
    /// The audit log.
    pub audit: AuditLog,
    /// Seconds since 1970.
    pub clock: Box<dyn Fn() -> i64 + Send + Sync>,
    /// The macOS login-items reader (`sfltool dumpbtm` as the helper's account, parsed here).
    pub login_items: Box<dyn LoginItemsReader>,
    /// The last `n` lines of the helper's log, oldest first.
    pub log_tail: Box<dyn Fn(usize) -> Vec<String> + Send + Sync>,
    /// Called after an `Uninstall` request was audited and answered (it removes the helper in the background).
    pub on_uninstall: Box<dyn Fn() + Send + Sync>,
}

/// The request handler.
pub struct Dispatcher {
    info: HelperInfo,
    traffic: Box<dyn ProcessTraffic>,
    audit: AuditLog,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    on_uninstall: Box<dyn Fn() + Send + Sync>,
    login_items: Box<dyn LoginItemsReader>,
    log_tail: Box<dyn Fn(usize) -> Vec<String> + Send + Sync>,
}

/// Who is asking, as the connection established it (never from a request, never from a process id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    /// The caller's process id, for the audit line only.
    pub pid: Option<u32>,
    /// The caller's Unix user id (`None` for a Windows account).
    pub uid: Option<u32>,
    /// What the restriction table gives this caller for the request.
    pub access: Access,
    /// What this caller is told about its own standing.
    pub view: CallerView,
}

impl Ctx {
    /// A caller with full access and the default view (tests, and requests made inside the helper).
    pub fn anonymous(pid: Option<u32>) -> Ctx {
        Ctx {
            pid,
            uid: None,
            access: Access::Full,
            view: CallerView {
                scope: Visibility::Own(OthersPolicy::Total),
                tools: vec![],
            },
        }
    }
}

fn remote(e: &PlatformError) -> RemoteError {
    RemoteError::new(error_code(e), e.to_string())
}

impl Dispatcher {
    /// A dispatcher over `parts`.
    pub fn new(parts: Parts) -> Dispatcher {
        Dispatcher {
            info: parts.info,
            traffic: parts.traffic,
            audit: parts.audit,
            clock: parts.clock,
            on_uninstall: parts.on_uninstall,
            login_items: parts.login_items,
            log_tail: parts.log_tail,
        }
    }

    /// Records a connection that was turned away (wrong user or program) without answering it.
    pub fn record_refused_caller(&self, peer: Option<u32>, why: &str) {
        let _ = self.audit.record(&AuditEntry {
            at: (self.clock)(),
            peer,
            what: "A connection was refused".into(),
            verdict: AuditVerdict::Refused,
            detail: why.chars().take(300).collect(),
        });
    }

    /// Answers one request from the authenticated peer `peer` (its process id).
    pub fn handle(&self, peer: Option<u32>, request: HelperRequest) -> Result<HelperReply, RemoteError> {
        self.handle_for(&Ctx::anonymous(peer), request)
    }

    /// Answers one request from the caller `ctx` describes.
    pub fn handle_for(&self, ctx: &Ctx, request: HelperRequest) -> Result<HelperReply, RemoteError> {
        let peer = ctx.pid;
        request.validate().map_err(|e| RemoteError::new(codes::REFUSED, e.to_string()))?;
        match request {
            HelperRequest::Info => Ok(HelperReply::About(self.info.clone(), ctx.view.clone())),
            HelperRequest::LoginItems => {
                let uid = ctx.uid.ok_or_else(|| {
                    RemoteError::new(codes::UNSUPPORTED, "Login items are read for a Unix account; this caller has none.")
                })?;
                self.login_items.read(uid).map(HelperReply::LoginItems).map_err(|e| remote(&e))
            }
            HelperRequest::LogTail(n) => Ok(HelperReply::Log((self.log_tail)(HelperRequest::log_len(n)))),
            HelperRequest::Audit(n) => Ok(HelperReply::Entries(self.audit.tail(HelperRequest::audit_len(n)))),
            HelperRequest::Traffic => {
                let mut counters = self.traffic.counters().map_err(|e| remote(&e))?;
                counters.truncate(MAX_COUNTERS);
                Ok(HelperReply::Counters(counters))
            }
            // The readers arrive with their work packages (ADR 0027 WP-4 to WP-10); until then the helper
            // says so, and the agent shows the reading as unavailable with this reason.
            HelperRequest::Processes(_)
            | HelperRequest::SocketOwners
            | HelperRequest::FileHolders { .. }
            | HelperRequest::Counters(_)
            | HelperRequest::MemoryModules
            | HelperRequest::Smart(_)
            | HelperRequest::InstallTool { .. }
            | HelperRequest::Firewall
            | HelperRequest::Extras(_) => Err(RemoteError::new(
                codes::UNSUPPORTED,
                "This reading is not available in this build of the helper yet.",
            )),
            HelperRequest::Uninstall => {
                self.audit
                    .record(&AuditEntry {
                        at: (self.clock)(),
                        peer,
                        what: "Remove the Dyno helper from this computer".into(),
                        verdict: AuditVerdict::Done,
                        detail: "Asked by the authorised user.".into(),
                    })
                    .map_err(|m| RemoteError::new(codes::FAILED, format!("{m} Nothing was removed.")))?;
                (self.on_uninstall)();
                Ok(HelperReply::Bye)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::btm::LoginItemsReply;
    use dyno_core::helper::HelperHost;
    use dyno_core::helper_requests::{ProcessQuery, SmartQuery};
    use dyno_core::helper_wire::MAX_AUDIT_REPLY;
    use dyno_core::network_traffic::TrafficCounter;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct Traffic(Result<Vec<TrafficCounter>, PlatformError>);
    impl ProcessTraffic for Traffic {
        fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
            self.0.clone()
        }
    }

    struct Items(Result<LoginItemsReply, PlatformError>);
    impl LoginItemsReader for Items {
        fn read(&self, uid: u32) -> Result<LoginItemsReply, PlatformError> {
            self.0.clone().map(|mut r| {
                r.other_accounts = uid;
                r
            })
        }
    }

    fn items_reply() -> LoginItemsReply {
        LoginItemsReply {
            items: vec![],
            truncated: 0,
            other_accounts: 0,
        }
    }

    struct Rig {
        d: Dispatcher,
        gone: Arc<AtomicBool>,
        _dir: tempfile::TempDir,
    }

    fn rig(host: HelperHost, traffic: Result<Vec<TrafficCounter>, PlatformError>) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let gone = Arc::new(AtomicBool::new(false));
        let flag = gone.clone();
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1.0.0".into(),
                api: 1,
                host,
                agent_exe: "/x/dyno-agent".into(),
            },
            traffic: Box::new(Traffic(traffic)),
            audit: AuditLog::new(dir.path().join("audit.jsonl")),
            clock: Box::new(|| 1_000),
            on_uninstall: Box::new(move || flag.store(true, Ordering::SeqCst)),
            login_items: Box::new(Items(Ok(items_reply()))),
            log_tail: Box::new(|n| (0..n.min(3)).map(|i| format!("line {i}")).collect()),
        });
        Rig { d, gone, _dir: dir }
    }

    fn audit(r: &Rig) -> Vec<AuditEntry> {
        match r.d.handle(None, HelperRequest::Audit(50)).unwrap() {
            HelperReply::Entries(e) => e,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn it_says_who_it_is() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        match r.d.handle(Some(4), HelperRequest::Info).unwrap() {
            HelperReply::About(i, _) => assert_eq!((i.version.as_str(), i.host), ("1.0.0", HelperHost::Linux)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn traffic_is_bounded_and_a_system_without_it_says_so() {
        let many: Vec<TrafficCounter> = (0..MAX_COUNTERS as u32 + 10)
            .map(|pid| TrafficCounter {
                pid,
                name: "p".into(),
                bytes_in: 1,
                bytes_out: 2,
            })
            .collect();
        let r = rig(HelperHost::Windows, Ok(many));
        match r.d.handle(None, HelperRequest::Traffic).unwrap() {
            HelperReply::Counters(c) => assert_eq!(c.len(), MAX_COUNTERS),
            other => panic!("{other:?}"),
        }
        let unsupported = rig(HelperHost::Linux, Err(PlatformError::Unsupported("no".into())));
        assert_eq!(
            unsupported.d.handle(None, HelperRequest::Traffic).unwrap_err().code,
            codes::UNSUPPORTED
        );
    }

    #[test]
    fn the_audit_reply_is_bounded_and_a_turned_away_caller_is_recorded() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        for _ in 0..MAX_AUDIT_REPLY + 20 {
            r.d.record_refused_caller(Some(3), &"x".repeat(1000));
        }
        match r.d.handle(None, HelperRequest::Audit(u32::MAX)).unwrap() {
            HelperReply::Entries(e) => {
                assert_eq!(e.len(), MAX_AUDIT_REPLY as usize);
                assert_eq!(e[0].detail.len(), 300);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn invalid_requests_are_refused_and_later_readers_say_unsupported() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        let bad = HelperRequest::Processes(ProcessQuery {
            limit: 0,
            ..Default::default()
        });
        assert_eq!(r.d.handle(None, bad).unwrap_err().code, codes::REFUSED);
        let bad = HelperRequest::Smart(SmartQuery { max_disks: 99 });
        assert_eq!(r.d.handle(None, bad).unwrap_err().code, codes::REFUSED);
        let ok = HelperRequest::Smart(SmartQuery::default());
        assert_eq!(r.d.handle(None, ok).unwrap_err().code, codes::UNSUPPORTED);
    }

    #[test]
    fn uninstalling_is_audited_then_handed_to_the_remover() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        assert_eq!(r.d.handle(Some(5), HelperRequest::Uninstall).unwrap(), HelperReply::Bye);
        assert!(r.gone.load(Ordering::SeqCst));
        assert!(audit(&r)[0].what.contains("Remove the Dyno helper"));
    }

    #[test]
    fn a_log_that_cannot_be_written_means_nothing_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1".into(),
                api: 1,
                host: HelperHost::Windows,
                agent_exe: "/x".into(),
            },
            traffic: Box::new(Traffic(Ok(vec![]))),
            audit: AuditLog::new(dir.path().join("missing").join("a.jsonl")),
            clock: Box::new(|| 5),
            on_uninstall: Box::new(|| {}),
            login_items: Box::new(Items(Ok(items_reply()))),
            log_tail: Box::new(|_| vec![]),
        });
        assert!(d
            .handle(None, HelperRequest::Uninstall)
            .unwrap_err()
            .message
            .contains("Nothing was removed"));
    }

    #[test]
    fn info_carries_the_callers_view_and_login_items_are_read_for_the_callers_uid() {
        let r = rig(HelperHost::Macos, Ok(vec![]));
        let view = CallerView {
            scope: Visibility::Full,
            tools: vec![],
        };
        let ctx = Ctx {
            pid: Some(9),
            uid: Some(501),
            access: Access::Full,
            view: view.clone(),
        };
        match r.d.handle_for(&ctx, HelperRequest::Info).unwrap() {
            HelperReply::About(_, v) => assert_eq!(v, view),
            other => panic!("{other:?}"),
        }
        match r.d.handle_for(&ctx, HelperRequest::LoginItems).unwrap() {
            HelperReply::LoginItems(l) => assert_eq!(l.other_accounts, 501, "the reader got the connection's uid"),
            other => panic!("{other:?}"),
        }
        // A caller without a Unix account (Windows) gets "unsupported", never somebody else's items.
        let e = r.d.handle_for(&Ctx::anonymous(None), HelperRequest::LoginItems).unwrap_err();
        assert_eq!(e.code, codes::UNSUPPORTED);
        let failing = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1".into(),
                api: 2,
                host: HelperHost::Macos,
                agent_exe: String::new(),
            },
            traffic: Box::new(Traffic(Ok(vec![]))),
            audit: AuditLog::new(r._dir.path().join("a2.jsonl")),
            clock: Box::new(|| 1),
            on_uninstall: Box::new(|| {}),
            login_items: Box::new(Items(Err(PlatformError::AccessDenied("no".into())))),
            log_tail: Box::new(|_| vec![]),
        });
        assert!(failing.handle_for(&ctx, HelperRequest::LoginItems).is_err());
    }

    #[test]
    fn the_log_tail_is_clamped_to_its_cap() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        match r.d.handle(None, HelperRequest::LogTail(u32::MAX)).unwrap() {
            HelperReply::Log(l) => assert_eq!(l, ["line 0", "line 1", "line 2"]),
            other => panic!("{other:?}"),
        }
    }
}
