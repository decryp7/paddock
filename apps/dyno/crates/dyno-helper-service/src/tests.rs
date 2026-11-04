#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Dyno's wiring of the helper install over the recording fakes: nothing is installed (ER-14).

use super::*;
use paddock_env::MapVars;
use paddock_service::privileged::{Assessment, UpdateReason};
use paddock_service::testing::{FakeElevator, FakeFiles, FakeSystem};
#[cfg(unix)]
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
    assert_eq!(
        s.windows_start(),
        paddock_service::WindowsStart::Auto,
        "the helper starts with the boot, not a delayed minute or two later"
    );
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

// A Linux install modelled with Linux paths (`/usr/lib/systemd/system/...`): on Windows `Path::join` and `Path::new` spell them with backslashes, which
// the fake system's string keys do not match, and the code under test only ever runs on Linux. Unix hosts run it.
#[cfg(unix)]
#[test]
fn the_helper_installs_and_removes_itself_on_linux_from_the_package() {
    let (sys, mut files) = (FakeSystem::new(), FakeFiles::new());
    files.put_file("/usr/bin/dyno-helper", b"deb", 0, 0o755);
    sys.put_file(
        "/usr/lib/systemd/system/dyno-helper.service",
        "[Service]\nExecStart=\"/usr/bin/dyno-helper\" \"serve\"\n",
    );
    // A stateful systemd: the unit runs and is enabled until `disable --now`, and again after `enable --now`.
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = running.clone();
    sys.on("systemctl is-active", move |_, _| {
        Ok(Output::ok(if flag.load(std::sync::atomic::Ordering::SeqCst) {
            "active\n"
        } else {
            "inactive\n"
        }))
    });
    let flag = running.clone();
    sys.on("systemctl is-enabled", move |_, _| {
        Ok(Output::ok(if flag.load(std::sync::atomic::Ordering::SeqCst) {
            "enabled\n"
        } else {
            "disabled\n"
        }))
    });
    let flag = running.clone();
    sys.on("systemctl enable --now", move |_, _| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Output::code(0))
    });
    let flag = running.clone();
    sys.on("systemctl disable --now", move |_, _| {
        flag.store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(Output::code(0))
    });
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
    // The program, its folder, the stop and the read-back that the registration is gone (ADR 0054 RC-2).
    assert_eq!(report.phases.len(), 4);
    assert!(files.facts_of(Path::new(program)).is_none());
    let commands = sys.commands();
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("/bin/launchctl bootout system/net.decryptology.dyno.helper")),
        "{commands:?}"
    );
}

// ---- the Linux package files (ADR 0027 section 1, WP-13): shipped as text, checked here against what the spec says ----

const PACKAGE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../app/src-tauri/linux");

fn package_file(name: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(PACKAGE_DIR).join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn the_shipped_unit_is_exactly_what_the_spec_renders_and_is_not_enabled_by_the_package() {
    let rendered = paddock_service::render::systemd_unit(&spec(), "/usr/bin/dyno-helper", None);
    if std::env::var_os("DYNO_UPDATE_PACKAGE_UNIT").is_some() {
        std::fs::write(std::path::Path::new(PACKAGE_DIR).join("dyno-helper.service"), &rendered).unwrap();
    }
    assert_eq!(
        package_file("dyno-helper.service"),
        rendered,
        "run with DYNO_UPDATE_PACKAGE_UNIT=1 to regenerate the unit from the spec"
    );
    // Shipped disabled: the package never enables it (the user's click does, through `dyno-helper install`).
    for script in ["postinst", "prerm", "postrm"] {
        assert!(
            !package_file(script).contains("systemctl enable") && !package_file(script).contains("enable --now"),
            "{script} must not enable the helper"
        );
    }
}

#[test]
fn the_polkit_rule_allows_starting_the_helpers_unit_from_a_local_active_session_and_nothing_else() {
    let rule = package_file("50-dyno-helper.rules");
    let code: String = rule
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for must in [
        r#"action.id == "org.freedesktop.systemd1.manage-units""#,
        r#"action.lookup("unit") == "dyno-helper.service""#,
        r#"action.lookup("verb") == "start""#,
        "subject.local && subject.active",
        "polkit.Result.YES",
    ] {
        assert!(code.contains(must), "{must}\n{code}");
    }
    assert_eq!(code.matches("polkit.addRule").count(), 1);
    assert_eq!(code.matches("polkit.Result.").count(), 1, "one outcome, and it is the narrow one");
    assert!(!code.contains("Result.YES;\n  }\n  return"), "no fall-through grant");
}

#[test]
fn the_maintainer_scripts_are_posix_sh_skip_without_systemd_and_touch_only_the_helpers_own_things() {
    for script in ["postinst", "prerm", "postrm"] {
        let text = package_file(script);
        assert!(text.starts_with("#!/bin/sh\n"), "{script}");
        assert!(text.contains("\nset -e\n"), "{script}");
        assert!(text.contains("/run/systemd/system"), "{script} skips where systemd is not running");
        assert!(text.trim_end().ends_with("exit 0"), "{script} succeeds");
        assert!(!text.contains("bash") && !text.contains("[["), "{script} is POSIX sh");
        for line in text.lines().filter(|l| l.contains("systemctl")) {
            assert!(
                line.contains("dyno-helper.service") || line.contains("daemon-reload"),
                "{script}: {line}"
            );
        }
    }
    let prerm = package_file("prerm");
    assert!(prerm.contains("remove | deconfigure)") && prerm.contains("disable --now dyno-helper.service"));
    assert!(prerm.contains("upgrade)") && prerm.contains("stop dyno-helper.service"));
    let postrm = package_file("postrm");
    assert!(postrm.contains("purge)") && postrm.contains("rm -r /var/lib/dyno-helper"));
    assert_eq!(postrm.matches("rm ").count(), 1, "one removal, of one fixed path, only on purge");
    assert!(
        package_file("postinst").contains("try-restart dyno-helper.service"),
        "an upgrade restarts it only when it was enabled"
    );
}

#[test]
fn the_windows_uninstaller_removes_the_service_with_the_bundled_copy_only_when_not_updating() {
    let hooks = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../app/src-tauri/windows/installer-hooks.nsh"
    ))
    .unwrap();
    let pre_uninstall = hooks
        .split("!macro NSIS_HOOK_PREUNINSTALL")
        .nth(1)
        .expect("the pre-uninstall hook")
        .split("!macroend")
        .next()
        .unwrap();
    assert!(
        pre_uninstall.contains(r"SYSTEM\CurrentControlSet\Services\DynoHelper"),
        "only when the service exists"
    );
    assert!(
        pre_uninstall.contains(r#"nsExec::ExecToLog '"$INSTDIR\dyno-helper.exe" uninstall'"#),
        "the bundled copy removes the installed one (a running program cannot delete itself)"
    );
    assert_eq!(pre_uninstall.matches("runas").count(), 0, "the uninstaller is elevated already: no second prompt");
    let guard = pre_uninstall.find("$UpdateMode <> 1").expect("only when not updating (a manual upgrade keeps the helper)");
    assert!(guard < pre_uninstall.find("dyno-helper.exe").unwrap(), "the update guard comes first");
    assert!(!pre_uninstall.contains("service uninstall"), "the agent has no Windows registration of its own");
}
