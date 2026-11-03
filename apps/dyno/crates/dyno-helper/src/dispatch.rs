//! What the helper does with a request (ADR 0016 section 4): it answers read-only questions (who it is, its audit
//! log, per-program traffic) and removes itself on request. It makes no change to the machine's settings. Pure over
//! its ports: tests give it a fake [`ProcessTraffic`] and a temporary audit log.

use crate::audit::AuditLog;
use dyno_core::helper_wire::{codes, error_code, AuditEntry, AuditVerdict, HelperInfo, HelperReply, HelperRequest, MAX_AUDIT_REPLY};
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
        match request {
            HelperRequest::Info => Ok(HelperReply::About(self.info.clone())),
            HelperRequest::Audit(n) => Ok(HelperReply::Entries(self.audit.tail(n.min(MAX_AUDIT_REPLY) as usize))),
            HelperRequest::Traffic => {
                let mut counters = self.traffic.counters().map_err(|e| remote(&e))?;
                counters.truncate(MAX_COUNTERS);
                Ok(HelperReply::Counters(counters))
            }
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
    use dyno_core::helper::HelperHost;
    use dyno_core::network_traffic::TrafficCounter;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct Traffic(Result<Vec<TrafficCounter>, PlatformError>);
    impl ProcessTraffic for Traffic {
        fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
            self.0.clone()
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
            HelperReply::About(i) => assert_eq!((i.version.as_str(), i.host), ("1.0.0", HelperHost::Linux)),
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
        });
        assert!(d
            .handle(None, HelperRequest::Uninstall)
            .unwrap_err()
            .message
            .contains("Nothing was removed"));
    }
}
