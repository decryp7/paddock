//! The parsers and backends read the output of the real tools, recorded on the real systems (ADR 0054 invariant 3: a scripted
//! answer is a recording, not a hand-written guess). Each file in `tests/recorded/` starts with `#!` lines that say where,
//! when and with which command it was recorded; the rest is the tool's raw output.

use crate::backend::{
    Backend, Job, LastExit, LaunchdFile, Systemd, Target, UnitFileState, UnitOrigin, WindowsService,
};
use crate::port::Output;
use crate::spec::{Kind, ServiceSpec};
use crate::testing::FakeSystem;

/// The recorded output `name` without its `#!` header.
fn recorded(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/recorded")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        text.starts_with("#! recorded "),
        "{name} says where it was recorded"
    );
    text.lines()
        .filter(|l| !l.starts_with("#!"))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn systemd(sys: &FakeSystem) -> Systemd {
    let spec = ServiceSpec::builder("net.decryptology.dyno.helper", "DynoHelper", "dyno-helper")
        .unit("paddock-fixture")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    sys.put_program("/usr/bin/dyno-helper");
    let b = Systemd::new(
        spec,
        "/etc/systemd/system/paddock-fixture.service".into(),
        Target::exe("/usr/bin/dyno-helper"),
        Box::new(sys.clone()),
    );
    sys.put_file(
        "/etc/systemd/system/paddock-fixture.service",
        &b.definition(),
    );
    b
}

#[test]
fn systemd_255_a_unit_stopped_by_systemctl_stop_is_not_running_with_no_last_exit() {
    let sys = FakeSystem::new();
    let mut b = systemd(&sys);
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        Output::ok(&recorded("systemd-255-stopped.txt")),
    );
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit, look.restarts),
        (Job::NotRunning, None, Some(0))
    );
}

#[test]
fn systemd_255_a_unit_killed_from_outside_is_failed_with_the_signal() {
    let sys = FakeSystem::new();
    let mut b = systemd(&sys);
    sys.answer("systemctl is-active", Output::ok("failed\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        Output::ok(&recorded("systemd-255-killed.txt")),
    );
    let look = b.inspect();
    assert_eq!(look.job, Job::Failed(LastExit::Signal(9)));
    assert_eq!(
        look.unit.unwrap().origin,
        crate::backend::UnitOrigin::Runtime
    );
}

#[test]
fn systemd_255_a_running_unit_has_no_exit_yet() {
    let sys = FakeSystem::new();
    let mut b = systemd(&sys);
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        Output::ok(&recorded("systemd-255-running.txt")),
    );
    let look = b.inspect();
    assert_eq!((look.job, look.last_exit), (Job::Running, None));
}

// ------------------------------------------------------------------------------------------------------ Windows 11

const HELPER_EXE: &str = r"C:\Program Files\Dyno Helper\dyno-helper.exe";

fn helper_service(sys: &FakeSystem) -> WindowsService {
    sys.put_program(HELPER_EXE);
    let spec = ServiceSpec::builder("net.decryptology.dyno.helper", "DynoHelper", "dyno-helper")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    WindowsService::new(spec, Target::exe(HELPER_EXE), Box::new(sys.clone()))
}

fn scm_answers(sys: &FakeSystem, query: &str) {
    sys.answer("sc query", Output::ok(&recorded(query)));
    sys.answer(
        "sc qc",
        Output::ok(&recorded("windows-11-sc-qc-helper.txt")),
    );
}

#[test]
fn windows_11_a_service_that_has_never_started_since_boot_has_no_last_exit() {
    let sys = FakeSystem::new();
    let mut b = helper_service(&sys);
    scm_answers(&sys, "windows-11-sc-query-never-started.txt");
    let look = b.inspect();
    assert_eq!((look.job, look.last_exit), (Job::NotRunning, None));
}

#[test]
fn windows_11_a_service_that_stopped_with_its_own_exit_code_is_failed_with_that_code() {
    let sys = FakeSystem::new();
    let mut b = helper_service(&sys);
    scm_answers(&sys, "windows-11-sc-query-service-specific-3.txt");
    let look = b.inspect();
    assert_eq!(look.job, Job::Failed(LastExit::ServiceSpecific(3)));
    assert_eq!(
        look.points_here,
        Some(true),
        "the quoted path with --service is the program"
    );
    let unit = look.unit.unwrap();
    assert_eq!(
        (
            unit.origin,
            unit.enabled,
            unit.path.to_string_lossy().into_owned()
        ),
        (
            UnitOrigin::ServiceManager,
            UnitFileState::Enabled,
            HELPER_EXE.to_string()
        )
    );
}

#[test]
fn windows_11_a_service_whose_process_was_killed_is_failed_with_the_win32_code_1067() {
    let sys = FakeSystem::new();
    let mut b = helper_service(&sys);
    scm_answers(&sys, "windows-11-sc-query-crashed-1067.txt");
    assert_eq!(b.inspect().job, Job::Failed(LastExit::Win32(1067)));
}

#[test]
fn windows_11_a_running_a_stopping_and_a_cleanly_stopped_service() {
    let sys = FakeSystem::new();
    let mut b = helper_service(&sys);
    scm_answers(&sys, "windows-11-sc-query-running.txt");
    let look = b.inspect();
    assert_eq!((look.job, look.last_exit), (Job::Running, None));
    // STOP_PENDING is neither running nor a failure.
    scm_answers(&sys, "windows-11-sc-query-stop-pending.txt");
    assert_eq!(b.inspect().job, Job::NotRunning);
    scm_answers(&sys, "windows-11-sc-query-stopped-clean.txt");
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit),
        (Job::NotRunning, Some(LastExit::Win32(0)))
    );
}

// ------------------------------------------------------------------------------------------------------- macOS 26

fn launchd(sys: &FakeSystem, fixture: &str) -> crate::backend::Inspection {
    let spec = ServiceSpec::builder("fix.term", "Fixture", "fixjob")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    sys.answer("/bin/launchctl print", Output::ok(&recorded(fixture)));
    LaunchdFile::new(
        spec,
        0,
        "/Library/LaunchDaemons/fix.term.plist".into(),
        Target::exe("/tmp/fixjob.sh"),
        Box::new(sys.clone()),
    )
    .inspect()
}

#[test]
fn macos_26_a_running_daemon_has_no_last_exit_and_no_restarts() {
    let look = launchd(&FakeSystem::new(), "macos-26-launchctl-print-running.txt");
    assert_eq!(
        (look.job, look.last_exit, look.restarts),
        (Job::Running, None, Some(0))
    );
}

#[test]
fn macos_26_a_daemon_stopped_by_sigterm_is_not_running_not_failed() {
    let look = launchd(
        &FakeSystem::new(),
        "macos-26-launchctl-print-killed-sigterm.txt",
    );
    assert_eq!(
        (look.job, look.last_exit),
        (Job::NotRunning, Some(LastExit::Signal(15)))
    );
}

#[test]
fn macos_26_a_daemon_killed_by_sigkill_is_failed_with_the_signal() {
    let look = launchd(
        &FakeSystem::new(),
        "macos-26-launchctl-print-killed-sigkill.txt",
    );
    assert_eq!(look.job, Job::Failed(LastExit::Signal(9)));
}

#[test]
fn macos_26_a_daemon_that_exited_zero_is_not_running_and_one_that_exited_three_is_failed() {
    let sys = FakeSystem::new();
    let look = launchd(&sys, "macos-26-launchctl-print-exited-0.txt");
    assert_eq!(
        (look.job, look.last_exit),
        (Job::NotRunning, Some(LastExit::Code(0)))
    );
    let look = launchd(&sys, "macos-26-launchctl-print-exited-3.txt");
    assert_eq!(
        (look.job, look.last_exit),
        (Job::Failed(LastExit::Code(3)), Some(LastExit::Code(3)))
    );
}
