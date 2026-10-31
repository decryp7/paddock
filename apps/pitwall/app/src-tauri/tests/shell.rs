//! Pitwall on the shell plugin, on Tauri's mock runtime: nothing is drawn, nothing outside the test's folder is touched,
//! no keychain is read (ER-14). The start as `run` composes it, without the OS plugins (single instance, autostart,
//! updater).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_shell::testing::{start, Started};
use paddock_window::launch::LaunchArgs;
use pitwall_app_lib::commands::SharedClient;
use pitwall_app_lib::quit::{MenuQuit, PendingAsk, QUIT_STOP_COMMAND};
use pitwall_app_lib::server_client::{unavailable, BoxFuture, FrameStream, ServerClient};
use pitwall_app_lib::{app, shell, Place, COMMANDS};
use pitwall_core::mgmt::CommandError;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

/// Pitwall as `run` composes it, its state and live feed included (its files in `dir`, a server that is not there).
fn composed(dir: &std::path::Path, launch: LaunchArgs) -> tauri::Builder<tauri::test::MockRuntime> {
    let shell = shell(Place::Test {
        dir: dir.to_path_buf(),
        server: None,
    });
    app(tauri::test::mock_builder(), shell, launch, paddock_window::tauri::HeadlessTray)
}

/// The start (rule 43, ADR 0029): the tray and the main window open, after every plugin is set up (a window built inside
/// a plugin hook deadlocks the start).
#[test]
fn the_start_opens_the_tray_and_the_main_window() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    assert_eq!(
        start(move || composed(&data, LaunchArgs::default())),
        Ok(Started {
            main_window: true,
            tray: true
        })
    );
}

/// Started at log-in (`--hidden`) with a tray: Pitwall stays in the tray without a window.
#[test]
fn a_hidden_start_stays_in_the_tray() {
    let launch = LaunchArgs {
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

/// The UI's contributions (`paddock-manifest.json`, written by the UI's contributions spec).
const MANIFEST: &str = include_str!("../../paddock-manifest.json");

/// The shell's Quit command (paddock-menu's `QUIT_COMMAND`), which the macOS app menu places last.
const QUIT_COMMAND: &str = "pd.app.quit";

/// The manifest, the dialog contracts, Pitwall's table and the shell's agree: no command in both tables, every name valid,
/// every dialog of the UI declared in Rust and every contract registered in the UI, no menu or key naming a missing
/// command, no health service or settings section on one side only.
#[test]
fn the_manifest_the_contracts_and_the_tables_agree() {
    let dir = tempfile::tempdir().unwrap();
    let problems = shell::<tauri::test::MockRuntime>(Place::Test {
        dir: dir.path().to_path_buf(),
        server: None,
    })
    .problems(Some(MANIFEST));
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The manifest repeats each of Pitwall's dialog contracts exactly (a stale manifest would hide a changed contract from the
/// UI's own lint).
#[test]
fn the_manifest_repeats_each_dialogs_contract_exactly() {
    use pitwall_app_lib::dialogs::DIALOGS;
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    let contracts = paddock_dialogs::Contracts::from_tables(&[&DIALOGS]).unwrap();
    for entry in DIALOGS.entries() {
        let m = &manifest["dialogs"][entry.id];
        let c = contracts.get(entry.id).unwrap();
        assert_eq!(m["width"].as_f64(), Some(entry.width_px()), "{} width", entry.id);
        assert_eq!(m["height"].as_f64(), entry.height_px(), "{} height", entry.id);
        assert_eq!(m["instanced"].as_bool(), Some(entry.instanced), "{} instanced", entry.id);
        assert_eq!(
            m["requiredChoice"].as_bool(),
            Some(entry.required_choice),
            "{} requiredChoice",
            entry.id
        );
        assert_eq!(
            m["modality"].as_str(),
            Some(format!("{:?}", c.modality).to_lowercase().as_str()),
            "{} modality",
            entry.id
        );
        assert_eq!(
            m["kind"].as_str(),
            Some(format!("{:?}", c.kind).to_lowercase().as_str()),
            "{} kind",
            entry.id
        );
    }
}

/// The macOS app menu's Quit is the shell's (contributed by the frames, with Mod+Q) and runs Pitwall's quit flow through
/// `APP_QUIT`; "Quit Pitwall and stop serving" is Pitwall's own item. Both are in the manifest.
#[test]
fn the_app_menu_quits_through_pitwall() {
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    let in_app_menu = |id: &str| {
        manifest["menus"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["menu"] == "app" && m["command"] == id)
    };
    assert!(in_app_menu(QUIT_COMMAND) && in_app_menu("pw.quit-stop"));
    assert!(manifest["keybindings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|k| k["command"] == QUIT_COMMAND && k["key"] == "Mod+Q"));
}

/// Every command the UI calls is in the table, the chat has room for its largest request, and the tray popover may
/// reach only the few commands it shows.
#[test]
fn the_command_table_covers_the_ui_and_narrows_the_tray() {
    let names: Vec<&str> = COMMANDS.specs().iter().map(|s| s.name()).collect();
    // Every command the UI's Tauri backend names (`run('x')`, `job('x')`, `invoke('x')`) is one of Pitwall's: a command the
    // table lacks is refused by the guard at run time.
    let backend = include_str!("../../src/app/core/tauri-backend.ts");
    let mut called = Vec::new();
    for opener in ["run('", "job('", "invoke('", "run<", "job<", "invoke<"] {
        for (i, _) in backend.match_indices(opener) {
            let rest = &backend[i + opener.len()..];
            let rest = rest
                .split_once("('")
                .map_or(rest, |(head, tail)| if head.len() < 40 { tail } else { rest });
            let name: String = rest.chars().take_while(|c| c.is_ascii_lowercase() || *c == '_').collect();
            if !name.is_empty() && rest[name.len()..].starts_with('\'') {
                called.push(name);
            }
        }
    }
    assert!(called.len() > 40, "the UI's commands were found: {called:?}");
    for name in &called {
        assert!(names.contains(&name.as_str()), "the UI calls `{name}`, which is not in COMMANDS");
    }
    let chat = COMMANDS.spec("chat").unwrap();
    assert!(
        chat.limit() >= pitwall_app_lib::commands::MAX_CHAT_BYTES,
        "a chat of the largest accepted size fits"
    );
    let tray: Vec<&str> = COMMANDS
        .specs()
        .iter()
        .filter(|s| s.windows().contains(paddock_commands::WindowClass::Tray))
        .map(|s| s.name())
        .collect();
    assert!(!tray.contains(&"delete_model") && !tray.contains(&"save_settings"), "{tray:?}");
}

/// The tray popup reaches exactly what it shows and does (least privilege): the popup's layout, values, header and shape, serving,
/// free memory, cancelling a download and quit. The pairing and update calls of the old popover are the main window's.
#[test]
fn the_tray_popup_reaches_exactly_its_commands() {
    let mut tray: Vec<&str> = COMMANDS
        .specs()
        .iter()
        .filter(|s| s.windows().contains(paddock_commands::WindowClass::Tray))
        .map(|s| s.name())
        .collect();
    tray.sort_unstable();
    assert_eq!(
        tray,
        [
            "cancel_download",
            "popover_header",
            "popover_shape",
            "popover_values",
            "popover_view",
            "quit_from_tray",
            "release_memory",
            "set_serving"
        ]
    );
    // Editing the layout is the Settings window's; the popup only reads it.
    let edit = COMMANDS.spec("edit_popover").unwrap();
    assert!(!edit.windows().contains(paddock_commands::WindowClass::Tray));
    assert!(edit.windows().contains(paddock_commands::WindowClass::Dialog));
}

const REGENERATE: &str = "cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell";

/// The UI's command names are generated from the table, so a typo in a dialog contract is a compile error.
#[test]
fn the_uis_command_names_are_generated_from_the_table() {
    let text = paddock_commands::ts::names_module(&COMMANDS, "APP_COMMANDS", "AppCommand", REGENERATE).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/app-commands.ts");
    paddock_commands::testing::sync_generated(&path, &text, paddock_commands::testing::WriteMode::from_env())
        .unwrap_or_else(|e| panic!("{e}"));
}

/// The UI's dialog contracts are generated from the dialogs table, so Rust and TypeScript cannot disagree (ADR 0026 §3).
#[test]
fn the_uis_dialog_contracts_are_generated_from_the_table() {
    let text = paddock_dialogs::ts::contracts_module("Pitwall", &pitwall_app_lib::dialogs::DIALOGS, REGENERATE);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/dialog-contracts.ts");
    paddock_commands::testing::sync_generated(&path, &text, paddock_commands::testing::WriteMode::from_env())
        .unwrap_or_else(|e| panic!("{e}"));
}

/// Every command runs on the async runtime and returns at once (rule 10). The shared scanner cannot see the management
/// methods (`commands.rs` makes them with the `rpc!` macro), so this reads the sources itself: every `#[tauri::command]`
/// is an `async fn`, the macro's own included, and every table entry is either a written command or an `rpc!` one.
#[test]
fn every_command_is_async() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut written = Vec::new();
    let mut by_macro = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let src = std::fs::read_to_string(&path).unwrap();
        for (i, _) in src.match_indices("#[tauri::command]") {
            let after = src[i..].lines().nth(1).unwrap_or_default().trim().to_string();
            assert!(
                after.starts_with("pub async fn "),
                "{}: a command that is not async: {after}",
                path.display()
            );
            let name: String = after["pub async fn ".len()..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            written.push(name);
        }
        for (i, _) in src.match_indices("\nrpc!(") {
            let body = &src[i..];
            if let Some(arrow) = body.find(" =>") {
                let head = &body[..arrow];
                let name: String = head.rsplit(char::is_whitespace).next().unwrap_or_default().to_string();
                by_macro.push(name);
            }
        }
    }
    assert!(written.iter().any(|n| n == "$name"), "the rpc! macro's command is async");
    for spec in COMMANDS.specs() {
        let name = spec.name().to_string();
        assert!(
            written.contains(&name) || by_macro.contains(&name),
            "{name} is in the table but no async command of that name was found"
        );
    }
}

/// Nothing is drawn: the menu bar of the test shows nothing (macOS builds native menus on the main thread only).
struct NoBar;

impl paddock_menu::tauri::NativeBar<tauri::test::MockRuntime> for NoBar {
    fn build(&self, _: &tauri::AppHandle<tauri::test::MockRuntime>, _: &[paddock_menu::NativeNode]) -> tauri::Result<()> {
        Ok(())
    }
    fn set_states(&self, _: &tauri::AppHandle<tauri::test::MockRuntime>, _: &[paddock_menu::model::Item]) -> tauri::Result<bool> {
        Ok(true)
    }
}

/// A stand-in for the Pitwall server: answers `settings` with the "When you quit" value it was given, records every call.
struct QuitServer {
    on_quit: &'static str,
    calls: std::sync::Mutex<Vec<String>>,
}

impl QuitServer {
    fn new(on_quit: &'static str) -> Arc<QuitServer> {
        Arc::new(QuitServer {
            on_quit,
            calls: Default::default(),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl ServerClient for QuitServer {
    fn call<'a>(
        &'a self,
        method: &'a str,
        _: serde_json::Value,
        _: Option<Duration>,
    ) -> BoxFuture<'a, Result<serde_json::Value, CommandError>> {
        self.calls.lock().unwrap().push(method.to_string());
        let reply = match method {
            "settings" => serde_json::json!({ "general": { "onQuit": self.on_quit } }),
            _ => serde_json::Value::Null,
        };
        Box::pin(async move { Ok(reply) })
    }

    fn open_stream<'a>(
        &'a self,
        _: &'a str,
        _: Option<serde_json::Value>,
        _: Duration,
    ) -> BoxFuture<'a, Result<Box<dyn FrameStream>, CommandError>> {
        Box::pin(async { Err(unavailable()) })
    }
}

/// Pitwall started in the tray (`--hidden`: no window is open, as after the user closed the main window) over `server`, with
/// the menu bar showing Quit and "Quit and stop serving".
fn tray_only_app(dir: &std::path::Path, server: Arc<QuitServer>) -> tauri::App<tauri::test::MockRuntime> {
    use paddock_menu::tauri::NativeMenu;
    let shell = shell(Place::Test {
        dir: dir.to_path_buf(),
        server: Some(server as SharedClient),
    })
    .setup(|app| {
        app.manage(NativeMenu::<tauri::test::MockRuntime>::with_bar("Pitwall", true, NoBar));
        Ok(())
    });
    let launch = LaunchArgs {
        hidden: true,
        ..Default::default()
    };
    let app = app(tauri::test::mock_builder(), shell, launch, paddock_window::tauri::HeadlessTray)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    assert!(app.get_webview_window("main").is_none(), "no window is open");
    let item =
        |id: &str| serde_json::json!({ "kind": "item", "id": id, "label": id, "keybinding": null, "checked": null, "enabled": true });
    let wire =
        serde_json::json!({ "menus": [{ "id": "app", "label": "Pitwall", "items": [item("pd.app.quit"), item(QUIT_STOP_COMMAND)] }] });
    let model = paddock_menu::model::parse(serde_json::from_value(wire).unwrap()).unwrap();
    app.state::<NativeMenu<tauri::test::MockRuntime>>()
        .apply(app.handle(), &model)
        .unwrap();
    app
}

/// Chooses `id` in the menu bar with no window open and waits for the quit the menu started (its task, not a time; the guard
/// only bounds a hang).
fn choose_without_window(app: &tauri::App<tauri::test::MockRuntime>, id: &str) {
    paddock_menu::tauri::menu_chosen(app.handle(), id);
    let task = app.state::<MenuQuit>().take().expect("the menu started Pitwall's quit");
    tauri::async_runtime::block_on(async move { tokio::time::timeout(Duration::from_secs(40), task).await })
        .expect("the quit finished")
        .expect("the quit did not panic");
}

fn quitting(app: &tauri::App<tauri::test::MockRuntime>) -> bool {
    app.state::<paddock_window::tauri::Windows<tauri::test::MockRuntime>>().quitting()
}

/// Quit from the menu bar with no window open (the main window was closed to the tray) runs Pitwall's own flow
/// (`Builder::menu_quit_without_window`), by the "When you quit" setting: "Ask each time" asks and does not quit.
#[test]
fn quit_from_the_menu_with_no_window_open_asks_when_the_setting_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let server = QuitServer::new("ask");
    let app = tray_only_app(dir.path(), server.clone());
    choose_without_window(&app, "pd.app.quit");
    assert!(app.state::<PendingAsk>().take(), "the question is waiting for a window");
    assert!(!quitting(&app), "asking does not quit");
    assert!(!server.calls().contains(&"stop_serving".to_string()));
}

/// The same with the setting "Stop serving": the server is told to stop before the quit begins.
#[test]
fn quit_from_the_menu_with_no_window_open_stops_serving_when_the_setting_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let server = QuitServer::new("stopServing");
    let app = tray_only_app(dir.path(), server.clone());
    choose_without_window(&app, "pd.app.quit");
    assert!(server.calls().contains(&"stop_serving".to_string()), "serving was stopped");
    assert!(quitting(&app), "then the quit began");
    assert!(!app.state::<PendingAsk>().take());
}

/// "Quit Pitwall and stop serving" from the menu bar with no window open does not just bring the window back (as any other
/// item does): it stops serving and quits, whatever the setting says.
#[test]
fn quit_and_stop_serving_from_the_menu_with_no_window_open_stops_and_quits() {
    let dir = tempfile::tempdir().unwrap();
    let server = QuitServer::new("ask");
    let app = tray_only_app(dir.path(), server.clone());
    choose_without_window(&app, QUIT_STOP_COMMAND);
    assert!(server.calls().contains(&"stop_serving".to_string()), "serving was stopped");
    assert!(quitting(&app), "then the quit began");
    assert!(app.get_webview_window("main").is_none(), "the window did not come back");
}

/// The tray popover's "Open Pitwall" and "Review": with no window open (main is destroyed when closed), the popover asks the
/// shell to focus `main` (`window_focus`), which builds it again; then it closes itself (`tray_close`). The tray's
/// capability grants the shell's tray set, and that set holds both commands.
#[test]
fn the_tray_popover_opens_the_main_window_when_none_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let app = composed(
        dir.path(),
        LaunchArgs {
            hidden: true,
            ..Default::default()
        },
    )
    .build(tauri::test::mock_context(tauri::test::noop_assets()))
    .unwrap();
    assert!(app.get_webview_window("main").is_none(), "started in the tray: no window");
    // `window_focus` is `paddock_window::tauri::reveal` (crates/paddock-window/src/tauri/commands.rs); Tauri's mock context
    // carries no plugin ACL, so the command's body is called here and the grant is checked from the files below.
    paddock_window::tauri::reveal(app.handle(), "main").expect("window_focus(main) from the tray");
    assert!(
        app.get_webview_window("main").is_some(),
        "window_focus from the tray built the main window again"
    );

    let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/tray.json")).unwrap();
    assert!(caps["permissions"].as_array().unwrap().iter().any(|p| p == "paddock-shell:tray"));
    let sets = include_str!("../../../../../crates/paddock-shell/permissions/window-classes.toml");
    let tray_set = &sets[sets.find("identifier = \"tray\"").unwrap()..];
    let tray_set = &tray_set[..tray_set.find("]").unwrap()];
    for p in ["allow-window-focus", "allow-tray-close"] {
        assert!(tray_set.contains(p), "the shell's tray set grants {p}");
    }
}
