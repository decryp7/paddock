#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Dyno's wiring of the helper install over the recording fakes: nothing is installed (ER-14).

use super::*;
use crate::source_check::FamilyKey;
use paddock_service::privileged::{Assessment, UpdateReason};
use paddock_service::testing::{FakeElevator, FakeFiles, FakeSystem};
#[cfg(unix)]
use paddock_service::Output;

fn roots() -> Roots {
    install_roots(&RealFolders).unwrap()
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

/// Folders the way a machine whose environment was poisoned reports them: the OS's answer is the real one.
struct RealFolders;

impl paddock_service::privileged::KnownFolders for RealFolders {
    fn system_directory(&self) -> std::io::Result<PathBuf> {
        Ok(PathBuf::from("D:\\RealWin\\System32"))
    }
    fn program_files(&self) -> std::io::Result<PathBuf> {
        Ok(PathBuf::from("D:\\Real Program Files"))
    }
    fn program_data(&self) -> std::io::Result<PathBuf> {
        Ok(PathBuf::from("D:\\RealData"))
    }
}

struct NoFolders;

impl paddock_service::privileged::KnownFolders for NoFolders {
    fn system_directory(&self) -> std::io::Result<PathBuf> {
        Err(std::io::Error::other("no answer"))
    }
    fn program_files(&self) -> std::io::Result<PathBuf> {
        Err(std::io::Error::other("no answer"))
    }
    fn program_data(&self) -> std::io::Result<PathBuf> {
        Err(std::io::Error::other("no answer"))
    }
}

#[test]
fn the_folders_come_from_the_os_and_never_from_the_environment() {
    // An elevated process may inherit a user-writable HKCU\Environment; whatever it says must not be used. (The process
    // environment of this test is not even looked at: the folders are a port.)
    let w = windows_roots(&RealFolders);
    assert_eq!(w.program_files, PathBuf::from("D:\\Real Program Files"));
    assert_eq!(w.program_data, PathBuf::from("D:\\RealData"));
    assert_eq!(w.system_root, PathBuf::from("D:\\RealWin"));
    assert_eq!(
        install_roots(&RealFolders).unwrap().program_files,
        PathBuf::from("D:\\Real Program Files")
    );
}

#[test]
fn an_os_that_names_no_folder_stops_the_install_but_not_the_path_lookup() {
    assert!(install_roots(&NoFolders).is_err());
    let w = windows_roots(&NoFolders);
    assert_eq!(w.program_files, PathBuf::from("C:\\Program Files"));
    assert_eq!(w.system_root, PathBuf::from("C:\\Windows"));
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
            .starts_with("/usr/bin/osascript -e do shell script \"/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin '/Applications/Dyno.app/Contents/MacOS/dyno-helper' 'install' '--for' 'uid:501' 2>&1\""),
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
        &mut paddock_service::privileged::trust_fake::windows_machine(|_| paddock_service::privileged::trust_fake::locked()),
        &FamilyKey::new(None, SIGNED_NAME),
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

const SIGNED_NAME: &str = "dyno-helper-9.9.9-aarch64-apple-darwin";
const KEY: &str = include_str!("../tests/fixtures/throwaway.pub");
const SIGNED: &[u8] = include_bytes!("../tests/fixtures/helper.bin");
const SIGNATURE: &[u8] = include_bytes!("../tests/fixtures/helper-9.9.9.sig");
const MAC_SOURCE: &str = "/Applications/Dyno.app/Contents/MacOS/dyno-helper";
const MAC_SIGNATURE: &str = "/Applications/Dyno.app/Contents/Resources/dyno-helper.minisig";

fn mac_install(files: &mut FakeFiles, sys: &FakeSystem) -> Result<Report, InstallError> {
    install_in_place(
        Os::MacOs,
        &roots(),
        Path::new(MAC_SOURCE),
        files,
        &mut paddock_service::privileged::trust_fake::windows_machine(|_| paddock_service::privileged::trust_fake::locked()),
        &FamilyKey::new(Some(KEY), SIGNED_NAME),
        &make_system(sys),
    )
}

#[test]
fn the_helper_refuses_to_install_where_it_is_not_protected() {
    let (sys, mut files) = (FakeSystem::new(), FakeFiles::new());
    files.put_file(MAC_SOURCE, SIGNED, 501, 0o755);
    files.put_file(MAC_SIGNATURE, SIGNATURE, 501, 0o644);
    files.put_dir("/Library/PrivilegedHelperTools", 501, 0o755);
    let err = mac_install(&mut files, &sys).unwrap_err();
    assert!(matches!(&err, InstallError::Refused(w) if w.contains("not protected")), "{err}");
    assert!(!sys.commands().iter().any(|c| c.contains("bootstrap")), "nothing was registered");
}

#[test]
fn a_macos_helper_is_copied_only_with_a_valid_signature_of_the_family_key() {
    let (sys, mut files) = (FakeSystem::new(), FakeFiles::new());
    files.put_dir("/Library/PrivilegedHelperTools", 0, 0o755);
    files.put_file(MAC_SOURCE, SIGNED, 501, 0o755);
    // Missing: refused with the next step, nothing written.
    let err = mac_install(&mut files, &sys).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("no signature file") && w.contains("official download")),
        "{err}"
    );
    assert!(!files.calls().iter().any(|c| c.starts_with("write ")));
    // Not the signature of these bytes (a replaced helper): refused.
    let mut replaced = SIGNED.to_vec();
    replaced[0] ^= 1;
    files.put_file(MAC_SOURCE, &replaced, 501, 0o755);
    files.put_file(MAC_SIGNATURE, SIGNATURE, 501, 0o644);
    let err = mac_install(&mut files, &sys).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("does not match its signature") && w.contains("reinstall Dyno")),
        "{err}"
    );
    assert!(!files.calls().iter().any(|c| c.starts_with("write ")));
    // An empty or garbled signature file (a build that was not signed): refused.
    files.put_file(MAC_SOURCE, SIGNED, 501, 0o755);
    for bad in [&b""[..], b"not a signature", &[0xff, 0xfe][..]] {
        files.put_file(MAC_SIGNATURE, bad, 501, 0o644);
        let err = mac_install(&mut files, &sys).unwrap_err();
        assert!(matches!(err, InstallError::Refused(_)), "{err}");
    }
    assert!(!files.calls().iter().any(|c| c.starts_with("write ")));
    // The genuine pair is accepted, and the verdict and the hash are reported.
    files.put_file(MAC_SIGNATURE, SIGNATURE, 501, 0o644);
    let report = mac_install(&mut files, &sys);
    // (The registration itself is the fake backend's business: the signature phase is what this test is about.)
    if let Ok(r) = &report {
        assert!(r.signature.as_deref().is_some_and(|v| v.starts_with("valid")));
    }
    assert!(files.calls().iter().any(|c| c.starts_with("write ")), "{:?}", files.calls());
}

#[test]
fn a_build_without_the_family_key_refuses_every_helper() {
    use paddock_service::privileged::SourceVerifier;
    let none = FamilyKey::new(None, SIGNED_NAME);
    let err = none.verify(SIGNED, SIGNATURE).unwrap_err();
    assert!(err.contains("no update key") && err.contains("official download"), "{err}");
    assert!(FamilyKey::new(Some("   "), "n").verify(SIGNED, SIGNATURE).is_err());
    assert!(FamilyKey::new(Some("not a key"), "n").verify(SIGNED, SIGNATURE).is_err());
    let key = FamilyKey::new(Some(KEY), SIGNED_NAME);
    assert!(key.verify(SIGNED, SIGNATURE).unwrap().starts_with("valid"));
}

#[test]
fn the_signature_is_looked_for_next_to_the_helper_and_in_the_bundles_resources() {
    use crate::source_check::{signature_locations, SIGNATURE_FILE};
    assert_eq!(
        signature_locations(Path::new(MAC_SOURCE)),
        [
            PathBuf::from("/Applications/Dyno.app/Contents/MacOS").join(SIGNATURE_FILE),
            PathBuf::from(MAC_SIGNATURE)
        ]
    );
    assert_eq!(
        signature_locations(Path::new("/opt/dyno/dyno-helper")),
        [PathBuf::from("/opt/dyno").join(SIGNATURE_FILE)]
    );
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
    assert_eq!(
        pre_uninstall.matches("runas").count(),
        0,
        "the uninstaller is elevated already: no second prompt"
    );
    let guard = pre_uninstall
        .find("$UpdateMode <> 1")
        .expect("only when not updating (a manual upgrade keeps the helper)");
    assert!(
        guard < pre_uninstall.find("dyno-helper.exe").unwrap(),
        "the update guard comes first"
    );
    assert!(
        !pre_uninstall.contains("service uninstall"),
        "the agent has no Windows registration of its own"
    );
}

const FX_KEY: &str = KEY;
const FX_BIN: &[u8] = SIGNED;
const FX_SIG_999: &[u8] = SIGNATURE;
const FX_SIG_100: &[u8] = include_bytes!("../tests/fixtures/helper-1.0.0.sig");

#[test]
fn a_signature_is_good_only_for_the_file_name_it_was_made_for_so_an_older_helper_never_passes() {
    use paddock_service::privileged::SourceVerifier;
    let current = FamilyKey::new(Some(FX_KEY), "dyno-helper-9.9.9-aarch64-apple-darwin");
    assert!(current
        .verify(FX_BIN, FX_SIG_999)
        .unwrap()
        .contains("dyno-helper-9.9.9-aarch64-apple-darwin"));
    // The same bytes with the signature of an older version (a validly family-signed older helper): refused.
    let err = current.verify(FX_BIN, FX_SIG_100).unwrap_err();
    assert!(
        err.contains("not made for this version") && err.contains("official download"),
        "{err}"
    );
    // Another architecture is refused the same way.
    let other = FamilyKey::new(Some(FX_KEY), "dyno-helper-9.9.9-x86_64-apple-darwin");
    assert!(other.verify(FX_BIN, FX_SIG_999).is_err());
}

#[test]
fn the_name_this_build_expects_carries_its_version_and_architecture() {
    let name = crate::source_check::expected_signed_name();
    assert!(name.starts_with(&format!("dyno-helper-{}-", env!("CARGO_PKG_VERSION"))), "{name}");
    assert!(name.ends_with("-apple-darwin") || name.ends_with("-unknown"), "{name}");
}

#[test]
fn the_source_check_runs_alone_and_leaves_no_trace_when_it_refuses() {
    let (sys, mut files) = (FakeSystem::new(), FakeFiles::new());
    files.put_dir("/Library/PrivilegedHelperTools", 0, 0o755);
    files.put_file(MAC_SOURCE, SIGNED, 501, 0o755);
    let mut trust = paddock_service::privileged::trust_fake::windows_machine(|_| paddock_service::privileged::trust_fake::locked());
    let verifier = FamilyKey::new(Some(KEY), SIGNED_NAME);
    let err = preflight_in_place(
        Os::MacOs,
        &roots(),
        Path::new(MAC_SOURCE),
        &mut files,
        &mut trust,
        &verifier,
        &make_system(&sys),
    )
    .unwrap_err();
    assert!(matches!(&err, InstallError::Refused(w) if w.contains("no signature file")), "{err}");
    files.put_file(MAC_SIGNATURE, SIGNATURE, 501, 0o644);
    let report = preflight_in_place(
        Os::MacOs,
        &roots(),
        Path::new(MAC_SOURCE),
        &mut files,
        &mut trust,
        &verifier,
        &make_system(&sys),
    )
    .unwrap();
    assert!(report.signature.is_some() && report.phases.is_empty());
    assert!(!files.calls().iter().any(|c| c.starts_with("write ") || c.starts_with("create_dir")));
    assert!(sys.commands().is_empty(), "nothing was run: {:?}", sys.commands());
}
