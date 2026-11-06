//! ADR 0054 section 1.1: a registration is written and read back through the port. `register` returns the post-write
//! inspection (never `Ok` when it disagrees with what was written), `Inspection` carries the typed last exit, restarts and
//! the definition in force, and the returned inspection equals a fresh `inspect()` for every backend (the contract case).
//! Every OS answer below has the shape of the real tool's output; the lifecycle suite records the real ones.

use crate::backend::{
    Backend, ElevatedWindowsService, Inspection, Job, LastExit, LaunchdBundled, LaunchdFile,
    Registration, Systemd, Target, UnitFile, UnitFileState, UnitOrigin, WindowsRun, WindowsService,
};
use crate::port::{AppServiceStatus, Output, RegisterError};
use crate::spec::{Kind, ServiceSpec};
use crate::testing::{FakeAppService, FakeElevator, FakeSystem};
use std::path::PathBuf;

const LABEL: &str = "net.decryptology.dyno.helper";

fn spec(kind: Kind) -> ServiceSpec {
    ServiceSpec::builder(LABEL, "Dyno", "dyno-helper")
        .kind(kind)
        .build()
        .unwrap()
}

// ------------------------------------------------------------------------------------------------------- systemd

const ETC_UNIT: &str = "/etc/systemd/system/dyno.service";
const EXE: &str = "/usr/lib/dyno/dyno-helper";

fn show(
    fragment: &str,
    file_state: &str,
    result: &str,
    restarts: u32,
    status: i32,
    code: i32,
) -> Output {
    Output::ok(&format!(
        "FragmentPath={fragment}\nUnitFileState={file_state}\nResult={result}\nNRestarts={restarts}\nExecMainStatus={status}\nExecMainCode={code}\n"
    ))
}

fn system_unit(sys: &FakeSystem) -> Systemd {
    sys.put_program(EXE);
    Systemd::new(
        spec(Kind::SystemService),
        ETC_UNIT.into(),
        Target::exe(EXE),
        Box::new(sys.clone()),
    )
}

fn healthy_unit(sys: &FakeSystem) {
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "enabled", "success", 0, 0, 0),
    );
}

#[test]
fn systemd_register_returns_the_post_write_inspection_with_the_unit_in_force() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    healthy_unit(&sys);
    let look = b.register().unwrap();
    assert_eq!(look.registration, Registration::Registered);
    assert_eq!(look.job, Job::Running);
    assert_eq!(
        look.unit,
        Some(UnitFile {
            path: PathBuf::from(ETC_UNIT),
            origin: UnitOrigin::Admin,
            enabled: UnitFileState::Enabled
        })
    );
    assert_eq!(look.restarts, Some(0));
    assert_eq!(look.last_exit, None, "a running main process has not ended");
    assert_eq!(
        look,
        b.inspect(),
        "contract: the returned inspection is a fresh one"
    );
}

#[test]
fn systemd_register_that_reads_back_disabled_is_not_as_written() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("disabled\n"));
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "disabled", "success", 0, 0, 0),
    );
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => {
            assert_eq!(look.registration, Registration::SwitchedOff);
            assert_eq!(look.job, Job::NotRunning);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn systemd_register_that_reads_back_another_definition_is_not_as_written() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    healthy_unit(&sys);
    // Something rewrote the unit between the write and the read-back (a drop-in or a package upgrade).
    sys.rewrite_after(
        "systemctl daemon-reload",
        ETC_UNIT,
        "[Service]\nExecStart=/opt/other/dyno-helper\n",
    );
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => {
            assert_eq!(look.definition_current, Some(false));
            assert_eq!(look.points_here, Some(false));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_systemd_unit_that_failed_reads_as_failed_with_its_exit_status_restarts_and_origin() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.put_file(ETC_UNIT, &b.definition());
    sys.answer("systemctl is-active", Output::ok("failed\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    sys.answer(
        "systemctl show",
        show(
            "/usr/lib/systemd/system/dyno.service",
            "enabled",
            "exit-code",
            4,
            3,
            1,
        ),
    );
    let look = b.inspect();
    assert_eq!(look.job, Job::Failed(LastExit::Code(3)));
    assert_eq!(look.last_exit, Some(LastExit::Code(3)));
    assert_eq!(look.restarts, Some(4));
    assert_eq!(look.unit.unwrap().origin, UnitOrigin::Packaged);
}

#[test]
fn a_systemd_unit_killed_by_a_signal_reads_the_signal_not_an_exit_code() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.put_file(ETC_UNIT, &b.definition());
    sys.answer("systemctl is-active", Output::ok("failed\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "enabled", "signal", 0, 11, 2),
    );
    assert_eq!(b.inspect().job, Job::Failed(LastExit::Signal(11)));
}

#[test]
fn a_systemd_unit_the_manager_cannot_describe_still_inspects_with_the_new_fields_empty() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.put_file(ETC_UNIT, &b.definition());
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        Output {
            code: Some(1),
            stderr: "Failed to get properties".into(),
            ..Output::default()
        },
    );
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit, look.restarts, look.unit),
        (Job::Running, None, None, None)
    );
}

#[test]
fn a_static_unit_is_manual_and_a_masked_one_is_masked() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.put_file(ETC_UNIT, &b.definition());
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "static", "success", 0, 0, 0),
    );
    assert_eq!(b.inspect().unit.unwrap().enabled, UnitFileState::Manual);
    sys.answer("systemctl is-enabled", Output::ok("masked\n"));
    sys.answer("systemctl show", show("", "masked", "success", 0, 0, 0));
    assert_eq!(
        b.inspect().unit,
        None,
        "no fragment, no definition in force"
    );
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "weird", "success", 0, 0, 0),
    );
    assert_eq!(b.inspect().unit.unwrap().enabled, UnitFileState::Other);
}

// -------------------------------------------------------------------------------------------------------- Windows

const SVC_EXE: &str = r"C:\Program Files\Dyno Helper\dyno-helper.exe";

fn scm(sys: &FakeSystem) -> WindowsService {
    sys.put_program(SVC_EXE);
    WindowsService::new(
        spec(Kind::SystemService),
        Target::exe(SVC_EXE),
        Box::new(sys.clone()),
    )
}

fn sc_query(state: &str, win32: &str, specific: &str) -> Output {
    Output::ok(&format!(
        "SERVICE_NAME: Dyno\n        TYPE               : 10  WIN32_OWN_PROCESS\n        STATE              : {state}\n        WIN32_EXIT_CODE    : {win32}\n        SERVICE_EXIT_CODE  : {specific}\n        CHECKPOINT         : 0x0\n        WAIT_HINT          : 0x0\n"
    ))
}

fn sc_qc(start: &str, program: &str) -> Output {
    Output::ok(&format!(
        "[SC] QueryServiceConfig SUCCESS\n\nSERVICE_NAME: Dyno\n        TYPE               : 10  WIN32_OWN_PROCESS\n        START_TYPE         : {start}\n        BINARY_PATH_NAME   : \"{program}\" --service\n"
    ))
}

#[test]
fn a_stopped_service_with_a_service_specific_code_reads_as_failed_with_that_code() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer(
        "sc query",
        sc_query("1  STOPPED", "1066 (0x42a)", "7  (0x7)"),
    );
    sys.answer("sc qc", sc_qc("2  AUTO_START  (DELAYED)", SVC_EXE));
    let look = b.inspect();
    assert_eq!(look.job, Job::Failed(LastExit::ServiceSpecific(7)));
    assert_eq!(look.last_exit, Some(LastExit::ServiceSpecific(7)));
    assert_eq!(
        look.restarts, None,
        "Windows counts restarts in the event log"
    );
}

#[test]
fn a_stopped_service_with_a_win32_code_reads_as_failed_with_that_code() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer(
        "sc query",
        sc_query("1  STOPPED", "1067 (0x42b)", "0  (0x0)"),
    );
    sys.answer("sc qc", sc_qc("2  AUTO_START", SVC_EXE));
    assert_eq!(b.inspect().job, Job::Failed(LastExit::Win32(1067)));
}

#[test]
fn a_service_that_stopped_cleanly_is_not_running_and_a_running_one_has_no_last_exit() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc qc", sc_qc("2  AUTO_START", SVC_EXE));
    sys.answer("sc query", sc_query("1  STOPPED", "0  (0x0)", "0  (0x0)"));
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit),
        (Job::NotRunning, Some(LastExit::Win32(0)))
    );
    sys.answer("sc query", sc_query("4  RUNNING", "0  (0x0)", "0  (0x0)"));
    let look = b.inspect();
    assert_eq!((look.job, look.last_exit), (Job::Running, None));
}

#[test]
fn the_service_manager_is_the_origin_and_the_start_type_is_the_enabled_state() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    for (start, want) in [
        ("2  AUTO_START  (DELAYED)", UnitFileState::Enabled),
        ("3  DEMAND_START", UnitFileState::Manual),
        ("4  DISABLED", UnitFileState::Disabled),
    ] {
        sys.answer("sc qc", sc_qc(start, SVC_EXE));
        let unit = b.inspect().unit.unwrap();
        assert_eq!(
            (unit.origin, unit.enabled, unit.path),
            (UnitOrigin::ServiceManager, want, PathBuf::from(SVC_EXE))
        );
    }
}

#[test]
fn a_service_registered_with_another_program_is_not_as_written() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    sys.answer("sc qc", sc_qc("2  AUTO_START", r"C:\Old\dyno-helper.exe"));
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => assert_eq!(look.points_here, Some(false)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_service_the_scm_reports_disabled_after_it_was_created_is_not_as_written() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc query", sc_query("1  STOPPED", "0", "0"));
    sys.answer("sc qc", sc_qc("4  DISABLED", SVC_EXE));
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => {
            assert_eq!(look.registration, Registration::SwitchedOff)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_service_the_scm_does_not_know_after_sc_create_succeeded_is_not_as_written() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc query", Output::code(1060));
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => {
            assert_eq!(look.registration, Registration::Absent)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_healthy_service_registers_and_the_returned_inspection_is_a_fresh_one() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    sys.answer("sc qc", sc_qc("2  AUTO_START  (DELAYED)", SVC_EXE));
    let look = b.register().unwrap();
    assert_eq!(look.job, Job::Running);
    assert_eq!(look, b.inspect());
}

#[test]
fn the_elevated_service_judges_what_the_elevated_install_left_behind() {
    let sys = FakeSystem::new();
    sys.put_program(SVC_EXE);
    let mut b = ElevatedWindowsService::new(
        spec(Kind::SystemService),
        PathBuf::from(SVC_EXE),
        Box::new(sys.clone()),
        Box::new(FakeElevator::new(&sys)),
    );
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    sys.answer("sc qc", sc_qc("2  AUTO_START", SVC_EXE));
    assert_eq!(b.register().unwrap().job, Job::Running);
    sys.answer("sc query", Output::code(1060));
    assert!(matches!(b.register(), Err(RegisterError::NotAsWritten(_))));
    assert!(matches!(
        b.replace_in_one_step(),
        Some(Err(RegisterError::NotAsWritten(_)))
    ));
}

#[test]
fn a_run_value_that_reads_back_missing_is_not_as_written_and_a_present_one_is_returned() {
    let sys = FakeSystem::new();
    sys.put_program(SVC_EXE);
    let mut b = WindowsRun::new(
        spec(Kind::UserAgent),
        Target::exe(SVC_EXE),
        Box::new(sys.clone()),
    );
    // `reg add` succeeds (no responder) but `reg query` finds nothing.
    sys.answer("reg query", Output::code(1));
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => {
            assert_eq!(look.registration, Registration::Absent)
        }
        other => panic!("{other:?}"),
    }
    sys.answer(
        "reg query",
        Output::ok(&format!("    Dyno    REG_SZ    {}\r\n", b.definition())),
    );
    let look = b.register().unwrap();
    assert_eq!(look.registration, Registration::Registered);
    assert_eq!(look.definition_current, Some(true));
    assert_eq!(look, b.inspect());
}

#[test]
fn a_run_value_the_user_switched_off_in_task_manager_is_their_choice_not_a_failed_write() {
    let sys = FakeSystem::new();
    sys.put_program(SVC_EXE);
    let mut b = WindowsRun::new(
        spec(Kind::UserAgent),
        Target::exe(SVC_EXE),
        Box::new(sys.clone()),
    );
    let value = b.definition();
    sys.on("reg query", move |args, _| {
        Ok(if args.iter().any(|a| a.contains("StartupApproved")) {
            Output::ok("    Dyno    REG_BINARY    030000000000000000000000\r\n")
        } else {
            Output::ok(&format!("    Dyno    REG_SZ    {value}\r\n"))
        })
    });
    assert_eq!(
        b.register().unwrap().registration,
        Registration::SwitchedOff
    );
}

// -------------------------------------------------------------------------------------------------------- launchd

const PLIST: &str = "/Users/ana/Library/LaunchAgents/net.decryptology.dyno.helper.plist";
const TARGET: &str = "gui/501/net.decryptology.dyno.helper";

fn launchd_print(state: &str, runs: u32, tail: &str) -> Output {
    Output::ok(&format!(
        "{TARGET} = {{\n\tstate = {state}\n\tprogram = /dev/dyno-helper\n\truns = {runs}\n{tail}\tproperties = keepalive | runatload\n}}\n"
    ))
}

fn launchd_file(sys: &FakeSystem) -> LaunchdFile {
    sys.put_program("/dev/dyno-helper");
    LaunchdFile::new(
        spec(Kind::UserAgent),
        501,
        PLIST.into(),
        Target::exe("/dev/dyno-helper"),
        Box::new(sys.clone()),
    )
}

#[test]
fn launchd_reads_the_runs_and_the_last_exit_code_of_a_job_that_died() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.put_file(PLIST, &b.definition());
    sys.answer(
        "/bin/launchctl print gui",
        launchd_print("waiting", 3, "\tlast exit code = 1\n"),
    );
    let look = b.inspect();
    assert_eq!(look.job, Job::Failed(LastExit::Code(1)));
    assert_eq!(
        (look.last_exit, look.restarts),
        (Some(LastExit::Code(1)), Some(2))
    );
}

#[test]
fn launchd_reads_the_terminating_signal() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.put_file(PLIST, &b.definition());
    sys.answer(
        "/bin/launchctl print gui",
        launchd_print(
            "waiting",
            2,
            "\tlast terminating signal = Segmentation fault: 11\n",
        ),
    );
    assert_eq!(b.inspect().job, Job::Failed(LastExit::Signal(11)));
}

#[test]
fn a_launchd_job_that_never_exited_has_no_last_exit_and_no_restarts() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.put_file(PLIST, &b.definition());
    sys.answer(
        "/bin/launchctl print gui",
        launchd_print("running", 1, "\tlast exit code = (never exited)\n"),
    );
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit, look.restarts),
        (Job::Running, None, Some(0))
    );
}

#[test]
fn a_launchd_job_that_exited_cleanly_keeps_the_zero_as_its_last_exit() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.put_file(PLIST, &b.definition());
    sys.answer(
        "/bin/launchctl print gui",
        launchd_print("waiting", 1, "\tlast exit code = 0\n"),
    );
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit),
        (Job::NotRunning, Some(LastExit::Code(0)))
    );
}

#[test]
fn a_launchd_file_names_the_plist_as_the_definition_in_force() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.put_file(PLIST, &b.definition());
    sys.answer("/bin/launchctl print gui", launchd_print("running", 1, ""));
    sys.answer(
        "/bin/launchctl print-disabled",
        Output::ok(&format!(
            "disabled services = {{\n\t\"{LABEL}\" => enabled\n}}\n"
        )),
    );
    assert_eq!(
        b.inspect().unit,
        Some(UnitFile {
            path: PathBuf::from(PLIST),
            origin: UnitOrigin::Admin,
            enabled: UnitFileState::Enabled
        })
    );
    sys.answer(
        "/bin/launchctl print-disabled",
        Output::ok(&format!(
            "disabled services = {{\n\t\"{LABEL}\" => disabled\n}}\n"
        )),
    );
    assert_eq!(b.inspect().unit.unwrap().enabled, UnitFileState::Disabled);
}

#[test]
fn a_launchd_file_register_returns_the_read_back_and_equals_a_fresh_inspection() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.answer("/bin/launchctl print gui", launchd_print("running", 1, ""));
    let look = b.register().unwrap();
    assert_eq!(
        (look.registration.clone(), look.job),
        (Registration::Registered, Job::Running)
    );
    assert_eq!(look, b.inspect());
}

#[test]
fn a_launchd_file_whose_plist_reads_back_another_text_is_not_as_written() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.rewrite_after(
        "/bin/launchctl bootstrap",
        PLIST,
        "something else rewrote it",
    );
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => assert_eq!(look.definition_current, Some(false)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bundled_agent_that_reads_back_not_registered_is_not_as_written_and_approval_is_its_own_error()
{
    let sys = FakeSystem::new();
    let app = FakeAppService::new(&sys);
    app.set_status(AppServiceStatus::NotRegistered);
    let mut b = LaunchdBundled::new(
        spec(Kind::UserAgent),
        501,
        Box::new(app.clone()),
        Box::new(sys.clone()),
    );
    // `register` reports success, but Background Task Management still has nothing.
    app.set_register_sets(Some(AppServiceStatus::NotRegistered));
    match b.register() {
        Err(RegisterError::NotAsWritten(look)) => {
            assert_eq!(look.registration, Registration::Absent)
        }
        other => panic!("{other:?}"),
    }
    app.set_register_sets(Some(AppServiceStatus::RequiresApproval));
    assert_eq!(b.register().unwrap_err(), RegisterError::NeedsApproval);
    app.set_register_sets(Some(AppServiceStatus::Enabled));
    sys.answer("/bin/launchctl print gui", launchd_print("running", 1, ""));
    let look = b.register().unwrap();
    assert_eq!(look, b.inspect());
}

#[test]
fn an_inspection_of_nothing_has_no_exit_restarts_or_unit() {
    let look = Inspection::absent();
    assert_eq!(
        (look.last_exit, look.restarts, look.unit),
        (None, None, None)
    );
}

#[test]
fn a_last_exit_knows_whether_it_failed_and_has_words() {
    assert!(!LastExit::Code(0).is_failure() && LastExit::Code(1).is_failure());
    assert!(LastExit::Signal(9).is_failure() && LastExit::Signal(11).is_failure());
    for clean in [1, 2, 13, 15] {
        assert!(
            !LastExit::Signal(clean).is_failure(),
            "SIGHUP, SIGINT, SIGPIPE and SIGTERM are how a service is asked to stop"
        );
    }
    assert!(!LastExit::Win32(0).is_failure() && LastExit::Win32(1067).is_failure());
    assert!(
        !LastExit::ServiceSpecific(0).is_failure() && LastExit::ServiceSpecific(7).is_failure()
    );
    assert_eq!(
        LastExit::ServiceSpecific(7).to_string(),
        "service-specific exit code 7"
    );
    assert_eq!(LastExit::Code(3).to_string(), "exit code 3");
    assert_eq!(LastExit::Signal(11).to_string(), "signal 11");
    assert_eq!(LastExit::Win32(1067).to_string(), "Win32 error 1067");
}

#[test]
fn a_not_as_written_error_says_what_the_os_holds_and_can_be_tried_again() {
    let mut look = Inspection::absent();
    look.registration = Registration::SwitchedOff;
    look.definition_current = Some(false);
    let e = RegisterError::NotAsWritten(look);
    assert!(e.retryable());
    let text = e.to_string();
    assert!(
        text.contains("SwitchedOff") && text.contains("another definition"),
        "{text}"
    );
}

#[test]
fn systemd_origins_follow_the_folder() {
    use std::path::Path;
    assert_eq!(
        UnitOrigin::of_systemd_path(Path::new("/usr/lib/systemd/system/a.service")),
        UnitOrigin::Packaged
    );
    assert_eq!(
        UnitOrigin::of_systemd_path(Path::new("/lib/systemd/system/a.service")),
        UnitOrigin::Packaged
    );
    assert_eq!(
        UnitOrigin::of_systemd_path(Path::new("/run/systemd/system/a.service")),
        UnitOrigin::Runtime
    );
    assert_eq!(
        UnitOrigin::of_systemd_path(Path::new("/etc/systemd/system/a.service")),
        UnitOrigin::Admin
    );
    assert_eq!(
        UnitOrigin::of_systemd_path(Path::new("/home/a/.config/systemd/user/a.service")),
        UnitOrigin::Admin
    );
}

#[test]
fn the_service_registers_its_event_source_before_it_is_created_and_removes_it_with_the_service() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    sys.answer("sc qc", sc_qc("2  AUTO_START", SVC_EXE));
    b.register().unwrap();
    let commands = sys.commands();
    let key = r"HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application\Dyno";
    let source = commands
        .iter()
        .position(|c| c.starts_with(&format!("reg add {key} /v EventMessageFile")));
    let create = commands.iter().position(|c| c.starts_with("sc create"));
    assert!(source.is_some() && source < create, "{commands:?}");
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with(&format!("reg add {key} /v TypesSupported")))
    );
    sys.clear_calls();
    sys.answer("sc query", Output::code(1060));
    b.unregister().unwrap();
    assert!(
        sys.commands().contains(&format!("reg delete {key} /f")),
        "{:?}",
        sys.commands()
    );
}

#[test]
fn an_event_source_that_cannot_be_registered_does_not_stop_the_service_from_being_registered() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("reg add", Output::code(5));
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    sys.answer("sc qc", sc_qc("2  AUTO_START", SVC_EXE));
    assert_eq!(b.register().unwrap().job, Job::Running);
}

#[test]
fn an_event_source_that_cannot_be_registered_or_removed_because_reg_hangs_is_only_logged() {
    use crate::port::RunError;
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.on("reg ", |_, _| Err(RunError::TimedOut("reg".into())));
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    sys.answer("sc qc", sc_qc("2  AUTO_START", SVC_EXE));
    assert_eq!(b.register().unwrap().job, Job::Running);
    sys.answer("sc query", Output::code(1060));
    b.unregister().unwrap();
    // A refusal on removal (not "no such key") is logged and does not fail the removal either.
    sys.on("reg ", |_, _| Ok(Output::code(5)));
    b.unregister().unwrap();
}

#[test]
fn a_systemd_manager_that_hangs_on_show_leaves_the_new_fields_empty() {
    use crate::port::RunError;
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.put_file(ETC_UNIT, &b.definition());
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    sys.on("systemctl show", |_, _| {
        Err(RunError::TimedOut("systemctl".into()))
    });
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit, look.restarts, look.unit),
        (Job::Running, None, None, None)
    );
}

#[test]
fn a_systemd_unit_stopped_by_systemctl_stop_with_sigterm_is_not_running_not_failed() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.put_file(ETC_UNIT, &b.definition());
    // `systemctl stop` of a unit that does not catch SIGTERM: the main process ended by signal 15, and systemd says success.
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "enabled", "success", 0, 15, 2),
    );
    let look = b.inspect();
    assert_eq!(look.job, Job::NotRunning);
    assert_eq!(
        look.last_exit,
        Some(LastExit::Signal(15)),
        "the detail stays"
    );
}

#[test]
fn a_systemd_result_other_than_success_is_a_failure_and_a_success_result_never_is() {
    let sys = FakeSystem::new();
    let mut b = system_unit(&sys);
    sys.put_file(ETC_UNIT, &b.definition());
    sys.answer("systemctl is-active", Output::ok("failed\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    // Killed from outside by SIGKILL: Result=signal, a real failure.
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "enabled", "signal", 0, 9, 2),
    );
    assert_eq!(b.inspect().job, Job::Failed(LastExit::Signal(9)));
    // A non-zero exit that the unit's SuccessExitStatus accepts is Result=success: not a failure.
    sys.answer(
        "systemctl show",
        show(ETC_UNIT, "enabled", "success", 0, 3, 1),
    );
    assert_eq!(b.inspect().job, Job::NotRunning);
}

#[test]
fn a_launchd_job_stopped_by_sigterm_is_not_running_not_failed() {
    let sys = FakeSystem::new();
    let mut b = launchd_file(&sys);
    sys.put_file(PLIST, &b.definition());
    sys.answer(
        "/bin/launchctl print gui",
        launchd_print("waiting", 2, "\tlast terminating signal = Terminated: 15\n"),
    );
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit),
        (Job::NotRunning, Some(LastExit::Signal(15)))
    );
}

#[test]
fn a_service_that_has_never_started_since_boot_has_no_last_exit_and_is_not_a_failure() {
    // ERROR_SERVICE_NEVER_STARTED (1077): demand or delayed-auto start not yet begun, or just created. The "press Repair"
    // bug of docs/lessons.md ("A delayed service looked like a broken one") came from reading this as damage.
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc qc", sc_qc("2  AUTO_START  (DELAYED)", SVC_EXE));
    sys.answer(
        "sc query",
        sc_query("1  STOPPED", "1077 (0x435)", "0  (0x0)"),
    );
    let look = b.inspect();
    assert_eq!((look.job, look.last_exit), (Job::NotRunning, None));
}

#[test]
fn register_reads_back_a_service_that_never_started_as_not_running() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    sys.answer("sc qc", sc_qc("2  AUTO_START  (DELAYED)", SVC_EXE));
    sys.answer(
        "sc query",
        sc_query("1  STOPPED", "1077 (0x435)", "0  (0x0)"),
    );
    let look = b.register().unwrap();
    assert_eq!(look.job, Job::NotRunning);
}

#[test]
fn the_event_source_stays_while_the_service_is_marked_for_deletion_and_still_running() {
    let sys = FakeSystem::new();
    let mut b = scm(&sys);
    let key = r"HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application\Dyno";
    // `sc delete` says "marked for deletion" (1072): a handle is open, the process still writes to its log.
    sys.answer("sc delete", Output::code(1072));
    sys.answer("sc query", sc_query("4  RUNNING", "0", "0"));
    sys.answer("sc qc", sc_qc("2  AUTO_START", SVC_EXE));
    b.unregister().unwrap();
    assert!(
        !sys.commands().contains(&format!("reg delete {key} /f")),
        "{:?}",
        sys.commands()
    );
    // Once the SCM no longer knows it, the source goes.
    sys.answer("sc delete", Output::ok(""));
    sys.answer("sc query", Output::code(1060));
    b.unregister().unwrap();
    assert!(
        sys.commands().contains(&format!("reg delete {key} /f")),
        "{:?}",
        sys.commands()
    );
}

// ---- ADR 0056 N-settle: the process the job runs, by identity (rule 53) ----

#[test]
fn launchd_names_the_process_of_the_job_by_identity() {
    let me = paddock_proc::ProcessId::of(std::process::id()).expect("own process");
    let sys = FakeSystem::new();
    sys.put_process(me);
    let mut b = launchd_file(&sys);
    sys.put_file(PLIST, &b.definition());
    sys.answer(
        "/bin/launchctl print gui",
        launchd_print("running", 1, &format!("\tpid = {}\n", me.pid())),
    );
    assert_eq!(b.job_process(), Some(me));
    sys.answer("/bin/launchctl print gui", launchd_print("waiting", 1, ""));
    assert_eq!(b.job_process(), None, "no pid line: no process");
}

#[test]
fn systemd_names_the_main_process_of_the_unit_by_identity() {
    let me = paddock_proc::ProcessId::of(std::process::id()).expect("own process");
    let sys = FakeSystem::new();
    sys.put_process(me);
    let mut b = system_unit(&sys);
    sys.answer(
        "systemctl show",
        Output::ok(&format!("MainPID={}\n", me.pid())),
    );
    assert_eq!(b.job_process(), Some(me));
    sys.answer("systemctl show", Output::ok("MainPID=0\n"));
    assert_eq!(b.job_process(), None);
}

#[test]
fn the_bundled_agent_names_the_process_of_the_job_by_identity() {
    let me = paddock_proc::ProcessId::of(std::process::id()).expect("own process");
    let sys = FakeSystem::new();
    sys.put_process(me);
    let app = FakeAppService::new(&sys);
    let mut b = LaunchdBundled::new(
        spec(Kind::UserAgent),
        501,
        Box::new(app),
        Box::new(sys.clone()),
    );
    sys.answer(
        "/bin/launchctl print gui",
        launchd_print("running", 1, &format!("\tpid = {}\n", me.pid())),
    );
    assert_eq!(b.job_process(), Some(me));
}

#[test]
fn a_bundled_agent_names_the_program_it_was_registered_with_and_whether_it_is_this_copy() {
    let sys = FakeSystem::new();
    let app = FakeAppService::new(&sys);
    app.set_status(AppServiceStatus::Enabled);
    let mut b = LaunchdBundled::new(
        spec(Kind::UserAgent),
        501,
        Box::new(app.clone()),
        Box::new(sys.clone()),
    );
    sys.answer("/bin/launchctl print gui", launchd_print("running", 1, ""));
    b.know_copy(
        Some("/Applications/A.app/Contents/MacOS/a"),
        Some("/Applications/A.app/Contents/MacOS/a"),
    );
    sys.put_program("/Applications/A.app/Contents/MacOS/a");
    let look = b.inspect();
    assert_eq!(
        look.program.as_deref(),
        Some("/Applications/A.app/Contents/MacOS/a")
    );
    assert_eq!(
        (look.program_exists, look.points_here),
        (Some(true), Some(true))
    );
    b.know_copy(
        Some("/Applications/A.app/Contents/MacOS/a"),
        Some("/Users/u/B.app/Contents/MacOS/b"),
    );
    let other = b.inspect();
    assert_eq!(
        other.program.as_deref(),
        Some("/Users/u/B.app/Contents/MacOS/b")
    );
    assert_eq!(
        (other.program_exists, other.points_here),
        (Some(false), Some(false))
    );
}
