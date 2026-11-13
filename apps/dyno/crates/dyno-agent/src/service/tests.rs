use super::*;
use std::sync::{Arc, Mutex};

type Answer = (String, (bool, String));

#[derive(Clone, Default)]
struct Script {
    calls: Arc<Mutex<Vec<String>>>,
    answers: Arc<Mutex<Vec<Answer>>>,
}

impl Script {
    fn answer(&self, contains: &str, ok: bool, out: &str) {
        self.answers.lock().unwrap().push((contains.into(), (ok, out.into())));
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Runner for Script {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<(bool, String), String> {
        let line = format!("{program} {}", args.join(" "));
        self.calls.lock().unwrap().push(line.clone());
        let answers = self.answers.lock().unwrap();
        Ok(answers
            .iter()
            .find(|(k, _)| line.contains(k.as_str()))
            .map(|(_, a)| a.clone())
            .unwrap_or((true, String::new())))
    }
}

fn service(os: Os, home: &std::path::Path, s: &Script) -> Service {
    Service::with("/opt/dyno/dyno-agent".into(), home.to_path_buf(), 501, Box::new(s.clone()), os).with_pause(|_| {})
}

#[test]
fn macos_installs_by_bootstrapping_after_a_bootout_and_reports_status() {
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let mut svc = service(Os::Mac, dir.path(), &s);
    assert_eq!(svc.status(), Status::NotInstalled);
    svc.install().unwrap();
    let calls = s.calls();
    assert!(calls[0].contains("bootout gui/501/net.decryptology.dyno.agent"), "{calls:?}");
    assert!(calls[1].contains("print gui/501/net.decryptology.dyno.agent"), "{calls:?}");
    assert!(calls[2].contains("bootstrap gui/501"), "{calls:?}");
    assert!(dir.path().join("Library/LaunchAgents/net.decryptology.dyno.agent.plist").exists());
    s.answer("print gui/501", true, "state = running");
    assert_eq!(
        svc.status(),
        Status::Installed {
            running: true,
            current: true
        }
    );
    svc.uninstall().unwrap();
    assert_eq!(svc.status(), Status::NotInstalled);
    svc.uninstall().unwrap();
}

#[test]
fn macos_reregisters_when_launchd_cannot_run_the_installed_job() {
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let mut svc = service(Os::Mac, dir.path(), &s);
    svc.install().unwrap();
    s.answer("print gui/501", false, "");
    let before = s.calls().len();
    assert!(svc.ensure().unwrap(), "a loaded-but-dead job is registered again");
    assert!(s.calls()[before..].iter().any(|c| c.contains("bootstrap")));
    // A healthy job is left alone.
    s.answers.lock().unwrap().clear();
    s.answer("print gui/501", true, "state = running");
    let before = s.calls().len();
    assert!(!svc.ensure().unwrap());
    assert_eq!(s.calls().len(), before + 1, "only the status query ran");
}

#[test]
fn a_service_for_another_executable_is_registered_again_and_nothing_is_installed_unasked() {
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let mut old = Service::with(
        "/old/dyno-agent".into(),
        dir.path().to_path_buf(),
        501,
        Box::new(s.clone()),
        Os::Mac,
    )
    .with_pause(|_| {});
    old.install().unwrap();
    s.answer("print", true, "state = running");
    let mut new = service(Os::Mac, dir.path(), &s);
    assert_eq!(
        new.status(),
        Status::Installed {
            running: true,
            current: false
        }
    );
    assert!(new.ensure().unwrap());
    assert_eq!(
        new.status(),
        Status::Installed {
            running: true,
            current: true
        }
    );
    let empty = paddock_testkit::private_tempdir().unwrap();
    assert!(!service(Os::Mac, empty.path(), &s).ensure().unwrap());
}

#[test]
fn windows_uses_the_per_user_run_key() {
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let mut svc = Service::with(
        r"C:\Dyno\dyno-agent.exe".into(),
        dir.path().to_path_buf(),
        0,
        Box::new(s.clone()),
        Os::Windows,
    );
    svc.install().unwrap();
    assert!(s.calls()[0].starts_with(r"reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Dyno agent"));
    assert!(s.calls()[0].contains(r#""C:\Dyno\dyno-agent.exe" run"#));
    s.answer("reg query", true, r#"Dyno agent REG_SZ "C:\Dyno\dyno-agent.exe" run"#);
    assert_eq!(
        svc.status(),
        Status::Installed {
            running: true,
            current: true
        }
    );
    s.answers.lock().unwrap().clear();
    s.answer("reg query", false, "");
    assert_eq!(svc.status(), Status::NotInstalled);
    svc.uninstall().unwrap();
}

/// After an update the old job can linger after `bootout`; bootstrapping over it does nothing, so the install waits
/// until launchd has dropped it, and gives up waiting after a bounded time.
#[test]
fn macos_bootstraps_only_after_launchd_dropped_the_old_job_and_the_wait_is_bounded() {
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let waited = Arc::new(Mutex::new(Duration::ZERO));
    let w = waited.clone();
    // launchd reports the old job three more times, then it is gone.
    let lingering = Arc::new(Mutex::new(3u32));
    struct Lingering(Script, Arc<Mutex<u32>>);
    impl Runner for Lingering {
        fn run(&mut self, program: &str, args: &[&str]) -> Result<(bool, String), String> {
            let out = self.0.run(program, args)?;
            if args.first() == Some(&"print") {
                let mut n = self.1.lock().unwrap();
                if *n > 0 {
                    *n -= 1;
                    return Ok((true, "state = spawn scheduled\n".into()));
                }
                return Ok((false, String::new()));
            }
            Ok(out)
        }
    }
    let mut svc = Service::with(
        "/opt/dyno/dyno-agent".into(),
        dir.path().to_path_buf(),
        501,
        Box::new(Lingering(s.clone(), lingering.clone())),
        Os::Mac,
    )
    .with_pause(move |d| *w.lock().unwrap() += d);
    svc.install().unwrap();
    let calls = s.calls();
    let verbs: Vec<&str> = calls.iter().map(|c| c.split(' ').nth(1).unwrap_or("")).collect();
    assert_eq!(verbs, ["bootout", "print", "print", "print", "print", "bootstrap"], "{calls:?}");
    assert_eq!(*waited.lock().unwrap(), GONE_POLL * 3);

    // A job that never goes away: wait GONE_WAIT, then bootstrap anyway.
    *lingering.lock().unwrap() = u32::MAX;
    *waited.lock().unwrap() = Duration::ZERO;
    svc.install().unwrap();
    assert_eq!(*waited.lock().unwrap(), GONE_WAIT);
    assert!(s.calls().last().unwrap().contains("bootstrap"));
}

#[test]
fn launchd_supervises_the_agent_and_a_windows_run_entry_does_not() {
    // macOS: a bootstrapped launch agent (KeepAlive) is started and restarted by launchd.
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let mut mac = service(Os::Mac, dir.path(), &s);
    assert!(!mac.supervised());
    mac.install().unwrap();
    assert!(mac.supervised());
    mac.uninstall().unwrap();
    assert!(!mac.supervised());

    // Windows: the Run entry starts the agent at the next logon only; nothing restarts it now.
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    s.answer("query", true, "Dyno    REG_SZ    \"/opt/dyno/dyno-agent\" run");
    let mut win = service(Os::Windows, dir.path(), &s);
    assert!(matches!(win.status(), Status::Installed { .. }));
    assert!(!win.supervised());
}

#[test]
fn inside_a_mac_app_bundle_the_service_commands_are_refused_with_the_way_to_manage_it() {
    let inside = Path::new("/Applications/Dyno.app/Contents/MacOS/dyno-agent");
    let m = refuse_inside_app_bundle(Os::Mac, inside).unwrap_err();
    assert!(m.contains("Login Items") && m.contains("same label"), "{m}");
    // The same path on other systems, and a dyno-agent outside a bundle on a Mac, are not refused.
    assert!(refuse_inside_app_bundle(Os::Linux, inside).is_ok());
    // On Linux the unit is the shared manager's: `Service` writes nothing there.
    let dir = paddock_testkit::private_tempdir().unwrap();
    let mut linux = service(Os::Linux, dir.path(), &Script::default());
    assert!(linux.install().is_err() && linux.uninstall().is_err());
    assert_eq!(linux.status(), Status::NotInstalled);
    assert!(!linux.supervised());
    assert!(refuse_inside_app_bundle(Os::Windows, inside).is_ok());
    assert!(refuse_inside_app_bundle(Os::Mac, Path::new("/usr/local/bin/dyno-agent")).is_ok());
    assert!(
        refuse_inside_app_bundle(Os::Mac, Path::new("/Users/a/.app/dyno-agent")).is_ok(),
        "a folder named just .app"
    );
    for outside in [
        "/Users/a/tools.app/bin/dyno-agent",
        "/Users/a/Dyno.app/Contents/Resources/dyno-agent",
        "/Contents/MacOS/dyno-agent",
        "/Users/a/X.app/Foo/MacOS/dyno-agent",
    ] {
        assert!(refuse_inside_app_bundle(Os::Mac, Path::new(outside)).is_ok(), "{outside}");
    }
}

#[test]
fn the_agents_windows_entry_never_shares_the_apps_value_name() {
    assert_ne!(spec::WINDOWS_AGENT_VALUE, spec::WINDOWS_APP_VALUE);
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let mut svc = Service::with(
        r"C:\Dyno\dyno-agent.exe".into(),
        dir.path().to_path_buf(),
        0,
        Box::new(s.clone()),
        Os::Windows,
    );
    // The app's own entry is not the agent's: `ensure` (the app's repair) finds the agent not installed and writes nothing.
    s.answer("/v Dyno agent", false, "");
    s.answer("/v Dyno", true, r#"Dyno REG_SZ C:\Dyno\dyno-app.exe --hidden"#);
    assert_eq!(svc.status(), Status::NotInstalled);
    assert!(!svc.ensure().unwrap());
    assert_eq!(svc.clear_stale_run_entry(), Ok(false));
    assert!(
        s.calls().iter().all(|c| !c.contains("reg add") && !c.contains("reg delete")),
        "{:?}",
        s.calls()
    );
    assert!(svc.app_is_logon_entry());
}

#[test]
fn the_1_0_0_entry_that_replaced_the_apps_is_removed_and_reported() {
    let dir = paddock_testkit::private_tempdir().unwrap();
    let s = Script::default();
    let mut svc = Service::with(
        r"C:\Dyno\dyno-agent.exe".into(),
        dir.path().to_path_buf(),
        0,
        Box::new(s.clone()),
        Os::Windows,
    );
    s.answer("/v Dyno", true, r#"Dyno REG_SZ "C:\Dyno\dyno-agent.exe" run"#);
    assert_eq!(svc.clear_stale_run_entry(), Ok(true));
    assert!(s
        .calls()
        .iter()
        .any(|c| c.starts_with(r"reg delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Dyno /f")));
    // Other systems have no such entry.
    let mut mac = service(Os::Mac, dir.path(), &s);
    assert_eq!(mac.clear_stale_run_entry(), Ok(false));
    assert!(!mac.app_is_logon_entry());
}
