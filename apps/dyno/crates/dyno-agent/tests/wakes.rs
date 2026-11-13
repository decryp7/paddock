#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Scheduled wake end to end over the fake platform (ER-14): a real IPC connection and the agent's own state, with `FakeWakeTimer`
//! standing in for the OS wake (it records, and sets nothing), `FakeUdp` for the network and `FakePower` for the hold. Time is passed
//! to `tick` as a value, so no test waits for a real minute and no wake is ever set on the machine it runs on.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::power::awake::AwakeView;
use dyno_core::power::schedule::{DynoJob, Repeat, RowState, WakeInput, WakeRoute, WakeRouteState, WakesView};
use dyno_core::protocol::Message;
use dyno_core::protocol::{Reply, Request};
use dyno_core::time::Timestamp;
use dyno_core::wake::{WakeScheduler, WakeTag};
use dyno_core::wol::{SecureOnEdit, WolDeviceInput};
use dyno_platform::fake::Fake;
use dyno_platform::wol::FakeWakeTimer;
use dyno_platform::wol::WolPorts;
use paddock_ipc::BodyPeer;
use std::sync::Arc;

struct Harness {
    running: Option<Running>,
    timer: Arc<FakeWakeTimer>,
    udp: Arc<dyno_core::wol::fake::FakeUdp>,
    dirs: Dirs,
    _tmp: paddock_testkit::PrivateTemp,
}

impl Harness {
    fn start() -> Harness {
        Harness::start_with_timer(|t| t)
    }

    /// A harness whose OS wake timer is `wrap` of the recording one (the recording one stays what `timer` lists).
    fn start_with_timer(wrap: impl FnOnce(Arc<FakeWakeTimer>) -> Arc<dyn WakeScheduler>) -> Harness {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        std::fs::create_dir_all(dirs.root()).unwrap();
        let (timer, udp) = (
            Arc::new(FakeWakeTimer::default()),
            Arc::new(dyno_core::wol::fake::FakeUdp::default()),
        );
        let fake = Fake::new();
        let mut platform = fake.platform();
        let base = WolPorts::fake();
        platform.wol = WolPorts {
            wake_timer: Some(wrap(timer.clone())),
            udp: udp.clone(),
            ..base
        };
        let running = Running::start(dirs.clone(), platform, Box::new(fake), Box::new(|_| Ok(()))).unwrap();
        Harness {
            running: Some(running),
            timer,
            udp,
            dirs,
            _tmp: tmp,
        }
    }

    fn client(&self) -> BodyPeer<Message> {
        connect(&self.dirs).unwrap().0
    }

    fn shared(&self) -> Arc<dyno_agent::agent::Shared> {
        self.running.as_ref().unwrap().shared.clone()
    }

    fn allow(&self, c: &BodyPeer<Message>, on: bool) {
        let Reply::Settings(s) = ask(c, Request::Settings).unwrap() else {
            panic!("settings")
        };
        let mut s = *s;
        s.power.wake.enabled = on;
        ask(c, Request::SaveSettings { settings: Box::new(s) }).unwrap();
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn input(name: &str) -> WakeInput {
    WakeInput {
        id: None,
        name: name.into(),
        minute_of_day: 7 * 60,
        repeat: Repeat::Daily,
        stay_awake_minutes: Some(30),
        wake_devices: vec![],
        job: None,
        enabled: true,
    }
}

fn view(r: Reply) -> WakesView {
    match r {
        Reply::PowerWakes(v) => *v,
        other => panic!("expected the schedule, got {other:?}"),
    }
}

fn set(c: &BodyPeer<Message>, i: WakeInput) -> Result<WakesView, String> {
    ask(c, Request::PowerSetWake { entry: i }).map(view)
}

#[test]
fn it_is_refused_until_the_person_turns_it_on_and_says_who_sets_the_wake() {
    let h = Harness::start();
    let c = h.client();
    let e = set(&c, input("Morning")).unwrap_err();
    assert!(e.contains("turned off in Settings"), "{e}");
    assert!(h.timer.list().unwrap().is_empty());
    let v = view(ask(&c, Request::PowerWakes).unwrap());
    let WakeRouteState::Agent { note, cant_tell } = v.route else {
        panic!("the agent sets wakes on this fake platform")
    };
    assert!(note.starts_with("Windows wakes"), "{note}");
    assert!(cant_tell.iter().any(|l| l.starts_with("Allow wake timers: Dyno can't tell")));
    assert!(cant_tell.iter().any(|l| l.starts_with("Modern Standby: Dyno can't tell")));
    assert!(v.rows.is_empty());
}

#[test]
fn a_schedule_is_armed_the_os_wake_is_set_through_the_route_and_cancel_clears_it() {
    let h = Harness::start();
    let c = h.client();
    h.allow(&c, true);
    let v = set(&c, input("Morning")).unwrap();
    assert_eq!(v.rows.len(), 1);
    let row = &v.rows[0];
    assert!(matches!(row.state, RowState::Armed { route: WakeRoute::Agent }), "{:?}", row.state);
    let at = row.entry.next_at.expect("armed for its next occurrence");
    let pending = h.timer.list().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!((pending[0].tag.as_str(), pending[0].at_utc), ("dyno-1", at.0 / 1000));
    // Changing it moves the same OS wake (the same tag); cancelling clears it.
    let mut change = input("Morning");
    change.id = Some(1);
    change.minute_of_day = 8 * 60;
    set(&c, change).unwrap();
    assert_eq!(h.timer.list().unwrap().len(), 1);
    let v = view(ask(&c, Request::PowerCancelWake { id: 1 }).unwrap());
    assert!(v.rows.is_empty());
    assert!(h.timer.list().unwrap().is_empty());
}

/// An OS wake timer whose `list` waits at a gate when the sync thread of the test calls it, so a test can hold that sync between reading
/// the saved schedules and listing the OS wakes.
struct GatedTimer {
    inner: Arc<FakeWakeTimer>,
    reached: std::sync::mpsc::Sender<()>,
    open: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl WakeScheduler for GatedTimer {
    fn schedule(&self, tag: &WakeTag, at_utc: i64) -> Result<(), dyno_core::ports::PlatformError> {
        self.inner.schedule(tag, at_utc)
    }
    fn cancel(&self, tag: &WakeTag) -> Result<bool, dyno_core::ports::PlatformError> {
        self.inner.cancel(tag)
    }
    fn list(&self) -> Result<Vec<dyno_core::wake::WakeEntry>, dyno_core::ports::PlatformError> {
        if std::thread::current().name() == Some("gated-sync") {
            let _ = self.reached.send(());
            let _ = self.open.lock().unwrap().recv();
        }
        self.inner.list()
    }
}

/// The agent's wake thread syncs every OS wake from the saved schedules when it starts (a settings change does the same) and cancels
/// the wakes no schedule owns. That sync read the schedules, then listed the OS wakes: a change saved in between lost its wake, because
/// the sync still thought nothing owned it. `a_schedule_is_armed...` failed once beside a build with an empty list after a change.
#[test]
fn a_sync_held_between_reading_and_listing_never_cancels_a_wake_set_meanwhile() {
    let (reached_tx, reached_rx) = std::sync::mpsc::channel();
    let (open_tx, open_rx) = std::sync::mpsc::channel();
    let h = Harness::start_with_timer(|inner| {
        Arc::new(GatedTimer {
            inner,
            reached: reached_tx,
            open: std::sync::Mutex::new(open_rx),
        })
    });
    let c = h.client();
    h.allow(&c, true);
    // A sync that has read the (empty) schedules and waits just before it lists the OS wakes.
    let shared = h.shared();
    let syncer = std::thread::Builder::new()
        .name("gated-sync".into())
        .spawn(move || shared.wakes.sync_all(&shared))
        .unwrap();
    reached_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the sync reaches its list of OS wakes");
    // A schedule is added meanwhile. Done at once without the fix; it waits for the sync with it.
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let adder = std::thread::spawn(move || {
        set(&c, input("Morning")).unwrap();
        let _ = done_tx.send(());
    });
    let _finished_before_the_sync = done_rx.recv_timeout(std::time::Duration::from_secs(3)).is_ok();
    open_tx.send(()).unwrap();
    syncer.join().unwrap();
    adder.join().unwrap();
    assert_eq!(
        h.timer.list().unwrap().len(),
        1,
        "the sync cancelled the OS wake of a schedule saved while it ran"
    );
}

#[test]
fn every_open_window_is_told_when_the_schedules_change() {
    use paddock_ipc::BodyOptions;
    let h = Harness::start();
    let c = h.client();
    h.allow(&c, true);
    let d = dyno_agent::discovery::Discovery::read(&h.dirs.discovery()).unwrap();
    let conn = dyno_agent::wire::connect(&d.endpoint, &d.token().unwrap()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let watcher = BodyPeer::start(
        conn,
        BodyOptions::new("the agent").on_push(move |m| {
            if let Message::WakesChanged(v) = m {
                let _ = tx.send(*v);
            }
        }),
    )
    .unwrap();
    ask(
        &watcher,
        Request::Subscribe {
            detail: dyno_core::protocol::LiveDetail::Summary,
        },
    )
    .unwrap();
    set(&c, input("Morning")).unwrap();
    let pushed = rx.recv_timeout(std::time::Duration::from_secs(30)).expect("the change is pushed");
    assert_eq!(pushed.rows.len(), 1);
    ask(&c, Request::PowerCancelWake { id: 1 }).unwrap();
    assert!(rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the cancel is pushed")
        .rows
        .is_empty());
    // A schedule that ran is pushed too.
    let v = set(&c, input("Evening")).unwrap();
    let _ = rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let shared = h.shared();
    shared.wakes.tick(&shared, v.rows[0].entry.next_at.unwrap().plus(1000));
    let ran = rx.recv_timeout(std::time::Duration::from_secs(30)).expect("a run is pushed");
    assert!(ran.rows[0].entry.last_run.is_some());
}

#[test]
fn bad_input_says_what_to_fix_and_sets_nothing() {
    let h = Harness::start();
    let c = h.client();
    h.allow(&c, true);
    let mut bad = input("Morning");
    bad.stay_awake_minutes = None;
    assert!(set(&c, bad).unwrap_err().contains("what happens"));
    let mut late = input("Morning");
    late.minute_of_day = 1440;
    assert!(set(&c, late).unwrap_err().contains("time of day"));
    let mut gone = input("Morning");
    gone.wake_devices = vec![42];
    assert!(set(&c, gone).unwrap_err().contains("no longer"));
    assert!(h.timer.list().unwrap().is_empty());
}

#[test]
fn an_os_refusal_is_shown_on_the_row_and_switching_the_setting_off_clears_every_wake() {
    let h = Harness::start();
    let c = h.client();
    h.allow(&c, true);
    set(&c, input("Morning")).unwrap();
    *h.timer.refuse.lock().unwrap() = Some("wake timers are off in the power plan".into());
    let v = set(&c, input("Evening")).unwrap();
    let evening = v.rows.iter().find(|r| r.entry.name == "Evening").unwrap();
    assert!(
        matches!(&evening.state, RowState::Failed { why } if why.contains("wake timers are off")),
        "{:?}",
        evening.state
    );
    *h.timer.refuse.lock().unwrap() = None;
    h.allow(&c, false);
    assert!(h.timer.list().unwrap().is_empty(), "off clears the OS wakes");
    // Saved schedules stay, shown as paused (never as armed with a next wake), until it is turned on again.
    let v = view(ask(&c, Request::PowerWakes).unwrap());
    assert!(
        v.rows.iter().all(|r| matches!(r.state, RowState::Paused)),
        "{:?}",
        v.rows.iter().map(|r| r.state.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn a_due_schedule_acts_stays_awake_wakes_the_computers_runs_its_job_and_arms_the_next() {
    let h = Harness::start();
    let c = h.client();
    h.allow(&c, true);
    ask(
        &c,
        Request::WolSave {
            device: WolDeviceInput {
                id: None,
                name: "NAS".into(),
                mac: "02:00:5e:10:00:01".into(),
                last_ip: None,
                interface: None,
                secure_on: SecureOnEdit::Keep,
            },
        },
    )
    .unwrap();
    let mut i = input("Morning");
    i.wake_devices = vec![1];
    i.job = Some(DynoJob::SoftwareInventory);
    let v = set(&c, i).unwrap();
    let due = v.rows[0].entry.next_at.unwrap();
    let shared = h.shared();
    shared.wakes.tick(&shared, due.minus(1000));
    assert!(h.udp.sent().is_empty(), "nothing happens before its time");
    shared.wakes.tick(&shared, due.plus(1000));
    assert!(matches!(shared.power.view().awake, AwakeView::Holding { .. }), "it stays awake");
    // The wake job runs on the pool; wait for its datagrams (a condition, not a sleep).
    let t = std::time::Instant::now();
    while h.udp.sent().is_empty() && t.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::yield_now();
    }
    assert!(!h.udp.sent().is_empty(), "the computer is woken");
    let v = view(ask(&c, Request::PowerWakes).unwrap());
    let e = &v.rows[0].entry;
    let run = e.last_run.as_ref().expect("how it went is kept");
    assert!(
        run.summary.contains("staying awake 30 min")
            && run.summary.contains("started the wake of 1 computer")
            && run.summary.contains("started its job"),
        "{}",
        run.summary
    );
    assert_eq!(e.next_at.unwrap().since(due), 24 * 3_600_000, "armed for the next day");
}

#[test]
fn a_wake_that_passed_while_nothing_ran_is_missed_never_run_late_and_a_one_time_wake_switches_off() {
    let h = Harness::start();
    let c = h.client();
    h.allow(&c, true);
    let mut once = input("Once");
    once.repeat = Repeat::Once;
    let v = set(&c, once).unwrap();
    let due: Timestamp = v.rows[0].entry.next_at.unwrap();
    let shared = h.shared();
    shared.wakes.tick(&shared, due.plus(3 * 3_600_000));
    assert!(matches!(shared.power.view().awake, AwakeView::Off), "a missed wake does not act");
    let v = view(ask(&c, Request::PowerWakes).unwrap());
    let e = &v.rows[0].entry;
    assert!(e.last_run.as_ref().unwrap().summary.starts_with("Missed"));
    assert!(!e.enabled && e.next_at.is_none());
    assert!(matches!(v.rows[0].state, RowState::Off));
    assert!(h.timer.list().unwrap().is_empty());
    let _ = WakeTag::parse("dyno-1").unwrap();
}
