//! Faults and failures against a real plugin process: crash, panic, hang, oversized frame, tampered program, old protocol.

use crate::common::Rig;
use paddock_guard::{Quarantine, QuarantinePolicy};
use paddock_jobs::time::Clock;
use paddock_plugin_api::compat::NamespaceSupport;
use paddock_plugin_host::{FaultKind, Grants, HostError, Installed, Status, Supervisor};
use paddock_plugin_test::fixture::{self, FixtureContract, FixtureReply, FixtureRequest};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

fn echo(s: &Supervisor) -> Result<serde_json::Value, HostError> {
    s.invoke(fixture::TOOL, "echo", json!({"text": "hi"}))
}

fn fault(r: Result<impl std::fmt::Debug, HostError>) -> (FaultKind, String) {
    match r {
        Err(HostError::Fault { kind, message }) => (kind, message),
        other => panic!("expected a fault, got {other:?}"),
    }
}

#[test]
fn a_crashing_plugin_is_restarted_after_the_back_off_and_then_quarantined() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    assert_eq!(s.status(), Status::Stopped);
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    assert_eq!(s.status(), Status::Running);

    // First crash: the process ends in the middle of a call.
    let (kind, _) = fault(s.invoke(fixture::TOOL, "crash", json!({})));
    assert_eq!(kind, FaultKind::Crash);
    let Status::Restarting { delay, .. } = s.status() else {
        panic!("{:?}", s.status())
    };
    assert_eq!(delay, Duration::from_secs(1));
    // It is not started again before the back-off has passed on the clock.
    let early = echo(&s).unwrap_err();
    assert!(early.to_string().contains("restarts in 1 s"), "{early}");
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    assert_eq!(s.status(), Status::Running);

    // Second crash: the back-off doubles.
    fault(s.invoke(fixture::TOOL, "crash", json!({})));
    let Status::Restarting { delay, .. } = s.status() else {
        panic!()
    };
    assert_eq!(delay, Duration::from_secs(2));
    rig.clock.advance(Duration::from_secs(2));
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    assert!(!rig.quarantine.is_quarantined(fixture::ID));

    // Third crash within the window: quarantined, and no longer called.
    fault(s.invoke(fixture::TOOL, "crash", json!({})));
    assert!(matches!(s.status(), Status::Quarantined { .. }));
    let refused = echo(&s).unwrap_err();
    assert!(
        matches!(&refused, HostError::Unavailable(m) if m.contains("disabled after repeated failures")),
        "{refused}"
    );
    assert!(rig.quarantine.is_quarantined(fixture::ID));

    // The quarantine survives the app: a new supervisor over the same file starts quarantined.
    let shared: Arc<dyn Clock> = Arc::new(rig.clock.clone());
    let (reloaded, problem) = Quarantine::persistent(
        QuarantinePolicy::default(),
        shared,
        rig.dir.path().join("quarantine.json"),
    );
    assert!(problem.is_none() && reloaded.is_quarantined(fixture::ID));
    let mut cfg = (*rig.cfg).clone();
    cfg.quarantine = Arc::new(reloaded);
    let installed = rig.plugins.load(fixture::ID).unwrap().unwrap();
    let again = Supervisor::new(Arc::new(cfg), installed);
    assert!(matches!(again.status(), Status::Quarantined { .. }));

    // The user re-enables it.
    s.release();
    assert_eq!(s.status(), Status::Stopped);
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    s.stop();
    assert_eq!(s.status(), Status::Stopped);
}

#[test]
fn a_typed_failure_is_not_a_fault_but_a_panic_in_a_contribution_is() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    for _ in 0..5 {
        match s.invoke(fixture::TOOL, "fail", json!({})) {
            Err(HostError::Failed(f)) => assert_eq!(f.message, "the fixture failed on purpose"),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
    assert_eq!(s.status(), Status::Running);
    // The plugin refusing a request on purpose (a typed error of its contract) is not counted either.
    let contract = s.remote::<FixtureContract>().unwrap();
    match contract.call(&FixtureRequest::Fail) {
        Err(HostError::Remote(r)) => assert_eq!(r.code, "fixture.failed"),
        other => panic!("{other:?}"),
    }
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
    // A panic is contained by the plugin's connection: the process stays, the fault is counted.
    let (kind, message) = fault(s.invoke(fixture::TOOL, "panic", json!({})));
    assert_eq!(kind, FaultKind::Crash);
    assert!(message.contains("panicked"));
    assert_eq!(s.status(), Status::Running);
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
    fault(contract.call(&FixtureRequest::Panic));
    fault(s.invoke(fixture::TOOL, "panic", json!({})));
    assert!(matches!(s.status(), Status::Quarantined { .. }));
    let q = rig.quarantine.status(fixture::ID);
    assert!(q.quarantined && q.last_error.unwrap().contains("panicked"));
}

#[test]
fn a_hanging_call_past_its_grace_is_a_fault_and_the_process_is_ended() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    echo(&s).unwrap();
    let caller = {
        let s = s.clone();
        std::thread::spawn(move || s.invoke(fixture::TOOL, "hang", json!({})))
    };
    // Two waits are pending on the clock: the call's timeout and the guard's budget. Pending (not merely started) waits
    // are the step boundary: a short wait another thread starts and ends meanwhile (the previous call's guard timer) cannot
    // be mistaken for one of them, and the advance cannot run before both are in place. Nothing sleeps.
    assert!(rig.clock.wait_for_pending(2, Duration::from_secs(30)));
    rig.clock.advance(Duration::from_secs(30));
    // The call timed out; the plugin ignores the cancel, so the cancel grace is the next wait, beside the guard's budget.
    assert!(rig.clock.wait_for_pending(2, Duration::from_secs(30)));
    rig.clock.advance(Duration::from_secs(2));
    let (kind, message) = fault(caller.join().unwrap());
    assert_eq!(kind, FaultKind::Hang);
    assert!(message.contains("stopped responding"));
    assert!(matches!(s.status(), Status::Restarting { .. }));
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
    // After the back-off it works again.
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
}

#[test]
fn an_oversized_frame_fails_one_call_and_counts_one_fault() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    // 512 KiB frames: a reply of 800 KB does not fit.
    let (kind, message) = fault(s.invoke(fixture::TOOL, "big", json!({"n": 800_000})));
    assert_eq!(kind, FaultKind::Garbled);
    assert!(message.contains("over the size limit"));
    assert_eq!(s.status(), Status::Running);
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    // The same through a contract.
    let contract = s.remote::<FixtureContract>().unwrap();
    fault(contract.call(&FixtureRequest::Big(800_000)));
    // A request that does not fit is not sent at all: the host's own fault, not the plugin's.
    let huge = s.invoke(fixture::TOOL, "echo", json!({"text": "x".repeat(900)}));
    assert!(huge.is_ok());
    let big_args = json!({"text": "t", "pad": "x".repeat(600_000)});
    assert!(matches!(
        s.invoke(fixture::TOOL, "echo", big_args),
        Err(HostError::Invalid(_))
    ));
    assert_eq!(
        contract.call(&FixtureRequest::Big(10)).unwrap(),
        FixtureReply::Bytes(vec![1; 10])
    );
}

#[test]
fn a_program_changed_after_install_is_a_quarantine_grade_fault_and_is_not_started() {
    let rig = Rig::new();
    let installed = rig.install("", Grants::none());
    let program = installed.program.clone();
    let s = Supervisor::new(rig.cfg.clone(), installed);
    echo(&s).unwrap();
    s.stop();
    // Somebody changes the installed binary.
    let mut bytes = std::fs::read(&program).unwrap();
    bytes.push(0);
    std::fs::write(&program, bytes).unwrap();
    let refused = echo(&s).unwrap_err();
    assert!(
        matches!(&refused, HostError::Unavailable(m) if m.contains("was not started") && m.contains("changed after installation")),
        "{refused}"
    );
    assert!(matches!(s.status(), Status::Quarantined { .. }));
    assert!(rig.quarantine.is_quarantined(fixture::ID));
    // Nothing runs it; reinstalling the package puts the verified program back.
    assert!(matches!(echo(&s).unwrap_err(), HostError::Unavailable(_)));
    let reinstalled = rig.install("", Grants::none());
    rig.quarantine.release(fixture::ID);
    let fresh = Supervisor::new(rig.cfg.clone(), reinstalled);
    assert_eq!(echo(&fresh).unwrap(), json!({"text": "hi"}));
}

#[test]
fn a_missing_program_is_refused_with_how_to_fix_it() {
    let rig = Rig::new();
    let installed = rig.install("", Grants::none());
    std::fs::remove_file(&installed.program).unwrap();
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let e = echo(&s).unwrap_err();
    assert!(
        e.to_string().contains("cannot start") && e.to_string().contains("reinstall it"),
        "{e}"
    );
    assert!(matches!(s.status(), Status::Refused(_)));
}

fn installed_with(
    rig: &Rig,
    manifest: paddock_plugin_package::Manifest,
    program: std::path::PathBuf,
    sha256: [u8; 32],
) -> Installed {
    let mut installed = rig.install("", Grants::none());
    installed.manifest = manifest;
    installed.program = program;
    installed.sha256 = sha256;
    installed
}

#[test]
fn a_manifest_for_protocol_one_is_refused_before_anything_starts() {
    let rig = Rig::new();
    let manifest = fixture::manifest("1.1", "paddock-plugin-fixture", "");
    let installed = installed_with(&rig, manifest, "/does/not/exist".into(), [0; 32]);
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let Status::Refused(why) = s.status() else {
        panic!("{:?}", s.status())
    };
    assert_eq!(
        why,
        "plugin net.example.fixture was built for protocol 1.1, which is no longer supported (this TestApp speaks 2.0); update it"
    );
    assert!(matches!(echo(&s), Err(HostError::Unavailable(m)) if m == why));
    // And a package for it cannot be installed.
    let program = rig.program("paddock-plugin-fixture");
    let pkg = rig.package(
        fixture::manifest("1.1", "paddock-plugin-fixture", ""),
        &program,
    );
    let inspection = rig
        .plugins
        .inspect(&pkg, &rig.trust, &rig.cfg.info)
        .unwrap();
    let blocked = inspection.installable().unwrap_err();
    assert!(
        blocked.to_string().contains("no longer supported"),
        "{blocked}"
    );
}

#[test]
fn a_binary_that_speaks_protocol_one_is_refused_at_the_handshake_with_who_must_update() {
    let rig = Rig::new();
    let program = rig.program("paddock-plugin-fixture-v1");
    let installed = rig.install_manifest(
        fixture::manifest("2.0", "paddock-plugin-fixture-v1", ""),
        &program,
        Grants::none(),
    );
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let e = echo(&s).unwrap_err();
    let HostError::Unavailable(text) = e else {
        panic!("{e:?}")
    };
    assert!(
        text.contains("protocol 1.1") && text.contains("needs an update"),
        "{text}"
    );
    assert_eq!(s.status(), Status::Refused(text));
    // Nobody crashed: nothing is counted.
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
}

#[test]
fn a_newer_contract_minor_than_the_app_serves_says_which_side_updates() {
    let rig = Rig::new();
    let perms = "";
    let mut toml = fixture::manifest_toml("2.0", "paddock-plugin-fixture", perms);
    toml = toml.replace("paddock.fixture.v1/1", "paddock.fixture.v1/2");
    let manifest = paddock_plugin_package::Manifest::parse(&toml).unwrap();
    let installed = installed_with(&rig, manifest.clone(), "/x".into(), [0; 32]);
    // The app serves minor 1 and knows 1.4 introduced minor 2.
    let mut cfg = (*rig.cfg).clone();
    cfg.info.namespaces = vec![NamespaceSupport::new("paddock.fixture.v1", 1).since(2, "1.4")];
    let s = Supervisor::new(Arc::new(cfg), installed.clone());
    assert_eq!(
        s.status(),
        Status::Refused("plugin net.example.fixture needs TestApp 1.4 or newer".into())
    );
    // Without release notes the text still says who updates.
    let plain = Supervisor::new(rig.cfg.clone(), installed.clone());
    let Status::Refused(text) = plain.status() else {
        panic!()
    };
    assert!(
        text.contains("needs a newer TestApp")
            && text.contains("version 2")
            && text.ends_with("update TestApp"),
        "{text}"
    );
    // An app that serves minor 2 in its config but whose contract code only knows minor 1 refuses the proxy.
    let mut cfg = (*rig.cfg).clone();
    cfg.info.namespaces = vec![NamespaceSupport::new("paddock.fixture.v1", 2)];
    let lenient = Supervisor::new(Arc::new(cfg), installed);
    let e = lenient.remote::<FixtureContract>().err().unwrap();
    assert!(
        matches!(&e, HostError::Unavailable(m) if m.contains("needs a newer TestApp")),
        "{e}"
    );
    // A plugin that does not speak the contract has no proxy.
    let rig2 = Rig::new();
    let s2 = rig2.supervisor("", Grants::none());
    struct Other;
    impl paddock_plugin_api::Contract for Other {
        const NS: &'static str = "other.v1";
        const MINOR: u16 = 0;
        type Request = ();
        type Reply = ();
    }
    assert!(
        matches!(s2.remote::<Other>().err().unwrap(), HostError::Invalid(m) if m.contains("does not speak"))
    );
}

#[test]
fn a_plugin_that_contributes_something_else_than_its_manifest_is_refused() {
    let rig = Rig::new();
    let toml = fixture::manifest_toml("2.0", "paddock-plugin-fixture", "").replace(
        "kind = \"mcpTool\"\nid = \"fixture_echo\"",
        "kind = \"mcpTool\"\nid = \"another\"",
    );
    let program = rig.program("paddock-plugin-fixture");
    let installed = rig.install_manifest(
        paddock_plugin_package::Manifest::parse(&toml).unwrap(),
        &program,
        Grants::none(),
    );
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let e = echo(&s).unwrap_err();
    let HostError::Unavailable(text) = e else {
        panic!("{e:?}")
    };
    assert!(
        text.contains("does not match its manifest") && text.contains("MCP tools"),
        "{text}"
    );
    assert!(matches!(s.status(), Status::Refused(_)));
    // The process was ended, not left running, and nobody is blamed with a fault.
    assert_eq!(rig.quarantine.status(fixture::ID).total, 0);
}

#[test]
fn a_plugin_that_cannot_be_started_at_all_is_a_counted_crash() {
    let rig = Rig::new();
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken");
    std::fs::write(&broken, "not a program").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let installed = rig.install_manifest(
        fixture::manifest("2.0", "paddock-plugin-fixture", ""),
        &broken,
        Grants::none(),
    );
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let (kind, message) = fault(echo(&s));
    assert_eq!(kind, FaultKind::Crash);
    assert!(message.contains("could not start"), "{message}");
    assert!(matches!(s.status(), Status::Restarting { .. }));
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
}

fn misbehaving(rig: &Rig, kind: &str) -> Supervisor {
    let binary = format!("misbehaving-{kind}");
    let program = rig.program("misbehaving");
    let installed = rig.install_manifest(
        fixture::manifest("2.0", &binary, ""),
        &program,
        Grants::none(),
    );
    Supervisor::new(rig.cfg.clone(), installed)
}

#[test]
fn a_plugin_that_describes_itself_in_nonsense_is_a_counted_garbled_fault() {
    let rig = Rig::new();
    let s = misbehaving(&rig, "garbled");
    let (kind, message) = fault(echo(&s));
    assert_eq!(kind, FaultKind::Garbled);
    assert!(
        message.contains("described itself in a way this app cannot read"),
        "{message}"
    );
    assert!(matches!(s.status(), Status::Restarting { .. }));
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
}

#[test]
fn a_plugin_that_dies_while_describing_itself_is_a_counted_crash() {
    let rig = Rig::new();
    let s = misbehaving(&rig, "exit");
    let (kind, message) = fault(echo(&s));
    assert_eq!(kind, FaultKind::Crash);
    assert!(message.contains("did not describe itself"), "{message}");
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
}

#[test]
fn a_plugin_that_never_describes_itself_is_a_hang_on_the_clock() {
    let rig = Rig::new();
    let s = misbehaving(&rig, "hang");
    let caller = {
        let s = s.clone();
        std::thread::spawn(move || echo(&s))
    };
    // The Describe call waits for the start timeout; the plugin ignores the cancel, so the grace is the next wait.
    assert!(rig.clock.wait_for_pending(1, Duration::from_secs(30)));
    rig.clock.advance(Duration::from_secs(15));
    assert!(rig.clock.wait_for_pending(1, Duration::from_secs(30)));
    rig.clock.advance(Duration::from_secs(2));
    let (kind, message) = fault(caller.join().unwrap());
    assert_eq!(kind, FaultKind::Hang);
    assert!(
        message.contains("did not describe itself in time"),
        "{message}"
    );
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
    assert!(s.wait_until_ended(Duration::from_secs(30)));
}

#[test]
fn settings_a_restarted_process_will_not_take_do_not_stop_it() {
    let rig = Rig::new();
    let s = misbehaving(&rig, "forgetful");
    s.apply_settings(json!({"greeting": "ahoy"})).unwrap();
    // The process ends; the next one refuses settings, which is logged and does not make it a failure.
    assert!(matches!(
        s.invoke(fixture::TOOL, "crash", json!({})),
        Err(HostError::Fault { .. })
    ));
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    assert_eq!(s.status(), Status::Running);
    assert_eq!(tool_greeting(&s), json!("hello"));
}

fn tool_greeting(s: &Supervisor) -> serde_json::Value {
    s.invoke(fixture::TOOL, "greet", json!({})).unwrap()
}

#[test]
fn a_process_that_ends_by_itself_is_noticed_as_a_crash_without_any_call() {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    assert_eq!(
        s.invoke(fixture::TOOL, "quit_soon", json!({})).unwrap(),
        json!("quitting")
    );
    assert!(s.wait_until_ended(Duration::from_secs(30)));
    // Asking for the status is enough to notice: nothing calls the plugin.
    let Status::Restarting { why, .. } = s.status() else {
        panic!("{:?}", s.status())
    };
    assert_eq!(why, "its process ended unexpectedly");
    assert_eq!(rig.quarantine.status(fixture::ID).total, 1);
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    // Waiting for a process that is not running does not wait.
    s.stop();
    assert!(s.wait_until_ended(Duration::from_secs(30)));
}

#[test]
fn a_back_off_that_gives_up_quarantines_at_once() {
    let rig = Rig::with(|c| {
        c.backoff = paddock_jobs::time::BackoffPolicy {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(2),
            reset_after: Duration::from_secs(300),
            give_up: Some(paddock_jobs::time::GiveUp {
                max_failures: 1,
                window: Duration::from_secs(600),
            }),
        };
    });
    let s = rig.supervisor("", Grants::none());
    fault(s.invoke(fixture::TOOL, "crash", json!({})));
    assert!(matches!(s.status(), Status::Restarting { .. }));
    rig.clock.advance(Duration::from_secs(1));
    fault(s.invoke(fixture::TOOL, "crash", json!({})));
    assert_eq!(
        s.status(),
        Status::Quarantined {
            why: rig.quarantine.status(fixture::ID).last_error.unwrap()
        }
    );
    assert!(rig.quarantine.is_quarantined(fixture::ID));
}

#[test]
fn a_data_folder_that_cannot_be_made_refuses_the_plugin_with_the_reason() {
    let rig = Rig::new();
    let mut installed = rig.install("", Grants::none());
    let file = rig.dir.path().join("a-file");
    std::fs::write(&file, "x").unwrap();
    installed.data_dir = file.join("data");
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let e = echo(&s).unwrap_err();
    assert!(
        matches!(&e, HostError::Unavailable(m) if m.contains("has no data folder")),
        "{e}"
    );
    assert!(matches!(s.status(), Status::Refused(_)));
}

#[test]
fn a_hung_call_that_the_connection_could_not_time_out_is_abandoned_by_the_guard() {
    // With a peer timeout longer than the guard's budget the guard gives up first: here the call's own timeout is the
    // operation's budget (30 s) and the hang grace is zero, so the guard's budget equals timeout + the cancel grace.
    let rig = Rig::with(|c| c.hang_grace = Duration::ZERO);
    let s = rig.supervisor("", Grants::none());
    echo(&s).unwrap();
    let caller = {
        let s = s.clone();
        std::thread::spawn(move || s.invoke(fixture::TOOL, "hang", json!({})))
    };
    assert!(rig.clock.wait_for_pending(2, Duration::from_secs(30)));
    // Both the call's timeout and the guard's budget (30 s + the 2 s cancel grace) are waiting; passing the whole budget at
    // once ends the guard's wait and the connection's grace in one step, whichever the threads see first.
    rig.clock.advance(Duration::from_secs(32));
    let (kind, _) = fault(caller.join().unwrap());
    assert_eq!(kind, FaultKind::Hang);
    assert!(rig.quarantine.status(fixture::ID).total >= 1);
    assert!(matches!(
        s.status(),
        Status::Restarting { .. } | Status::Quarantined { .. }
    ));
}

#[test]
fn a_plugin_the_quarantine_will_not_admit_is_not_called_even_when_the_supervisor_thought_it_was_fine()
 {
    let rig = Rig::new();
    let s = rig.supervisor("", Grants::none());
    echo(&s).unwrap();
    // A call somewhere else overran its budget and is still running.
    rig.quarantine
        .record(fixture::ID, paddock_guard::Failure::Hang, "stuck");
    let e = echo(&s).unwrap_err();
    assert!(
        matches!(&e, HostError::Unavailable(m) if m.contains("is not responding")),
        "{e}"
    );
    rig.quarantine.hang_ended(fixture::ID);
    assert_eq!(echo(&s).unwrap(), json!({"text": "hi"}));
    // Quarantined by faults the supervisor did not see (another window's calls).
    for _ in 0..3 {
        rig.quarantine
            .record(fixture::ID, paddock_guard::Failure::Panic, "elsewhere");
    }
    let e = echo(&s).unwrap_err();
    assert!(
        matches!(&e, HostError::Unavailable(m) if m.contains("disabled after repeated failures")),
        "{e}"
    );
    assert!(matches!(s.status(), Status::Quarantined { .. }));
}

#[test]
fn a_plugin_that_cannot_be_served_stays_refused_when_it_is_switched_off_and_on() {
    let rig = Rig::new();
    let installed = installed_with(
        &rig,
        fixture::manifest("1.1", "paddock-plugin-fixture", ""),
        "/x".into(),
        [0; 32],
    );
    let s = Supervisor::new(rig.cfg.clone(), installed);
    assert!(matches!(s.status(), Status::Refused(_)));
    s.set_enabled(false);
    assert_eq!(s.status(), Status::Disabled);
    s.set_enabled(true);
    assert!(matches!(s.status(), Status::Refused(m) if m.contains("no longer supported")));
    // Releasing something that is not held changes nothing, and a disabled plugin stays disabled when released.
    s.release();
    assert!(matches!(s.status(), Status::Refused(_)));
    s.set_enabled(false);
    s.release();
    assert_eq!(s.status(), Status::Disabled);
}

#[test]
fn a_plugin_with_no_contributions_has_no_settings_and_no_tools() {
    let rig = Rig::new();
    let program = rig.program("misbehaving");
    let installed = rig.install_manifest(
        fixture::bare_manifest("misbehaving-bare"),
        &program,
        Grants::none(),
    );
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let e = s.apply_settings(json!({})).unwrap_err();
    assert!(
        matches!(&e, HostError::Invalid(m) if m == "plugin net.example.bare has no settings"),
        "{e}"
    );
    assert!(
        matches!(s.invoke("t", "o", json!({})), Err(HostError::Invalid(m)) if m.contains("has no tool"))
    );
    assert_eq!(s.status(), Status::Running);
}

#[test]
fn an_mcp_answer_over_the_budget_is_refused_even_when_the_plugin_ignores_the_budget() {
    let rig = Rig::new();
    let program = rig.program("misbehaving");
    let granted = Grants::none().allow(paddock_plugin_package::Permission::McpExpose);
    let installed = rig.install_manifest(
        fixture::manifest("2.0", "misbehaving-greedy", ""),
        &program,
        granted,
    );
    let s = Supervisor::new(rig.cfg.clone(), installed);
    let budget = paddock_plugin_api::protocol::McpBudget {
        max_bytes: 1000,
        max_ms: 5000,
    };
    let e = s
        .mcp_tool(fixture::MCP_TOOL, json!({"text": "hi"}), budget)
        .unwrap_err();
    assert!(
        matches!(&e, HostError::Failed(f) if f.message.contains("the budget is 1000")),
        "{e}"
    );
    let wide = paddock_plugin_api::protocol::McpBudget {
        max_bytes: 100_000,
        max_ms: 5000,
    };
    assert!(
        s.mcp_tool(fixture::MCP_TOOL, json!({"text": "hi"}), wide)
            .is_ok()
    );
}
