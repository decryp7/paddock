//! Coming back after "Quit and stop measuring" over a macOS simulation (Dyno blocker B4 and the 2026-11-05 Quit rules):
//! `SMAppService` and launchd are fakes behind the ports, so the calls the OS would see are the calls recorded here.
//! Nothing touches the real launchd or Login Items.

use crate::ServiceError;
use crate::backend::{DisabledMark, LaunchdBundled};
use crate::manager::{LIGHT_LOOKS, Manager};
use crate::plan::{Hold, Reason};
use crate::port::{AppService, AppServiceStatus, Output, RegisterError, RunError};
use crate::record::RecordFile;
use crate::repair::Timings;
use crate::spec::ServiceSpec;
use crate::stop::{StopMechanism, StopTimings, Stopped, stop};
use crate::testing::{Call, FakeSystem};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LABEL: &str = "net.decryptology.dyno.agent";
const TARGET: &str = "gui/501/net.decryptology.dyno.agent";
const SPAWN_FAILED: &str = "gui/501/net.decryptology.dyno.agent = {\n\tstate = spawn scheduled\n\tlast exit code = 78: EX_CONFIG\n\tjob state = spawn failed\n\tproperties = partial import | keepalive | runatload | has LWCR\n}\n";
const RUNNING: &str = "gui/501/net.decryptology.dyno.agent = {\n\tstate = running\n\tresource coalition = {\n\t\tstate = active\n\t}\n\tjob state = running\n\tproperties = keepalive | runatload | has LWCR\n}\n";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Held {
    Nothing,
    Running,
    Refused,
}

struct World {
    status: AppServiceStatus,
    mark: DisabledMark,
    job: Held,
    /// A `register` brings the job after this many `launchctl print` asks (`None`: it never comes).
    job_after_asks: Option<u32>,
    asks_since_register: u32,
    register_error: Option<RegisterError>,
    /// `bootout` leaves the job in place this many `print` asks.
    lingers: u32,
    /// launchd refuses the job whatever is registered (the stale constraint survives).
    refuses_always: bool,
    booted_out: bool,
    calls: Vec<String>,
}

type Sim = Arc<Mutex<World>>;

fn world(status: AppServiceStatus, mark: DisabledMark, job: Held) -> Sim {
    Arc::new(Mutex::new(World {
        status,
        mark,
        job,
        job_after_asks: Some(0),
        asks_since_register: 0,
        register_error: None,
        lingers: 0,
        refuses_always: false,
        booted_out: false,
        calls: Vec::new(),
    }))
}

struct SimApp(Sim);

impl AppService for SimApp {
    fn status(&mut self) -> AppServiceStatus {
        self.0.lock().unwrap().status
    }
    fn register(&mut self) -> Result<(), RegisterError> {
        let mut w = self.0.lock().unwrap();
        w.calls.push("register".into());
        if let Some(e) = w.register_error.clone() {
            if e == RegisterError::NeedsApproval {
                w.status = AppServiceStatus::RequiresApproval;
            }
            return Err(e);
        }
        w.status = AppServiceStatus::Enabled;
        w.asks_since_register = 0;
        // A refused job (stale constraint) stays refused until it was booted out and registered again.
        if w.job != Held::Refused || w.booted_out {
            w.job = Held::Nothing;
            w.booted_out = false;
        }
        Ok(())
    }
    fn unregister(&mut self) -> Result<(), String> {
        let mut w = self.0.lock().unwrap();
        w.calls.push("unregister".into());
        w.status = AppServiceStatus::NotRegistered;
        Ok(())
    }
    fn open_login_items(&mut self) {
        self.0.lock().unwrap().calls.push("openLoginItems".into());
    }
}

fn wire(sim: &Sim, sys: &FakeSystem) {
    let s = sim.clone();
    sys.on("/bin/launchctl print gui", move |_, _| {
        let mut w = s.lock().unwrap();
        w.asks_since_register += 1;
        if w.job == Held::Nothing
            && w.status == AppServiceStatus::Enabled
            && w.job_after_asks.is_some_and(|n| w.asks_since_register > n)
        {
            w.job = Held::Running;
        }
        if w.refuses_always {
            return Ok(Output::ok(SPAWN_FAILED));
        }
        if w.booted_out && w.lingers > 0 {
            w.lingers -= 1;
            return Ok(Output::ok(RUNNING));
        }
        Ok(match w.job {
            Held::Nothing => Output {
                code: Some(113),
                stdout: String::new(),
                stderr: "Could not find service".into(),
            },
            Held::Running => Output::ok(RUNNING),
            Held::Refused => Output::ok(SPAWN_FAILED),
        })
    });
    let s = sim.clone();
    sys.on("/bin/launchctl print-disabled", move |_, _| {
        let w = s.lock().unwrap();
        match w.mark {
            DisabledMark::Disabled => Ok(Output::ok(&format!(
                "disabled services = {{\n\t\"{LABEL}\" => disabled\n}}\n"
            ))),
            DisabledMark::Enabled => Ok(Output::ok(&format!(
                "disabled services = {{\n\t\"{LABEL}\" => enabled\n}}\n"
            ))),
            DisabledMark::NotListed => Ok(Output::ok(
                "disabled services = {\n\t\"other\" => enabled\n}\n",
            )),
            DisabledMark::Unreadable => Err(RunError::TimedOut("/bin/launchctl".into())),
        }
    });
    let s = sim.clone();
    sys.on("/bin/launchctl bootout", move |_, _| {
        let mut w = s.lock().unwrap();
        w.calls.push("bootout".into());
        w.booted_out = true;
        if w.lingers == 0 {
            w.job = Held::Nothing;
        }
        Ok(Output::code(0))
    });
}

fn spec() -> ServiceSpec {
    ServiceSpec::builder(LABEL, "Dyno", "dyno-agent")
        .build()
        .unwrap()
}

fn manager(sim: &Sim, sys: &FakeSystem, dir: &Path, version: &str) -> Manager {
    wire(sim, sys);
    let backend = LaunchdBundled::new(
        spec(),
        501,
        Box::new(SimApp(sim.clone())),
        Box::new(sys.clone()),
    );
    // Like Dyno's: the service answers when launchd runs the job.
    let running = sim.clone();
    Manager::new("Dyno", Box::new(backend), dir, LABEL, version, move || {
        running.lock().unwrap().job == Held::Running
    })
    .with_timings(Timings {
        start_wait: Duration::from_secs(6),
        ..Timings::default()
    })
}

fn recorded(dir: &Path, version: &str) {
    RecordFile::new(dir, LABEL).save(version, None).unwrap();
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

fn resume(m: &mut Manager) -> (Result<bool, ServiceError>, Vec<String>) {
    let mut said = Vec::new();
    let r = m.resume(&never(), &mut |s| said.push(s.to_string()));
    (r, said)
}

fn calls(sim: &Sim) -> Vec<String> {
    sim.lock().unwrap().calls.clone()
}

fn no_changes(sys: &FakeSystem) -> bool {
    sys.calls().iter().all(|c| match c {
        Call::Run(s) => !s.contains("bootout") && !s.contains("bootstrap"),
        _ => true,
    })
}

#[test]
fn a_resume_calls_nothing_for_an_item_switched_off_unreadable_or_missing() {
    use AppServiceStatus as S;
    use DisabledMark as D;
    for (status, mark) in [
        (S::Enabled, D::Disabled),
        (S::Enabled, D::Unreadable),
        (S::NotRegistered, D::Disabled),
        (S::RequiresApproval, D::NotListed),
        (S::NotFound, D::Enabled),
    ] {
        let dir = tempfile::tempdir().unwrap();
        recorded(dir.path(), "1.0.0");
        let sim = world(status, mark, Held::Nothing);
        let sys = FakeSystem::new();
        let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
        let (r, said) = resume(&mut m);
        assert!(!r.unwrap(), "{status:?} {mark:?}");
        assert!(
            calls(&sim).is_empty(),
            "{status:?} {mark:?}: {:?}",
            calls(&sim)
        );
        assert!(no_changes(&sys));
        assert!(
            said.iter().any(|s| s.contains("stays as it is")),
            "{said:?}"
        );
    }
}

#[test]
fn the_light_path_registers_the_enabled_item_once_and_waits_for_the_job_without_removing_anything()
{
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    sim.lock().unwrap().job_after_asks = Some(4);
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let (r, said) = resume(&mut m);
    assert!(r.unwrap());
    // Registering an already registered item is what brings the job back; unregister or bootout would make macOS
    // announce a new background item ("Background Items Added") again.
    assert_eq!(calls(&sim), ["register"]);
    assert!(
        said.iter().any(|s| s.contains("holds the job again")),
        "{said:?}"
    );
    assert!(sys.now() > Duration::ZERO, "it waited between looks");
}

#[test]
fn a_version_change_goes_to_the_full_repair_and_never_the_light_path() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "0.9.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::Enabled,
        Held::Nothing,
    );
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let (r, _) = resume(&mut m);
    assert!(r.unwrap());
    assert_eq!(
        calls(&sim),
        ["unregister", "bootout", "register"],
        "the first change is the removal, not a light register"
    );
    assert_eq!(m.recorded().registered_version.as_deref(), Some("1.0.0"));
}

#[test]
fn a_failed_light_path_falls_back_to_the_repair_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    {
        let mut w = sim.lock().unwrap();
        w.register_error = Some(RegisterError::Other("denied".into()));
        w.job_after_asks = None;
    }
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let (r, said) = resume(&mut m);
    // Every register fails, so the repair cannot bring it back either: the error is the repair's.
    assert!(matches!(r, Err(ServiceError::Failed(_))), "{r:?} {said:?}");
    assert_eq!(calls(&sim)[0], "register");
    assert!(
        calls(&sim).contains(&"unregister".to_string()),
        "{:?}",
        calls(&sim)
    );
    assert!(
        said.iter()
            .any(|s| s.contains("failed (denied)") && s.contains("full repair")),
        "{said:?}"
    );
}

#[test]
fn a_light_path_that_loads_no_job_is_bounded_and_falls_back_to_the_repair() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    sim.lock().unwrap().job_after_asks = None;
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let (_, said) = resume(&mut m);
    assert!(said.iter().any(|s| s.contains("loaded no job")), "{said:?}");
    let c = calls(&sim);
    assert_eq!(c[0], "register");
    assert_eq!(&c[1..4], ["unregister", "bootout", "register"], "{c:?}");
    let prints = sys
        .commands()
        .iter()
        .filter(|c| c.contains("launchctl print gui"))
        .count();
    assert!(
        prints >= LIGHT_LOOKS as usize,
        "the light path looked {LIGHT_LOOKS} times first: {prints}"
    );
}

#[test]
fn the_background_resume_never_opens_login_items_even_when_the_repair_meets_an_approval() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    sim.lock().unwrap().register_error = Some(RegisterError::NeedsApproval);
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let (r, _) = resume(&mut m);
    assert!(
        !r.unwrap(),
        "waiting for the user's approval is theirs to give"
    );
    assert!(
        !calls(&sim).contains(&"openLoginItems".to_string()),
        "{:?}",
        calls(&sim)
    );
}

#[test]
fn only_a_job_the_os_holds_can_be_stopped_and_a_stop_is_judged_by_reading_the_os_back() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    sim.lock().unwrap().job_after_asks = None;
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    assert_eq!(
        m.stop_mechanism(),
        None,
        "nothing to stop when launchd holds no job"
    );
    sim.lock().unwrap().job = Held::Running;
    let mechanism = m.stop_mechanism().unwrap();
    assert_eq!(
        mechanism,
        StopMechanism::Launchd {
            target: TARGET.into()
        }
    );
    // bootout reports success but launchd keeps the job: not stopped.
    sim.lock().unwrap().lingers = u32::MAX;
    let mut s = sys.clone();
    let timings = StopTimings {
        gone_wait: Duration::from_secs(2),
        poll: Duration::from_millis(500),
    };
    assert!(stop(&mut s, &mechanism, timings).is_err());
    // launchd drops it: stopped, and the registration was never touched.
    sim.lock().unwrap().lingers = 0;
    assert_eq!(stop(&mut s, &mechanism, timings).unwrap(), Stopped::Stopped);
    assert!(!calls(&sim).iter().any(|c| c == "unregister"));
    // And coming back is the light path: one register, nothing removed.
    sim.lock().unwrap().job_after_asks = Some(0);
    let before = calls(&sim).len();
    let (r, _) = resume(&mut m);
    assert!(r.unwrap());
    assert_eq!(&calls(&sim)[before..], ["register"]);
}

#[test]
fn supervision_needs_an_enabled_item_that_is_not_switched_off_and_a_job_launchd_holds() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        (
            AppServiceStatus::Enabled,
            DisabledMark::NotListed,
            Held::Running,
            true,
        ),
        (
            AppServiceStatus::Enabled,
            DisabledMark::Disabled,
            Held::Running,
            false,
        ),
        (
            AppServiceStatus::RequiresApproval,
            DisabledMark::NotListed,
            Held::Running,
            false,
        ),
        (
            AppServiceStatus::Enabled,
            DisabledMark::NotListed,
            Held::Nothing,
            false,
        ),
    ];
    for (status, mark, job, want) in cases {
        let sim = world(status, mark, job);
        sim.lock().unwrap().job_after_asks = None;
        let sys = FakeSystem::new();
        let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
        assert_eq!(m.supervises(), want, "{status:?} {mark:?} {job:?}");
    }
}

#[test]
fn an_update_replaces_the_registration_once_and_a_bundled_downgrade_takes_it_over() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "0.9.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Running,
    );
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let started = m.on_start(&never());
    assert!(
        matches!(
            started,
            crate::manager::Started::Repaired(Reason::VersionChanged, _)
        ),
        "{started:?}"
    );
    sim.lock().unwrap().calls.clear();
    assert_eq!(
        m.on_start(&never()),
        crate::manager::Started::Left(Hold::Healthy)
    );
    assert!(calls(&sim).is_empty());
    // A newer record with no other copy's program known (the bundled registration belongs to whichever copy runs) does not pin it.
    recorded(dir.path(), "2.0.0");
    assert!(matches!(
        m.on_start(&never()),
        crate::manager::Started::Repaired(Reason::VersionChanged, _)
    ));
}

#[test]
fn a_registration_launchd_keeps_refusing_fails_with_what_launchd_said_not_a_guess() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "0.9.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::Enabled,
        Held::Refused,
    );
    // The refusal survives every bootout (the job lingers) and every register.
    {
        let mut w = sim.lock().unwrap();
        w.refuses_always = true;
        w.job_after_asks = None;
    }
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let (r, _) = resume(&mut m);
    let Err(ServiceError::Failed(why)) = r else {
        panic!("{r:?}")
    };
    assert!(
        why.contains("observed:") && why.contains("refuses to start it"),
        "{why}"
    );
    assert!(why.contains("Login Items"), "the next step is named: {why}");
    assert_eq!(
        m.recorded().registered_version.as_deref(),
        Some("0.9.0"),
        "a failed repair keeps the old version"
    );
}

// ---- review round: scenarios the first tests did not contain

use crate::manager::Started;

fn timed_out_prints(sys: &FakeSystem) {
    sys.on("/bin/launchctl print gui", |_, _| {
        Err(RunError::TimedOut("/bin/launchctl".into()))
    });
}

#[test]
fn an_unreadable_job_never_installs_unasked_or_re_enables_a_switched_off_item() {
    // (a) not registered, `launchctl print` times out, no record; (b) the switch list and the job both time out, older record.
    for (mark, record) in [
        (DisabledMark::NotListed, None),
        (DisabledMark::Unreadable, Some("1.0.0")),
    ] {
        let dir = tempfile::tempdir().unwrap();
        if let Some(v) = record {
            recorded(dir.path(), v);
        }
        let sim = world(AppServiceStatus::NotRegistered, mark, Held::Nothing);
        let sys = FakeSystem::new();
        let mut m = manager(&sim, &sys, dir.path(), "1.1.0");
        timed_out_prints(&sys);
        assert_eq!(
            m.on_start(&never()),
            Started::Left(Hold::NotInstalled),
            "{mark:?}"
        );
        assert!(calls(&sim).is_empty(), "{mark:?}: {:?}", calls(&sim));
        assert!(no_changes(&sys));
    }
}

#[test]
fn a_job_with_no_record_is_left_alone_not_removed_and_not_registered_again() {
    // (c) macOS has no record (the app runs from a disk image), a job runs, an older version registered it.
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::NotFound,
        DisabledMark::NotListed,
        Held::Running,
    );
    sim.lock().unwrap().register_error =
        Some(RegisterError::Unavailable("open from Applications".into()));
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.1.0");
    assert!(matches!(
        m.on_start(&never()),
        Started::Left(Hold::Unavailable(_))
    ));
    assert!(calls(&sim).is_empty(), "{:?}", calls(&sim));
    assert!(no_changes(&sys));
    assert_eq!(
        sim.lock().unwrap().job,
        Held::Running,
        "the running agent is still there"
    );
}

#[test]
fn a_refused_job_is_repaired_after_a_downgrade_or_an_unverifiable_record() {
    for recorded_version in ["1.2.0", "999.0.0"] {
        let dir = tempfile::tempdir().unwrap();
        recorded(dir.path(), recorded_version);
        let sim = world(
            AppServiceStatus::Enabled,
            DisabledMark::NotListed,
            Held::Refused,
        );
        let sys = FakeSystem::new();
        let mut m = manager(&sim, &sys, dir.path(), "1.1.0");
        let started = m.on_start(&never());
        assert!(
            matches!(started, Started::Repaired(Reason::Refused, _)),
            "{recorded_version}: {started:?}"
        );
        assert_eq!(
            &calls(&sim)[..2],
            ["unregister", "bootout"],
            "{recorded_version}"
        );
    }
}

#[test]
fn a_registration_that_never_answers_is_not_remembered_so_the_next_start_looks_again() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Running,
    );
    sim.lock().unwrap().job_after_asks = None;
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.1.0");
    let first = m.on_start(&never());
    assert!(
        matches!(
            first,
            Started::Repaired(Reason::VersionChanged, RepairOutcome::Registered)
        ),
        "{first:?}"
    );
    assert_eq!(
        m.recorded().registered_version.as_deref(),
        Some("1.0.0"),
        "a dead agent is not remembered as done"
    );
    sim.lock().unwrap().calls.clear();
    let second = m.on_start(&never());
    assert!(
        matches!(second, Started::Repaired(Reason::VersionChanged, _)),
        "{second:?}"
    );
    assert!(!calls(&sim).is_empty(), "the second start repairs again");
    // Once it answers (the job runs), the version is remembered and the third start leaves it alone.
    sim.lock().unwrap().job_after_asks = Some(0);
    assert_eq!(
        m.on_start(&never()),
        Started::Marked,
        "up now: settled without a removal"
    );
    assert_eq!(m.recorded().registered_version.as_deref(), Some("1.1.0"));
    assert_eq!(m.on_start(&never()), Started::Left(Hold::Healthy));
}

use crate::repair::RepairOutcome;

#[test]
fn a_resume_whose_repair_registers_but_never_starts_is_an_error_not_a_job() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "0.9.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::Enabled,
        Held::Nothing,
    );
    sim.lock().unwrap().job_after_asks = None;
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.0.0");
    let (r, said) = resume(&mut m);
    assert!(
        matches!(r, Err(ServiceError::Failed(ref why)) if why.contains("did not start")),
        "{r:?}"
    );
    assert!(
        said.iter().any(|s| s.contains("did not start in time")),
        "{said:?}"
    );
}

#[test]
fn registering_from_the_settings_switch_does_not_wait_for_the_service_to_answer() {
    let elapsed = |without: bool| {
        let dir = tempfile::tempdir().unwrap();
        let sim = world(
            AppServiceStatus::NotRegistered,
            DisabledMark::NotListed,
            Held::Nothing,
        );
        sim.lock().unwrap().job_after_asks = None;
        let sys = FakeSystem::new();
        let timings = Timings {
            start_wait: Duration::from_secs(120),
            ..Timings::default()
        };
        let timings = if without {
            timings.without_start_wait()
        } else {
            timings
        };
        wire(&sim, &sys);
        let backend = LaunchdBundled::new(
            spec(),
            501,
            Box::new(SimApp(sim.clone())),
            Box::new(sys.clone()),
        );
        let mut m = Manager::new(
            "Dyno",
            Box::new(backend),
            dir.path(),
            LABEL,
            "1.0.0",
            || false,
        )
        .with_timings(timings);
        assert!(m.install(&never()).is_ok());
        sys.now()
    };
    assert!(elapsed(false) >= Duration::from_secs(120));
    assert!(
        elapsed(true) < Duration::from_secs(10),
        "{:?}",
        elapsed(true)
    );
}

#[test]
fn a_busy_start_looks_again_a_bounded_number_of_times() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Running,
    );
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.1.0");
    let held = RecordFile::new(dir.path(), LABEL)
        .lock()
        .unwrap();
    let started = m.on_start_patient(&never(), 3, Duration::from_secs(5));
    assert_eq!(started, Started::Busy);
    assert!(sys.now() >= Duration::from_secs(15), "{:?}", sys.now());
    drop(held);
    assert!(matches!(
        m.on_start_patient(&never(), 3, Duration::from_secs(5)),
        Started::Repaired(..)
    ));
}

struct LagApp(Sim);

impl AppService for LagApp {
    fn status(&mut self) -> AppServiceStatus {
        self.0.lock().unwrap().status
    }
    fn register(&mut self) -> Result<(), RegisterError> {
        // Succeeds and loads the job, but the status read right after still says what it said a moment ago.
        let mut w = self.0.lock().unwrap();
        w.calls.push("register".into());
        w.job = Held::Running;
        w.status = if w.calls.iter().filter(|c| *c == "register").count() == 1 {
            AppServiceStatus::NotRegistered
        } else {
            AppServiceStatus::Enabled
        };
        Ok(())
    }
    fn unregister(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().calls.push("unregister".into());
        Ok(())
    }
    fn open_login_items(&mut self) {}
}

#[test]
fn a_status_that_lags_behind_the_register_looks_for_the_job_before_replacing_anything() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.1.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    let sys = FakeSystem::new();
    wire(&sim, &sys);
    let backend = LaunchdBundled::new(
        spec(),
        501,
        Box::new(LagApp(sim.clone())),
        Box::new(sys.clone()),
    );
    let mut m = Manager::new(
        "Dyno",
        Box::new(backend),
        dir.path(),
        LABEL,
        "1.1.0",
        || false,
    );
    let (r, _) = resume(&mut m);
    assert!(r.unwrap());
    assert_eq!(
        calls(&sim),
        ["register"],
        "one register, nothing removed, no second banner"
    );
}

#[test]
fn an_unattended_install_never_opens_login_items_and_the_asked_one_does() {
    let make = |dir: &Path| {
        let sim = world(
            AppServiceStatus::NotRegistered,
            DisabledMark::Disabled,
            Held::Nothing,
        );
        let sys = FakeSystem::new();
        let m = manager(&sim, &sys, dir, "1.0.0");
        (sim, m)
    };
    let dir = tempfile::tempdir().unwrap();
    let (sim, mut m) = make(dir.path());
    assert!(matches!(
        m.install_unattended(&never()),
        Err(ServiceError::SwitchedOff)
    ));
    assert!(calls(&sim).is_empty(), "{:?}", calls(&sim));
    let (sim, mut m) = make(dir.path());
    assert!(matches!(
        m.install(&never()),
        Err(ServiceError::SwitchedOff)
    ));
    assert_eq!(calls(&sim), ["openLoginItems"]);
}

// ---- second review round: a registration the OS accepted is not replaced again while it is only slow

fn count(sim: &Sim, what: &str) -> usize {
    calls(sim).iter().filter(|c| *c == what).count()
}

fn unanswered(dir: &Path, version: &str) -> (Sim, FakeSystem, Manager) {
    let sim = world(
        AppServiceStatus::NotRegistered,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    let sys = FakeSystem::new();
    let m = manager(&sim, &sys, dir, version);
    (sim, sys, m)
}

#[test]
fn the_settings_switch_then_the_next_start_registers_once() {
    let dir = tempfile::tempdir().unwrap();
    let sim = world(
        AppServiceStatus::NotRegistered,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    sim.lock().unwrap().job_after_asks = Some(2);
    let sys = FakeSystem::new();
    // The switch: returns at once (nothing answers yet) ...
    wire(&sim, &sys);
    let backend = LaunchdBundled::new(
        spec(),
        501,
        Box::new(SimApp(sim.clone())),
        Box::new(sys.clone()),
    );
    let running = sim.clone();
    let mut m = Manager::new(
        "Dyno",
        Box::new(backend),
        dir.path(),
        LABEL,
        "1.1.0",
        move || running.lock().unwrap().job == Held::Running,
    )
    .with_timings(Timings::default().without_start_wait());
    assert!(m.install(&never()).is_ok());
    assert_eq!(calls(&sim), ["unregister", "bootout", "register"]);
    // ... and the next start, with launchd's agent running, removes nothing and records the version.
    let mut next = manager(&sim, &sys, dir.path(), "1.1.0");
    assert_eq!(next.on_start(&never()), Started::Marked);
    assert_eq!(count(&sim, "register"), 1, "{:?}", calls(&sim));
    assert_eq!(count(&sim, "unregister"), 1);
    assert_eq!(next.recorded().registered_version.as_deref(), Some("1.1.0"));
}

#[test]
fn a_first_run_then_the_start_repair_waits_for_a_slow_job_and_registers_once() {
    let dir = tempfile::tempdir().unwrap();
    let (sim, _sys, mut m) = unanswered(dir.path(), "1.1.0");
    sim.lock().unwrap().job_after_asks = Some(2);
    let first = m.install_unattended(&never());
    assert!(
        matches!(
            first,
            Ok(RepairOutcome::Recovered | RepairOutcome::Registered)
        ),
        "{first:?}"
    );
    let before = count(&sim, "register");
    assert_eq!(before, 1);
    let next = m.on_start_patient(&never(), 3, Duration::from_secs(5));
    assert!(
        matches!(next, Started::Marked | Started::Left(Hold::Healthy)),
        "settled without a second registration: {next:?}"
    );
    assert_eq!(
        count(&sim, "register"),
        1,
        "no second register: {:?}",
        calls(&sim)
    );
    assert_eq!(count(&sim, "unregister"), 1);
}

#[test]
fn a_registration_that_is_slow_but_healthy_settles_without_any_removal() {
    let dir = tempfile::tempdir().unwrap();
    let (sim, sys, mut m) = unanswered(dir.path(), "1.1.0");
    RecordFile::new(dir.path(), LABEL)
        .set_state(crate::record::State::Attempting {
            version: "1.1.0".into(),
            attempts: 1,
            phase: crate::record::Phase::Awaiting,
        })
        .unwrap();
    {
        let mut w = sim.lock().unwrap();
        w.status = AppServiceStatus::Enabled;
        w.job_after_asks = Some(2);
        w.asks_since_register = 0;
    }
    assert_eq!(m.on_start(&never()), Started::Marked);
    assert!(calls(&sim).is_empty(), "{:?}", calls(&sim));
    assert!(no_changes(&sys));
    assert!(sys.now() > Duration::ZERO, "it waited, it did not replace");
}

#[test]
fn a_registration_that_never_comes_up_is_tried_a_few_times_per_version_and_then_held() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Nothing,
    );
    sim.lock().unwrap().job_after_asks = None;
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.1.0");
    for start in 1..=3 {
        let started = m.on_start(&never());
        assert!(
            matches!(
                started,
                Started::Repaired(Reason::VersionChanged, RepairOutcome::Registered)
            ),
            "{start}: {started:?}"
        );
    }
    assert_eq!(count(&sim, "register"), 3);
    assert_eq!(m.on_start(&never()), Started::Left(Hold::Unanswered(3)));
    assert_eq!(m.on_start(&never()), Started::Left(Hold::Unanswered(3)));
    assert_eq!(
        count(&sim, "register"),
        3,
        "held: nothing more is registered"
    );
    // A newer version starts again.
    let mut next = manager(&sim, &sys, dir.path(), "1.2.0");
    assert!(matches!(
        next.on_start(&never()),
        Started::Repaired(Reason::VersionChanged, _)
    ));
}

#[test]
fn a_refused_job_is_still_replaced_when_this_version_already_tried() {
    let dir = tempfile::tempdir().unwrap();
    recorded(dir.path(), "1.0.0");
    RecordFile::new(dir.path(), LABEL)
        .set_state(crate::record::State::Attempting {
            version: "1.1.0".into(),
            attempts: 1,
            phase: crate::record::Phase::Awaiting,
        })
        .unwrap();
    let sim = world(
        AppServiceStatus::Enabled,
        DisabledMark::NotListed,
        Held::Refused,
    );
    let sys = FakeSystem::new();
    let mut m = manager(&sim, &sys, dir.path(), "1.1.0");
    assert!(matches!(
        m.on_start(&never()),
        Started::Repaired(Reason::Refused, _)
    ));
}

#[test]
fn a_first_run_that_loses_the_lock_waits_and_then_registers() {
    let dir = tempfile::tempdir().unwrap();
    let (sim, sys, mut m) = unanswered(dir.path(), "1.1.0");
    let held = RecordFile::new(dir.path(), LABEL)
        .lock()
        .unwrap();
    let r = m.install_patient(&never(), true, 3, Duration::from_secs(5));
    assert!(matches!(r, Err(ServiceError::Busy)));
    assert!(sys.now() >= Duration::from_secs(15), "{:?}", sys.now());
    assert!(calls(&sim).is_empty());
    drop(held);
    assert!(
        m.install_patient(&never(), true, 3, Duration::from_secs(5))
            .is_ok()
    );
}
