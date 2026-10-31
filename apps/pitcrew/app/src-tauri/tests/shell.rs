#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! Pitcrew on Tauri's mock runtime: no window reaches the desktop, no service is contacted and nothing is written outside a
//! temp folder (rule 7). The shell plugin with Pitcrew's parts, the capabilities per window class, the service as a health
//! service, jobs with their result, and the checks every app runs over its generated files (the dialog contracts and
//! `paddock-manifest.json` against the plugin and the app's table). It also writes the UI's command names.

use paddock_commands::testing::{sync_generated, WriteMode};
use paddock_shell::{AppIdentity, PLUGIN_NAME};
use pitcrew_app_lib::dialogs::DIALOGS;
use pitcrew_app_lib::dto::ConnectionState;
use pitcrew_app_lib::service_client::{ClientError, ServiceClient};
use pitcrew_app_lib::{health, invoke_handler, shell, Place, APP_ID, APP_NAME, COMMANDS, MANIFEST, RELEASES_PAGE, UPDATE_FEED, VERSION};
use pitcrew_core::protocol::{Request, Response};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_utils::acl::capability::Capability;
use tauri_utils::acl::manifest::{Manifest, PermissionFile};
use tauri_utils::acl::resolved::Resolved;

/// How long a test waits for an event from a background thread before it fails (never a fixed sleep).
const WAIT: Duration = Duration::from_secs(10);

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ui_dir() -> PathBuf {
    app_dir().join("../src")
}

/// The app's capability files, as Tauri reads them.
fn capability_files() -> Vec<Capability> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(app_dir().join("capabilities"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .collect()
}

/// The plugin's permission files (Tauri's per-command ones and the generated sets), as its build step lists them.
fn plugin_permissions() -> Vec<PermissionFile> {
    let list = std::fs::read_to_string(env!("PADDOCK_SHELL_PERMISSION_FILES")).unwrap();
    let paths: Vec<PathBuf> = serde_json::from_str(&list).unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("window-classes.toml")),
        "the plugin lists its window-class sets: {paths:?}"
    );
    paths
        .iter()
        .map(|p| toml_permission(&std::fs::read_to_string(p).unwrap()))
        .collect()
}

fn toml_permission(text: &str) -> PermissionFile {
    toml::from_str(text).unwrap_or_else(|e| panic!("{e}"))
}

/// The mock context with the app's own capabilities resolved through Tauri's ACL (core permissions left out: the mock
/// runtime has no core plugins to resolve them against).
fn context() -> tauri::Context<MockRuntime> {
    let mut ctx = mock_context(noop_assets());
    let acl = BTreeMap::from([(PLUGIN_NAME.to_string(), Manifest::new(plugin_permissions(), None))]);
    let caps: BTreeMap<String, Capability> = capability_files()
        .into_iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c).unwrap();
            let perms: Vec<Value> = v["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| !p.as_str().is_some_and(|s| s.starts_with("core:")))
                .cloned()
                .collect();
            v["permissions"] = Value::Array(perms);
            (c.identifier.clone(), serde_json::from_value(v).unwrap())
        })
        .collect();
    let resolved = Resolved::resolve(&acl, caps, tauri_utils::platform::Target::current()).unwrap();
    *ctx.runtime_authority_mut() = tauri::runtime_authority!(acl, resolved);
    ctx
}

fn app(dir: &Path) -> App<MockRuntime> {
    mock_builder()
        .plugin(shell(Arc::new(Down), Place::Test { dir: dir.to_path_buf() }).build())
        .invoke_handler(invoke_handler())
        .build(context())
        .unwrap()
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default()).build().unwrap()
}

/// Calls `cmd` from `w`: a shell command through the plugin (`shell: true`), else the app's own.
fn call(w: &WebviewWindow<MockRuntime>, shell: bool, cmd: &str, args: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: if shell {
            format!("plugin:{PLUGIN_NAME}|{cmd}")
        } else {
            cmd.to_string()
        },
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

/// Every payload of `event` from now on.
fn events(app: &App<MockRuntime>, event: &str) -> Receiver<Value> {
    let (tx, rx) = channel();
    app.listen_any(event.to_string(), move |e| {
        let _ = tx.send(serde_json::from_str::<Value>(e.payload()).unwrap());
    });
    rx
}

/// Waits for the first payload that `done` accepts (bounded; fails the test with what it saw).
fn wait_for(rx: &Receiver<Value>, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    let mut seen = Vec::new();
    loop {
        match rx.recv_timeout(WAIT) {
            Ok(v) if done(&v) => return v,
            Ok(v) => seen.push(v),
            Err(_) => panic!("no {what} within {WAIT:?}; saw {seen:#?}"),
        }
    }
}

fn service<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    snapshot["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["service"] == id)
        .unwrap()
}

/// A service that never answers: no test reaches a real one.
struct Down;

impl ServiceClient for Down {
    fn request(&self, _: Request) -> Result<Response, ClientError> {
        Err(ClientError::Unavailable("tests have no service".into()))
    }
}

const REGENERATE: &str = "cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell";

#[test]
fn the_uis_command_names_are_generated_from_the_apps_table() {
    let text = paddock_commands::ts::names_module(&COMMANDS, "APP_COMMANDS", "AppCommand", REGENERATE).unwrap();
    let path = ui_dir().join("app/generated/app-commands.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// The UI's dialog contracts are generated from the dialogs table, so Rust and TypeScript cannot disagree (ADR 0026 §3).
#[test]
fn the_uis_dialog_contracts_are_generated_from_the_table() {
    let text = paddock_dialogs::ts::contracts_module("the app", &DIALOGS, REGENERATE);
    let path = ui_dir().join("app/generated/dialog-contracts.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn every_command_of_the_app_is_async() {
    paddock_commands::testing::assert_async(&COMMANDS, &[&app_dir().join("src")]);
}

#[test]
fn the_contracts_and_manifest_agree_with_the_plugin_and_the_apps_table() {
    let builder = shell::<MockRuntime>(Arc::new(Down), Place::Test { dir: std::env::temp_dir() });
    assert_eq!(builder.problems(Some(MANIFEST)), Vec::<String>::new());
    assert_eq!(builder.service_ids(), [health::SERVICE]);
}

#[test]
fn each_window_class_gets_exactly_the_shells_set_for_it() {
    let classes: BTreeMap<String, (Vec<String>, Vec<String>)> = capability_files()
        .into_iter()
        .map(|c| {
            let v = serde_json::to_value(&c).unwrap();
            let list = |k: &str| {
                v[k].as_array()
                    .unwrap()
                    .iter()
                    .map(|w| w.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            };
            (c.identifier.clone(), (list("windows"), list("permissions")))
        })
        .collect();
    let owned = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        classes,
        BTreeMap::from([
            (
                "shell-dialog".to_string(),
                (owned(&["d-*"]), owned(&["core:event:default", "paddock-shell:dialog"]))
            ),
            (
                "shell-main".to_string(),
                (
                    owned(&["main", "w-*"]),
                    // Dragging by the overlay title bar (macOS) needs these two window permissions.
                    owned(&[
                        "core:event:default",
                        "paddock-shell:main",
                        "core:window:allow-start-dragging",
                        "core:window:allow-internal-toggle-maximize"
                    ])
                )
            ),
        ])
    );
}

#[test]
fn the_app_starts_with_its_identity_and_updates() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let identity = app.handle().state::<AppIdentity>().inner().clone();
    assert_eq!(
        (identity.id.as_str(), identity.name.as_str(), identity.version.as_str()),
        (APP_ID, APP_NAME, VERSION)
    );
    let about = call(&main, true, "about_info", json!({})).unwrap();
    assert_eq!(about["appName"], APP_NAME, "{about}");
    assert!(
        !dir.path().join("config/settings.json").exists(),
        "settings stay in memory in a test"
    );
}

#[test]
fn a_lost_service_is_one_outage_with_a_repair_and_a_return_ends_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let events = events(&app, "paddock://service-health");
    health::report_with(
        app.handle(),
        &ConnectionState::Unavailable {
            message: "not running".into(),
        },
        true,
    );
    let down = wait_for(&events, "the outage", |s| service(s, health::SERVICE)["state"] == "down");
    assert_eq!(service(&down, health::SERVICE)["repair"], "restart");
    assert_eq!(service(&down, health::SERVICE)["cause"]["code"], "pc.service.unavailable");
    let notices = call(&main, true, "notifications_list", json!({})).unwrap();
    assert!(notices.to_string().contains("health:pc.service"), "{notices}");
    health::report(
        app.handle(),
        &ConnectionState::Connected {
            service_version: "1.0.2".into(),
        },
    );
    wait_for(&events, "the recovery", |s| service(s, health::SERVICE)["state"] == "ok");
    // A service that was never installed is its own outage, with the OS prompt named.
    health::report_with(
        app.handle(),
        &ConnectionState::Unavailable {
            message: "not found".into(),
        },
        false,
    );
    let down = wait_for(&events, "the not-installed outage", |s| {
        service(s, health::SERVICE)["state"] == "down"
    });
    assert_eq!(service(&down, health::SERVICE)["cause"]["code"], "pc.service.not-installed");
}

#[test]
fn a_job_returns_its_id_at_once_and_its_result_is_read_once() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let jobs = events(&app, "paddock://jobs");
    // The fake service is down, so the job fails with a message instead of hanging; the command itself returns at once.
    let id = call(&main, false, "clean_disk", json!({ "runner": "build-pc" })).unwrap();
    let id = id.as_u64().unwrap();
    let ended = wait_for(&jobs, "the job's end", |j| j["id"] == id && j["state"] == "failed");
    assert!(ended["message"].as_str().unwrap_or_default().contains("service"), "{ended}");
    assert_eq!(call(&main, false, "job_result", json!({ "id": id })).unwrap(), Value::Null);
}

#[test]
fn the_tray_menu_is_valid_and_uses_the_shells_items() {
    let v = pitcrew_app_lib::tray::view(&[], false);
    assert!(paddock_window::tray::validate(&v.menu).is_ok());
}

fn release_workflow() -> String {
    std::fs::read_to_string(app_dir().join("../../../../.gitea/workflows/pitcrew-release.yml")).unwrap()
}

/// The update feed is the moving release that pitcrew-release.yml publishes `latest.json` on (installed copies read it),
/// and the updater plugin's configured endpoint is the same address.
#[test]
fn the_update_feed_is_the_release_the_workflow_publishes_latest_json_on() {
    let workflow = release_workflow();
    let tag = workflow
        .lines()
        .find_map(|l| l.trim().strip_prefix("latest-tag:"))
        .map(|t| t.trim().to_string())
        .expect("the workflow names the moving release");
    let feed = format!("https://dev.decryptology.net/decryp7/paddock/releases/download/{tag}/latest.json");
    assert_eq!(UPDATE_FEED, feed);
    let conf: Value = serde_json::from_str(&std::fs::read_to_string(app_dir().join("tauri.conf.json")).unwrap()).unwrap();
    assert_eq!(conf["plugins"]["updater"]["endpoints"], json!([feed]));
}

/// The releases page is the family's, filtered to the tags the workflow releases from.
#[test]
fn the_releases_page_lists_the_tags_the_workflow_releases_from() {
    let workflow = release_workflow();
    let prefix = workflow
        .lines()
        .find_map(|l| l.trim().strip_prefix("tags: [\"")?.strip_suffix("*\"]"))
        .expect("the workflow releases from a tag pattern");
    assert_eq!(prefix, "pitcrew-v");
    assert_eq!(
        RELEASES_PAGE,
        format!("https://dev.decryptology.net/decryp7/paddock/releases?q={prefix}")
    );
}

/// The UI's list of screens a deep link may open and Rust's list of screens a window may ask for (`open_page`) are one list.
#[test]
fn the_screens_a_deep_link_and_open_page_may_open_agree() {
    let ts = std::fs::read_to_string(app_dir().join("../src/app/core/deep-link.ts")).unwrap();
    let list = ts
        .lines()
        .find(|l| l.contains("const PAGES = new Set(["))
        .expect("deep-link.ts has PAGES");
    let mut ui: Vec<String> = list.split('\'').skip(1).step_by(2).map(str::to_string).collect();
    ui.sort();
    let mut rust: Vec<String> = ["/runners", "/activity", "/insights", "/storage", "/logs", "/updates"]
        .iter()
        .filter(|r| pitcrew_app_lib::events::open_page_route(r).is_some())
        .map(|r| r.trim_start_matches('/').to_string())
        .collect();
    rust.sort();
    assert_eq!(ui, rust, "deep-link.ts PAGES and events::open_page_route");
}

/// The registration job's kind is one string in Rust and in the UI (the register window's follower, the mock, the dialog's result rule).
#[test]
fn the_registration_job_kind_is_the_same_in_rust_and_the_ui() {
    let ts = std::fs::read_to_string(app_dir().join("../src/app/core/job-kinds.ts")).unwrap();
    assert!(ts.contains(&format!("REGISTER_JOB_KIND = '{}'", pitcrew_app_lib::jobs::DIALOG_JOB_KIND)));
    for file in ["../src/app/shell/window-jobs.ts", "../src/app/core/mock-backend.ts"] {
        let text = std::fs::read_to_string(app_dir().join(file)).unwrap();
        assert!(
            !text.contains(pitcrew_app_lib::jobs::DIALOG_JOB_KIND),
            "{file} names the kind itself; use REGISTER_JOB_KIND"
        );
    }
}

/// Pitcrew's start as `run` composes it (rule 43, ADR 0029), on the mock runtime with a headless tray and no service: the
/// tray and the main window open after every plugin is set up (a window built inside a plugin hook deadlocks).
#[test]
fn the_start_opens_the_tray_and_the_main_window() {
    use paddock_shell::testing::{start, Started};
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    assert_eq!(
        start(move || composed(&data, paddock_window::launch::LaunchArgs::default())),
        Ok(Started {
            main_window: true,
            tray: true
        })
    );
}

/// Started at log-in (`--hidden`) with a tray: Pitcrew stays in the tray without a window.
#[test]
fn a_hidden_start_stays_in_the_tray() {
    use paddock_shell::testing::{start, Started};
    let launch = paddock_window::launch::LaunchArgs {
        hidden: true,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    assert_eq!(
        start(move || composed(&data, launch)),
        Ok(Started {
            main_window: false,
            tray: true
        })
    );
}

/// Pitcrew as `run` composes it, without the OS plugins, the native tray or a service.
fn composed(dir: &Path, launch: paddock_window::launch::LaunchArgs) -> tauri::Builder<MockRuntime> {
    let shell = shell(Arc::new(Down), Place::Test { dir: dir.to_path_buf() });
    pitcrew_app_lib::app(mock_builder(), shell, launch, paddock_window::tauri::HeadlessTray)
}
