//! The Tauri adapter on Tauri's mock runtime: the commands through the guarded invoke path, the `paddock://settings` event
//! to every window, the start-up problem as one notification, dialog contracts and the routing (plan S10).
#![cfg(feature = "tauri")]

use paddock_commands::DialogGrant;
use paddock_commands::testing::assert_async;
use paddock_health::notify::NotificationCentre;
use paddock_settings::tauri::{
    COMMANDS, EVENT, PROBLEM_CAUSE, handler, handler_with_grants, manage, route,
};
use paddock_settings::{FieldKind, SectionOwner, SettingField, Settings, SettingsSection, shell};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[tauri::command]
async fn ping() -> String {
    "pong".into()
}

struct Grant(&'static [&'static str]);

impl DialogGrant for Grant {
    fn dialog(&self) -> &str {
        "pd.settings"
    }
    fn allows(&self, command: &str) -> bool {
        self.0.contains(&command)
    }
}

fn app_with(
    settings: impl Fn(tauri::ipc::Invoke<MockRuntime>) -> bool + Send + Sync + 'static,
) -> App<MockRuntime> {
    mock_builder()
        .invoke_handler(route(settings, tauri::generate_handler![ping]))
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
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

fn general() -> SettingsSection {
    SettingsSection::new("sd.general", "General", SectionOwner::App)
        .field(SettingField::new(
            "repeat",
            "Repeat",
            FieldKind::Int {
                min: 1,
                max: 10,
                step: 1,
                unit: None,
            },
            1,
        ))
        .field(
            SettingField::new(
                "volume",
                "Volume",
                FieldKind::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.1,
                    unit: None,
                },
                0.5,
            )
            .needs_restart(),
        )
}

fn events(app: &App<MockRuntime>) -> Arc<Mutex<Vec<Value>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    app.listen_any(EVENT, move |e| {
        sink.lock()
            .unwrap()
            .push(serde_json::from_str(e.payload()).unwrap())
    });
    seen
}

#[test]
fn the_event_is_named_by_the_shell_scheme() {
    assert_eq!(EVENT, "paddock://settings");
}

#[test]
fn a_window_reads_changes_and_resets_and_every_window_hears_the_change() {
    let app = app_with(handler());
    let main = window(&app, "main");
    let second = window(&app, "w-1");
    let seen = events(&app);
    let settings = manage(app.handle(), Settings::in_memory());
    assert!(
        Arc::ptr_eq(&settings, &manage(app.handle(), Settings::in_memory())),
        "a second set-up keeps the first"
    );
    settings.register(Arc::new(shell::appearance())).unwrap();
    settings.register(Arc::new(general())).unwrap();

    let overview = invoke(&second, "settings_overview", json!({})).unwrap();
    let ids: Vec<&str> = overview["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["pd.appearance", "sd.general"]);
    assert_eq!(
        overview["sections"][1]["values"],
        json!({ "repeat": 1, "volume": 0.5 })
    );
    assert_eq!(
        (
            overview["restartPending"].clone(),
            overview["problem"].clone()
        ),
        (json!(false), Value::Null)
    );

    let set = invoke(
        &main,
        "settings_set",
        json!({ "section": "sd.general", "values": { "volume": 0.8 } }),
    )
    .unwrap();
    assert_eq!(set["applied"], true);
    assert_eq!(set["restartRequired"], true);
    let refused = invoke(
        &main,
        "settings_set",
        json!({ "section": "sd.general", "values": { "repeat": 0 } }),
    )
    .unwrap();
    assert_eq!(refused["applied"], false);
    assert_eq!(
        refused["errors"],
        json!([{ "key": "repeat", "message": "Must be between 1 and 10." }])
    );
    let reset = invoke(
        &main,
        "settings_reset",
        json!({ "section": "sd.general", "key": "volume" }),
    )
    .unwrap();
    assert_eq!(reset["values"], json!({ "repeat": 1, "volume": 0.5 }));
    invoke(
        &main,
        "settings_set",
        json!({ "section": "pd.appearance", "values": { "theme": "dark" } }),
    )
    .unwrap();
    let all = invoke(
        &main,
        "settings_reset",
        json!({ "section": "pd.appearance" }),
    )
    .unwrap();
    assert_eq!(all["values"]["theme"], "system");

    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen.len(),
        4,
        "volume, its reset, the theme and its reset: {seen:?}"
    );
    assert_eq!(
        seen[0],
        json!({ "section": "sd.general", "values": { "repeat": 1, "volume": 0.8 }, "restartPending": true })
    );
    assert_eq!(
        invoke(&second, "settings_overview", json!({})).unwrap()["restartPending"],
        true
    );
}

#[test]
fn errors_come_back_with_a_code_and_a_message() {
    let app = app_with(handler());
    let main = window(&app, "main");
    let e = invoke(&main, "settings_overview", json!({})).unwrap_err();
    assert_eq!(
        e,
        json!({ "code": "notSetUp", "message": "Settings are not set up in this app.", "retryable": false })
    );
    assert_eq!(
        invoke(
            &main,
            "settings_set",
            json!({ "section": "x", "values": {} })
        )
        .unwrap_err()["code"],
        "notSetUp"
    );

    let settings = manage(app.handle(), Settings::in_memory());
    settings
        .register(paddock_settings::section(general(), |v| {
            match v.int("repeat") {
                Some(7) => Err("seven is unlucky".into()),
                _ => Ok(()),
            }
        }))
        .unwrap();
    let unknown = invoke(
        &main,
        "settings_set",
        json!({ "section": "sd.none", "values": {} }),
    )
    .unwrap_err();
    assert_eq!(unknown["code"], "unknownSection");
    let field = invoke(
        &main,
        "settings_reset",
        json!({ "section": "sd.general", "key": "nope" }),
    )
    .unwrap_err();
    assert_eq!(field["code"], "unknownField");
    let apply = invoke(
        &main,
        "settings_set",
        json!({ "section": "sd.general", "values": { "repeat": 7 } }),
    )
    .unwrap_err();
    assert_eq!(
        (apply["code"].as_str(), apply["retryable"].as_bool()),
        (Some("applyFailed"), Some(false))
    );
    assert!(
        apply["message"]
            .as_str()
            .unwrap()
            .contains("seven is unlucky")
    );
    // Malformed arguments are refused by Tauri before the service is asked.
    assert!(invoke(&main, "settings_set", json!({ "section": 3, "values": {} })).is_err());
}

#[test]
fn a_damaged_file_is_one_notification_and_defaults() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("settings.json");
    std::fs::write(&path, "{ damaged").unwrap();
    let app = app_with(handler());
    let main = window(&app, "main");
    paddock_health::tauri::init(app.handle(), paddock_health::tauri::Setup::new(Vec::new()))
        .unwrap();
    let settings = manage(app.handle(), Settings::open(&path));
    settings.register(Arc::new(general())).unwrap();
    // A second start of the adapter does not post again.
    manage(app.handle(), Settings::open(&path));

    let centre = app.state::<Arc<NotificationCentre>>();
    let listed = centre.snapshot("main").notices;
    assert_eq!(listed.len(), 1, "{listed:?}");
    let notice = &listed[0];
    assert_eq!(notice.title, "Settings were reset to defaults");
    assert!(
        notice.body.contains("settings.json.corrupt"),
        "{}",
        notice.body
    );
    assert_eq!(
        notice.primary.as_ref().map(|a| a.kind.as_str()),
        Some("openSettings")
    );
    assert_eq!(notice.count, 1);
    assert!(PROBLEM_CAUSE.starts_with("settings:"));

    let overview = invoke(&main, "settings_overview", json!({})).unwrap();
    assert_eq!(overview["problem"]["kind"], "damaged");
    assert_eq!(
        overview["sections"][0]["values"],
        json!({ "repeat": 1, "volume": 0.5 })
    );
}

#[test]
fn without_a_notification_centre_the_problem_is_only_shown_in_settings() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("settings.json");
    std::fs::write(&path, "[]").unwrap();
    let app = app_with(handler());
    let main = window(&app, "main");
    manage(app.handle(), Settings::open(&path));
    assert_eq!(
        invoke(&main, "settings_overview", json!({})).unwrap()["problem"]["kind"],
        "damaged"
    );
}

#[test]
fn dialogs_change_settings_only_through_their_contract() {
    let plain = app_with(handler());
    manage(plain.handle(), Settings::in_memory())
        .register(Arc::new(general()))
        .unwrap();
    let dialog = window(&plain, "d-main-1");
    assert_eq!(
        invoke(&dialog, "settings_overview", json!({})).unwrap_err()["code"],
        "denied"
    );

    let granted = app_with(handler_with_grants(|_app, label| {
        (label == "d-main-1").then_some(Grant(&["settings_overview", "settings_set"]))
    }));
    manage(granted.handle(), Settings::in_memory())
        .register(Arc::new(general()))
        .unwrap();
    let dialog = window(&granted, "d-main-1");
    assert!(invoke(&dialog, "settings_overview", json!({})).is_ok());
    assert_eq!(
        invoke(
            &dialog,
            "settings_set",
            json!({ "section": "sd.general", "values": { "repeat": 2 } })
        )
        .unwrap()["applied"],
        true
    );
    let e = invoke(
        &dialog,
        "settings_reset",
        json!({ "section": "sd.general" }),
    )
    .unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["dialog"].as_str()),
        (Some("denied"), Some("pd.settings"))
    );
    let popout = window(&granted, "p-main-1");
    assert!(
        invoke(&popout, "settings_overview", json!({})).is_ok(),
        "every window may read"
    );
    assert_eq!(
        invoke(
            &popout,
            "settings_set",
            json!({ "section": "sd.general", "values": {} })
        )
        .unwrap_err()["code"],
        "denied"
    );
}

#[test]
fn other_commands_reach_the_apps_handler() {
    let app = app_with(handler());
    let main = window(&app, "main");
    assert_eq!(invoke(&main, "ping", json!({})), Ok("pong".into()));
}

#[test]
fn every_command_is_async_and_in_the_table() {
    assert_async(
        &COMMANDS,
        &[&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
    );
    assert_eq!(
        COMMANDS.kind_of("settings_overview"),
        paddock_commands::Kind::Query
    );
    assert_eq!(
        COMMANDS.kind_of("settings_set"),
        paddock_commands::Kind::Mutation
    );
    assert_eq!(
        COMMANDS.kind_of("settings_reset"),
        paddock_commands::Kind::Mutation
    );
}

#[test]
fn a_change_with_missing_or_mistyped_arguments_is_refused_before_it_runs() {
    let app = app_with(handler());
    let main = window(&app, "main");
    let settings = manage(app.handle(), Settings::in_memory());
    settings.register(Arc::new(general())).unwrap();
    for (cmd, body) in [
        ("settings_set", json!({ "values": {} })),
        ("settings_set", json!({ "section": 3, "values": {} })),
        ("settings_reset", json!({})),
        ("settings_reset", json!({ "section": ["sd.general"] })),
    ] {
        assert!(invoke(&main, cmd, body).is_err(), "{cmd}");
    }
    assert_eq!(
        settings.values("sd.general").unwrap().get("repeat"),
        Some(&json!(1))
    );
}
