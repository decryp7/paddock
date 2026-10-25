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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let empty = tempfile::tempdir().unwrap();
    assert!(!service(Os::Mac, empty.path(), &s).ensure().unwrap());
}

#[test]
fn linux_writes_a_unit_and_enables_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = Script::default();
    let mut svc = service(Os::Linux, dir.path(), &s);
    svc.install().unwrap();
    assert_eq!(
        s.calls(),
        vec!["systemctl --user daemon-reload", "systemctl --user enable --now dyno-agent.service"]
    );
    let unit = std::fs::read_to_string(dir.path().join(".config/systemd/user/dyno-agent.service")).unwrap();
    assert!(unit.contains("ExecStart=/opt/dyno/dyno-agent run"));
    s.answer("is-active", true, "active\n");
    assert_eq!(
        svc.status(),
        Status::Installed {
            running: true,
            current: true
        }
    );
    svc.uninstall().unwrap();
    assert_eq!(svc.status(), Status::NotInstalled);
}

#[test]
fn a_failing_command_is_reported_with_what_failed() {
    let dir = tempfile::tempdir().unwrap();
    let s = Script::default();
    s.answer("enable", false, "");
    let e = service(Os::Linux, dir.path(), &s).install().unwrap_err();
    assert!(e.contains("systemctl"), "{e}");
}

#[test]
fn windows_uses_the_per_user_run_key() {
    let dir = tempfile::tempdir().unwrap();
    let s = Script::default();
    let mut svc = Service::with(
        r"C:\Dyno\dyno-agent.exe".into(),
        dir.path().to_path_buf(),
        0,
        Box::new(s.clone()),
        Os::Windows,
    );
    svc.install().unwrap();
    assert!(s.calls()[0].starts_with(r"reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Dyno"));
    assert!(s.calls()[0].contains(r#""C:\Dyno\dyno-agent.exe" run"#));
    s.answer("reg query", true, r#"Dyno REG_SZ "C:\Dyno\dyno-agent.exe" run"#);
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
    let dir = tempfile::tempdir().unwrap();
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
