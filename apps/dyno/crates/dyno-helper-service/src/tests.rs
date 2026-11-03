#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Dyno's wiring of the helper install over the recording fakes: nothing is installed (ER-14).

use super::*;
use paddock_env::MapVars;
use paddock_service::privileged::{Assessment, UpdateReason};
use paddock_service::testing::{FakeElevator, FakeFiles, FakeSystem};
use paddock_service::Output;

fn roots() -> Roots {
    windows_roots(&MapVars::new())
}

#[test]
fn the_spec_is_valid_and_carries_the_adrs_hardening() {
    let s = spec();
    assert_eq!((s.label(), s.name(), s.program()), (LABEL, SERVICE_NAME, PROGRAM));
    assert_eq!(s.unit_file_name(), "dyno-helper.service");
    assert_eq!(s.kind(), Kind::SystemService);
    let h = s.hardening().unwrap();
    assert_eq!(h.privileges(), ["SeChangeNotifyPrivilege"]);
    assert!(h.users_start());
    let unit = paddock_service::render::systemd_unit(&s, "/usr/bin/dyno-helper", None);
    for key in [
        "ProtectSystem=strict",
        "IPAddressDeny=any",
        "NoNewPrivileges=yes",
        "RestrictAddressFamilies=AF_UNIX AF_NETLINK",
        "StateDirectory=dyno-helper",
        "RuntimeDirectory=dyno-helper",
        "ExecStart=\"/usr/bin/dyno-helper\" \"serve\"",
        "Restart=on-failure",
    ] {
        assert!(unit.contains(key), "{key}\n{unit}");
    }
    assert!(!unit.contains("network-online"), "a helper with no network waits for none");
    let sc = paddock_service::render::sc_commands(&paddock_service::render::scm_config(&s, "C:\\p\\dyno-helper.exe"));
    assert!(sc.iter().any(|c| c[..3] == ["privs", "DynoHelper", "SeChangeNotifyPrivilege"]));
}

#[test]
fn program_files_comes_from_the_environment() {
    assert_eq!(roots().program_files, PathBuf::from("C:\\Program Files"));
    let vars = MapVars::new().with("ProgramFiles", "D:\\Programs");
    assert_eq!(windows_roots(&vars).program_files, PathBuf::from("D:\\Programs"));
    assert_eq!(
        windows_roots(&MapVars::new().with("ProgramFiles", "")).program_files,
        PathBuf::from("C:\\Program Files")
    );
}

#[test]
fn every_prompt_is_announced_in_the_words_the_user_will_see() {
    assert!(prompt_notice(Os::MacOs).contains("osascript"));
    assert!(prompt_notice(Os::Windows).contains("dyno-helper.exe"));
    assert!(prompt_notice(Os::Linux).contains("/usr/bin/dyno-helper"));
}

#[test]
fn turning_on_and_off_asks_once_through_the_os_prompt() {
    let owner = HelperUser::Uid(501);
    let (log, mut elevator) = (FakeSystem::new(), FakeElevator::new(&FakeSystem::new()));
    let mut sys = log.clone();
    let helper = Path::new("/Applications/Dyno.app/Contents/MacOS/dyno-helper");
    turn_on(Os::MacOs, helper, &owner, &mut sys, &mut elevator).unwrap();
    let commands = log.commands();
    assert_eq!(commands.len(), 1);
    assert!(
        commands[0]
            .starts_with("/usr/bin/osascript -e do shell script \"'/Applications/Dyno.app/Contents/MacOS/dyno-helper' 'install' '--for' 'uid:501' 2>&1\""),
        "{}",
        commands[0]
    );
    assert!(commands[0].ends_with("with administrator privileges"));
    log.clear_calls();
    turn_off(Os::Linux, Path::new("/usr/bin/dyno-helper"), &mut sys, &mut elevator).unwrap();
    assert_eq!(log.commands(), ["pkexec /usr/bin/dyno-helper uninstall"]);
    let windows_log = FakeSystem::new();
    let mut elevator = FakeElevator::new(&windows_log);
    turn_on(Os::Windows, Path::new("C:\\Dyno\\dyno-helper.exe"), &owner, &mut sys, &mut elevator).unwrap();
    assert!(windows_log
        .calls()
        .iter()
        .any(|c| matches!(c, paddock_service::testing::Call::Os(s) if s == "elevated C:\\Dyno\\dyno-helper.exe install --for uid:501")));
    elevator.results([Err(paddock_service::RegisterError::Declined)]);
    assert_eq!(
        turn_on(Os::Windows, Path::new("C:\\x.exe"), &owner, &mut sys, &mut elevator),
        Err(ElevationError::Declined)
    );
}

#[test]
fn only_a_protocol_or_minimum_version_change_asks_the_user() {
    let here = env!("CARGO_PKG_VERSION");
    assert_eq!(assess(None), Assessment::NotInstalled);
    assert_eq!(assess(Some(("1.1.0", HELPER_API))), Assessment::Ready { newer_available: false });
    assert_eq!(
        assess(Some(("1.1.0", HELPER_API + 1))),
        Assessment::UpdateRequired(UpdateReason::Protocol {
            installed: HELPER_API + 1,
            bundled: HELPER_API
        })
    );
    assert!(matches!(
        assess(Some(("1.0.0", HELPER_API))),
        Assessment::UpdateRequired(UpdateReason::BelowMinimum { .. })
    ));
    // A helper at this build's version is current whatever else changed.
    let ready = assess(Some((here, HELPER_API)));
    assert!(matches!(
        ready,
        Assessment::Ready { .. } | Assessment::UpdateRequired(UpdateReason::BelowMinimum { .. })
    ));
}

fn make_system(sys: &FakeSystem) -> impl Fn() -> Box<dyn System> + '_ {
    move || Box::new(sys.clone())
}

#[test]
fn the_helper_installs_and_removes_itself_on_linux_from_the_package() {
    let (sys, mut files) = (FakeSystem::new(), FakeFiles::new());
    files.put_file("/usr/bin/dyno-helper", b"deb", 0, 0o755);
    sys.put_file(
        "/usr/lib/systemd/system/dyno-helper.service",
        "[Service]\nExecStart=\"/usr/bin/dyno-helper\" \"serve\"\n",
    );
    sys.answer("systemctl is-active", Output::ok("active\n"));
    let report = install_in_place(
        Os::Linux,
        &roots(),
        Path::new("/usr/bin/dyno-helper"),
        &mut files,
        &make_system(&sys),
    )
    .unwrap();
    assert!(!report.replaced_existing || report.phases.len() >= 3);
    assert!(
        sys.commands().contains(&"systemctl enable --now dyno-helper.service".to_string()),
        "{:?}",
        sys.commands()
    );
    sys.answer(
        "systemctl is-enabled",
        Output {
            code: Some(1),
            stdout: "disabled\n".into(),
            ..Output::default()
        },
    );
    sys.answer(
        "systemctl is-active",
        Output {
            code: Some(3),
            stdout: "inactive\n".into(),
            ..Output::default()
        },
    );
    uninstall_in_place(Os::Linux, &roots(), &mut files, &make_system(&sys)).unwrap();
    assert!(sys.commands().contains(&"systemctl disable --now dyno-helper.service".to_string()));
}

#[test]
fn the_helper_refuses_to_install_where_it_is_not_protected() {
    let (sys, mut files) = (FakeSystem::new(), FakeFiles::new());
    files.put_file("/Applications/Dyno.app/Contents/MacOS/dyno-helper", b"x", 501, 0o755);
    files.put_dir("/Library/PrivilegedHelperTools", 501, 0o755);
    let err = install_in_place(
        Os::MacOs,
        &roots(),
        Path::new("/Applications/Dyno.app/Contents/MacOS/dyno-helper"),
        &mut files,
        &make_system(&sys),
    )
    .unwrap_err();
    assert!(matches!(err, InstallError::Refused(_)), "{err}");
    assert!(!sys.commands().iter().any(|c| c.contains("bootstrap")), "nothing was registered");
}

#[test]
fn the_helper_removes_itself_files_first_and_stops_last() {
    let (sys, mut files) = (FakeSystem::new(), FakeFiles::new());
    let program = "/Library/PrivilegedHelperTools/net.decryptology.dyno.helper";
    files
        .put_dir("/Library/PrivilegedHelperTools", 0, 0o755)
        .put_file(program, b"x", 0, 0o755);
    let report = uninstall_self_in_place(Os::MacOs, &roots(), &mut files, &make_system(&sys)).unwrap();
    assert_eq!(report.phases.len(), 3);
    assert!(files.facts_of(Path::new(program)).is_none());
    let commands = sys.commands();
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("/bin/launchctl bootout system/net.decryptology.dyno.helper")),
        "{commands:?}"
    );
}
