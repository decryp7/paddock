//! Pitwall on the shell plugin, on Tauri's mock runtime: nothing is drawn, nothing outside the test's folder is touched,
//! no keychain is read (ER-14). The start as `run` composes it, without the OS plugins (single instance, autostart,
//! updater).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_shell::testing::{start, Started};
use paddock_window::launch::LaunchArgs;
use pitwall_app_lib::{app, shell, Place, COMMANDS};

/// Pitwall as `run` composes it, its state and live feed included (its files in `dir`, a server that is not there).
fn composed(dir: &std::path::Path, launch: LaunchArgs) -> tauri::Builder<tauri::test::MockRuntime> {
    let shell = shell(Place::Test { dir: dir.to_path_buf() });
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

/// The macOS app menu's Quit is Pitwall's: the UI contributes the shell's Quit command (placed last by the shell) and "Quit
/// Pitwall and stop serving", both run through Pitwall's quit flow, and Mod+Q quits.
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
    assert!(tray.contains(&"overview") && tray.contains(&"set_serving"), "{tray:?}");
    assert!(!tray.contains(&"delete_model") && !tray.contains(&"save_settings"), "{tray:?}");
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

/// Pitwall wires its Offline gate into the shell's updater: before the server ever answered, a check is refused as
/// "could not be confirmed" without touching the network; after a live `server` frame says Offline mode is off, it passes
/// the gate (begun, not run: no network in a test); after one says it is on, it is refused as Offline mode.
#[test]
fn the_updater_is_gated_by_what_the_server_said_about_offline_mode() {
    use paddock_updater::{tauri::Updates, UpdateError};
    use pitwall_app_lib::offline::OfflineKnown;
    use std::sync::Arc;
    use tauri::Manager;
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
    let updates = app.state::<Arc<Updates>>().inner().clone();
    let known = app.state::<Arc<OfflineKnown>>().inner().clone();
    assert_eq!(
        updates.updater.begin_check().unwrap_err(),
        UpdateError::OfflineUnknown,
        "first run: not confirmed"
    );
    known.observe_frame(r#"{"kind":"server","server":{"offline":false}}"#);
    // Past the gate: a test build has no signing key, so the next answer is "not available in this build" (or, with a key,
    // a started check, abandoned before it runs).
    match updates.updater.begin_check() {
        Ok((_, ticket)) => updates.updater.abandon(ticket),
        Err(e) => assert_eq!(e, UpdateError::NotAvailable, "the offline gate let it through"),
    }
    known.observe_frame(r#"{"kind":"server","server":{"offline":true}}"#);
    assert_eq!(updates.updater.begin_check().unwrap_err(), UpdateError::OfflineMode);
    assert!(dir.path().join("offline-known.json").is_file(), "the last confirmed value is kept");
}

/// Quit from the macOS app menu with no window open must still quit (or ask). Today the shell's menu routes `pd.app.quit`
/// to a window and drops it when none is open (paddock-menu `route`); the shared fix (a "no target window" handler, branch
/// `fix/menu-quit-without-window`) is not on this branch yet. Run with `--ignored` to see it fail.
#[test]
#[ignore = "fails until the shell's no-window menu handler lands (fix/menu-quit-without-window); then Pitwall wires quit::request to it"]
fn quit_from_the_menu_with_no_window_open_still_quits_or_asks() {
    use tauri::Manager;
    let dir = tempfile::tempdir().unwrap();
    // Started in the tray (`--hidden`): no window is open, as after the user closed the main window.
    let app = composed(
        dir.path(),
        LaunchArgs {
            hidden: true,
            ..Default::default()
        },
    )
    .build(tauri::test::mock_context(tauri::test::noop_assets()))
    .unwrap();
    assert!(app.get_webview_window("main").is_none(), "no window is open");
    paddock_window::tauri::menu_chosen(app.handle(), "pd.app.quit");
    let pending = app.state::<pitwall_app_lib::quit::PendingAsk>().take();
    let quitting = app.state::<paddock_window::tauri::Windows<tauri::test::MockRuntime>>().quitting();
    assert!(pending || quitting, "the menu's Quit asked or quit with no window open");
}

/// The tray popover's "Open Pitwall" and "Review": with no window open (main is destroyed when closed), the popover asks the
/// shell to focus `main` (`window_focus`), which builds it again; then it closes itself (`tray_close`). The tray's
/// capability grants the shell's tray set, and that set holds both commands.
#[test]
fn the_tray_popover_opens_the_main_window_when_none_is_open() {
    use tauri::Manager;
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
