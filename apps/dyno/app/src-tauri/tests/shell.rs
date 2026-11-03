//! The shell plugin with Dyno's parts, on Tauri's mock runtime: nothing is drawn, nothing outside the temp folder
//! is touched (rule 7).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_app_lib::dialogs::DIALOGS;
use dyno_app_lib::health::Hub;
use dyno_app_lib::{shell, Place, COMMANDS, MANIFEST};
use paddock_commands::testing::{sync_generated, WriteMode};

fn builder() -> paddock_shell::Builder<tauri::test::MockRuntime> {
    let dir = tempfile::tempdir().unwrap();
    shell(
        Place::Test {
            dir: dir.path().to_path_buf(),
        },
        &Hub::default(),
        std::sync::Arc::new(dyno_app_lib::shell::NoPixels),
    )
}

/// The contract file, the manifest, Dyno's table and the shell's table agree (a dialog contract naming a missing or
/// closed command, a command both tables use, a service or settings section the UI does not know).
#[test]
fn the_dialog_contracts_the_manifest_and_the_tables_agree() {
    let problems = builder().problems(Some(MANIFEST));
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The manifest repeats each dialog's kind, modality, instancing and size; they must be what Rust declares (a stale manifest
/// would hide a changed contract from the UI's own lint).
#[test]
fn the_manifest_repeats_each_dialogs_contract_exactly() {
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

/// Dyno's services are the ones the UI registers, in the status bar's order.
#[test]
fn the_services_are_the_ones_the_ui_lists() {
    assert_eq!(builder().service_ids(), vec!["dy.agent"]);
}

/// Dyno's table never shadows a shell command.
#[test]
fn no_command_is_in_both_tables() {
    for spec in COMMANDS.specs() {
        assert!(paddock_shell::COMMANDS.spec(spec.name()).is_none(), "{}", spec.name());
    }
}

const REGENERATE: &str = "cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell";

/// The UI's command names are generated from the table, so a typo in a dialog contract is a compile error.
#[test]
fn the_uis_command_names_are_generated_from_the_table() {
    let text = paddock_commands::ts::names_module(&COMMANDS, "APP_COMMANDS", "AppCommand", REGENERATE).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/app-commands.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// The UI's dialog contracts are generated from the dialogs table, so Rust and TypeScript cannot disagree (ADR 0026 §3).
#[test]
fn the_uis_dialog_contracts_are_generated_from_the_table() {
    let text = paddock_dialogs::ts::contracts_module("the app", &DIALOGS, REGENERATE);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/dialog-contracts.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// Every command runs on the async runtime and returns at once (rule 10).
#[test]
fn every_command_is_async() {
    paddock_commands::testing::assert_async(&COMMANDS, &[&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")]);
}

// --- Capabilities per window class, resolved through Tauri's own ACL ---

use paddock_shell::PLUGIN_NAME;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tauri_utils::acl::capability::Capability;
use tauri_utils::acl::manifest::{Manifest, PermissionFile};
use tauri_utils::acl::resolved::Resolved;

fn capability_files() -> Vec<Capability> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities"))
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

/// The plugin's permission files, as its build step lists them (`DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH`, see build.rs).
fn plugin_permissions() -> Vec<PermissionFile> {
    let list = std::fs::read_to_string(env!("PADDOCK_SHELL_PERMISSION_FILES")).unwrap();
    let paths: Vec<PathBuf> = serde_json::from_str(&list).unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("window-classes.toml")),
        "the plugin lists its window-class sets: {paths:?}"
    );
    paths
        .iter()
        .map(|p| toml::from_str(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

/// Every capability resolves against the plugin's permissions (a permission the plugin does not have fails here). Core
/// permissions are left out: the mock runtime has no core plugins to resolve them against.
#[test]
fn the_capabilities_resolve_against_the_shell_plugins_permissions() {
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
    Resolved::resolve(&acl, caps, tauri_utils::platform::Target::current()).unwrap();
}

/// Each window class gets exactly the shell's set for it, and nothing of its own.
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
                        "core:window:allow-start-dragging",
                        "core:window:allow-internal-toggle-maximize",
                        "paddock-shell:main"
                    ])
                )
            ),
            (
                "shell-tray".to_string(),
                (owned(&["tray", "tray-*"]), owned(&["core:event:default", "paddock-shell:tray"]))
            ),
        ])
    );
}

/// The update feed is the moving release that dyno-release.yml publishes latest.json on (installed copies read it).
#[test]
fn the_update_feed_is_the_release_the_workflow_publishes_latest_json_on() {
    let workflow =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../.gitea/workflows/dyno-release.yml"))
            .unwrap();
    let tag = workflow
        .lines()
        .find_map(|l| l.trim().strip_prefix("latest-tag:"))
        .map(|t| t.trim().to_string())
        .expect("the workflow names the moving release");
    assert_eq!(
        dyno_app_lib::UPDATE_FEED,
        format!("https://dev.decryptology.net/decryp7/paddock/releases/download/{tag}/latest.json")
    );
}

/// Dyno's start as `run` composes it (rule 43), on the mock runtime with a headless tray: the tray and the main window
/// open, after every plugin is set up (a window built inside a plugin hook deadlocked Dyno's start).
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
    // The link thread creates its folder after the start returns; wait on that, with a hang guard only a lost thread reaches.
    let began = std::time::Instant::now();
    while !dir.path().join("data").is_dir() {
        assert!(
            began.elapsed() < std::time::Duration::from_secs(30),
            "the agent link never started, in the test's folder"
        );
        std::thread::yield_now();
    }
}

/// A tray click opens the shell's popover, which needs a spec registered at the start: the browser build's e2e of the `#/tray` page
/// cannot see that (the 1.0 VM test found a tray that did nothing: "no tray popover is set up in this app").
#[test]
fn the_start_registers_the_tray_popover_a_click_opens() {
    use paddock_window::tauri::Popovers;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let has = composed(&data, paddock_window::launch::LaunchArgs::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .map(|mut app| {
                // One iteration runs Tauri's set-up (the mock runtime has no event loop).
                #[allow(deprecated)]
                app.run_iteration(|_, _| {});
                tauri::Manager::try_state::<Popovers>(app.handle()).map(|p| p.has(None))
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(has);
    });
    // A hang guard only a deadlocked start reaches.
    let has = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("the start finished");
    assert_eq!(
        has,
        Ok(Some(true)),
        "Dyno registers no tray popover, so a click on its tray icon opens nothing"
    );
}

/// About's "See all releases" calls `files_open_url`, which needs the shell's user-files service: without `Builder::files` the
/// command fails with a missing-state error and the link does nothing (the 1.0.0 report).
#[test]
fn the_start_sets_up_the_user_files_service_the_about_link_needs() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let has = composed(&data, paddock_window::launch::LaunchArgs::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .map(|mut app| {
                // One iteration runs Tauri's set-up (the mock runtime has no event loop).
                #[allow(deprecated)]
                app.run_iteration(|_, _| {});
                tauri::Manager::try_state::<std::sync::Arc<paddock_files::Files>>(app.handle()).is_some()
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(has);
    });
    let has = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("the start finished");
    assert_eq!(has, Ok(true), "files_open_url has no service: About's releases link cannot open the browser");
}

/// Started at log-in (`--hidden`) with a tray: Dyno stays in the tray without a window.
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

/// Dyno as `run` composes it, the agent link included (its files in `dir`, no agent process, no tray icons), without the
/// OS plugins.
fn composed(dir: &std::path::Path, launch: paddock_window::launch::LaunchArgs) -> tauri::Builder<tauri::test::MockRuntime> {
    let shell = shell(
        Place::Test { dir: dir.to_path_buf() },
        &Hub::default(),
        std::sync::Arc::new(dyno_app_lib::shell::NoPixels),
    );
    dyno_app_lib::app(tauri::test::mock_builder(), shell, launch, paddock_window::tauri::HeadlessTray)
}

/// Nothing is drawn: the menu bar of the test is a bar that shows nothing (macOS builds native menus on the main thread
/// only).
struct NoBar;

impl paddock_menu::tauri::NativeBar<tauri::test::MockRuntime> for NoBar {
    fn build(&self, _: &tauri::AppHandle<tauri::test::MockRuntime>, _: &[paddock_menu::NativeNode]) -> tauri::Result<()> {
        Ok(())
    }
    fn set_states(&self, _: &tauri::AppHandle<tauri::test::MockRuntime>, _: &[paddock_menu::model::Item]) -> tauri::Result<bool> {
        Ok(true)
    }
}

/// The model Dyno's main window sends (its app menu: Settings and Quit with Mod+Q; `QUIT_CONTRIBUTIONS`).
fn dyno_menu_model() -> paddock_menu::model::AppMenu {
    let item = |id: &str, label: &str, key: Option<&str>| serde_json::json!({ "kind": "item", "id": id, "label": label, "keybinding": key, "checked": null, "enabled": true });
    let wire = serde_json::json!({ "menus": [{ "id": "app", "label": "Dyno", "items": [
        item("pd.settings.open", "Settings\u{2026}", Some("Mod+,")),
        item("pd.app.quit", "Quit", Some("Mod+Q")),
    ] }] });
    paddock_menu::model::parse(serde_json::from_value(wire).unwrap()).unwrap()
}

/// Quit in the menu bar (Cmd+Q) with Dyno's last window closed to the tray: the window is gone, the tray keeps the app
/// running, and Quit still begins the quit (the exit's teardown, Dyno's agent among it) instead of being dropped.
#[test]
fn quit_from_the_menu_bar_works_after_the_last_window_closed_to_the_tray() {
    use paddock_menu::tauri::{menu_chosen, NativeMenu};
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::{Manager, WindowEvent};
    let dir = tempfile::tempdir().unwrap();
    let shell = shell(
        Place::Test {
            dir: dir.path().to_path_buf(),
        },
        &Hub::default(),
        std::sync::Arc::new(dyno_app_lib::shell::NoPixels),
    )
    .setup(|app| {
        app.manage(NativeMenu::<tauri::test::MockRuntime>::with_bar("Dyno", true, NoBar));
        Ok(())
    });
    let builder = dyno_app_lib::app(
        mock_builder(),
        shell,
        paddock_window::launch::LaunchArgs::default(),
        paddock_window::tauri::HeadlessTray,
    );
    let mut app = builder.build(mock_context(noop_assets())).unwrap();
    // One iteration runs Tauri's set-up, which opens the tray and the main window.
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
    let menu = app.state::<NativeMenu<tauri::test::MockRuntime>>();
    let model = dyno_menu_model();
    let quit = paddock_menu::native_spec(&model, "Dyno");
    assert!(
        paddock_menu::native::outline(&quit).contains("Quit <pd.app.quit>  CmdOrCtrl+Q"),
        "the app menu has Quit with Cmd+Q"
    );
    menu.apply(app.handle(), &model).unwrap();
    let windows = app.state::<paddock_window::tauri::Windows<tauri::test::MockRuntime>>();
    let main = app.get_webview_window("main").expect("the start opened the main window");
    let window = main.as_ref().window();
    paddock_menu::tauri::on_window_event(&window, &WindowEvent::Focused(true));
    assert_eq!(menu.target().as_deref(), Some("main"));
    // Closed to the tray: the window is destroyed, the app keeps running.
    paddock_menu::tauri::on_window_event(&window, &WindowEvent::Destroyed);
    assert_eq!(menu.target(), None, "no window is the menu bar's target");
    assert!(!windows.quitting());
    menu_chosen(app.handle(), "pd.app.quit");
    assert!(windows.quitting(), "Quit began the quit");
}

// ---- the popup's wiring: visibility, navigation, readings ----

mod widget_wiring {
    use super::*;
    use dyno_app_lib::link::Sink;
    use dyno_app_lib::shell::{NoPixels, ShellOf};
    use dyno_core::protocol::{LiveUpdate, Overhead};
    use dyno_core::time::Timestamp;
    use dyno_core::widgets::popover_values::{Navigate, Page};
    use dyno_core::widgets::{Metric, ReadingQuery, Thresholds};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::{Listener, WebviewUrl, WebviewWindowBuilder};

    fn app() -> tauri::App<MockRuntime> {
        mock_builder().build(mock_context(noop_assets())).unwrap()
    }

    fn shell(app: &tauri::App<MockRuntime>) -> Arc<ShellOf<MockRuntime>> {
        ShellOf::new(app.handle().clone(), Hub::default(), Arc::new(NoPixels))
    }

    fn update(cpu: f32) -> LiveUpdate {
        let mut system = dyno_platform::fake::quiet_sample(Timestamp(1_000));
        system.cpu.total = cpu;
        LiveUpdate {
            system,
            pressure: dyno_core::metrics::MemoryPressure::Normal,
            top_cpu: vec![],
            top_memory: vec![],
            overhead: Overhead::default(),
            processes: None,
            groups: None,
            other_users: None,
        }
    }

    /// The popup's values reach the webview only while the popup is visible.
    #[test]
    fn values_are_emitted_only_while_the_popup_is_visible() {
        let app = app();
        let shell = shell(&app);
        let visible = Arc::new(AtomicBool::new(false));
        let gate = visible.clone();
        shell.set_popover_gate(Arc::new(move || gate.load(Ordering::SeqCst)));
        let (tx, rx) = channel::<String>();
        app.listen("dyno://popover-values", move |e| {
            let _ = tx.send(e.payload().to_string());
        });
        shell.live(update(11.0));
        visible.store(true, Ordering::SeqCst);
        shell.live(update(77.0));
        // Events arrive in order: the first one is the visible tick's, so the hidden tick sent nothing.
        let first = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a values event while visible");
        assert!(first.contains(r#""cpuPercent":77.0"#), "{first}");
        visible.store(false, Ordering::SeqCst);
        shell.live(update(12.0));
        visible.store(true, Ordering::SeqCst);
        shell.live(update(88.0));
        let next = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(
            next.contains(r#""cpuPercent":88.0"#),
            "the hidden tick between sent nothing: {next}"
        );
    }

    /// An open popup counts as visible at once and gets what is known; it does not wait for the next tick.
    #[test]
    fn opening_the_popup_returns_the_values_known_now() {
        let app = app();
        let shell = shell(&app);
        shell.live(update(42.0));
        let v = shell.popover_opened();
        assert!(v.notice.is_none());
        assert!(v.placements.iter().any(|p| p.id.as_str() == "cpu-memory"), "{v:?}");
    }

    /// Opening counts as visible even when the shell's own gate says no; a preview's snapshot does not.
    #[test]
    fn opening_marks_the_popup_visible_at_once_and_a_snapshot_does_not() {
        let app = app();
        let shell = shell(&app);
        shell.set_popover_gate(Arc::new(|| false));
        shell.live(update(5.0));
        let _ = shell.popover_snapshot();
        assert!(!shell.popover_wants_values(), "a preview never opens the popup");
        let _ = shell.popover_opened();
        assert!(shell.popover_wants_values());
    }

    /// The commands' bodies: a snapshot and a preview never mark the popup visible (only the popup's own first paint does).
    #[test]
    fn the_snapshot_and_preview_commands_never_open_the_popup() {
        let app = app();
        let shell = shell(&app);
        shell.live(update(5.0));
        shell.set_popover_gate(Arc::new(|| false));
        dyno_app_lib::commands::snapshot_value(&shell).unwrap();
        assert!(!shell.popover_wants_values(), "snapshot_value opened the popup");
        let v = dyno_app_lib::commands::preview_value(&shell).unwrap();
        assert!(!shell.popover_wants_values(), "preview_value opened the popup");
        assert!(
            v["placements"]
                .as_array()
                .is_some_and(|p| p.iter().any(|x| x["id"] == "cpu-memory")),
            "{v}"
        );
    }

    /// With no link to ask, the wish to ask is kept (the on-change kinds stay due), not used up.
    #[test]
    fn asking_with_no_link_keeps_the_wish() {
        let app = app();
        let shell = shell(&app);
        shell.live(update(5.0));
        let _ = shell.popover_opened();
        assert!(shell.popover_refresh_wanted(), "the ask found no link and must stay due");
    }

    /// A navigation waits for the main window, once, and the event goes to the main window only.
    #[test]
    fn a_navigation_is_kept_for_the_main_window_and_goes_to_it_alone() {
        let app = app();
        let main = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default()).build().unwrap();
        let tray = WebviewWindowBuilder::new(&app, "tray", WebviewUrl::default()).build().unwrap();
        let (main_tx, main_rx) = channel::<String>();
        let (tray_tx, tray_rx) = channel::<String>();
        main.listen("dyno://navigate", move |e| {
            let _ = main_tx.send(e.payload().to_string());
        });
        tray.listen("dyno://navigate", move |e| {
            let _ = tray_tx.send(e.payload().to_string());
        });
        let shell = shell(&app);
        shell.navigate(Navigate::Page(Page::Alarms));
        assert_eq!(
            main_rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),
            r#"{"page":"alarms"}"#
        );
        assert!(tray_rx.try_recv().is_err(), "the tray window is not told");
        // A window that was still being created misses the event and asks for it as it starts, once.
        assert_eq!(shell.take_navigation(), Some(Navigate::Page(Page::Alarms)));
        assert_eq!(shell.take_navigation(), None);
    }

    /// The dashboard's and the previews' readings come from the newest live update, with tones from their thresholds.
    #[test]
    fn readings_follow_the_newest_live_update() {
        let app = app();
        let shell = shell(&app);
        let q = vec![ReadingQuery {
            metric: Metric::CpuTotal,
            thresholds: Thresholds {
                warning: Some(70.0),
                critical: Some(90.0),
            },
        }];
        assert_eq!(
            shell.metric_readings(&q)[0].reason.as_deref(),
            Some("Waiting for the first reading.")
        );
        shell.live(update(95.0));
        let r = &shell.metric_readings(&q)[0];
        assert_eq!(r.value, Some(95.0));
        assert_eq!(r.tone, dyno_core::widgets::dashboard::ValueTone::Bad);
        shell.live(update(10.0));
        assert_eq!(shell.metric_readings(&q)[0].value, Some(10.0));
    }
}

/// Report a problem resolves the real folders (the app's own, and the machine's), which a test never does (ER-14): the test place
/// composes the shell without it. Pinned on the composed builder, not on a helper, so a changed guard in `shell` is seen.
#[test]
fn a_test_place_composes_the_shell_without_report_a_problem() {
    let dir = tempfile::tempdir().unwrap();
    let test_shell = shell::<tauri::test::MockRuntime>(
        Place::Test {
            dir: dir.path().to_path_buf(),
        },
        &Hub::default(),
        std::sync::Arc::new(dyno_app_lib::shell::NoPixels),
    );
    assert!(!test_shell.diagnostics_installed(), "a test place must not set up Report a problem");
}
