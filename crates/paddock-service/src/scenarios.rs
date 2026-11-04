//! End-to-end scenarios over the fakes: the failures that reached users (R03), every failure path the plan names, and
//! the exact commands and files each mechanism writes. Nothing here touches the OS: launchd is a small simulation
//! behind the [`System`] and [`AppService`] ports, time is virtual.

use crate::ServiceError;
use crate::backend::{
    ElevatedWindowsService, LaunchdBundled, LaunchdFile, Systemd, Target, WindowsRun,
    WindowsService,
};
use crate::manager::{Manager, Started};
use crate::plan::{Hold, Reason};
use crate::port::{AppService, AppServiceStatus, Output, RegisterError, RunError};
use crate::repair::{RepairOutcome, Timings};
use crate::spec::{Kind, ServiceSpec};
use crate::status::Condition;
use crate::testing::{Call, FakeAppService, FakeElevator, FakeSystem};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LABEL: &str = "net.decryptology.pitwall.service";
const TARGET: &str = "gui/501/net.decryptology.pitwall.service";

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn pitwall() -> ServiceSpec {
    ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .bundle_id("net.decryptology.pitwall")
        .supervised_env("PITWALL_SERVICE")
        .build()
        .unwrap()
}

// ------------------------------------------------------------------------------------------------ launchd simulation

/// launchd and `SMAppService` as R03 showed them: a job keeps the launch constraint (the cdhash) of the build that
/// registered it; registering over a live job changes nothing; only bootout, then register, gives a job for the
/// current build. Like macOS 26 in the C21 VM smoke, `SMAppService.status` of an app that never registered is
/// `NotFound` (Background Task Management has no record yet); after `unregister` it is `NotRegistered`.
#[derive(Debug)]
struct Launchd {
    /// The build that is installed now.
    build: u32,
    /// `SMAppService` says registered.
    registered: bool,
    /// Background Task Management has a record of the app (it registered once).
    known: bool,
    /// The job and the build its constraint is for.
    job: Option<u32>,
    /// The user switched the item off in Login Items.
    switched_off: bool,
    /// `bootout` hangs (the helper times out) and does nothing.
    bootout_hangs: bool,
    /// After a bootout the job lingers this long (virtual time).
    linger: Duration,
    /// When the job was booted out.
    booted_out_at: Option<Duration>,
    /// `register` refuses with this.
    register_error: Option<RegisterError>,
}

impl Launchd {
    fn healthy(&self) -> bool {
        self.job == Some(self.build)
    }
}

type Sim = Arc<Mutex<Launchd>>;

fn launchd(build: u32, registered_by: Option<u32>) -> Sim {
    Arc::new(Mutex::new(Launchd {
        build,
        registered: registered_by.is_some(),
        known: registered_by.is_some(),
        job: registered_by,
        switched_off: false,
        bootout_hangs: false,
        linger: Duration::ZERO,
        booted_out_at: None,
        register_error: None,
    }))
}

const SPAWN_FAILED: &str = "gui/501/net.decryptology.pitwall.service = {\n\tstate = spawn scheduled\n\tlast exit code = 78: EX_CONFIG\n\tjob state = spawn failed\n\tproperties = partial import | keepalive | runatload | has LWCR\n}\n";
/// macOS 26's report of a running job: nested blocks (the coalitions) carry their own `state = active`.
const HEALTHY: &str = "gui/501/net.decryptology.pitwall.service = {\n\tstate = running\n\tresource coalition = {\n\t\tstate = active\n\t}\n\tjob state = running\n\tjetsam coalition = {\n\t\tstate = active\n\t}\n\tproperties = keepalive | runatload | has LWCR\n}\n";

fn wire(sim: &Sim, sys: &FakeSystem) {
    let s = sim.clone();
    sys.on("/bin/launchctl print gui", move |_, now| {
        let mut l = s.lock().unwrap();
        if let Some(at) = l.booted_out_at
            && now >= at + l.linger
        {
            l.job = None;
            l.booted_out_at = None;
        }
        Ok(match l.job {
            None => Output {
                code: Some(113),
                stdout: String::new(),
                stderr: "Could not find service".into(),
            },
            Some(b) if b == l.build => Output::ok(HEALTHY),
            Some(_) => Output::ok(SPAWN_FAILED),
        })
    });
    let s = sim.clone();
    sys.on("/bin/launchctl print-disabled", move |_, _| {
        let off = if s.lock().unwrap().switched_off {
            "disabled"
        } else {
            "enabled"
        };
        Ok(Output::ok(&format!(
            "disabled services = {{\n\t\"{LABEL}\" => {off}\n}}\n"
        )))
    });
    let s = sim.clone();
    sys.on("/bin/launchctl bootout", move |_, now| {
        let mut l = s.lock().unwrap();
        if l.bootout_hangs {
            return Err(RunError::TimedOut("/bin/launchctl".into()));
        }
        if l.job.is_none() {
            return Ok(Output::code(3));
        }
        l.booted_out_at = Some(now);
        Ok(Output::code(0))
    });
}

struct SimApp(Sim, FakeSystem);

impl AppService for SimApp {
    fn status(&mut self) -> AppServiceStatus {
        let l = self.0.lock().unwrap();
        match (l.registered, &l.register_error) {
            (_, Some(RegisterError::NeedsApproval)) => AppServiceStatus::RequiresApproval,
            (true, _) => AppServiceStatus::Enabled,
            (false, _) if !l.known => AppServiceStatus::NotFound,
            (false, _) => AppServiceStatus::NotRegistered,
        }
    }
    fn register(&mut self) -> Result<(), RegisterError> {
        self.1.record(Call::Os("register".into()));
        let mut l = self.0.lock().unwrap();
        if let Some(e) = l.register_error.clone() {
            return Err(e);
        }
        l.registered = true;
        l.known = true;
        if l.job.is_none() {
            l.job = Some(l.build);
        }
        Ok(())
    }
    fn unregister(&mut self) -> Result<(), String> {
        self.1.record(Call::Os("unregister".into()));
        self.0.lock().unwrap().registered = false;
        Ok(())
    }
    fn open_login_items(&mut self) {
        self.1.record(Call::Os("openLoginItems".into()));
    }
}

fn mac_manager(sim: &Sim, sys: &FakeSystem, dir: &Path, version: &str) -> Manager {
    wire(sim, sys);
    let backend = LaunchdBundled::new(
        pitwall(),
        501,
        Box::new(SimApp(sim.clone(), sys.clone())),
        Box::new(sys.clone()),
    );
    let s = sim.clone();
    Manager::new(
        "Pitwall",
        Box::new(backend),
        dir,
        LABEL,
        version,
        move || s.lock().unwrap().healthy(),
    )
}

/// The OS-changing calls, in order (reads and waits left out).
fn changes(sys: &FakeSystem) -> Vec<String> {
    sys.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Os(s) => Some(s),
            Call::Run(s) if s.contains(" bootout ") || s.contains("bootstrap") => Some(s),
            Call::Run(s)
                if s.starts_with("systemctl") && !s.contains("is-") && !s.contains(" show ") =>
            {
                Some(s)
            }
            Call::Run(s) if s.starts_with("reg add") || s.starts_with("reg delete") => Some(s),
            Call::Run(s)
                if s.starts_with("sc ")
                    && !s.starts_with("sc query")
                    && !s.starts_with("sc qc") =>
            {
                Some(s)
            }
            Call::Write(p, _) => Some(format!("write {}", p.display())),
            Call::Remove(p) => Some(format!("remove {}", p.display())),
            Call::Spawn(s) => Some(format!("spawn {s}")),
            _ => None,
        })
        .collect()
}

#[test]
fn r03_an_update_replaces_the_stale_registration_exactly_once_and_recovers() {
    let d = tempfile::tempdir().unwrap();
    // Build 1 (version 1.0.3) registered the agent; the app is now build 2 (1.0.4): launchd refuses it (EX_CONFIG).
    let sim = launchd(2, Some(1));
    sim.lock().unwrap().linger = Duration::from_secs(4);
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    crate::record::RecordFile::new(d.path(), LABEL)
        .save("1.0.3", None)
        .unwrap();
    assert_eq!(m.status().condition, Condition::Refused);
    let started = m.on_start(&no_cancel());
    assert_eq!(
        started,
        Started::Repaired(Reason::Refused, RepairOutcome::Recovered)
    );
    assert_eq!(
        changes(&sys),
        [
            "unregister",
            &format!("/bin/launchctl bootout {TARGET}"),
            "register"
        ]
    );
    assert!(
        sys.now() >= Duration::from_secs(4),
        "it waited until launchd dropped the job before registering"
    );
    assert!(sim.lock().unwrap().healthy());
    assert_eq!(m.recorded().registered_version.as_deref(), Some("1.0.4"));
    // Exactly once: the next start (and the one after) leaves it alone.
    sys.clear_calls();
    assert_eq!(m.on_start(&no_cancel()), Started::Left(Hold::Healthy));
    assert_eq!(m.on_start(&no_cancel()), Started::Left(Hold::Healthy));
    assert!(changes(&sys).is_empty());
    assert_eq!(m.status().condition, Condition::Running);
    let notice = m.tracker().current(std::time::Instant::now()).unwrap();
    assert_eq!(notice.state, crate::tracker::RepairState::Succeeded);
}

#[test]
fn rule_24_a_healthy_registration_is_still_replaced_once_after_an_update() {
    // macOS pins the build: the service still runs the old binary's registration, which fails at its next restart.
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(2));
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    crate::record::RecordFile::new(d.path(), LABEL)
        .save("1.0.3", None)
        .unwrap();
    assert_eq!(m.status().condition, Condition::Running);
    assert_eq!(
        m.on_start(&no_cancel()),
        Started::Repaired(Reason::VersionChanged, RepairOutcome::Recovered)
    );
    assert_eq!(
        changes(&sys),
        [
            "unregister",
            &format!("/bin/launchctl bootout {TARGET}"),
            "register"
        ]
    );
    sys.clear_calls();
    assert_eq!(m.on_start(&no_cancel()), Started::Left(Hold::Healthy));
    assert!(changes(&sys).is_empty(), "exactly once per version change");
}

#[test]
fn r03_registering_over_the_live_job_is_what_failed_and_it_is_never_done() {
    // Without the bootout the simulation stays refused: proof that it models R03 and that the sequence is needed.
    let sim = launchd(2, Some(1));
    let sys = FakeSystem::new();
    wire(&sim, &sys);
    let mut app = SimApp(sim.clone(), sys.clone());
    app.unregister().unwrap();
    app.register().unwrap();
    assert!(
        !sim.lock().unwrap().healthy(),
        "re-registering over a live job keeps the old constraint"
    );
}

#[test]
fn a_stale_registration_with_the_same_version_is_repaired_while_the_service_is_down() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    crate::record::RecordFile::new(d.path(), LABEL)
        .save("1.0.4", None)
        .unwrap();
    assert_eq!(
        m.on_start(&no_cancel()),
        Started::Repaired(Reason::Refused, RepairOutcome::Recovered)
    );
}

#[test]
fn a_bootout_that_hangs_is_bounded_and_ends_with_next_steps() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    sim.lock().unwrap().bootout_hangs = true;
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    let Started::Repaired(_, RepairOutcome::Failed(why)) = m.on_start(&no_cancel()) else {
        panic!()
    };
    assert!(
        why.contains("still refuses") && why.contains("Repair again"),
        "{why}"
    );
    let t = Timings::default();
    assert!(
        sys.now() <= t.worst_case(),
        "{:?} > {:?}",
        sys.now(),
        t.worst_case()
    );
    let registers = changes(&sys).iter().filter(|c| *c == "register").count();
    assert_eq!(registers, t.max_cycles as usize + 1);
    assert_eq!(
        m.recorded().registered_version,
        None,
        "a failed repair is tried again at the next start"
    );
    assert!(m.status().can_repair);
    // launchd lets go later: the next start repairs it.
    sim.lock().unwrap().bootout_hangs = false;
    assert_eq!(
        m.on_start(&no_cancel()),
        Started::Repaired(Reason::Refused, RepairOutcome::Recovered)
    );
}

#[test]
fn approval_is_the_users_and_the_repair_stops_at_it() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    sim.lock().unwrap().register_error = Some(RegisterError::NeedsApproval);
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    // The status already says RequiresApproval: no automatic repair.
    assert_eq!(m.on_start(&no_cancel()), Started::Left(Hold::NeedsApproval));
    assert!(matches!(
        m.repair(&no_cancel()),
        Err(ServiceError::NeedsApproval)
    ));
    assert!(changes(&sys).contains(&"openLoginItems".to_string()));
    assert!(!changes(&sys).contains(&"register".to_string()));
    assert_eq!(m.status().condition, Condition::NeedsApproval);
    assert!(m.status().can_open_login_items);
}

#[test]
fn register_refused_during_a_repair_points_at_login_items() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    let s = sim.clone();
    // macOS asks for approval only when the new registration is made.
    let app = SimApp(sim.clone(), sys.clone());
    drop(app);
    sys.on("/bin/launchctl bootout", move |_, _| {
        let mut l = s.lock().unwrap();
        l.job = None;
        l.register_error = Some(RegisterError::Other("busy".into()));
        Ok(Output::code(0))
    });
    let Started::Repaired(_, RepairOutcome::Failed(why)) = m.on_start(&no_cancel()) else {
        panic!()
    };
    assert!(why.contains("busy"), "{why}");
    sim.lock().unwrap().register_error = Some(RegisterError::NeedsApproval);
    sim.lock().unwrap().registered = true;
    assert!(matches!(
        m.repair(&no_cancel()),
        Err(ServiceError::NeedsApproval)
    ));
}

#[test]
fn what_the_user_switched_off_is_never_switched_back_on() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    sim.lock().unwrap().switched_off = true;
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    assert_eq!(m.on_start(&no_cancel()), Started::Left(Hold::SwitchedOff));
    assert!(matches!(
        m.repair(&no_cancel()),
        Err(ServiceError::SwitchedOff)
    ));
    assert_eq!(
        changes(&sys),
        ["openLoginItems"],
        "Login Items opened for the user, nothing registered"
    );
    let st = m.status();
    assert_eq!(
        (st.condition, st.can_repair, st.can_open_login_items),
        (Condition::SwitchedOff, false, true)
    );
}

#[test]
fn nothing_is_installed_unasked_and_install_registers_and_remembers() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(1, None);
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.0");
    // Regression (C21 VM smoke): on a fresh Mac SMAppService says NotFound until the first register; that is "not
    // installed" (Install offered and it registers), not "cannot be registered from here".
    assert_eq!(
        SimApp(sim.clone(), sys.clone()).status(),
        AppServiceStatus::NotFound
    );
    assert_eq!(m.on_start(&no_cancel()), Started::Left(Hold::NotInstalled));
    assert!(changes(&sys).is_empty());
    let st = m.status();
    assert_eq!(st.condition, Condition::NotInstalled);
    assert!(st.can_install, "{st:?}");
    assert_eq!(m.install(&no_cancel()).unwrap(), RepairOutcome::Recovered);
    assert_eq!(
        changes(&sys),
        [
            "unregister",
            &format!("/bin/launchctl bootout {TARGET}"),
            "register"
        ]
    );
    assert!(sim.lock().unwrap().healthy());
    assert_eq!(m.recorded().registered_version.as_deref(), Some("1.0.0"));
    m.uninstall().unwrap();
    assert_eq!(m.recorded().registered_version, None);
    assert!(!sim.lock().unwrap().registered);
}

#[test]
fn a_second_copy_repairing_at_the_same_time_is_told_to_wait() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4").with_clock(|| 1_000);
    let other = crate::record::RecordFile::new(d.path(), LABEL);
    let held = other.lock(1_000).unwrap();
    assert_eq!(m.on_start(&no_cancel()), Started::Busy);
    assert!(matches!(m.repair(&no_cancel()), Err(ServiceError::Busy)));
    assert!(matches!(m.uninstall(), Err(ServiceError::Busy)));
    assert!(changes(&sys).is_empty());
    drop(held);
    assert!(matches!(m.on_start(&no_cancel()), Started::Repaired(..)));
}

// -------------------------------------------------------------------------------------------------- launchd file

#[test]
fn a_plist_file_is_written_and_bootstrapped_and_a_daemon_needs_rights() {
    let sys = FakeSystem::new();
    let plist =
        PathBuf::from("/Users/ana/Library/LaunchAgents/net.decryptology.pitwall.service.plist");
    let target =
        Target::exe("/dev/pitwall-server").log("/Users/ana/Library/Logs/Pitwall/service.log");
    let mut b = LaunchdFile::new(pitwall(), 501, plist.clone(), target, Box::new(sys.clone()));
    use crate::backend::Backend;
    assert_eq!(b.inspect(), crate::backend::Inspection::absent());
    b.register().unwrap();
    assert_eq!(
        changes(&sys),
        [
            format!("write {}", plist.display()),
            format!("/bin/launchctl bootstrap gui/501 {}", plist.display())
        ]
    );
    let text = sys.file(&plist).unwrap();
    assert_eq!(Some(text.clone()), b.definition().into());
    assert!(text.contains("<string>/dev/pitwall-server</string>"));
    sys.put_program("/dev/pitwall-server");
    let i = b.inspect();
    assert_eq!(
        (i.points_here, i.program_exists, i.definition_current),
        (Some(true), Some(true), Some(true))
    );
    b.unregister().unwrap();
    b.unregister().unwrap();
    // A launch daemon: writing /Library/LaunchDaemons is refused without rights.
    let daemon = ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    sys.fail_writes(Some(std::io::ErrorKind::PermissionDenied));
    let mut d = LaunchdFile::new(
        daemon,
        0,
        "/Library/LaunchDaemons/x.plist".into(),
        Target::exe("/p"),
        Box::new(sys.clone()),
    );
    assert_eq!(d.register(), Err(RegisterError::NeedsElevation));
    sys.fail_writes(Some(std::io::ErrorKind::Other));
    assert!(matches!(d.register(), Err(RegisterError::Other(_))));
    sys.fail_writes(None);
    sys.answer(
        "/bin/launchctl bootstrap system",
        Output {
            code: Some(1),
            stdout: String::new(),
            stderr: "Bootstrap failed: 1: Operation not permitted".into(),
        },
    );
    assert_eq!(d.register(), Err(RegisterError::NeedsElevation));
    sys.answer(
        "/bin/launchctl bootstrap system",
        Output {
            code: Some(5),
            stdout: String::new(),
            stderr: "Bootstrap failed: 5: Input/output error".into(),
        },
    );
    assert!(matches!(d.register(), Err(RegisterError::Other(e)) if e.contains("Input/output")));
    sys.on("/bin/launchctl bootstrap system", |_, _| {
        Err(RunError::TimedOut("launchctl".into()))
    });
    assert!(matches!(d.register(), Err(RegisterError::Other(e)) if e.contains("in time")));
    sys.fail_removes(Some(std::io::ErrorKind::PermissionDenied));
    assert!(d.unregister().is_err());
}

#[test]
fn a_plist_file_that_points_at_a_moved_app_is_re_pointed() {
    let d = tempfile::tempdir().unwrap();
    let sys = FakeSystem::new();
    let plist =
        PathBuf::from("/Users/ana/Library/LaunchAgents/net.decryptology.pitwall.service.plist");
    let old = LaunchdFile::new(
        pitwall(),
        501,
        plist.clone(),
        Target::exe("/Old/pitwall-server"),
        Box::new(sys.clone()),
    );
    sys.put_file(&plist, &old.definition());
    sys.answer("/bin/launchctl print gui", Output::ok("state = waiting\n"));
    sys.put_program("/New/pitwall-server");
    let b = LaunchdFile::new(
        pitwall(),
        501,
        plist.clone(),
        Target::exe("/New/pitwall-server"),
        Box::new(sys.clone()),
    );
    let mut m = Manager::new("Pitwall", Box::new(b), d.path(), LABEL, "1.0.0", || true);
    crate::record::RecordFile::new(d.path(), LABEL)
        .save("1.0.0", None)
        .unwrap();
    assert_eq!(m.status().condition, Condition::Running);
    // The old copy was deleted: missing program.
    let Started::Repaired(reason, _) = m.on_start(&no_cancel()) else {
        panic!()
    };
    assert_eq!(reason, Reason::ProgramMissing);
    assert!(sys.file(&plist).unwrap().contains("/New/pitwall-server"));
}

#[test]
fn launchd_reports_that_cannot_be_read_are_unknown_not_healthy() {
    use crate::backend::{Backend, Job};
    let sys = FakeSystem::new();
    sys.on("/bin/launchctl print", |_, _| {
        Err(RunError::TimedOut("launchctl".into()))
    });
    let mut b = LaunchdBundled::new(
        pitwall(),
        501,
        Box::new(FakeAppService::new(&sys)),
        Box::new(sys.clone()),
    );
    let i = b.inspect();
    assert_eq!(i.job, Job::Unknown);
    assert!(!b.job_gone(), "an unanswered question is not 'gone'");
    sys.answer(
        "/bin/launchctl print gui",
        Output {
            code: Some(1),
            ..Output::default()
        },
    );
    assert_eq!(b.inspect().job, Job::Unknown);
    sys.answer("/bin/launchctl print gui", Output::ok("state = waiting\n"));
    assert_eq!(b.inspect().job, Job::NotRunning);
    sys.answer(
        "/bin/launchctl bootout",
        Output {
            code: Some(37),
            stderr: "Operation not permitted".into(),
            ..Output::default()
        },
    );
    assert!(b.evict().unwrap_err().contains("Operation not permitted"));
    let app = FakeAppService::new(&sys);
    app.set_status(AppServiceStatus::NotFound);
    let mut b = LaunchdBundled::new(pitwall(), 501, Box::new(app), Box::new(sys.clone()));
    // NotFound is what a never-registered app reads: absent, so Install registers (a real failure to register from
    // here comes back from `register` itself).
    assert_eq!(
        b.inspect().registration,
        crate::backend::Registration::Absent
    );
    assert!(b.open_login_items());
    assert!(
        b.capabilities().pinned_identity
            && b.mechanism() == crate::backend::Mechanism::LaunchdBundled
    );
}

// --------------------------------------------------------------------------------------------------------- systemd

fn unit_path() -> PathBuf {
    PathBuf::from("/home/ana/.config/systemd/user/pitwall.service")
}

fn systemd(sys: &FakeSystem, exe: &str) -> Systemd {
    Systemd::new(
        pitwall(),
        unit_path(),
        Target::exe(exe).working_dir("/home/ana/.local/share/pitwall"),
        Box::new(sys.clone()),
    )
}

#[test]
fn a_systemd_user_unit_is_written_reloaded_and_enabled() {
    let d = tempfile::tempdir().unwrap();
    let sys = FakeSystem::new();
    sys.put_program("/usr/bin/pitwall-server");
    sys.answer(
        "systemctl --user is-active",
        Output {
            code: Some(3),
            stdout: "inactive\n".into(),
            ..Output::default()
        },
    );
    let b = systemd(&sys, "/usr/bin/pitwall-server");
    let expected = b.definition();
    let mut m = Manager::new("Pitwall", Box::new(b), d.path(), LABEL, "1.0.0", || true);
    m.install(&no_cancel()).unwrap();
    assert_eq!(
        changes(&sys),
        [
            "systemctl --user disable --now pitwall.service".to_string(),
            format!("remove {}", unit_path().display()),
            "systemctl --user daemon-reload".to_string(),
            "systemctl --user stop pitwall.service".to_string(),
            format!("write {}", unit_path().display()),
            "systemctl --user daemon-reload".to_string(),
            "systemctl --user enable --now pitwall.service".to_string(),
        ]
    );
    assert_eq!(sys.file(&unit_path()).unwrap(), expected);
    assert_eq!(m.definition().as_deref(), Some(expected.as_str()));
    // An update with the same unit text: only remembered, no re-registration (the OS does not pin the build here).
    sys.clear_calls();
    sys.answer("systemctl --user is-active", Output::ok("active\n"));
    let mut m2 = Manager::new(
        "Pitwall",
        Box::new(systemd(&sys, "/usr/bin/pitwall-server")),
        d.path(),
        LABEL,
        "1.0.1",
        || true,
    );
    assert_eq!(m2.on_start(&no_cancel()), Started::Marked);
    assert!(changes(&sys).is_empty());
    assert_eq!(m2.recorded().registered_version.as_deref(), Some("1.0.1"));
}

#[test]
fn systemd_failure_paths_say_what_to_do() {
    use crate::backend::{Backend, Job, Registration};
    let sys = FakeSystem::new();
    let mut b = systemd(&sys, "/usr/bin/pitwall-server");
    // No user manager (ssh without a session).
    sys.answer(
        "systemctl --user daemon-reload",
        Output {
            code: Some(1),
            stderr: "Failed to connect to bus: No medium found".into(),
            ..Output::default()
        },
    );
    let Err(RegisterError::Unavailable(why)) = b.register() else {
        panic!()
    };
    assert!(why.contains("log in on the machine"), "{why}");
    sys.answer("systemctl --user daemon-reload", Output::code(0));
    sys.answer(
        "systemctl --user enable",
        Output {
            code: Some(1),
            stderr: "Unit pitwall.service has a bad setting.".into(),
            ..Output::default()
        },
    );
    assert!(matches!(b.register(), Err(RegisterError::Other(e)) if e.contains("bad setting")));
    sys.answer(
        "systemctl --user enable",
        Output {
            code: Some(1),
            stderr: "Interactive authentication required.".into(),
            ..Output::default()
        },
    );
    assert_eq!(b.register(), Err(RegisterError::NeedsElevation));
    sys.on("systemctl --user enable", |_, _| {
        Err(RunError::NotStarted("systemctl: not found".into()))
    });
    assert!(
        matches!(b.register(), Err(RegisterError::Unavailable(e)) if e.contains("is systemd installed"))
    );
    sys.fail_writes(Some(std::io::ErrorKind::PermissionDenied));
    assert_eq!(b.register(), Err(RegisterError::NeedsElevation));
    sys.fail_writes(Some(std::io::ErrorKind::Other));
    assert!(matches!(b.register(), Err(RegisterError::Other(_))));
    sys.fail_writes(None);
    // Inspect: unreachable manager, switched off, failed, unknown.
    sys.put_file(unit_path(), &b.definition());
    sys.answer(
        "systemctl --user is-active",
        Output {
            code: Some(1),
            stderr: "Failed to connect to bus".into(),
            ..Output::default()
        },
    );
    assert!(matches!(
        b.inspect().registration,
        Registration::Unavailable(_)
    ));
    sys.on("systemctl --user is-active", |_, _| {
        Err(RunError::TimedOut("systemctl".into()))
    });
    assert!(matches!(
        b.inspect().registration,
        Registration::Unavailable(_)
    ));
    assert!(!b.job_gone());
    sys.answer(
        "systemctl --user is-active",
        Output {
            code: Some(3),
            stdout: "failed\n".into(),
            ..Output::default()
        },
    );
    sys.answer(
        "systemctl --user is-enabled",
        Output {
            code: Some(1),
            stdout: "disabled\n".into(),
            ..Output::default()
        },
    );
    let i = b.inspect();
    assert_eq!(
        (i.registration, i.job),
        (Registration::SwitchedOff, Job::NotRunning)
    );
    assert!(b.job_gone());
    sys.answer("systemctl --user is-active", Output::ok("weird\n"));
    assert_eq!(b.inspect().job, Job::Unknown);
    // Evict and unregister.
    sys.answer(
        "systemctl --user stop",
        Output {
            code: Some(5),
            stderr: "Unit pitwall.service not loaded.".into(),
            ..Output::default()
        },
    );
    assert!(b.evict().is_ok());
    sys.answer(
        "systemctl --user stop",
        Output {
            code: Some(1),
            stderr: "Job failed".into(),
            ..Output::default()
        },
    );
    assert!(b.evict().is_err());
    sys.on("systemctl --user stop", |_, _| {
        Err(RunError::TimedOut("systemctl".into()))
    });
    assert!(b.evict().is_err());
    sys.fail_removes(Some(std::io::ErrorKind::PermissionDenied));
    assert!(b.unregister().is_err());
    sys.fail_removes(None);
    assert!(b.unregister().is_ok());
    assert!(
        b.unregister().is_ok(),
        "removing what is not there succeeds"
    );
    sys.answer(
        "systemctl --user daemon-reload",
        Output {
            code: Some(1),
            stderr: "Failed".into(),
            ..Output::default()
        },
    );
    assert!(b.unregister().is_err());
    // A unit that cannot be read.
    let sys2 = FakeSystem::new();
    sys2.on("x", |_, _| Ok(Output::code(0)));
    let system = ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    let s = Systemd::new(
        system,
        "/etc/systemd/system/pitwall.service".into(),
        Target::exe("/p"),
        Box::new(sys2),
    );
    assert_eq!(s.mechanism(), crate::backend::Mechanism::SystemdSystem);
    assert!(
        !s.capabilities().automatic,
        "a system unit is changed only on request"
    );
    assert!(s.definition().contains("WantedBy=multi-user.target"));
}

#[test]
fn a_unit_whose_program_was_moved_or_belongs_to_an_older_copy_is_re_pointed_and_a_newer_copys_is_left()
 {
    let d = tempfile::tempdir().unwrap();
    let sys = FakeSystem::new();
    sys.answer("systemctl --user is-active", Output::ok("active\n"));
    let old = systemd(&sys, "/opt/pitwall-1.1/pitwall-server");
    sys.put_file(unit_path(), &old.definition());
    sys.put_program("/opt/pitwall-1.1/pitwall-server")
        .put_program("/opt/pitwall-1.0/pitwall-server");
    // Version 1.1 registered it; 1.0 (another copy) starts: the newer copy keeps it.
    crate::record::RecordFile::new(d.path(), LABEL)
        .save("1.1.0", None)
        .unwrap();
    let mut older = Manager::new(
        "Pitwall",
        Box::new(systemd(&sys, "/opt/pitwall-1.0/pitwall-server")),
        d.path(),
        LABEL,
        "1.0.0",
        || true,
    );
    assert_eq!(
        older.on_start(&no_cancel()),
        Started::Left(Hold::NewerOwner("1.1.0".into()))
    );
    assert_eq!(older.status().condition, Condition::Running);
    assert!(changes(&sys).is_empty());
    // 1.1 is deleted: 1.0 takes the registration over (the program is gone).
    sys.remove_program(Path::new("/opt/pitwall-1.1/pitwall-server"));
    assert_eq!(
        older.on_start(&no_cancel()),
        Started::Repaired(Reason::ProgramMissing, RepairOutcome::Recovered)
    );
    assert!(
        sys.file(&unit_path())
            .unwrap()
            .contains("/opt/pitwall-1.0/pitwall-server")
    );
    assert_eq!(
        older.recorded().registered_version.as_deref(),
        Some("1.0.0")
    );
    // 1.2 starts: it takes it over from the older copy.
    sys.put_program("/opt/pitwall-1.2/pitwall-server");
    let mut newer = Manager::new(
        "Pitwall",
        Box::new(systemd(&sys, "/opt/pitwall-1.2/pitwall-server")),
        d.path(),
        LABEL,
        "1.2.0",
        || true,
    );
    assert_eq!(
        newer.on_start(&no_cancel()),
        Started::Repaired(Reason::Moved, RepairOutcome::Recovered)
    );
}

// ---- ADR 0054 RC-2: the packaged unit's turn-off is judged by what systemd says afterwards, never by an ignored result

const PACKAGED_UNIT: &str = "/usr/lib/systemd/system/pitwall.service";

fn packaged_system_unit(sys: &FakeSystem) -> Systemd {
    let spec = ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    let b = Systemd::new(
        spec,
        PACKAGED_UNIT.into(),
        Target::exe("/usr/bin/pitwall-server"),
        Box::new(sys.clone()),
    )
    .packaged();
    sys.put_file(PACKAGED_UNIT, &b.definition());
    sys.put_program("/usr/bin/pitwall-server");
    b
}

#[test]
fn a_packaged_unit_whose_disable_now_failed_and_is_still_enabled_and_running_is_not_reported_removed()
 {
    use crate::backend::Backend as _;
    let sys = FakeSystem::new();
    let mut b = packaged_system_unit(&sys);
    sys.answer(
        "systemctl disable --now",
        Output {
            code: Some(1),
            stderr: "Failed to disable unit: Access denied\n".into(),
            ..Output::default()
        },
    );
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    assert!(
        b.unregister().is_err(),
        "the unit is still enabled and active, so the turn-off did not happen"
    );
}

#[test]
fn a_packaged_unit_that_reads_back_disabled_and_stopped_is_removed_whatever_the_tool_said() {
    use crate::backend::Backend as _;
    let sys = FakeSystem::new();
    let mut b = packaged_system_unit(&sys);
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("disabled\n"));
    assert!(b.unregister().is_ok());
    assert!(
        sys.file(Path::new(PACKAGED_UNIT)).is_some(),
        "dpkg owns the packaged file; it stays"
    );
}

#[test]
fn a_packaged_unit_whose_disable_now_said_yes_but_reads_back_enabled_is_still_present() {
    use crate::backend::{Backend as _, Job, Registration, UnregisterError};
    let sys = FakeSystem::new();
    let mut b = packaged_system_unit(&sys);
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    match b.unregister() {
        Err(UnregisterError::StillPresent(look)) => {
            assert_eq!(
                (look.registration, look.job),
                (Registration::Registered, Job::Running)
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_failed_disable_now_is_the_tools_own_words_unless_the_read_back_shows_the_unit_gone() {
    use crate::backend::{Backend as _, UnregisterError};
    let sys = FakeSystem::new();
    let mut b = packaged_system_unit(&sys);
    sys.answer(
        "systemctl disable --now",
        Output {
            code: Some(1),
            stderr: "Failed to disable unit: Read-only file system\n".into(),
            ..Output::default()
        },
    );
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    match b.unregister() {
        Err(UnregisterError::Failed(why)) => {
            assert!(why.contains("Read-only file system"), "{why}")
        }
        other => panic!("{other:?}"),
    }
    // The same refusal, but the unit already reads back disabled and stopped: nothing is left to do.
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("disabled\n"));
    assert!(b.unregister().is_ok());
}

#[test]
fn a_unit_systemd_does_not_know_is_already_gone_and_an_unreachable_manager_says_what_to_do() {
    use crate::backend::{Backend as _, Registration, UnregisterError};
    let sys = FakeSystem::new();
    // Packaged, but the package is gone: no unit file and systemd says it does not exist.
    let spec = ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    let mut gone = Systemd::new(
        spec,
        PACKAGED_UNIT.into(),
        Target::exe("/p"),
        Box::new(sys.clone()),
    )
    .packaged();
    sys.answer(
        "systemctl disable --now",
        Output {
            code: Some(1),
            stderr: "Failed to disable unit: Unit file pitwall.service does not exist.\n".into(),
            ..Output::default()
        },
    );
    let look = gone.unregister().unwrap();
    assert_eq!(look.registration, Registration::Absent);
    // No systemd session at all.
    sys.answer(
        "systemctl disable --now",
        Output {
            code: Some(1),
            stderr: "Failed to connect to bus: No medium found\n".into(),
            ..Output::default()
        },
    );
    assert!(
        matches!(gone.unregister(), Err(UnregisterError::Unavailable(w)) if w.contains("systemd"))
    );
    // The tool cannot be started: nothing is left to read back as gone, so the words are the tool's.
    let sys = FakeSystem::new();
    let mut present = packaged_system_unit(&sys);
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    sys.on("systemctl disable --now", |_, _| {
        Err(RunError::TimedOut("systemctl".into()))
    });
    assert!(
        matches!(present.unregister(), Err(UnregisterError::Failed(w)) if w.contains("disable --now"))
    );
}

#[test]
fn every_unregister_error_has_words_and_the_other_backends_return_what_they_read_back() {
    use crate::backend::{Backend as _, Inspection, Registration, UnregisterError};
    assert!(
        UnregisterError::NeedsElevation
            .to_string()
            .contains("administrator")
    );
    assert_eq!(UnregisterError::Failed("x".into()).to_string(), "x");
    assert_eq!(UnregisterError::Unavailable("y".into()).to_string(), "y");
    let still = UnregisterError::StillPresent(Inspection {
        registration: Registration::Registered,
        ..Inspection::absent()
    });
    assert!(still.to_string().contains("Registered"));
    assert!(Inspection::absent().withdrawn());
    let sys = FakeSystem::new();
    let mut run = WindowsRun::new(pitwall(), Target::exe("C:\\p.exe"), Box::new(sys.clone()));
    assert_eq!(run.unregister().unwrap().registration, Registration::Absent);
    let mut file = LaunchdFile::new(
        pitwall(),
        501,
        "/p.plist".into(),
        Target::exe("/p"),
        Box::new(sys),
    );
    assert_eq!(
        file.unregister().unwrap().registration,
        Registration::Absent
    );
}

// --------------------------------------------------------------------------------------------------------- Windows

const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const DYNO_EXE: &str = r"C:\Users\Ana\AppData\Local\Dyno\dyno-agent.exe";

fn dyno() -> ServiceSpec {
    ServiceSpec::builder("net.decryptology.dyno.agent", "Dyno", "dyno-agent")
        .args(["run"])
        .build()
        .unwrap()
}

#[test]
fn a_run_entry_is_added_started_switched_off_by_the_user_and_removed() {
    use crate::backend::{Backend, Registration};
    let d = tempfile::tempdir().unwrap();
    let sys = FakeSystem::new();
    sys.put_program(DYNO_EXE);
    let mut m = Manager::new(
        "Dyno",
        Box::new(WindowsRun::new(
            dyno(),
            Target::exe(DYNO_EXE),
            Box::new(sys.clone()),
        )),
        d.path(),
        "net.decryptology.dyno.agent",
        "1.0.0",
        || true,
    );
    // The Run value is absent until `reg add` has run, then it reads back as written.
    let value = format!("\"{DYNO_EXE}\" run");
    sys.emulate_run_registry();
    m.install(&no_cancel()).unwrap();
    assert_eq!(
        changes(&sys),
        [
            format!("reg delete {RUN} /v Dyno /f"),
            format!("reg add {RUN} /v Dyno /t REG_SZ /d {value} /f"),
            format!("spawn {DYNO_EXE} run")
        ]
    );
    // Registered now; the same definition after an update is only remembered.
    sys.clear_calls();
    sys.answer(
        "reg query HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        Output::ok(&format!("    Dyno    REG_SZ    {value}\r\n")),
    );
    let mut b = WindowsRun::new(dyno(), Target::exe(DYNO_EXE), Box::new(sys.clone()));
    let i = b.inspect();
    assert_eq!(
        (i.registration.clone(), i.points_here, i.definition_current),
        (Registration::Registered, Some(true), Some(true))
    );
    let mut m2 = Manager::new(
        "Dyno",
        Box::new(b),
        d.path(),
        "net.decryptology.dyno.agent",
        "1.0.1",
        || true,
    );
    assert_eq!(m2.on_start(&no_cancel()), Started::Marked);
    assert!(changes(&sys).is_empty());
    // Task Manager > Startup switched it off.
    sys.answer("reg query HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run", Output::ok("    Dyno    REG_BINARY    030000000000000000000000\r\n"));
    assert_eq!(m2.on_start(&no_cancel()), Started::Left(Hold::SwitchedOff));
    // Removing stops the running agent first.
    m2.uninstall().unwrap();
    assert_eq!(changes(&sys), [format!("reg delete {RUN} /v Dyno /f")]);
    assert!(
        sys.commands()
            .iter()
            .any(|c| c == &format!("{DYNO_EXE} stop"))
    );
}

#[test]
fn run_entry_failure_paths() {
    use crate::backend::{Backend, Registration};
    let sys = FakeSystem::new();
    let mut b = WindowsRun::new(dyno(), Target::exe(DYNO_EXE), Box::new(sys.clone()));
    sys.on("reg query", |_, _| {
        Err(RunError::NotStarted("reg.exe missing".into()))
    });
    assert!(matches!(
        b.inspect().registration,
        Registration::Unavailable(_)
    ));
    sys.answer(
        "reg add",
        Output {
            code: Some(1),
            stderr: "ERROR: Access is denied.".into(),
            ..Output::default()
        },
    );
    assert!(matches!(b.register(), Err(RegisterError::Other(e)) if e.contains("Access is denied")));
    sys.on("reg add", |_, _| Err(RunError::TimedOut("reg".into())));
    assert!(b.register().is_err());
    sys.answer("reg add", Output::code(0));
    sys.fail_spawns(Some("not a program"));
    assert_eq!(
        b.register(),
        Err(RegisterError::Other("not a program".into()))
    );
    sys.answer(
        "reg delete",
        Output {
            code: Some(1),
            ..Output::default()
        },
    );
    assert!(
        b.unregister().is_ok(),
        "a value that is not there is removed"
    );
    sys.answer(
        "reg delete",
        Output {
            code: Some(5),
            stderr: "denied".into(),
            ..Output::default()
        },
    );
    assert!(b.unregister().is_err());
    sys.on("reg delete", |_, _| Err(RunError::TimedOut("reg".into())));
    assert!(b.unregister().is_err());
    // The registered agent is another copy's: that one is asked to stop; a missing one is skipped.
    sys.answer(
        "reg query",
        Output::ok("    Dyno    REG_SZ    \"C:\\Old\\dyno-agent.exe\" run\r\n"),
    );
    assert!(
        b.evict().is_ok(),
        "the old program is gone: nothing to stop"
    );
    sys.put_program("C:\\Old\\dyno-agent.exe");
    sys.answer("C:\\Old\\dyno-agent.exe stop", Output::code(2));
    assert!(b.evict().is_err());
    sys.on("C:\\Old\\dyno-agent.exe stop", |_, _| {
        Err(RunError::TimedOut("agent".into()))
    });
    assert!(b.evict().is_err());
    let i = b.inspect();
    assert_eq!((i.points_here, i.program_exists), (Some(false), Some(true)));
    assert!(b.job_gone() && b.capabilities().automatic);
}

#[test]
fn the_windows_service_is_created_with_sc_and_its_errors_are_read_by_number() {
    use crate::backend::{Backend, Job, Registration};
    let sys = FakeSystem::new();
    let svc = ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .display_name("Pitwall local LLM server")
        .description("Serves local language models.")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    const EVENT_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application\Pitwall";
    let exe = r"C:\Program Files\Pitwall\pitwall-server.exe";
    let mut b = WindowsService::new(svc.clone(), Target::exe(exe), Box::new(sys.clone()));
    b.register().unwrap();
    assert_eq!(
        sys.commands(),
        [
            format!(
                "reg add {EVENT_KEY} /v EventMessageFile /t REG_EXPAND_SZ /d %SystemRoot%\\System32\\EventCreate.exe /f"
            ),
            format!("reg add {EVENT_KEY} /v TypesSupported /t REG_DWORD /d 7 /f"),
            format!(
                "sc create Pitwall binPath= \"{exe}\" --service start= delayed-auto DisplayName= Pitwall local LLM server"
            ),
            "sc description Pitwall Serves local language models.".to_string(),
            "sc failure Pitwall reset= 86400 actions= restart/10000/restart/60000/restart/300000"
                .to_string(),
            "sc failureflag Pitwall 1".to_string(),
            "sc start Pitwall".to_string(),
            // The read-back (ADR 0054 section 1.1).
            "sc query Pitwall".to_string(),
            "sc qc Pitwall".to_string(),
        ]
    );
    sys.answer("sc create", Output::code(5));
    assert_eq!(b.register(), Err(RegisterError::NeedsElevation));
    sys.answer("sc create", Output::code(1072));
    assert!(matches!(b.register(), Err(RegisterError::Other(e)) if e.contains("Services window")));
    sys.on("sc create", |_, _| Err(RunError::TimedOut("sc".into())));
    assert!(b.register().is_err());
    sys.answer("sc create", Output::code(0));
    sys.answer("sc start", Output::code(1056));
    assert!(b.register().is_ok(), "already running");
    sys.answer("sc start", Output::code(1053));
    assert!(b.register().is_err());
    sys.on("sc start", |_, _| Err(RunError::TimedOut("sc".into())));
    assert!(b.register().is_err());
    // Inspect.
    sys.answer("sc query", Output::code(1060));
    assert_eq!(b.inspect(), crate::backend::Inspection::absent());
    assert!(b.job_gone());
    sys.answer("sc query", Output::ok("STATE : 4  RUNNING"));
    sys.answer(
        "sc qc",
        Output::ok(&format!(
            "START_TYPE : 2 AUTO_START\r\nBINARY_PATH_NAME : \"{exe}\" --service\r\n"
        )),
    );
    sys.put_program(exe);
    let i = b.inspect();
    assert_eq!(
        (i.registration, i.job, i.points_here, i.program_exists),
        (
            Registration::Registered,
            Job::Running,
            Some(true),
            Some(true)
        )
    );
    assert!(!b.job_gone());
    sys.answer("sc qc", Output::ok("START_TYPE : 4 DISABLED\r\n"));
    assert_eq!(b.inspect().registration, Registration::SwitchedOff);
    sys.answer("sc query", Output::ok("STATE : 1  STOPPED"));
    assert_eq!(b.inspect().job, Job::NotRunning);
    assert!(b.job_gone());
    sys.answer("sc query", Output::ok("garbage"));
    assert_eq!(b.inspect().job, Job::Unknown);
    sys.answer("sc query", Output::code(5));
    assert!(matches!(
        b.inspect().registration,
        Registration::Unavailable(_)
    ));
    sys.on("sc query", |_, _| Err(RunError::NotStarted("sc".into())));
    assert!(matches!(
        b.inspect().registration,
        Registration::Unavailable(_)
    ));
    // Remove and stop.
    for (code, ok) in [(0, true), (1060, true), (1072, true), (5, false)] {
        sys.answer("sc delete", Output::code(code));
        assert_eq!(b.unregister().is_ok(), ok, "{code}");
    }
    sys.on("sc delete", |_, _| Err(RunError::TimedOut("sc".into())));
    assert!(b.unregister().is_err());
    for (code, ok) in [(0, true), (1062, true), (1060, true), (5, false)] {
        sys.answer("sc stop", Output::code(code));
        assert_eq!(b.evict().is_ok(), ok, "{code}");
    }
    sys.on("sc stop", |_, _| Err(RunError::TimedOut("sc".into())));
    assert!(b.evict().is_err());
}

/// An elevator that remembers the elevated install happened (the scripted SCM then reads this copy's program back).
struct MarksInstalled(FakeElevator, Arc<std::sync::atomic::AtomicBool>);

impl crate::port::Elevator for MarksInstalled {
    fn run_elevated(
        &mut self,
        program: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<i32, RegisterError> {
        let code = self.0.run_elevated(program, args, timeout)?;
        if code == 0 && args == ["install"] {
            self.1.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(code)
    }
}

#[test]
fn from_the_app_the_service_changes_only_through_one_prompt_on_request() {
    let d = tempfile::tempdir().unwrap();
    let sys = FakeSystem::new();
    let svc = ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    let exe = PathBuf::from(r"C:\Program Files\Pitwall\pitwall-server.exe");
    sys.put_program(&exe);
    sys.answer("sc query", Output::ok("STATE : 4  RUNNING"));
    sys.answer(
        "sc qc",
        Output::ok("BINARY_PATH_NAME : \"C:\\Other\\pitwall-server.exe\" --service\r\n"),
    );
    sys.put_program("C:\\Other\\pitwall-server.exe");
    let elevator = FakeElevator::new(&sys);
    // The elevated install re-registers the service for this copy: the SCM reads it back as this program afterwards.
    let installed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let here = format!("BINARY_PATH_NAME : \"{}\" --service\r\n", exe.display());
    let flag = installed.clone();
    sys.on("sc qc", move |_, _| {
        Ok(Output::ok(
            &if flag.load(std::sync::atomic::Ordering::SeqCst) {
                here.clone()
            } else {
                "BINARY_PATH_NAME : \"C:\\Other\\pitwall-server.exe\" --service\r\n".to_string()
            },
        ))
    });
    let b = ElevatedWindowsService::new(
        svc.clone(),
        exe.clone(),
        Box::new(sys.clone()),
        Box::new(MarksInstalled(elevator.clone(), installed)),
    );
    let mut m = Manager::new("Pitwall", Box::new(b), d.path(), LABEL, "1.0.0", || true);
    crate::record::RecordFile::new(d.path(), LABEL)
        .save("1.0.0", None)
        .unwrap();
    // Another copy's service: offered, never prompted at start.
    assert_eq!(m.on_start(&no_cancel()), Started::Offered(Reason::Moved));
    assert!(changes(&sys).is_empty());
    assert!(m.status().can_repair);
    // Repair: one elevated install (it replaces the old service itself).
    assert_eq!(m.repair(&no_cancel()).unwrap(), RepairOutcome::Recovered);
    assert_eq!(
        changes(&sys),
        [format!("elevated {} install", exe.display())]
    );
    // Declined, failed.
    elevator.results([Err(RegisterError::Declined)]);
    assert!(matches!(
        m.repair(&no_cancel()),
        Err(ServiceError::Declined)
    ));
    elevator.results([Ok(1)]);
    assert!(
        matches!(m.repair(&no_cancel()), Err(ServiceError::Failed(e)) if e.contains("exit code 1"))
    );
    // Uninstall: one elevated uninstall.
    sys.clear_calls();
    sys.answer("sc query", Output::code(1060));
    m.uninstall().unwrap();
    assert_eq!(
        changes(&sys),
        [format!("elevated {} uninstall", exe.display())]
    );
    elevator.results([Err(RegisterError::Declined)]);
    sys.answer("sc query", Output::ok("STATE : 4  RUNNING"));
    assert!(matches!(m.uninstall(), Err(ServiceError::Failed(e)) if e.contains("declined")));
    // The installer re-registered it for this version: only remembered.
    sys.answer(
        "sc qc",
        Output::ok(&format!(
            "BINARY_PATH_NAME : \"{}\" --service\r\n",
            exe.display()
        )),
    );
    let b =
        ElevatedWindowsService::new(svc, exe.clone(), Box::new(sys.clone()), Box::new(elevator));
    let mut m = Manager::new("Pitwall", Box::new(b), d.path(), LABEL, "1.0.1", || true);
    assert_eq!(m.on_start(&no_cancel()), Started::Marked);
}

#[test]
fn repair_refuses_what_it_cannot_or_need_not_do() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(1, Some(1));
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.0");
    crate::record::RecordFile::new(d.path(), LABEL)
        .save("2.0.0", None)
        .unwrap();
    assert!(
        matches!(m.repair(&no_cancel()), Ok(RepairOutcome::Recovered)),
        "Repair pressed: the user's call"
    );
    // An app that really cannot register from where it is (NotFound, and `register` refuses): Install says why,
    // and nothing is left registered.
    let app = FakeAppService::new(&sys);
    app.set_status(AppServiceStatus::NotFound);
    app.register_results([Err(RegisterError::Unavailable(
        "macOS cannot find Pitwall's launch agent".into(),
    ))]);
    let b = LaunchdBundled::new(pitwall(), 501, Box::new(app), Box::new(sys.clone()));
    let mut m = Manager::new("Pitwall", Box::new(b), d.path(), LABEL, "1.0.0", || false);
    assert!(matches!(
        m.repair(&no_cancel()),
        Err(ServiceError::Unavailable(w)) if w.contains("cannot find")
    ));
    assert_eq!(m.check(), crate::plan::Plan::Leave(Hold::NotInstalled));
    assert_eq!(m.mechanism(), crate::backend::Mechanism::LaunchdBundled);
    let st = m.status();
    assert_eq!(st.condition, Condition::NotInstalled);
    assert!(st.can_install);
    // NewerOwner refusal for an explicit repair cannot happen (requested wins), but uninstall of nothing works.
    let sys = FakeSystem::new();
    let b = Systemd::new(
        pitwall(),
        unit_path(),
        Target::exe("/p"),
        Box::new(sys.clone()),
    );
    sys.answer(
        "systemctl --user is-active",
        Output {
            code: Some(3),
            stdout: "inactive\n".into(),
            ..Output::default()
        },
    );
    let mut m = Manager::new("Pitwall", Box::new(b), d.path(), LABEL, "1.0.0", || false)
        .with_program("/p")
        .with_timings(Timings::default())
        .with_tracker(Arc::new(crate::tracker::RepairTracker::new()));
    m.uninstall().unwrap();
    assert_eq!(m.recorded(), crate::record::Recorded::default());
}

#[test]
fn a_cancelled_start_changes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    let cancel = AtomicBool::new(true);
    assert_eq!(
        m.on_start(&cancel),
        Started::Repaired(Reason::Refused, RepairOutcome::Cancelled)
    );
    assert!(changes(&sys).is_empty());
    assert_eq!(m.recorded().registered_version, None);
}

#[test]
fn a_job_that_launchd_drops_slowly_is_waited_for_and_never_registered_over_too_early() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    sim.lock().unwrap().linger = Duration::from_secs(12);
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    assert_eq!(
        m.on_start(&no_cancel()),
        Started::Repaired(Reason::Refused, RepairOutcome::Recovered)
    );
    // The register came after the 12 s linger: find its position in the call log.
    let calls = sys.calls();
    let register_at = calls
        .iter()
        .position(|c| *c == Call::Os("register".into()))
        .unwrap();
    let waited: Duration = calls[..register_at]
        .iter()
        .filter_map(|c| {
            if let Call::Pause(d) = c {
                Some(*d)
            } else {
                None
            }
        })
        .sum();
    assert!(waited >= Duration::from_secs(12), "{waited:?}");
}

#[test]
fn backend_contract_details() {
    use crate::backend::{Backend, Job, Mechanism, Registration};
    let sys = FakeSystem::new();
    // Defaults of the trait: no one-step replacement, no Login Items, no definition for SMAppService.
    let mut bundled = LaunchdBundled::new(
        pitwall(),
        501,
        Box::new(FakeAppService::new(&sys)),
        Box::new(sys.clone()),
    );
    assert!(bundled.replace_in_one_step().is_none());
    assert_eq!(bundled.definition(), None);
    // launchd's disabled list that cannot be read is "not switched off".
    sys.answer("/bin/launchctl print-disabled", Output::code(1));
    assert_eq!(bundled.inspect().registration, Registration::Absent);
    // A plist or unit that cannot be read.
    sys.fail_reads(Some(std::io::ErrorKind::PermissionDenied));
    let mut file = LaunchdFile::new(
        pitwall(),
        501,
        "/p.plist".into(),
        Target::exe("/p"),
        Box::new(sys.clone()),
    );
    assert!(
        matches!(file.inspect().registration, Registration::Unavailable(w) if w.contains("cannot be read"))
    );
    assert!(!file.open_login_items());
    let mut unit = systemd(&sys, "/p");
    assert!(
        matches!(unit.inspect().registration, Registration::Unavailable(w) if w.contains("cannot be read"))
    );
    sys.fail_reads(None);
    // A plist file the user switched off.
    sys.put_file("/p.plist", &file.definition());
    sys.answer(
        "/bin/launchctl print-disabled",
        Output::ok(&format!("\"{LABEL}\" => disabled\n")),
    );
    assert_eq!(file.inspect().registration, Registration::SwitchedOff);
    // Through the trait: the file-based definitions.
    let as_dyn: [&dyn Backend; 2] = [&file, &unit];
    assert!(as_dyn.iter().all(|b| b.definition().is_some()));
    let run = WindowsRun::new(dyno(), Target::exe(DYNO_EXE), Box::new(sys.clone()));
    assert_eq!(
        (run.mechanism(), Backend::definition(&run).unwrap()),
        (Mechanism::WindowsRun, format!("\"{DYNO_EXE}\" run"))
    );
    // The Windows service: mechanism, capabilities, a configuration that cannot be read.
    let svc = ServiceSpec::builder(LABEL, "Pitwall", "pitwall-server")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    let mut ws = WindowsService::new(svc.clone(), Target::exe("C:\\p.exe"), Box::new(sys.clone()));
    assert_eq!(ws.mechanism(), Mechanism::WindowsService);
    assert!(ws.capabilities().automatic);
    sys.answer("sc query", Output::ok("STATE : 4  RUNNING"));
    sys.answer("sc qc", Output::code(5));
    let i = ws.inspect();
    assert_eq!(
        (i.registration, i.job, i.program),
        (Registration::Registered, Job::Running, None)
    );
    // From the app: registering directly is one elevated install too; mechanism and evict.
    let mut el = ElevatedWindowsService::new(
        svc,
        "C:\\p.exe".into(),
        Box::new(sys.clone()),
        Box::new(FakeElevator::new(&sys)),
    );
    assert!(el.register().is_ok() && el.evict().is_ok());
    assert_eq!(el.mechanism(), Mechanism::WindowsService);
    assert!(!el.capabilities().automatic);
    sys.answer("sc query", Output::code(1060));
    assert!(el.job_gone());
}

#[test]
fn manager_contract_details() {
    let d = tempfile::tempdir().unwrap();
    let sim = launchd(2, Some(1));
    let sys = FakeSystem::new();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    assert_eq!(m.check(), crate::plan::Plan::Replace(Reason::Refused));
    // A repair that already works (in another thread of this app) makes the next one wait.
    let t = m.tracker();
    assert!(t.start(std::time::Instant::now()));
    assert_eq!(m.on_start(&no_cancel()), Started::Busy);
    assert!(matches!(m.uninstall(), Err(ServiceError::Busy)));
    t.finish(&RepairOutcome::Cancelled, std::time::Instant::now());
    // The state file was written by a newer app: the repair works, the version is not overwritten (and says so in the log).
    std::fs::write(
        d.path().join(format!("service-{LABEL}.json")),
        r#"{"version": 99}"#,
    )
    .unwrap();
    let mut m = mac_manager(&sim, &sys, d.path(), "1.0.4");
    assert!(matches!(
        m.on_start(&no_cancel()),
        Started::Repaired(Reason::Refused, RepairOutcome::Recovered)
    ));
    assert!(
        std::fs::read_to_string(d.path().join(format!("service-{LABEL}.json")))
            .unwrap()
            .contains("99")
    );
    // Uninstall goes on when stopping the job fails.
    let d = tempfile::tempdir().unwrap();
    let sys = FakeSystem::new();
    sys.answer(
        "systemctl --user stop",
        Output {
            code: Some(1),
            stderr: "Job failed".into(),
            ..Output::default()
        },
    );
    sys.answer(
        "systemctl --user is-active",
        Output {
            code: Some(3),
            stdout: "inactive\n".into(),
            ..Output::default()
        },
    );
    let mut m = Manager::new(
        "Pitwall",
        Box::new(systemd(&sys, "/p")),
        d.path(),
        LABEL,
        "1.0.0",
        || false,
    );
    m.uninstall().unwrap();
    // A job that never stops is waited for a bounded time.
    sys.answer("systemctl --user is-active", Output::ok("active\n"));
    sys.clear_calls();
    m.uninstall().unwrap();
    let paused: Duration = sys
        .calls()
        .iter()
        .filter_map(|c| {
            if let Call::Pause(d) = c {
                Some(*d)
            } else {
                None
            }
        })
        .sum();
    assert_eq!(paused, Timings::default().gone_wait);
}
