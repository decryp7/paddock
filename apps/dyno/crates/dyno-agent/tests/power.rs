#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Keep awake end to end over the fake platform (ER-14): a real IPC connection and the agent's power thread, with
//! `FakePower` standing in for the OS, so no test holds a real machine awake and nothing can put it to sleep.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::power::awake::{AwakeScope, AwakeSpec, AwakeUntil, AwakeView};
use dyno_core::power::{Capability, PowerView};
use dyno_core::protocol::{LiveDetail, Message, Reply, Request};
use dyno_platform::fake::Fake;
use dyno_platform::PowerPorts;
use paddock_ipc::{BodyOptions, BodyPeer};
use paddock_power::{FakeEvents, FakePower, PowerError, PowerEvent};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// How long a test waits for the agent's power thread before it calls the run hung (a guard, never the expected wait).
const HANG_GUARD: Duration = Duration::from_secs(30);

struct Harness {
    running: Option<Running>,
    os: Arc<FakePower>,
    events: Arc<FakeEvents>,
    actions: Arc<dyno_platform::fake::FakePowerActions>,
    signals: Arc<dyno_platform::fake::FakeIdleSignals>,
    fake: Fake,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn start() -> Harness {
        Harness::start_with(|_| {})
    }

    /// Starts the agent; `seed` runs on the data folder first (to leave a marker behind, say).
    fn start_with(seed: impl FnOnce(&Dirs)) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        std::fs::create_dir_all(dirs.root()).unwrap();
        seed(&dirs);
        let (os, events) = (Arc::new(FakePower::new()), Arc::new(FakeEvents::new()));
        let fake = Fake::new();
        fake.add_process(dyno_platform::fake::process(100, "busy", 60.0));
        let mut platform = fake.platform();
        let (actions, signals) = (
            Arc::new(dyno_platform::fake::FakePowerActions::default()),
            Arc::new(dyno_platform::fake::FakeIdleSignals::default()),
        );
        platform.power = PowerPorts {
            os: os.clone(),
            events: events.clone(),
            actions: actions.clone(),
            signals: signals.clone(),
        };
        let running = Running::start(dirs.clone(), platform, Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        Harness {
            running: Some(running),
            os,
            events,
            actions,
            signals,
            fake,
            dirs,
            _tmp: tmp,
        }
    }

    fn client(&self) -> BodyPeer<Message> {
        connect(&self.dirs).unwrap().0
    }

    /// A connection that receives the pushed power views.
    fn watcher(&self) -> (BodyPeer<Message>, Receiver<PowerView>) {
        let d = dyno_agent::discovery::Discovery::read(&self.dirs.discovery()).unwrap();
        let conn = dyno_agent::wire::connect(&d.endpoint, &d.token().unwrap()).unwrap();
        let (tx, rx) = channel();
        let peer = BodyPeer::start(
            conn,
            BodyOptions::new("the agent").on_push(move |m| {
                if let Message::PowerChanged(v) = m {
                    let _ = tx.send(*v);
                }
            }),
        )
        .unwrap();
        ask(
            &peer,
            Request::Subscribe {
                detail: LiveDetail::Summary,
            },
        )
        .unwrap();
        (peer, rx)
    }

    fn marker(&self) -> std::path::PathBuf {
        self.dirs.root().join("keep-awake.active")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn indefinite() -> AwakeSpec {
    AwakeSpec {
        scope: AwakeScope::System,
        until: AwakeUntil::Indefinite,
    }
}

fn power(r: Reply) -> PowerView {
    match r {
        Reply::Power(v) => *v,
        other => panic!("expected a power view, got {other:?}"),
    }
}

fn next_view(rx: &Receiver<PowerView>) -> PowerView {
    rx.recv_timeout(HANG_GUARD).expect("a pushed power view")
}

#[test]
fn a_hold_is_taken_with_the_reason_shown_pushed_and_released_on_stop() {
    let h = Harness::start();
    let (_w, pushed) = h.watcher();
    let c = h.client();
    assert!(matches!(power(ask(&c, Request::PowerState).unwrap()).awake, AwakeView::Off));
    let spec = AwakeSpec {
        scope: AwakeScope::SystemAndDisplay,
        until: AwakeUntil::For { minutes: 60 },
    };
    let v = power(ask(&c, Request::PowerKeepAwake { spec: spec.clone() }).unwrap());
    let AwakeView::Holding {
        spec: held,
        ends: Some(_),
        os_ref,
        ..
    } = v.awake
    else {
        panic!("not holding: {:?}", v.awake)
    };
    assert_eq!((held, os_ref.as_str()), (spec, "fake#1"));
    assert!(
        matches!(next_view(&pushed).awake, AwakeView::Holding { .. }),
        "the change is pushed to every app"
    );
    assert_eq!(h.os.log(), ["acquire#1 SystemAndDisplay keeping awake for 1 h, you asked"]);
    assert!(h.marker().exists(), "a marker says a hold is active");
    assert!(matches!(power(ask(&c, Request::PowerStopKeepAwake).unwrap()).awake, AwakeView::Off));
    assert!(matches!(next_view(&pushed).awake, AwakeView::Off));
    assert_eq!(h.os.live(), 0);
    assert!(!h.marker().exists());
}

#[test]
fn a_new_request_replaces_the_hold_without_a_gap() {
    let h = Harness::start();
    let c = h.client();
    ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap();
    ask(
        &c,
        Request::PowerKeepAwake {
            spec: AwakeSpec {
                scope: AwakeScope::System,
                until: AwakeUntil::For { minutes: 5 },
            },
        },
    )
    .unwrap();
    assert_eq!(
        h.os.log(),
        [
            "acquire#1 System keeping awake until you stop it, you asked",
            "acquire#2 System keeping awake for 5 min, you asked",
            "release#1"
        ]
    );
}

#[test]
fn an_invalid_request_is_refused_before_the_os() {
    let h = Harness::start();
    let c = h.client();
    let spec = AwakeSpec {
        scope: AwakeScope::System,
        until: AwakeUntil::For { minutes: 0 },
    };
    let e = ask(&c, Request::PowerKeepAwake { spec }).unwrap_err();
    assert!(e.contains("1 minute"), "{e}");
    assert!(h.os.log().is_empty());
}

#[test]
fn the_os_ending_the_hold_is_shown_and_it_is_taken_again_on_wake() {
    let h = Harness::start();
    let (_w, pushed) = h.watcher();
    let c = h.client();
    ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap();
    assert!(matches!(next_view(&pushed).awake, AwakeView::Holding { .. }));
    // The notification arrives once the power thread subscribed; the fake reports whether anyone listens.
    while !h.events.emit(PowerEvent::Sleeping) {
        std::thread::yield_now();
    }
    let AwakeView::Waiting { why, .. } = next_view(&pushed).awake else {
        panic!("not waiting")
    };
    assert_eq!(why, "the computer went to sleep");
    assert_eq!(h.os.live(), 0, "released while asleep");
    assert!(h.events.emit(PowerEvent::Woke));
    assert!(matches!(next_view(&pushed).awake, AwakeView::Holding { os_ref, .. } if os_ref == "fake#2"));
    assert_eq!(h.os.live(), 1);
}

#[test]
fn a_refusal_ends_it_with_a_reason_and_the_capability_says_why() {
    let h = Harness::start();
    let c = h.client();
    h.os.fail_with(PowerError::Unavailable {
        reason: "this Linux has no logind".into(),
    });
    let v = power(ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap());
    let AwakeView::Ended { why, .. } = v.awake else {
        panic!("not ended")
    };
    assert!(why.contains("this Linux has no logind"), "{why}");
    assert!(matches!(v.caps.keep_awake, Capability::CantTell { reason } if reason.contains("logind")));
    assert!(!h.marker().exists());
    h.os.succeed();
    let v = power(ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap());
    assert!(matches!(v.awake, AwakeView::Holding { .. }));
    assert_eq!(v.caps.keep_awake, Capability::Known);
}

#[test]
fn turning_the_feature_off_refuses_starts_and_ends_a_hold() {
    let h = Harness::start();
    let c = h.client();
    ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap();
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    s.power.keep_awake.enabled = false;
    ask(&c, Request::SaveSettings { settings: s }).unwrap();
    assert_eq!(h.os.live(), 0);
    assert!(matches!(power(ask(&c, Request::PowerState).unwrap()).awake, AwakeView::Off));
    let e = ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap_err();
    assert!(e.contains("turned off in Settings"), "{e}");
}

#[test]
fn a_marker_from_an_agent_that_did_not_stop_cleanly_is_shown_and_the_hold_is_not_taken_again() {
    let h = Harness::start_with(|dirs| std::fs::write(dirs.root().join("keep-awake.active"), b"1").unwrap());
    let c = h.client();
    let v = power(ask(&c, Request::PowerState).unwrap());
    assert!(matches!(v.awake, AwakeView::Ended { ref why, .. } if why == "Dyno restarted"));
    assert!(h.os.log().is_empty(), "not taken again");
    assert!(!h.marker().exists(), "the marker is consumed");
    // "Turn on again" is a normal start.
    assert!(matches!(
        power(ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap()).awake,
        AwakeView::Holding { .. }
    ));
}

#[test]
fn a_clean_stop_of_the_agent_releases_the_hold_and_removes_the_marker() {
    let mut h = Harness::start();
    let c = h.client();
    ask(&c, Request::PowerKeepAwake { spec: indefinite() }).unwrap();
    let marker = h.marker();
    h.running.take().unwrap().stop();
    assert_eq!(h.os.live(), 0);
    assert!(!marker.exists());
}

#[test]
fn a_program_is_followed_by_pid_and_start_time_the_agent_adds_the_start_time() {
    let h = Harness::start();
    let c = h.client();
    // The fake machine's processes (pids 100..102) are in the table after the sampler's first round; wait on that condition.
    let spec = |pid| AwakeSpec {
        scope: AwakeScope::System,
        until: AwakeUntil::WhileProcess {
            process: dyno_core::process::ProcessRef { pid, started: None },
        },
    };
    let began = std::time::Instant::now();
    let view = loop {
        match ask(&c, Request::PowerKeepAwake { spec: spec(100) }) {
            Ok(r) => break power(r),
            Err(e) => {
                assert!(e.contains("not running"), "{e}");
                assert!(began.elapsed() < HANG_GUARD, "the process table never filled");
                std::thread::yield_now();
            }
        }
    };
    let AwakeView::Holding { spec, .. } = view.awake else {
        panic!("not holding")
    };
    let AwakeUntil::WhileProcess { process } = spec.until else {
        panic!("not a program hold")
    };
    assert_eq!(process.pid, 100);
    assert!(process.started.is_some(), "the agent filled the start time in");
    let e = ask(
        &c,
        Request::PowerKeepAwake {
            spec: AwakeSpec {
                scope: AwakeScope::System,
                until: AwakeUntil::WhileProcess {
                    process: dyno_core::process::ProcessRef {
                        pid: 999_999,
                        started: None,
                    },
                },
            },
        },
    )
    .unwrap_err();
    assert!(e.contains("not running"), "{e}");
}

fn energy(c: &BodyPeer<Message>, days: u32) -> dyno_core::power::energy::EnergyReport {
    match ask(c, Request::EnergyReport { days }).unwrap() {
        Reply::Energy(r) => *r,
        other => panic!("expected an energy report, got {other:?}"),
    }
}

#[test]
fn the_energy_report_starts_with_the_agents_own_start_and_prices_only_the_numbers_given() {
    let h = Harness::start();
    let c = h.client();
    let began = std::time::Instant::now();
    let report = loop {
        let r = energy(&c, 7);
        if r.tracking_since.is_some() {
            break r;
        }
        assert!(began.elapsed() < HANG_GUARD, "the agent never noted its start");
        std::thread::yield_now();
    };
    assert_eq!(report.days.len(), 7);
    assert_eq!(report.source, dyno_core::power::energy::EnergySource::NotSet);
    assert!(report.totals.kwh.is_none());
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    s.power.energy.watts_on = Some(60.0);
    s.power.energy.tariff_per_kwh = Some(0.3);
    ask(&c, Request::SaveSettings { settings: s }).unwrap();
    let priced = energy(&c, 1);
    assert_eq!(priced.source, dyno_core::power::energy::EnergySource::Estimated);
    assert!(priced.totals.kwh.is_some() && priced.totals.cost.is_some() && priced.totals.co2_kg.is_none());
}

#[test]
fn nothing_is_noted_when_the_energy_log_is_off() {
    let h = Harness::start();
    let c = h.client();
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    s.power.energy.enabled = false;
    ask(&c, Request::SaveSettings { settings: s }).unwrap();
    let r = energy(&c, 1);
    assert!(!r.enabled);
    // An OS event while it is off adds nothing; the report asks the store, so a note would show as "tracking since".
    h.events.emit(PowerEvent::Sleeping);
    h.events.emit(PowerEvent::Woke);
    let r2 = energy(&c, 1);
    assert_eq!(r.tracking_since, r2.tracking_since);
}

// ---- idle and timed actions (D5): fakes only, the clock is moved by the test, nothing sleeps or shuts down a real machine ----

use dyno_core::power::idle::{BlockerKind, Cause, IdleAction, IdleSettings, IdleView, Repeat, TimedAction};

impl Harness {
    fn shared(&self) -> Arc<dyno_agent::agent::Shared> {
        self.running.as_ref().unwrap().shared.clone()
    }

    /// Waits until the sampler measured the (fake) machine, so an exception that reads it is not "cannot tell".
    fn wait_sampled(&self) {
        let shared = self.shared();
        let began = std::time::Instant::now();
        loop {
            {
                let core = dyno_agent::agent::lock(&shared.core);
                if !core.recent.is_empty() && !core.latest.processes.is_empty() {
                    return;
                }
            }
            assert!(began.elapsed() < HANG_GUARD, "the sampler never measured the fake machine");
            std::thread::yield_now();
        }
    }

    /// One reading round of the idle rule, `skew_ms` after the last one on this service's own clock.
    fn tick(&self, skew_ms: i64) -> IdleView {
        let shared = self.shared();
        shared.power.skew_clock(skew_ms);
        shared.power.idle_tick(&shared);
        shared.power.view().idle
    }

    fn idle_on(&self, f: impl FnOnce(&mut IdleSettings)) {
        let c = self.client();
        let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
            panic!()
        };
        s.power.idle.enabled = true;
        s.power.idle.after_secs = 60;
        s.power.idle.countdown_secs = 30;
        f(&mut s.power.idle);
        ask(&c, Request::SaveSettings { settings: s }).unwrap();
        self.wait_sampled();
    }
}

#[test]
fn idle_and_timed_actions_are_off_by_default_and_never_act_on_a_quiet_machine() {
    let h = Harness::start();
    h.wait_sampled();
    h.fake.with(|s| s.idle_secs = Some(100_000));
    assert_eq!(h.tick(60_000), IdleView::Disabled);
    assert!(h.actions.requests().is_empty());
}

#[test]
fn an_idle_computer_gets_a_countdown_and_is_asked_once_at_zero_after_a_fresh_read() {
    let h = Harness::start();
    let (_w, pushed) = h.watcher();
    h.idle_on(|_| {});
    h.fake.with(|s| s.idle_secs = Some(120));
    let IdleView::Countdown { action, cause, .. } = h.tick(1_000) else {
        panic!("no countdown")
    };
    assert_eq!((action, cause), (IdleAction::Sleep, Cause::Idle));
    let began = std::time::Instant::now();
    while !matches!(next_view(&pushed).idle, IdleView::Countdown { .. }) {
        assert!(began.elapsed() < HANG_GUARD, "the countdown was never pushed to the app");
    }
    assert!(h.actions.requests().is_empty(), "nothing happens before zero");
    assert!(matches!(h.tick(10_000), IdleView::Countdown { .. }));
    assert!(matches!(h.tick(25_000), IdleView::Acting { .. }), "zero: re-read, all pass, asked");
    assert!(matches!(h.tick(5_000), IdleView::Acting { .. }));
    assert_eq!(h.actions.requests(), [IdleAction::Sleep], "one attempt, the normal way");
}

#[test]
fn activity_during_the_countdown_cancels_it() {
    let h = Harness::start();
    h.idle_on(|_| {});
    h.fake.with(|s| s.idle_secs = Some(120));
    assert!(matches!(h.tick(1_000), IdleView::Countdown { .. }));
    h.fake.with(|s| s.idle_secs = Some(2));
    assert!(matches!(h.tick(40_000), IdleView::Watching { .. }));
    assert!(h.actions.requests().is_empty());
}

#[test]
fn an_exception_that_holds_or_cannot_be_read_blocks_and_says_which() {
    let h = Harness::start();
    h.idle_on(|_| {});
    h.fake.with(|s| s.idle_secs = Some(500));
    h.signals.set(|r| r.audio = dyno_core::metrics::Reading::Available(true));
    let IdleView::Watching { blockers, .. } = h.tick(1_000) else {
        panic!("not watching")
    };
    assert_eq!(blockers.iter().map(|b| b.kind).collect::<Vec<_>>(), [BlockerKind::Audio]);
    h.signals.set(|r| {
        r.audio = dyno_core::metrics::Reading::Available(false);
        r.stay_awake = dyno_core::metrics::Reading::Unavailable("Windows only lets an administrator list that".into());
    });
    let IdleView::Watching { blockers, .. } = h.tick(1_000) else {
        panic!()
    };
    assert!(
        blockers[0].cant_tell && blockers[0].reason.contains("administrator"),
        "{blockers:?}"
    );
    h.signals
        .set(|r| r.stay_awake = dyno_core::metrics::Reading::Available(vec!["Zoom".into()]));
    let IdleView::Watching { blockers, .. } = h.tick(1_000) else {
        panic!()
    };
    assert!(blockers[0].reason.contains("Zoom"));
    assert!(h.actions.requests().is_empty());
    // The person can switch an exception off; then it no longer blocks.
    h.idle_on(|i| i.exceptions.other_stay_awake = false);
    assert!(matches!(h.tick(1_000), IdleView::Countdown { .. }));
}

#[test]
fn cancel_and_snooze_stop_the_action_and_a_failed_request_is_not_retried() {
    let h = Harness::start();
    let c = h.client();
    h.idle_on(|_| {});
    h.fake.with(|s| s.idle_secs = Some(120));
    h.tick(1_000);
    let v = power(ask(&c, Request::PowerIdleCancel).unwrap());
    assert!(matches!(v.idle, IdleView::Watching { .. }));
    assert!(
        matches!(h.tick(60_000), IdleView::Watching { .. }),
        "still idle, but not re-armed without new activity"
    );
    assert!(h.actions.requests().is_empty());
    h.fake.with(|s| s.idle_secs = Some(1));
    h.tick(1_000);
    h.fake.with(|s| s.idle_secs = Some(200));
    assert!(matches!(h.tick(1_000), IdleView::Countdown { .. }), "new activity re-armed it");
    let v = power(ask(&c, Request::PowerIdleSnooze { minutes: 30 }).unwrap());
    let IdleView::Watching { snoozed_until, .. } = v.idle else {
        panic!()
    };
    assert!(snoozed_until.is_some());
    assert!(matches!(h.tick(60_000), IdleView::Watching { .. }));
    // After the snooze a refusing system is asked once and the rule stops until there is new activity.
    h.actions
        .fail_with(dyno_core::ports::PlatformError::AccessDenied("polkit refused".into()));
    h.tick(31 * 60_000);
    h.tick(40_000);
    let IdleView::Stopped { why } = h.shared().power.view().idle else {
        panic!("not stopped")
    };
    assert!(why.contains("polkit refused"), "{why}");
    assert_eq!(h.actions.requests().len(), 1);
    h.tick(120_000);
    assert_eq!(h.actions.requests().len(), 1, "no retry in the same idle period");
}

#[test]
fn an_action_the_system_cannot_do_is_shown_in_the_capabilities() {
    let h = Harness::start();
    h.actions
        .make_unavailable(IdleAction::Hibernate, "macOS decides by itself when to hibernate");
    let v = power(ask(&h.client(), Request::PowerState).unwrap());
    assert_eq!(v.caps.sleep, Capability::Known);
    assert!(matches!(v.caps.hibernate, Capability::CantTell { ref reason } if reason.contains("by itself")));
    assert_eq!(v.caps.shut_down, Capability::Known);
}

#[test]
fn a_timed_action_warns_then_asks_once_and_a_once_entry_is_done() {
    let h = Harness::start();
    h.wait_sampled();
    let c = h.client();
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    let minute = dyno_core::time::local_minute(dyno_agent::agent::now(), &dyno_core::time::FixedZone(0));
    s.power.timed.enabled = true;
    s.power.timed.entries = vec![TimedAction {
        id: 7,
        action: IdleAction::Hibernate,
        at_minute: minute,
        repeat: Repeat::Once,
        last_day: None,
        done: false,
    }];
    ask(&c, Request::SaveSettings { settings: s }).unwrap();
    h.fake.with(|s| s.idle_secs = Some(1));
    let IdleView::Countdown { action, cause, .. } = h.tick(1_000) else {
        panic!("no countdown")
    };
    assert_eq!((action, cause), (IdleAction::Hibernate, Cause::Timed));
    assert!(h.actions.requests().is_empty());
    assert!(
        matches!(h.tick(10_000), IdleView::Countdown { .. }),
        "using the computer does not cancel a timed action"
    );
    assert!(
        matches!(h.tick(61_000), IdleView::Acting { .. }),
        "the countdown is the idle rule's 60 s"
    );
    assert_eq!(h.actions.requests(), [IdleAction::Hibernate]);
    let Reply::Settings(after) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    assert!(after.power.timed.entries[0].done, "a once entry does not run again");
    // A window's older copy of the settings does not bring it back.
    let mut stale = (*after).clone();
    stale.power.timed.entries[0].done = false;
    stale.power.timed.entries[0].last_day = None;
    ask(&c, Request::SaveSettings { settings: Box::new(stale) }).unwrap();
    let Reply::Settings(kept) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    assert!(kept.power.timed.entries[0].done);
}

#[test]
fn snoozing_a_timed_countdown_delays_the_action_and_a_once_entry_still_runs() {
    let h = Harness::start();
    h.wait_sampled();
    let c = h.client();
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    let minute = dyno_core::time::local_minute(dyno_agent::agent::now(), &dyno_core::time::FixedZone(0));
    s.power.timed.enabled = true;
    s.power.timed.entries = vec![TimedAction {
        id: 7,
        action: IdleAction::Hibernate,
        at_minute: minute,
        repeat: Repeat::Once,
        last_day: None,
        done: false,
    }];
    ask(&c, Request::SaveSettings { settings: s }).unwrap();
    h.fake.with(|s| s.idle_secs = Some(1));
    assert!(matches!(h.tick(1_000), IdleView::Countdown { cause: Cause::Timed, .. }));
    let v = power(ask(&c, Request::PowerIdleSnooze { minutes: 30 }).unwrap());
    assert!(!matches!(v.idle, IdleView::Countdown { .. }), "snooze hides the countdown");
    h.tick(10 * 60_000);
    assert!(h.actions.requests().is_empty(), "snoozed: nothing is asked");
    // The snooze is over: the countdown shows again, and only after it does the action run.
    let back = h.tick(21 * 60_000);
    assert!(
        matches!(
            back,
            IdleView::Countdown {
                action: IdleAction::Hibernate,
                cause: Cause::Timed,
                ..
            }
        ),
        "a snoozed timed action comes back after the snooze, got {back:?}"
    );
    assert!(h.actions.requests().is_empty());
    assert!(matches!(h.tick(61_000), IdleView::Acting { .. }));
    assert_eq!(h.actions.requests(), [IdleAction::Hibernate]);
    let Reply::Settings(after) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    assert!(after.power.timed.entries[0].done);
}

#[test]
fn cancelling_a_snoozed_timed_action_ends_it_for_today() {
    let h = Harness::start();
    h.wait_sampled();
    let c = h.client();
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    let minute = dyno_core::time::local_minute(dyno_agent::agent::now(), &dyno_core::time::FixedZone(0));
    s.power.timed.enabled = true;
    s.power.timed.entries = vec![TimedAction {
        id: 7,
        action: IdleAction::Hibernate,
        at_minute: minute,
        repeat: Repeat::Daily,
        last_day: None,
        done: false,
    }];
    ask(&c, Request::SaveSettings { settings: s }).unwrap();
    h.fake.with(|s| s.idle_secs = Some(1));
    assert!(matches!(h.tick(1_000), IdleView::Countdown { .. }));
    ask(&c, Request::PowerIdleSnooze { minutes: 5 }).unwrap();
    ask(&c, Request::PowerIdleCancel).unwrap();
    assert!(!matches!(h.tick(10 * 60_000), IdleView::Countdown { .. }), "cancelled for today");
    assert!(h.actions.requests().is_empty());
}

#[test]
fn invalid_timed_settings_are_refused_with_what_to_fix() {
    let h = Harness::start();
    let c = h.client();
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    s.power.timed.entries = vec![TimedAction {
        id: 1,
        action: IdleAction::Sleep,
        at_minute: 1440,
        repeat: Repeat::Daily,
        last_day: None,
        done: false,
    }];
    let e = ask(&c, Request::SaveSettings { settings: s }).unwrap_err();
    assert!(e.contains("00:00"), "{e}");
}

#[test]
fn the_idle_view_never_reports_a_rule_that_is_switched_off_even_while_timed_actions_are_on() {
    let h = Harness::start();
    h.wait_sampled();
    let c = h.client();
    let Reply::Settings(mut s) = ask(&c, Request::Settings).unwrap() else {
        panic!()
    };
    // A timed action that is already done keeps the timed rules on without ever firing; the idle rule stays off.
    let minute = (dyno_core::time::local_minute(dyno_agent::agent::now(), &dyno_core::time::FixedZone(0)) + 720) % 1440;
    s.power.idle.enabled = false;
    s.power.timed.enabled = true;
    s.power.timed.entries = vec![TimedAction {
        id: 1,
        action: IdleAction::Sleep,
        at_minute: minute,
        repeat: Repeat::Weekdays(0b111_1111),
        last_day: None,
        done: true,
    }];
    ask(&c, Request::SaveSettings { settings: s }).unwrap();
    h.fake.with(|s| s.idle_secs = Some(100_000));
    h.signals.set(|r| r.audio = dyno_core::metrics::Reading::Available(true));
    for ms in [1_000, 120_000] {
        assert_eq!(h.tick(ms), IdleView::Disabled, "the idle rule is off, so the card says so");
    }
    assert!(h.actions.requests().is_empty());
    assert_eq!(power(ask(&c, Request::PowerState).unwrap()).idle, IdleView::Disabled);
}
