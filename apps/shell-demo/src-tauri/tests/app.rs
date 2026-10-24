//! The app on Tauri's mock runtime: no window reaches the desktop and nothing is written outside a temp folder (rule 7).
//! The shell plugin with the demo's parts, the demo's guarded commands, the sample engine (health with a working and a
//! failing repair, jobs with phases and Cancel), the capabilities per window class, and the checks every app runs over
//! its generated files (the dialog contracts and `paddock-manifest.json` against the plugin and the app's table).
//! It also writes the UI's command names (`src/app/generated/app-commands.ts` of the UI project).

use paddock_commands::testing::{WriteMode, sync_generated};
use paddock_health::ServiceSpec;
use paddock_shell::{AppIdentity, PLUGIN_NAME};
use paddock_shell_demo::agent;
use paddock_shell_demo::engine::{self, DemoJob, Engine};
use paddock_shell_demo::{
    APP_ID, APP_NAME, COMMANDS, MANIFEST, Place, VERSION, invoke_handler, shell, shell_with_log,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
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
    app_dir().join("../../../ui/projects/shell-demo")
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
        .map(|p| {
            serde_json::from_str(&std::fs::read_to_string(p).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        })
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
    let acl = BTreeMap::from([(
        PLUGIN_NAME.to_string(),
        Manifest::new(plugin_permissions(), None),
    )]);
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
        .plugin(
            shell(Place::Test {
                dir: dir.to_path_buf(),
            })
            .build(),
        )
        .invoke_handler(invoke_handler())
        .build(context())
        .unwrap()
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

/// Calls `cmd` from `w`: a shell command through the plugin (`shell: true`), else the app's own.
fn call(
    w: &WebviewWindow<MockRuntime>,
    shell: bool,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
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

const REGENERATE: &str = "cd apps/shell-demo/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app";

#[test]
fn the_uis_command_names_are_generated_from_the_apps_table() {
    let text =
        paddock_commands::ts::names_module(&COMMANDS, "APP_COMMANDS", "AppCommand", REGENERATE)
            .unwrap();
    let path = ui_dir().join("src/app/generated/app-commands.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn every_command_of_the_app_is_async() {
    paddock_commands::testing::assert_async(&COMMANDS, &[&app_dir().join("src")]);
}

#[test]
fn the_contracts_and_manifest_agree_with_the_plugin_and_the_apps_table() {
    let builder = shell::<MockRuntime>(Place::Test {
        dir: std::env::temp_dir(),
    });
    assert_eq!(builder.problems(Some(MANIFEST)), Vec::<String>::new());
    assert_eq!(
        builder.service_ids(),
        [engine::ENGINE, engine::INDEX, agent::HEALTH_ID]
    );
    assert_eq!(
        builder.section_ids(),
        ["pd.notifications", "sd.general", "sd.storage"]
    );
}

#[test]
fn a_drifted_ui_is_reported() {
    let dir = std::env::temp_dir();
    let missing = shell::<MockRuntime>(Place::Test { dir: dir.clone() })
        .health(vec![ServiceSpec::new(engine::ENGINE, "the demo engine")]);
    let p = missing.problems(Some(MANIFEST));
    assert!(p.iter().any(|x| x.contains(engine::INDEX)), "{p:?}");
    let renamed = MANIFEST.replace("\"about_info\"", "\"about_infos\"");
    let p = shell::<MockRuntime>(Place::Test { dir: dir.clone() }).problems(Some(&renamed));
    assert!(p.iter().any(|x| x.contains("about_infos")), "{p:?}");
    let bad = shell::<MockRuntime>(Place::Test { dir })
        .dialog_contracts("{")
        .problems(None);
    assert!(bad[0].contains("not valid"), "{bad:?}");
}

#[test]
fn each_window_class_gets_exactly_the_shells_set_for_it() {
    let classes: BTreeMap<String, (Vec<String>, Vec<String>)> = capability_files()
        .into_iter()
        .map(|c| {
            let v = serde_json::to_value(&c).unwrap();
            let windows = v["windows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| w.as_str().unwrap().to_string())
                .collect();
            let perms = v["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap().to_string())
                .collect();
            (c.identifier, (windows, perms))
        })
        .collect();
    let expect = |w: &[&str], set: &str| {
        (
            w.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            vec![
                "core:event:default".to_string(),
                format!("paddock-shell:{set}"),
            ],
        )
    };
    assert_eq!(
        classes,
        BTreeMap::from([
            ("shell-dialog".to_string(), expect(&["d-*"], "dialog")),
            ("shell-main".to_string(), expect(&["main", "w-*"], "main")),
            ("shell-popout".to_string(), expect(&["p-*"], "popout")),
            (
                "shell-tray".to_string(),
                expect(&["tray", "tray-*"], "tray")
            ),
        ])
    );
}

#[test]
fn the_app_starts_with_its_identity_sections_and_updates() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let identity = app.handle().state::<AppIdentity>().inner().clone();
    assert_eq!(
        (
            identity.id.as_str(),
            identity.name.as_str(),
            identity.version.as_str()
        ),
        (APP_ID, APP_NAME, VERSION)
    );
    let version = call(&main, true, "shell_version", json!({})).unwrap();
    assert_eq!(version["app"]["name"], APP_NAME, "{version}");

    let overview = call(&main, true, "settings_overview", json!({})).unwrap();
    let ids: Vec<&str> = overview["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    for id in ["pd.appearance", "sd.general", "sd.storage"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let about = call(&main, true, "about_info", json!({})).unwrap();
    assert_eq!(about["appName"], APP_NAME, "{about}");
    let legal: Vec<&str> = about["legal"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert_eq!(legal, ["licence-mit", "licence-apache"]);
    // Nothing of the app's was written outside the test's folder: settings and messages are in memory.
    assert!(!dir.path().join("config/settings.json").exists());
}

#[test]
fn the_ai_access_settings_are_served_and_only_the_key_dialog_issues_keys() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let view = call(&main, true, "mcp_get", json!({})).unwrap();
    assert_eq!(
        view["settings"]["enabled"], false,
        "off until the user turns it on"
    );
    let tools: Vec<(&str, bool)> = view["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["name"].as_str().unwrap(), t["action"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        tools,
        [
            ("export_report", true),
            ("job_log", false),
            ("runner_status", false)
        ]
    );
    let saved = call(
        &main,
        true,
        "mcp_save",
        json!({"settings": {"enabled": true, "resources": true, "disabledTools": [], "allowedActions": []}}),
    )
    .unwrap();
    let shown: Vec<&str> = saved["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["exposed"] == true)
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        shown,
        ["job_log", "runner_status"],
        "an action stays hidden until it is allowed"
    );
    assert!(
        call(&main, true, "mcp_issue_key", json!({"client": "Pitwall"})).is_err(),
        "a main window cannot issue a key"
    );
}

#[test]
fn the_logging_section_is_the_apps_log_and_the_level_applies_at_once() {
    use paddock_env::{AppId, MapVars, Os};
    use paddock_log::{LogConfig, LogLevel, Logging};
    let dir = tempfile::tempdir().unwrap();
    let id = AppId::new(APP_ID, APP_NAME).unwrap();
    let vars = MapVars::new().with(&id.home_var(), dir.path());
    let dirs = paddock_env::AppDirs::resolve(&id, Os::current(), &vars, None, true).unwrap();
    let config = LogConfig::new(&id, "app", VERSION)
        .with_log_dir(dirs.logs())
        .with_stdout(false)
        .with_panic_hook(false);
    Logging::new(config).scoped(|handle| {
        let app = mock_builder()
            .plugin(
                shell_with_log::<MockRuntime>(
                    Place::Test {
                        dir: dir.path().to_path_buf(),
                    },
                    Some(handle.clone()),
                )
                .build(),
            )
            .invoke_handler(invoke_handler())
            .build(context())
            .unwrap();
        let main = window(&app, "main");
        let overview = call(&main, true, "settings_overview", json!({})).unwrap();
        let logging = overview["sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "pd.logging")
            .unwrap();
        assert_eq!(logging["values"]["level"], "info");
        let set = call(
            &main,
            true,
            "settings_set",
            json!({ "section": "pd.logging", "values": { "level": "warn" } }),
        )
        .unwrap();
        assert_eq!(set["applied"], true, "{set}");
        assert_eq!(handle.level(), LogLevel::Warn);
        // The report is made from the app's own folders, in the test's folder only.
        assert!(!dir.path().join("config/settings.json").exists());
    });
}

#[test]
fn a_broken_engine_is_repaired_and_a_broken_index_keeps_its_cause() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let health = events(&app, "paddock://service-health");

    call(
        &main,
        false,
        "demo_break",
        json!({ "service": "sd.engine" }),
    )
    .unwrap();
    let down = wait_for(&health, "the engine's outage", |s| {
        service(s, engine::ENGINE)["state"] == "down"
    });
    assert_eq!(service(&down, engine::ENGINE)["repair"], "restart");
    let status = call(&main, false, "demo_status", json!({})).unwrap();
    assert_eq!(status["engine"], "stopped", "{status}");

    call(
        &main,
        true,
        "health_repair",
        json!({ "service": "sd.engine" }),
    )
    .unwrap();
    wait_for(&health, "the engine's recovery", |s| {
        service(s, engine::ENGINE)["state"] == "ok"
    });
    assert_eq!(
        call(&main, false, "demo_status", json!({})).unwrap()["engine"],
        "running"
    );

    call(&main, false, "demo_break", json!({ "service": "sd.index" })).unwrap();
    let down = wait_for(&health, "the index's outage", |s| {
        service(s, engine::INDEX)["state"] == "down"
    });
    assert_eq!(service(&down, engine::INDEX)["repair"], "reset");
    call(
        &main,
        true,
        "health_repair",
        json!({ "service": "sd.index" }),
    )
    .unwrap();
    let failed = wait_for(&health, "the index's failed repair", |s| {
        service(s, engine::INDEX)["state"] == "down"
            && service(s, engine::INDEX)["cause"]["code"] == "sd.index.rebuild"
    });
    assert_eq!(
        service(&failed, engine::INDEX)["cause"]["hint"],
        "Free some disk space, then try again."
    );

    let notices = call(&main, true, "notifications_list", json!({})).unwrap();
    let causes: Vec<String> = notices["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["cause"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(causes.contains(&"health:sd.index".to_string()), "{notices}");
}

#[test]
fn a_service_the_demo_does_not_have_is_refused_at_the_edge() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    assert!(
        call(
            &main,
            false,
            "demo_break",
            json!({ "service": "sd.nothing" })
        )
        .is_err()
    );
    assert!(call(&main, false, "demo_job_start", json!({ "kind": "forever" })).is_err());
}

#[test]
fn the_import_goes_through_its_phases_on_their_own_scales_and_ends_done() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let jobs = events(&app, "paddock://jobs");
    let registry = engine::registry(app.handle()).unwrap();
    let (id, handle) = app
        .state::<Engine>()
        .start(&registry, DemoJob::Import)
        .unwrap();
    handle.join().unwrap();

    // Rust listeners hear each snapshot as it is sent, and the job's last one is sent before `join` returns.
    let snapshots: Vec<Value> = jobs.try_iter().filter(|s| s["id"] == id.0).collect();
    let mut phases: Vec<String> = Vec::new();
    let mut last = (String::new(), 0u64);
    let mut uncancellable = false;
    for s in &snapshots {
        let Some(p) = s.get("phase").filter(|p| !p.is_null()) else {
            continue;
        };
        let pid = p["id"].as_str().unwrap().to_string();
        let done = p["done"].as_u64().unwrap();
        if pid == last.0 {
            assert!(
                done >= last.1,
                "progress went back in {pid}: {} then {done}",
                last.1
            );
        } else {
            phases.push(pid.clone());
        }
        if pid == "find" {
            assert!(p["total"].is_null(), "finding files has no total: {s}");
        }
        if pid == "index" && s["cancellable"] == false {
            uncancellable = true;
        }
        last = (pid, done);
    }
    let end = snapshots.last().unwrap();
    assert_eq!(end["state"], "done", "{end}");
    assert_eq!(phases, ["find", "read", "index"]);
    assert!(uncancellable, "writing the index cannot be cancelled");
}

#[test]
fn a_quiet_job_runs_until_the_window_cancels_it() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let jobs = events(&app, "paddock://jobs");
    let id = call(&main, false, "demo_job_start", json!({ "kind": "quiet" })).unwrap();
    let id = id.as_u64().unwrap();
    wait_for(&jobs, "the quiet job running", |s| {
        s["id"] == id && s["state"] == "running"
    });
    let status = call(&main, false, "demo_status", json!({})).unwrap();
    assert_eq!(status["queued"], 1, "{status}");
    assert_eq!(
        call(&main, true, "job_cancel", json!({ "id": id })).unwrap(),
        true
    );
    let end = wait_for(&jobs, "the quiet job's end", |s| {
        s["id"] == id && s["state"] == "cancelled"
    });
    assert_eq!(end["reason"], "user", "{end}");
}

#[test]
fn each_command_answers_only_the_window_classes_its_table_names() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let tray = window(&app, "tray");
    let dialog = window(&app, "d-main-1");
    // The tray popover may start a job but not break a service; a dialog without an open contract reaches nothing.
    assert!(call(&tray, false, "demo_job_start", json!({ "kind": "import" })).is_ok());
    let refused = call(
        &tray,
        false,
        "demo_break",
        json!({ "service": "sd.engine" }),
    )
    .unwrap_err();
    assert_eq!(refused["code"], "denied", "{refused}");
    let refused = call(&dialog, false, "demo_status", json!({})).unwrap_err();
    assert_eq!(refused["code"], "denied", "{refused}");
}

#[test]
fn the_version_has_one_source_and_the_changelog_has_its_notes() {
    let conf: Value =
        serde_json::from_str(&std::fs::read_to_string(app_dir().join("tauri.conf.json")).unwrap())
            .unwrap();
    assert!(
        conf.get("version").is_none(),
        "tauri.conf.json must not carry a version: Cargo.toml is the single source"
    );
    assert_eq!(conf["productName"], APP_NAME);
    let parts: Vec<&str> = VERSION.split('.').collect();
    assert!(
        parts.len() == 3 && parts.iter().all(|p| p.parse::<u32>().is_ok()),
        "{VERSION} is a release number"
    );
    let changelog = std::fs::read_to_string(app_dir().join("../CHANGELOG.md")).unwrap();
    assert!(
        changelog
            .lines()
            .any(|l| l.starts_with("## ") && l.contains(VERSION)),
        "CHANGELOG.md has no section for {VERSION}"
    );
}

#[test]
fn the_tray_menu_is_valid_and_uses_the_shells_items() {
    let view = paddock_shell_demo::tray::view();
    assert_eq!(paddock_window::tray::validate(&view.menu), Ok(()));
    assert_eq!(paddock_shell_demo::tray::POPOVER_ROUTE, "index.html#/tray");
}

#[test]
fn the_tray_shows_what_needs_attention_and_repairs_it_through_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let tray = window(&app, "tray");
    let health = events(&app, "paddock://service-health");
    let calm = call(&tray, false, "demo_tray_state", json!({})).unwrap();
    assert_eq!(calm, json!({ "problems": [], "job": null }));

    call(
        &main,
        false,
        "demo_break",
        json!({ "service": "sd.engine" }),
    )
    .unwrap();
    wait_for(&health, "the engine's outage", |s| {
        service(s, engine::ENGINE)["state"] == "down"
    });
    let id = call(&tray, false, "demo_job_start", json!({ "kind": "quiet" })).unwrap();
    let state = call(&tray, false, "demo_tray_state", json!({})).unwrap();
    assert_eq!(state["problems"][0]["service"], engine::ENGINE, "{state}");
    assert_eq!(state["problems"][0]["repair"], "restart", "{state}");
    assert_eq!(state["job"]["id"], id, "{state}");

    call(
        &tray,
        false,
        "demo_tray_repair",
        json!({ "service": "sd.engine" }),
    )
    .unwrap();
    wait_for(&health, "the engine's recovery", |s| {
        service(s, engine::ENGINE)["state"] == "ok"
    });
    let refused = call(
        &tray,
        false,
        "demo_tray_repair",
        json!({ "service": "sd.engine" }),
    )
    .unwrap_err();
    assert_eq!(refused["code"], "refused", "{refused}");
    // The main window has the chip's Repair; the tray command is the tray's only.
    let denied = call(
        &main,
        false,
        "demo_tray_repair",
        json!({ "service": "sd.engine" }),
    )
    .unwrap_err();
    assert_eq!(denied["code"], "denied", "{denied}");
    call(&main, true, "job_cancel", json!({ "id": id })).unwrap();
}

/// The files the bundle ships for the agent are generated from its spec (plan X4): the launch agent in
/// `Contents/Library/LaunchAgents` and the NSIS hooks. A stale one fails; `PADDOCK_WRITE_TS=1` rewrites them.
#[test]
fn the_agents_bundled_plist_and_installer_hooks_are_generated_from_its_spec() {
    let plist = app_dir().join("macos/agent.plist");
    sync_generated(&plist, &agent::bundled_plist(), WriteMode::from_env()).unwrap();
    sync_generated(
        &app_dir().join("windows/installer-hooks.nsh"),
        &agent::nsis_hooks(),
        WriteMode::from_env(),
    )
    .unwrap();
    let mac: Value = serde_json::from_str(
        &std::fs::read_to_string(app_dir().join("tauri.macos.conf.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        mac["bundle"]["macOS"]["files"][format!("Library/LaunchAgents/{}.plist", agent::LABEL)],
        "macos/agent.plist"
    );
    let win: Value = serde_json::from_str(
        &std::fs::read_to_string(app_dir().join("tauri.windows.conf.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        win["bundle"]["windows"]["nsis"]["installerHooks"],
        "windows/installer-hooks.nsh"
    );
}

/// The agent in the app: its health entry, the Background service page's commands and the Start at login section, all
/// over the recording fake in tests (nothing is registered with the OS).
#[test]
fn the_agent_is_a_service_of_the_app_with_its_health_entry() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let status = call(&main, true, "service_status", json!({})).unwrap();
    assert_eq!(status["condition"], "notInstalled", "{status}");
    let health = call(&main, true, "health_snapshot", json!({})).unwrap();
    assert_eq!(
        service(&health, agent::HEALTH_ID)["repair"],
        "install",
        "{health}"
    );
    let overview = call(&main, true, "settings_overview", json!({})).unwrap();
    assert!(
        overview["sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "pd.login")
    );
}

/// The agent is the app's: same version (each read from its own Cargo.toml) and bundled as `externalBin`.
#[test]
fn the_agent_carries_the_apps_version_and_is_bundled_next_to_it() {
    assert_eq!(agent::VERSION, VERSION);
    assert_eq!(
        (agent::APP_ID, agent::APP_NAME),
        (APP_ID, APP_NAME),
        "the agent shares the app's folders"
    );
    let conf: Value =
        serde_json::from_str(&std::fs::read_to_string(app_dir().join("tauri.conf.json")).unwrap())
            .unwrap();
    assert_eq!(
        conf["bundle"]["externalBin"],
        json!(["binaries/shell-demo-agent"])
    );
    assert!(
        conf["build"]["beforeBuildCommand"]
            .as_str()
            .unwrap()
            .contains("sidecar.mjs")
    );
}
