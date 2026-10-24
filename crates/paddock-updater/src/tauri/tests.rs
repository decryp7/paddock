//! The adapter on a mock Tauri app (`tauri::test`): the commands on the real invoke path behind the command guard, the
//! status events, jobs on the app's registry, the configuration fallback, the background thread and the exit hook. The
//! update server is an in-memory transport: no network.

use super::*;
use crate::{UpdatePhase, WhatsNewDecision};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_download::{Config, Get, Headers, Response, Transport, TransportError};
use serde_json::{Value, json};
use std::io::Cursor;
use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc;

type App = ::tauri::App<MockRuntime>;

const MANIFEST: &str = "https://updates.test/latest.json";
const PACKAGE: &str = "https://updates.test/App.tar.gz";

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(p).unwrap().trim().to_string()
}

fn package() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/update.bin")).unwrap()
}

/// The fake update server: `latest.json` announcing 9.9.9 for this system, and the signed package.
struct Server;

impl Transport for Server {
    fn get(&self, g: &Get<'_>) -> Result<Response, TransportError> {
        let target = Target::current(None).keys().pop().unwrap();
        let body = match g.url {
            MANIFEST => json!({
                "version": "9.9.9",
                "platforms": { target: { "url": PACKAGE, "signature": fixture("update.bin.sig"), "size": package().len() } }
            })
            .to_string()
            .into_bytes(),
            PACKAGE => package(),
            _ => Vec::new(),
        };
        Ok(Response {
            status: if body.is_empty() { 404 } else { 200 },
            headers: Headers {
                content_length: Some(body.len().to_string()),
                ..Headers::default()
            },
            body: Box::new(Cursor::new(body)),
        })
    }
}

#[derive(Default)]
struct FakeInstaller(Mutex<Vec<bool>>);

impl Installer for FakeInstaller {
    fn install(&self, _file: &Path, _manifest: &Value, relaunch: bool) -> Result<(), UpdateError> {
        self.0.lock().unwrap().push(relaunch);
        Ok(())
    }
}

struct Hooks(Mutex<Vec<String>>, bool);

impl ServiceHooks for Hooks {
    fn on_version_change(&self, previous: Option<&str>, current: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .push(format!("{previous:?} -> {current}"));
        if self.1 {
            Err("the service manager did not answer".into())
        } else {
            Ok(())
        }
    }
}

fn downloader() -> Arc<Downloader> {
    Arc::new(Downloader::new(Arc::new(Server)).with_config(Config {
        disk_reserve: 0,
        ..Config::default()
    }))
}

const CHANGELOG: &str = "# Changelog\n\n## Unreleased\n\n### Added\n- later\n\n## 0.0.1 (2026-10-01)\n\n### Fixed\n- old\n";

fn builder(dir: &Path) -> UpdatesBuilder {
    UpdatesBuilder::new("Demo")
        .dirs(dir.join("config"), dir.join("cache"))
        .downloader(downloader())
        .background(false)
        .changelog(CHANGELOG)
        .legal("notices", "Third-party notices", "# Third-party notices")
        .build_info(Some("abc"), Some("2026-10-24"))
        .homepage("https://example.net")
        .releases_page("https://example.net/releases")
}

fn app(config: Option<Value>) -> App {
    let mut ctx = mock_context(noop_assets());
    if let Some(c) = config {
        ctx.config_mut().plugins.0.insert("updater".into(), c);
    }
    mock_builder()
        .invoke_handler(handler_with_grants(|_app, label| {
            (label == "d-main-1").then_some(UpdatesDialog)
        }))
        .build(ctx)
        .unwrap()
}

/// The Updates dialog's contract: the update steps, not the main window's start-up decision.
struct UpdatesDialog;

impl paddock_commands::DialogGrant for UpdatesDialog {
    fn dialog(&self) -> &str {
        "pd.updates"
    }
    fn allows(&self, command: &str) -> bool {
        command.starts_with("updates_")
    }
}

fn window(app: &App, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

/// Every status event, in order.
fn statuses(app: &App) -> mpsc::Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    app.listen_any(STATUS_EVENT, move |e| {
        let _ = tx
            .lock()
            .unwrap()
            .send(serde_json::from_str::<Value>(e.payload()).unwrap());
    });
    rx
}

/// Waits (event-driven, bounded) for a status whose phase is `kind`.
fn wait_for(rx: &mpsc::Receiver<Value>, kind: &str) -> Value {
    loop {
        let s = rx
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("no '{kind}' status"));
        if s["phase"]["kind"] == kind {
            return s;
        }
    }
}

#[test]
fn every_command_is_async_and_in_the_table() {
    paddock_commands::testing::assert_async(
        &crate::COMMANDS,
        &[&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
    );
}

#[test]
fn without_the_state_every_command_is_refused_and_exit_does_nothing() {
    let app = app(None);
    let w = window(&app, "main");
    for spec in crate::COMMANDS.specs() {
        let args = json!({ "enabled": true, "id": "notices", "preferences": { "autoCheck": false, "channel": "stable" } });
        let e = invoke(&w, spec.name(), args).unwrap_err();
        assert!(
            e.to_string().contains("state not managed"),
            "{}: {e}",
            spec.name()
        );
    }
    assert_eq!(on_exit(app.handle()), ExitAction::Nothing);
}

#[test]
fn a_build_without_a_key_serves_about_and_says_updates_are_not_available() {
    let app = app(None);
    let dir = tempfile::tempdir().unwrap();
    let updates = install(
        app.handle(),
        builder(dir.path()).channel("stable", "Stable", MANIFEST),
    )
    .unwrap();
    assert!(
        Arc::ptr_eq(
            &updates,
            &install(app.handle(), builder(dir.path())).unwrap()
        ),
        "a second install keeps the first"
    );
    let w = window(&app, "main");
    let status = invoke(&w, "updates_status", json!({})).unwrap();
    assert_eq!(status["phase"]["kind"], "unavailable");
    assert_eq!(status["endpointHost"], Value::Null);
    let e = invoke(&w, "updates_check", json!({})).unwrap_err();
    assert_eq!(
        (e["kind"].as_str(), e["message"].as_str()),
        (
            Some("notAvailable"),
            Some("Updates are not available in this build.")
        )
    );
    // About: the version is the engine's (the app's package version, rule 6), never a copy.
    let info = invoke(&w, "about_info", json!({})).unwrap();
    assert_eq!(info["version"], app.package_info().version.to_string());
    assert_eq!(
        (
            info["appName"].as_str(),
            info["commit"].as_str(),
            info["built"].as_str()
        ),
        (Some("Demo"), Some("abc"), Some("2026-10-24"))
    );
    assert_eq!(info["updatesAvailable"], false);
    assert_eq!(
        info["legal"],
        json!([{ "id": "notices", "title": "Third-party notices" }])
    );
    assert_eq!(
        (info["homepage"].as_str(), info["releasesPage"].as_str()),
        (
            Some("https://example.net"),
            Some("https://example.net/releases")
        )
    );
    assert_eq!(
        invoke(&w, "about_legal", json!({ "id": "notices" })).unwrap(),
        "# Third-party notices"
    );
    let e = invoke(&w, "about_legal", json!({ "id": "gpl" })).unwrap_err();
    assert_eq!(e["message"], "there is no legal document 'gpl'");
    // What's New is the running version's: this changelog has none for it, and older notes are not shown instead.
    let notes = invoke(&w, "about_whats_new", json!({})).unwrap();
    assert_eq!(notes["version"], app.package_info().version.to_string());
    assert_eq!(notes["release"], Value::Null);
    assert_eq!(
        invoke(&w, "about_whats_new_start", json!({})).unwrap(),
        json!(WhatsNewDecision::Remember)
    );
    assert_eq!(
        invoke(&w, "about_whats_new_start", json!({})).unwrap(),
        json!(WhatsNewDecision::Nothing)
    );
}

#[test]
fn check_download_and_install_on_quit_run_as_jobs_and_report_by_events() {
    let app = app(None);
    let dir = tempfile::tempdir().unwrap();
    let installer = Arc::new(FakeInstaller::default());
    let b = builder(dir.path())
        .channel("stable", "Stable", MANIFEST)
        .public_key(None)
        .public_key(Some(&fixture("key.pub")))
        .installer(installer.clone())
        .installer_kind("app")
        .max_download_bytes(1 << 20);
    install(app.handle(), b).unwrap();
    let rx = statuses(&app);
    let w = window(&app, "main");
    let checking = invoke(&w, "updates_check", json!({})).unwrap();
    assert_eq!(
        checking["phase"]["kind"], "checking",
        "the command returns at once"
    );
    assert!(checking["phase"]["job"].is_u64(), "listed as a job");
    let available = wait_for(&rx, "available");
    assert_eq!(available["phase"]["info"]["version"], "9.9.9");
    assert_eq!(available["endpointHost"], "updates.test");
    let downloading = invoke(&w, "updates_download", json!({})).unwrap();
    assert_eq!(downloading["phase"]["kind"], "downloading");
    let job = downloading["phase"]["job"].as_u64().unwrap();
    wait_for(&rx, "ready");
    let jobs = app
        .state::<paddock_jobs::tauri::Jobs>()
        .registry()
        .snapshots();
    assert!(
        jobs.iter().any(|j| j.id.0 == job),
        "the download is a job of the app's registry"
    );
    // A dialog may run the steps too.
    let dialog = window(&app, "d-main-1");
    let ready = invoke(
        &dialog,
        "updates_install_on_quit",
        json!({ "enabled": true }),
    )
    .unwrap();
    assert_eq!(ready["phase"]["installOnQuit"], true);
    assert!(
        invoke(&dialog, "about_whats_new_start", json!({})).is_err(),
        "main window only"
    );
    assert_eq!(
        on_exit(app.handle()),
        ExitAction::Installed { relaunch: false }
    );
    assert_eq!(installer.0.lock().unwrap().as_slice(), [false]);
}

#[test]
fn cancel_preferences_and_restart_go_through_the_commands() {
    let app = app(None);
    let dir = tempfile::tempdir().unwrap();
    let b = builder(dir.path())
        .channel("stable", "Stable", MANIFEST)
        .channel("beta", "Beta", MANIFEST)
        .public_key(Some(&fixture("key.pub")));
    install(app.handle(), b).unwrap();
    let rx = statuses(&app);
    let w = window(&app, "main");
    invoke(&w, "updates_check", json!({})).unwrap();
    wait_for(&rx, "available");
    // Download, then drop the verified download: the update stays available.
    invoke(&w, "updates_download", json!({})).unwrap();
    wait_for(&rx, "ready");
    assert_eq!(
        invoke(&w, "updates_cancel", json!({})).unwrap()["phase"]["kind"],
        "available"
    );
    // Without an installer Restart is refused with a next step.
    invoke(&w, "updates_download", json!({})).unwrap();
    wait_for(&rx, "ready");
    let e = invoke(&w, "updates_restart", json!({})).unwrap_err();
    assert_eq!(e["kind"], "installFailed");
    assert!(e["nextStep"].as_str().unwrap().contains("releases page"));
    let prefs = |channel: &str| json!({ "preferences": { "autoCheck": true, "channel": channel } });
    assert_eq!(
        invoke(&w, "updates_set_preferences", prefs("beta")).unwrap_err()["kind"],
        "busy",
        "not while an update is ready"
    );
    invoke(&w, "updates_cancel", json!({})).unwrap();
    let s = invoke(&w, "updates_set_preferences", prefs("beta")).unwrap();
    assert_eq!(
        (
            s["channel"].as_str(),
            s["autoCheck"].as_bool(),
            s["phase"]["kind"].as_str()
        ),
        (Some("beta"), Some(true), Some("idle"))
    );
    assert_eq!(
        invoke(&w, "updates_set_preferences", prefs("nightly")).unwrap_err()["kind"],
        "invalid"
    );
    // A cancel of a running check names its job, which the adapter cancels; the job's end then ends the check.
    let updates = app.state::<Arc<Updates>>().inner().clone();
    let (_, ticket) = updates.updater.begin_check().unwrap();
    updates.updater.set_job(ticket, 4242);
    assert_eq!(
        invoke(&w, "updates_cancel", json!({})).unwrap()["phase"]["kind"],
        "checking"
    );
    assert_eq!(on_exit(app.handle()), ExitAction::Nothing);
}

#[test]
fn the_tauri_configuration_gives_the_address_and_the_key() {
    let app = app(Some(
        json!({ "endpoints": [MANIFEST], "pubkey": fixture("key.pub") }),
    ));
    let dir = tempfile::tempdir().unwrap();
    let hooks = Arc::new(Hooks(Mutex::new(Vec::new()), false));
    let updates = install(
        app.handle(),
        builder(dir.path()).service_hooks(hooks.clone()),
    )
    .unwrap();
    let w = window(&app, "main");
    let status = invoke(&w, "updates_status", json!({})).unwrap();
    assert_eq!(status["phase"]["kind"], "idle");
    assert_eq!(
        status["channels"],
        json!([{ "id": "stable", "label": "Stable" }])
    );
    assert_eq!(
        invoke(&w, "about_info", json!({})).unwrap()["updatesAvailable"],
        true
    );
    // The background thread runs the services' hook once and stops when asked.
    let handle = background(updates.clone(), Duration::from_millis(5)).unwrap();
    updates.stop();
    handle.thread().unpark();
    handle.join().unwrap();
    assert_eq!(hooks.0.lock().unwrap().len(), 1);
}

#[test]
fn defaults_come_from_the_app_and_a_bad_legal_id_is_refused() {
    let first = app(None);
    // No downloader given and none managed: a new HTTPS one (nothing is contacted).
    let dir = tempfile::tempdir().unwrap();
    let plain = UpdatesBuilder::new("Demo")
        .background(false)
        .dirs(dir.path().join("c"), dir.path().join("d"));
    assert!(install(first.handle(), plain).is_ok());
    let second = app(None);
    let bad = UpdatesBuilder::new("Demo")
        .background(false)
        .dirs(dir.path().join("c2"), dir.path().join("d2"))
        .legal("Bad Id", "x", "y");
    let e = install(second.handle(), bad).err().unwrap();
    assert!(
        e.to_string()
            .starts_with("cannot register the legal document")
    );
    assert!(
        InstallError::Downloader("x".into())
            .to_string()
            .contains("downloader")
    );
    // A downloader the app manages is shared; the background thread stops when asked.
    let third = app(None);
    let shared = paddock_download::tauri::manage(third.handle(), Downloader::new(Arc::new(Server)));
    let b = UpdatesBuilder::new("Demo")
        .background(true)
        .dirs(dir.path().join("c3"), dir.path().join("d3"));
    let updates = install(third.handle(), b).unwrap();
    updates.stop();
    assert!(Arc::strong_count(&shared) >= 2);
    assert!(matches!(
        updates.updater.status().phase,
        UpdatePhase::Unavailable
    ));
}

#[test]
fn malformed_arguments_are_refused_before_the_updater_sees_them() {
    let app = app(None);
    let dir = tempfile::tempdir().unwrap();
    install(app.handle(), builder(dir.path())).unwrap();
    let w = window(&app, "main");
    assert!(invoke(&w, "updates_install_on_quit", json!({})).is_err());
    assert!(
        invoke(
            &w,
            "updates_set_preferences",
            json!({ "preferences": { "channel": 3 } })
        )
        .is_err()
    );
    assert!(invoke(&w, "about_legal", json!({})).is_err());
}

#[test]
fn the_automatic_check_runs_in_the_background_and_a_failed_registration_is_logged() {
    let app = app(None);
    let dir = tempfile::tempdir().unwrap();
    let hooks = Arc::new(Hooks(Mutex::new(Vec::new()), true));
    let b = builder(dir.path())
        .channel("stable", "Stable", MANIFEST)
        .public_key(Some(&fixture("key.pub")))
        .service_hooks(hooks.clone())
        .auto_check_delay_secs(0);
    let updates = install(app.handle(), b).unwrap();
    let rx = statuses(&app);
    let w = window(&app, "main");
    invoke(
        &w,
        "updates_set_preferences",
        json!({ "preferences": { "autoCheck": true, "channel": "stable" } }),
    )
    .unwrap();
    let handle = background(updates.clone(), Duration::from_millis(5)).unwrap();
    wait_for(&rx, "available");
    updates.stop();
    handle.thread().unpark();
    handle.join().unwrap();
    assert_eq!(
        hooks.0.lock().unwrap().len(),
        1,
        "tried once; again at the next start"
    );
}

#[test]
fn an_update_over_the_limit_or_a_closed_pool_ends_the_step_at_once() {
    let app = app(None);
    let dir = tempfile::tempdir().unwrap();
    let b = builder(dir.path())
        .channel("stable", "Stable", MANIFEST)
        .public_key(Some(&fixture("key.pub")))
        .max_download_bytes(1000);
    let updates = install(app.handle(), b).unwrap();
    let rx = statuses(&app);
    let w = window(&app, "main");
    invoke(&w, "updates_check", json!({})).unwrap();
    wait_for(&rx, "available");
    let s = invoke(&w, "updates_download", json!({})).unwrap();
    assert_eq!(
        (
            s["phase"]["kind"].as_str(),
            s["phase"]["error"]["kind"].as_str()
        ),
        (Some("failed"), Some("tooLarge"))
    );
    // At shutdown the pool takes no more work: the step is abandoned, not left running.
    updates.pool.shutdown();
    assert_eq!(
        invoke(&w, "updates_check", json!({})).unwrap()["phase"]["kind"],
        "idle"
    );
    assert!(
        updates.updater.begin_check().is_ok() && updates.updater.cancel().is_ok(),
        "the updater itself still works"
    );
}

#[test]
fn a_download_whose_job_cannot_start_is_abandoned_and_a_failed_install_is_logged() {
    let app = app(None);
    let dir = tempfile::tempdir().unwrap();
    let installer = Arc::new(FailingInstaller);
    let b = builder(dir.path())
        .channel("stable", "Stable", MANIFEST)
        .public_key(Some(&fixture("key.pub")))
        .installer(installer);
    let updates = install(app.handle(), b).unwrap();
    let rx = statuses(&app);
    let w = window(&app, "main");
    invoke(&w, "updates_check", json!({})).unwrap();
    wait_for(&rx, "available");
    invoke(&w, "updates_download", json!({})).unwrap();
    wait_for(&rx, "ready");
    invoke(&w, "updates_install_on_quit", json!({ "enabled": true })).unwrap();
    assert_eq!(
        on_exit(app.handle()),
        ExitAction::Failed { relaunch: false }
    );
    // At shutdown the pool takes no more work: a download is abandoned, the update stays available.
    invoke(&w, "updates_check", json!({})).unwrap();
    wait_for(&rx, "available");
    updates.pool.shutdown();
    assert_eq!(
        invoke(&w, "updates_download", json!({})).unwrap()["phase"]["kind"],
        "available"
    );
}

struct FailingInstaller;

impl Installer for FailingInstaller {
    fn install(&self, _file: &Path, _manifest: &Value, _relaunch: bool) -> Result<(), UpdateError> {
        Err(UpdateError::InstallFailed {
            detail: "read-only volume".into(),
        })
    }
}
