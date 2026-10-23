#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Tuning end to end over the fake platform and a fake tuning system: observe-only until a click, previews with the
//! inverse, a recorded undo, outside-change detection, administrator-only items blocked where no helper can exist, failures contained.

use dyno_agent::agent::lock;
use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::activity::{ActivityKind, UndoState};
use dyno_core::facts::{PowerPlan, SystemFacts};
use dyno_core::health::FixKind;
use dyno_core::helper::{HelperAction, SleepSource, TuningHost, POWER_SAVER_SCHEME};
use dyno_core::metrics::{BatterySample, PowerSource, Reading};
use dyno_core::ports::{PlatformError, TuningSystem};
use dyno_core::protocol::{Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_core::tuning::TuningSuggestion;
use dyno_ipc::Peer;
use dyno_platform::fake::{quiet_sample, Fake};
use dyno_platform::tuning::FakeTuning;

/// A computer where the privileged helper cannot be installed (never the real one: tests do not look at the machine).
struct NoHelper;

impl dyno_agent::helper_centre::HelperControl for NoHelper {
    fn can_install(&self) -> Result<(), String> {
        Err("no helper in tests".into())
    }
    fn status(&mut self) -> dyno_core::helper_wire::HelperStatus {
        dyno_core::helper_wire::assess(
            TuningHost::Macos,
            Err("no helper in tests".into()),
            dyno_core::helper_wire::Probe::Absent,
            "1",
        )
    }
    fn install(&mut self) -> Result<dyno_core::helper_wire::HelperStatus, dyno_core::protocol::AgentError> {
        Err(dyno_core::protocol::AgentError::new(
            dyno_core::protocol::ErrorCode::Unsupported,
            "no helper in tests",
        ))
    }
    fn uninstall(&mut self) -> Result<dyno_core::helper_wire::HelperStatus, dyno_core::protocol::AgentError> {
        Err(dyno_core::protocol::AgentError::new(
            dyno_core::protocol::ErrorCode::Unsupported,
            "no helper in tests",
        ))
    }
}

struct Harness {
    running: Option<Running>,
    tuning: FakeTuning,
    peer: Peer<Message>,
    _tmp: tempfile::TempDir,
}

fn battery(source: PowerSource) -> Reading<BatterySample> {
    Reading::Available(BatterySample {
        charge: 80.0,
        source,
        health: 95.0,
        cycles: None,
        seconds_to_empty: None,
    })
}

impl Harness {
    fn start(host: TuningHost, power: Option<PowerSource>, facts: Option<SystemFacts>) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        let mut sample = quiet_sample(Timestamp(0));
        if let Some(p) = power {
            sample.battery = battery(p);
        }
        fake.push_sample(sample);
        fake.with(|s| s.facts = facts);
        let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        let tuning = FakeTuning::new(host);
        *lock(&running.shared.ports.tuning) = Box::new(tuning.clone());
        *lock(&running.shared.ports.helper) = Box::new(NoHelper);
        let peer = connect(&dirs).unwrap().0;
        let h = Harness {
            running: Some(running),
            tuning,
            peer,
            _tmp: tmp,
        };
        for _ in 0..100 {
            if ask(&h.peer, Request::Health).is_ok() {
                return h;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("the agent never took a first measurement");
    }

    fn list(&self) -> Vec<TuningSuggestion> {
        let Reply::Tuning(l) = ask(&self.peer, Request::Tuning).unwrap() else {
            panic!("tuning")
        };
        l
    }

    fn apply(&self, id: &str) -> Result<dyno_core::activity::ActivityEntry, String> {
        match ask(&self.peer, Request::ApplyTuning { id: id.into() })? {
            Reply::Activity(e) => Ok(e),
            other => panic!("{other:?}"),
        }
    }

    fn revert(&self, id: &str, force: bool) -> Result<dyno_core::activity::ActivityEntry, String> {
        match ask(&self.peer, Request::RevertTuning { id: id.into(), force })? {
            Reply::Activity(e) => Ok(e),
            other => panic!("{other:?}"),
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn sleep(minutes: u32) -> HelperAction {
    HelperAction::SetSleepMinutes {
        source: SleepSource::Battery,
        minutes,
    }
}

fn windows_laptop() -> Harness {
    let h = Harness::start(TuningHost::Windows, Some(PowerSource::Battery), None);
    h.tuning.set_value(&sleep(0), "0");
    h
}

#[test]
fn listing_changes_nothing_and_previews_the_change_and_its_inverse() {
    let h = windows_laptop();
    let l = h.list();
    let s = l.iter().find(|s| s.id == "sleepOnBattery").expect("a sleep suggestion");
    assert_eq!(s.preview, ["Sleep after 15 minutes of idling when on battery"]);
    assert_eq!(s.undo_preview, ["Never sleep when on battery"]);
    assert!(!s.applied && !s.needs_admin && s.blocked.is_none() && !s.changed_outside);
    assert!(h.tuning.applied().is_empty(), "observe-only until a click");
}

#[test]
fn apply_is_logged_with_its_undo_and_the_activity_undo_reverts_it() {
    let h = windows_laptop();
    let e = h.apply("sleepOnBattery").unwrap();
    assert_eq!(e.kind, ActivityKind::Tuning);
    assert_eq!(e.undo, UndoState::Available);
    assert_eq!(h.tuning.applied(), [sleep(15)]);
    let l = h.list();
    let s = l.iter().find(|s| s.id == "sleepOnBattery").unwrap();
    assert!(s.applied && !s.changed_outside);
    let e2 = h.apply("sleepOnBattery").unwrap_err();
    assert!(e2.contains("already applied"), "{e2}");

    let Reply::Activity(undone) = ask(&h.peer, Request::Undo { activity_id: e.id.clone() }).unwrap() else {
        panic!("undo")
    };
    assert_eq!(undone.undo, UndoState::Undone);
    assert_eq!(h.tuning.applied(), [sleep(15), sleep(0)]);
    assert!(!h.list().iter().any(|s| s.applied), "nothing is applied any more");
}

#[test]
fn a_change_made_outside_dyno_is_detected_before_undo() {
    let h = windows_laptop();
    h.apply("sleepOnBattery").unwrap();
    // Someone sets the timer to 5 minutes in Settings.
    h.tuning.set_value(&sleep(0), "5");
    let l = h.list();
    assert!(l.iter().find(|s| s.id == "sleepOnBattery").unwrap().changed_outside);
    let err = h.revert("sleepOnBattery", false).unwrap_err();
    assert!(err.contains("Something else changed"), "{err}");
    assert_eq!(h.tuning.applied(), [sleep(15)], "the user's own setting was not overwritten");
    let e = h.revert("sleepOnBattery", true).unwrap();
    assert!(e.title.starts_with("Reverted"), "{}", e.title);
    assert_eq!(h.tuning.applied(), [sleep(15), sleep(0)]);
    assert!(h.revert("sleepOnBattery", false).unwrap_err().contains("nothing to revert"));
}

#[test]
fn administrator_items_are_listed_but_not_applicable_where_the_helper_cannot_be_installed() {
    let h = Harness::start(TuningHost::Macos, Some(PowerSource::Battery), None);
    h.tuning.set_value(&sleep(0), "0");
    let l = h.list();
    let s = l.iter().find(|s| s.id == "sleepOnBattery").unwrap();
    assert!(s.needs_admin);
    assert!(s.blocked.as_deref().unwrap().contains("administrator rights"));
    let err = h.apply("sleepOnBattery").unwrap_err();
    assert!(err.contains("administrator rights"), "{err}");
    assert!(h.tuning.applied().is_empty());
}

#[test]
fn a_failing_system_leaves_no_record_and_unknown_ids_are_not_found() {
    let h = windows_laptop();
    h.tuning.fail_next(PlatformError::Failed("powercfg failed: boom".into()));
    let err = h.apply("sleepOnBattery").unwrap_err();
    assert!(err.contains("boom"), "{err}");
    assert!(!h.list().iter().any(|s| s.applied));
    assert!(h.apply("nonsense").unwrap_err().contains("no such suggestion"));
}

/// A system that accepts every change and keeps reading the old value.
struct Stuck(FakeTuning);

impl TuningSystem for Stuck {
    fn host(&self) -> TuningHost {
        self.0.host()
    }
    fn read(&mut self, a: &HelperAction) -> Result<String, PlatformError> {
        let mut inner = self.0.clone();
        inner.read(a)
    }
    fn apply(&mut self, _: &HelperAction) -> Result<(), PlatformError> {
        Ok(())
    }
}

#[test]
fn a_change_the_system_ignores_is_reported_not_recorded() {
    let h = windows_laptop();
    *lock(&h.running.as_ref().unwrap().shared.ports.tuning) = Box::new(Stuck(h.tuning.clone()));
    let err = h.apply("sleepOnBattery").unwrap_err();
    assert!(err.contains("did not take effect"), "{err}");
    assert!(!h.list().iter().any(|s| s.applied));
}

#[test]
fn the_health_report_links_to_the_tuning_catalogue() {
    let mut facts = SystemFacts::unknown("Windows 11");
    facts.power_plan = Reading::Available(PowerPlan {
        id: POWER_SAVER_SCHEME.into(),
        name: "Power saver".into(),
        power_saver: true,
    });
    let h = Harness::start(TuningHost::Windows, None, Some(facts));
    let Reply::Health(report) = ask(&h.peer, Request::Health).unwrap() else {
        panic!("health")
    };
    let fix = report.reasons.iter().find_map(|r| r.fix.as_ref()).expect("a fix");
    assert_eq!(fix.kind, FixKind::Tuning("powerPlan".into()));
    assert!(fix.preview[0].contains("Balanced"));
    h.apply("powerPlan").unwrap();
    assert_eq!(
        h.tuning.applied(),
        [HelperAction::SetPowerScheme(dyno_core::helper::BALANCED_SCHEME.into())]
    );
    let Reply::Health(after) = ask(&h.peer, Request::Health).unwrap() else {
        panic!("health")
    };
    assert!(after
        .reasons
        .iter()
        .all(|r| r.fix.as_ref().is_none_or(|f| f.kind != FixKind::Tuning("powerPlan".into()))));
}
