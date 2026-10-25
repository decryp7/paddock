//! What the helper does with a request (ADR 0016 section 4): refuse everything outside the closed list, validate
//! again, audit before and after, change one thing at a time, read the setting back. Pure over its ports: tests give
//! it a fake [`TuningSystem`], a fake [`ProcessTraffic`] and a temporary audit log.

use crate::audit::AuditLog;
use dyno_core::helper::{HelperAction, HelperOutcome, TuningHost};
use dyno_core::helper_wire::{codes, error_code, AuditEntry, AuditVerdict, HelperInfo, HelperReply, HelperRequest, MAX_AUDIT_REPLY};
use dyno_core::network_traffic::MAX_COUNTERS;
use dyno_core::ports::{PlatformError, ProcessTraffic, TuningSystem};
use dyno_ipc::RemoteError;
use std::sync::{Mutex, PoisonError};

/// Everything a [`Dispatcher`] needs from outside.
pub struct Parts {
    /// What the helper tells about itself.
    pub info: HelperInfo,
    /// The settings it may change (the administrator changes, as root).
    pub tuning: Box<dyn TuningSystem>,
    /// Per-program traffic counters.
    pub traffic: Box<dyn ProcessTraffic>,
    /// The audit log.
    pub audit: AuditLog,
    /// Seconds since 1970.
    pub clock: Box<dyn Fn() -> i64 + Send + Sync>,
    /// Called after an `Uninstall` request was audited and answered (it removes the helper in the background).
    pub on_uninstall: Box<dyn Fn() + Send + Sync>,
}

/// The request handler. Changes are serialised by the lock around the tuning system.
pub struct Dispatcher {
    host: TuningHost,
    info: HelperInfo,
    tuning: Mutex<Box<dyn TuningSystem>>,
    traffic: Box<dyn ProcessTraffic>,
    audit: AuditLog,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    on_uninstall: Box<dyn Fn() + Send + Sync>,
}

fn remote(e: &PlatformError) -> RemoteError {
    RemoteError::new(error_code(e), e.to_string())
}

impl Dispatcher {
    /// A dispatcher over `parts`. Also marks a change that a previous run began and did not finish.
    pub fn new(parts: Parts) -> Dispatcher {
        let d = Dispatcher {
            host: parts.info.host,
            info: parts.info,
            tuning: Mutex::new(parts.tuning),
            traffic: parts.traffic,
            audit: parts.audit,
            clock: parts.clock,
            on_uninstall: parts.on_uninstall,
        };
        d.audit.recover((d.clock)());
        d
    }

    /// Records a connection that was turned away (wrong user or program) without answering it.
    pub fn record_refused_caller(&self, peer: Option<u32>, why: &str) {
        let _ = self.audit.record(&AuditEntry {
            at: (self.clock)(),
            peer,
            what: "A connection was refused".into(),
            verdict: AuditVerdict::Refused,
            before: None,
            after: None,
            detail: why.chars().take(300).collect(),
        });
    }

    fn entry(&self, peer: Option<u32>, what: &str, verdict: AuditVerdict) -> AuditEntry {
        AuditEntry {
            at: (self.clock)(),
            peer,
            what: what.chars().take(500).collect(),
            verdict,
            before: None,
            after: None,
            detail: String::new(),
        }
    }

    /// Checks that `action` is one the helper makes on this system. Refusals are audited.
    fn allow(&self, peer: Option<u32>, action: &HelperAction) -> Result<(), RemoteError> {
        let refuse = |why: String| {
            let _ = self.audit.record(&AuditEntry {
                detail: why.clone(),
                ..self.entry(peer, &action.preview().join("; "), AuditVerdict::Refused)
            });
            RemoteError::new(codes::REFUSED, why)
        };
        action.validate().map_err(refuse)?;
        if !action.needs_admin_on(self.host) {
            return Err(refuse(
                "That is not a change the helper makes: it needs no administrator rights.".into(),
            ));
        }
        Ok(())
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
            HelperRequest::Read(action) => {
                self.allow(peer, &action)?;
                let mut tuning = self.tuning.lock().unwrap_or_else(PoisonError::into_inner);
                tuning.read(&action).map(HelperReply::Value).map_err(|e| remote(&e))
            }
            HelperRequest::Apply(action) => self.apply(peer, &action),
            HelperRequest::Uninstall => {
                let mut e = self.entry(peer, "Remove the Dyno helper from this computer", AuditVerdict::Applied);
                e.detail = "Asked by the authorised user.".into();
                self.audit
                    .record(&e)
                    .map_err(|m| RemoteError::new(codes::FAILED, format!("{m} Nothing was removed.")))?;
                (self.on_uninstall)();
                Ok(HelperReply::Bye)
            }
        }
    }

    fn apply(&self, peer: Option<u32>, action: &HelperAction) -> Result<HelperReply, RemoteError> {
        self.allow(peer, action)?;
        let what = action.preview().join("; ");
        let expected = action.setting().1;
        let mut tuning = self.tuning.lock().unwrap_or_else(PoisonError::into_inner);
        let before = tuning.read(action).ok();
        // The trace comes first: a change the helper cannot record is not made.
        let mut started = self.entry(peer, &what, AuditVerdict::Started);
        started.before = before.clone();
        self.audit
            .record(&started)
            .map_err(|m| RemoteError::new(codes::FAILED, format!("{m} Nothing was changed.")))?;
        let end = |verdict, after: Option<String>, detail: String| {
            let mut e = self.entry(peer, &what, verdict);
            e.before = before.clone();
            e.after = after;
            e.detail = detail;
            let _ = self.audit.record(&e);
        };
        if let Err(e) = tuning.apply(action) {
            end(AuditVerdict::Failed, None, e.to_string());
            return Err(remote(&e));
        }
        match tuning.read(action) {
            Ok(after) if after != expected => {
                let why = format!("{what} did not take effect: the setting reads {after}, expected {expected}.");
                end(AuditVerdict::Failed, Some(after), why.clone());
                Err(RemoteError::new(codes::FAILED, why))
            }
            Ok(after) => {
                end(AuditVerdict::Applied, Some(after), String::new());
                Ok(HelperReply::Outcome(HelperOutcome::Done))
            }
            // The change went through but cannot be read back here; the agent reads it with its own eyes where it can.
            Err(e) => {
                end(AuditVerdict::Applied, None, format!("Applied; could not read it back: {e}"));
                Ok(HelperReply::Outcome(HelperOutcome::Done))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper::MachineStartup;
    use dyno_core::network_traffic::TrafficCounter;
    use dyno_platform::tuning::FakeTuning;
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
        fake: FakeTuning,
        gone: Arc<AtomicBool>,
        _dir: tempfile::TempDir,
    }

    fn rig(host: TuningHost, traffic: Result<Vec<TrafficCounter>, PlatformError>) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeTuning::new(host);
        let gone = Arc::new(AtomicBool::new(false));
        let flag = gone.clone();
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1.0.0".into(),
                api: 1,
                host,
                agent_exe: "/x/dyno-agent".into(),
            },
            tuning: Box::new(fake.clone()),
            traffic: Box::new(Traffic(traffic)),
            audit: AuditLog::new(dir.path().join("audit.jsonl")),
            clock: Box::new(|| 1_000),
            on_uninstall: Box::new(move || flag.store(true, Ordering::SeqCst)),
        });
        Rig { d, fake, gone, _dir: dir }
    }

    fn audit(r: &Rig) -> Vec<AuditEntry> {
        match r.d.handle(None, HelperRequest::Audit(50)).unwrap() {
            HelperReply::Entries(e) => e,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn it_says_who_it_is() {
        let r = rig(TuningHost::Linux, Ok(vec![]));
        match r.d.handle(Some(4), HelperRequest::Info).unwrap() {
            HelperReply::About(i) => assert_eq!((i.version.as_str(), i.host), ("1.0.0", TuningHost::Linux)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_administrator_change_is_audited_applied_and_read_back() {
        let r = rig(TuningHost::Windows, Ok(vec![]));
        r.fake.set_value(&HelperAction::SetHibernate(false), "on");
        let a = HelperAction::SetHibernate(false);
        assert_eq!(
            r.d.handle(Some(77), HelperRequest::Apply(a.clone())).unwrap(),
            HelperReply::Outcome(HelperOutcome::Done)
        );
        assert_eq!(r.fake.applied(), vec![a]);
        let log = audit(&r);
        assert_eq!(
            log.iter().map(|e| e.verdict).collect::<Vec<_>>(),
            [AuditVerdict::Applied, AuditVerdict::Started]
        );
        assert_eq!(
            (log[0].before.as_deref(), log[0].after.as_deref(), log[0].peer),
            (Some("on"), Some("off"), Some(77))
        );
        assert!(log[0].what.contains("hibernation off"));
    }

    #[test]
    fn a_change_that_does_not_need_administrator_rights_is_not_the_helpers_to_make() {
        let r = rig(TuningHost::Windows, Ok(vec![]));
        let not_admin = HelperAction::SetPowerScheme(dyno_core::helper::BALANCED_SCHEME.into());
        let e = r.d.handle(Some(1), HelperRequest::Apply(not_admin.clone())).unwrap_err();
        assert_eq!(e.code, codes::REFUSED);
        assert!(r.fake.applied().is_empty());
        assert_eq!(
            r.d.handle(Some(1), HelperRequest::Read(not_admin)).unwrap_err().code,
            codes::REFUSED
        );
        // Whether a change is administrator-only depends on the system: macOS sleep timers are, Windows ones are not.
        let sleep = HelperAction::SetSleepMinutes {
            source: dyno_core::helper::SleepSource::Ac,
            minutes: 5,
        };
        assert_eq!(
            r.d.handle(None, HelperRequest::Apply(sleep.clone())).unwrap_err().code,
            codes::REFUSED
        );
        let mac = rig(TuningHost::Macos, Ok(vec![]));
        assert!(mac.d.handle(None, HelperRequest::Apply(sleep)).is_ok());
        let log = audit(&r);
        assert!(log.iter().all(|e| e.verdict == AuditVerdict::Refused) && log.len() == 3);
    }

    #[test]
    fn invalid_arguments_are_refused_again_even_though_the_agent_validated() {
        let r = rig(TuningHost::Windows, Ok(vec![]));
        for bad in [
            HelperAction::SetDefenderExcluded {
                paths: vec!["C:\\Windows\\System32".into()],
                excluded: true,
            },
            HelperAction::SetDefenderExcluded {
                paths: vec!["relative".into()],
                excluded: true,
            },
            HelperAction::SetMachineStartupEnabled {
                location: MachineStartup::RunKey,
                name: "a\\b".into(),
                enabled: false,
            },
            HelperAction::SetChargeLimit(3),
        ] {
            assert_eq!(r.d.handle(None, HelperRequest::Apply(bad)).unwrap_err().code, codes::REFUSED);
        }
        assert!(r.fake.applied().is_empty());
        assert!(audit(&r).iter().all(|e| e.verdict == AuditVerdict::Refused && !e.detail.is_empty()));
    }

    #[test]
    fn a_failing_change_is_reported_with_its_kind_and_audited() {
        let r = rig(TuningHost::Windows, Ok(vec![]));
        r.fake.fail_next(PlatformError::AccessDenied("Tamper Protection is on".into()));
        let e =
            r.d.handle(None, HelperRequest::Apply(HelperAction::SetHibernate(true)))
                .unwrap_err();
        assert_eq!((e.code.as_str(), e.message.as_str()), (codes::DENIED, "Tamper Protection is on"));
        let log = audit(&r);
        assert_eq!(log[0].verdict, AuditVerdict::Failed);
        assert_eq!(log[0].detail, "Tamper Protection is on");
    }

    #[test]
    fn a_change_that_does_not_read_back_as_asked_fails() {
        struct Stubborn;
        impl TuningSystem for Stubborn {
            fn host(&self) -> TuningHost {
                TuningHost::Windows
            }
            fn read(&mut self, _: &HelperAction) -> Result<String, PlatformError> {
                Ok("on".into())
            }
            fn apply(&mut self, _: &HelperAction) -> Result<(), PlatformError> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1".into(),
                api: 1,
                host: TuningHost::Windows,
                agent_exe: "/x".into(),
            },
            tuning: Box::new(Stubborn),
            traffic: Box::new(Traffic(Ok(vec![]))),
            audit: AuditLog::new(dir.path().join("a.jsonl")),
            clock: Box::new(|| 5),
            on_uninstall: Box::new(|| {}),
        });
        let e = d.handle(None, HelperRequest::Apply(HelperAction::SetHibernate(false))).unwrap_err();
        assert_eq!(e.code, codes::FAILED);
        assert!(e.message.contains("did not take effect") && e.message.contains("reads on"));
    }

    #[test]
    fn a_log_that_cannot_be_written_means_nothing_is_changed() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeTuning::new(TuningHost::Windows);
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1".into(),
                api: 1,
                host: TuningHost::Windows,
                agent_exe: "/x".into(),
            },
            tuning: Box::new(fake.clone()),
            traffic: Box::new(Traffic(Ok(vec![]))),
            audit: AuditLog::new(dir.path().join("missing").join("a.jsonl")),
            clock: Box::new(|| 5),
            on_uninstall: Box::new(|| {}),
        });
        let e = d.handle(None, HelperRequest::Apply(HelperAction::SetHibernate(false))).unwrap_err();
        assert!(e.message.contains("Nothing was changed"));
        assert!(fake.applied().is_empty());
        assert!(d
            .handle(None, HelperRequest::Uninstall)
            .unwrap_err()
            .message
            .contains("Nothing was removed"));
    }

    #[test]
    fn reads_return_the_value_and_traffic_is_bounded() {
        let many: Vec<TrafficCounter> = (0..MAX_COUNTERS as u32 + 10)
            .map(|pid| TrafficCounter {
                pid,
                name: "p".into(),
                bytes_in: 1,
                bytes_out: 2,
            })
            .collect();
        let r = rig(TuningHost::Windows, Ok(many));
        r.fake.set(&HelperAction::SetHibernate(true));
        assert_eq!(
            r.d.handle(None, HelperRequest::Read(HelperAction::SetHibernate(true))).unwrap(),
            HelperReply::Value("on".into())
        );
        match r.d.handle(None, HelperRequest::Traffic).unwrap() {
            HelperReply::Counters(c) => assert_eq!(c.len(), MAX_COUNTERS),
            other => panic!("{other:?}"),
        }
        let unsupported = rig(TuningHost::Linux, Err(PlatformError::Unsupported("no".into())));
        assert_eq!(
            unsupported.d.handle(None, HelperRequest::Traffic).unwrap_err().code,
            codes::UNSUPPORTED
        );
        assert_eq!(
            unsupported
                .d
                .handle(None, HelperRequest::Read(HelperAction::SetChargeLimit(80)))
                .unwrap_err()
                .code,
            codes::UNSUPPORTED
        );
    }

    #[test]
    fn the_audit_reply_is_bounded_and_a_turned_away_caller_is_recorded() {
        let r = rig(TuningHost::Linux, Ok(vec![]));
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
        let r = rig(TuningHost::Linux, Ok(vec![]));
        assert_eq!(r.d.handle(Some(5), HelperRequest::Uninstall).unwrap(), HelperReply::Bye);
        assert!(r.gone.load(Ordering::SeqCst));
        assert!(audit(&r)[0].what.contains("Remove the Dyno helper"));
    }

    #[test]
    fn a_change_the_last_run_began_is_marked_interrupted_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(dir.path().join("audit.jsonl"));
        log.record(&AuditEntry {
            at: 1,
            peer: None,
            what: "Turn hibernation off".into(),
            verdict: AuditVerdict::Started,
            before: Some("on".into()),
            after: None,
            detail: String::new(),
        })
        .unwrap();
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1".into(),
                api: 1,
                host: TuningHost::Linux,
                agent_exe: "/x".into(),
            },
            tuning: Box::new(FakeTuning::new(TuningHost::Linux)),
            traffic: Box::new(Traffic(Ok(vec![]))),
            audit: log,
            clock: Box::new(|| 9),
            on_uninstall: Box::new(|| {}),
        });
        match d.handle(None, HelperRequest::Audit(5)).unwrap() {
            HelperReply::Entries(e) => assert_eq!((e[0].verdict, e[0].at), (AuditVerdict::Interrupted, 9)),
            other => panic!("{other:?}"),
        }
    }
}
