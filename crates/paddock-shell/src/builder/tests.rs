//! The window and run-event glue of the plugin, on Tauri's mock runtime. The mock runtime does not deliver window or run
//! events by itself, so the hooks are called the way the runtime calls them.

use super::*;
use crate::setup::{NotificationAccess, PitwallPort, SetupItem};
use paddock_menu::NativeNode;
use paddock_menu::model::Item;
use paddock_menu::tauri::{NativeBar, NativeMenu};
use paddock_startup::{InstanceScope, Lifecycle, Mode, lifecycle::Start};
use paddock_window::tauri::WindowConfig;
use std::path::{Path, PathBuf};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// A menu bar that shows nothing (macOS builds native menus on the main thread only).
struct NoBar;

impl NativeBar<MockRuntime> for NoBar {
    fn build(&self, _: &AppHandle<MockRuntime>, _: &[NativeNode]) -> tauri::Result<()> {
        Ok(())
    }
    fn set_states(&self, _: &AppHandle<MockRuntime>, _: &[Item]) -> tauri::Result<bool> {
        Ok(true)
    }
}

fn identity() -> AppIdentity {
    AppIdentity::new("demo", "Shell Demo", "1.2.3")
}

fn shell() -> Builder<MockRuntime> {
    Builder::new(identity())
        .in_memory()
        .native_menu(false)
        .windows(|_| Windows::builder(WindowConfig::new("Shell Demo")).build())
}

fn app(builder: Builder<MockRuntime>) -> tauri::App<MockRuntime> {
    mock_builder()
        .plugin(builder.build())
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn window(app: &tauri::App<MockRuntime>, label: &str) -> Window<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
        .as_ref()
        .window()
}

/// Files called `running.json` below `dir`: the run lifecycle's marker.
fn markers(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(d).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.file_name().is_some_and(|n| n == "running.json") {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn the_menu_bar_follows_the_focus_and_the_close_of_every_window() {
    let app = app(shell().setup(|app| {
        app.manage(NativeMenu::<MockRuntime>::with_bar(
            "Shell Demo",
            false,
            NoBar,
        ));
        Ok(())
    }));
    let menu = app.state::<NativeMenu<MockRuntime>>();
    let w1 = window(&app, "w-1");
    assert_eq!(menu.target(), None);
    on_window_event(&w1, &WindowEvent::Focused(true));
    assert_eq!(menu.target().as_deref(), Some("w-1"));
    // Losing the focus moves nothing: the menu bar stays with the last focused window.
    on_window_event(&w1, &WindowEvent::Focused(false));
    assert_eq!(menu.target().as_deref(), Some("w-1"));
    on_window_event(&w1, &WindowEvent::Destroyed);
    assert_eq!(menu.target(), None);
}

/// Dyno closes its last window to the tray (the window is destroyed) and keeps running; the menu bar still shows
/// "Quit", and choosing it must quit the app although no window is the target any more.
#[test]
fn quit_from_the_menu_bar_works_when_the_last_window_was_destroyed() {
    let exits = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = exits.clone();
    let app = app(Builder::new(identity())
        .in_memory()
        .native_menu(false)
        .windows(move |_| {
            Windows::builder(WindowConfig::new("Shell Demo"))
                .exit_app(move |_, code| seen.lock().unwrap().push(code))
                .build()
        })
        .setup(|app| {
            app.manage(NativeMenu::<MockRuntime>::with_bar(
                "Shell Demo",
                true,
                NoBar,
            ));
            Ok(())
        }));
    let menu = app.state::<NativeMenu<MockRuntime>>();
    let quit = serde_json::json!({ "menus": [{ "id": "app", "label": "Shell Demo", "items": [
        { "kind": "item", "id": "pd.app.quit", "label": "Quit Shell Demo", "keybinding": null, "checked": null, "enabled": true }
    ] }] });
    let model = paddock_menu::model::parse(serde_json::from_value(quit).unwrap()).unwrap();
    menu.apply(app.handle(), &model).unwrap();
    assert!(menu.shown_ids().contains(&"pd.app.quit".to_string()));
    let w = window(&app, "main");
    on_window_event(&w, &WindowEvent::Focused(true));
    on_window_event(&w, &WindowEvent::Destroyed);
    assert_eq!(menu.target(), None);
    paddock_menu::tauri::menu_chosen(app.handle(), "pd.app.quit");
    assert_eq!(*exits.lock().unwrap(), [0], "Quit asks the app to exit");
}

/// An app with its own quit path (Pitwall) replaces the shell's.
#[test]
fn the_app_can_replace_what_quit_does_without_a_window() {
    let asked = Arc::new(std::sync::Mutex::new(0));
    let seen = asked.clone();
    let app = app(shell()
        .menu_quit_without_window(move |_| *seen.lock().unwrap() += 1)
        .setup(|app| {
            app.manage(NativeMenu::<MockRuntime>::with_bar(
                "Shell Demo",
                true,
                NoBar,
            ));
            Ok(())
        }));
    let menu = app.state::<NativeMenu<MockRuntime>>();
    let quit = serde_json::json!({ "menus": [{ "id": "app", "label": "Shell Demo", "items": [
        { "kind": "item", "id": "pd.app.quit", "label": "Quit Shell Demo", "keybinding": null, "checked": null, "enabled": true }
    ] }] });
    let model = paddock_menu::model::parse(serde_json::from_value(quit).unwrap()).unwrap();
    menu.apply(app.handle(), &model).unwrap();
    paddock_menu::tauri::menu_chosen(app.handle(), "pd.app.quit");
    assert_eq!(*asked.lock().unwrap(), 1);
}

/// The policy for a click with no window open, on both kinds of shell: Quit goes to the override, else the window layer's
/// quit, else the given exit; any other item only reopens the main window where there is a window layer.
/// A tray's items arrive at the same menu-event handler as the menu bar's (`NativeMenu::on_no_window` takes every id the
/// menu bar shows, and only those), so a tray id must never be a menu-bar id. The tray's are `pd.show` and `pd.quit`; the
/// menu bar's Quit is `pd.app.quit`.
#[test]
fn tray_ids_and_menu_bar_ids_do_not_overlap() {
    let tray = [paddock_window::tray::SHOW_ID, paddock_window::tray::QUIT_ID];
    assert!(!tray.contains(&paddock_menu::model::QUIT_COMMAND));
    assert!(
        tray.iter().all(|id| !id.starts_with("pd.app.")),
        "{tray:?}: pd.app.* is the menu bar's app menu"
    );
}

#[test]
fn a_click_without_a_window_quits_or_brings_the_main_window_back() {
    let plain = app(Builder::new(identity()).in_memory().native_menu(false));
    assert!(plain.try_state::<Windows<MockRuntime>>().is_none());
    let exited = std::cell::Cell::new(0);
    menu_without_window_using(plain.handle(), "pd.app.quit", None, |_| {
        exited.set(exited.get() + 1)
    });
    assert_eq!(exited.get(), 1, "no window layer: the exit given");
    // Another item has nothing to reopen without a window layer.
    menu_without_window_using(plain.handle(), "pd.about", None, |_| {
        unreachable!("not a quit")
    });
    assert!(plain.get_webview_window("main").is_none());
    // With an override, Quit is the app's.
    let own = Arc::new(std::sync::Mutex::new(0));
    let seen = own.clone();
    let quit: Arc<MenuQuit<MockRuntime>> = Arc::new(move |_| *seen.lock().unwrap() += 1);
    menu_without_window_using(plain.handle(), "pd.app.quit", Some(&*quit), |_| {
        unreachable!("the app quits")
    });
    assert_eq!(*own.lock().unwrap(), 1);
}

/// The installed handler: a click on a shell that has no window layer does nothing for an item that is not Quit.
#[test]
fn a_menu_click_without_a_window_layer_reaches_the_policy() {
    let app = app(Builder::new(identity())
        .in_memory()
        .native_menu(false)
        .setup(|app| {
            app.manage(NativeMenu::<MockRuntime>::with_bar(
                "Shell Demo",
                true,
                NoBar,
            ));
            Ok(())
        }));
    let menu = app.state::<NativeMenu<MockRuntime>>();
    let about = serde_json::json!({ "menus": [{ "id": "app", "label": "Shell Demo", "items": [
        { "kind": "item", "id": "pd.about", "label": "About", "keybinding": null, "checked": null, "enabled": true }
    ] }] });
    let model = paddock_menu::model::parse(serde_json::from_value(about).unwrap()).unwrap();
    menu.apply(app.handle(), &model).unwrap();
    assert_eq!(
        paddock_menu::tauri::menu_chosen(app.handle(), "pd.about"),
        None
    );
    assert!(app.get_webview_window("main").is_none());
}

/// A menu item chosen with no window open brings the main window back, after the hook, like a Dock click.
#[test]
fn a_menu_item_without_a_window_reopens_the_main_window() {
    let app = app(shell());
    menu_without_window(app.handle(), "pd.settings.open", None);
    wait_for_main(&app);
}

#[test]
fn window_events_without_the_window_layer_still_reach_the_other_parts() {
    let app = app(Builder::new(identity())
        .in_memory()
        .native_menu(false)
        .setup(|app| {
            app.manage(NativeMenu::<MockRuntime>::with_bar(
                "Shell Demo",
                false,
                NoBar,
            ));
            Ok(())
        }));
    assert!(app.try_state::<Windows<MockRuntime>>().is_none());
    let w = window(&app, "main");
    on_window_event(&w, &WindowEvent::Focused(true));
    assert_eq!(
        app.state::<NativeMenu<MockRuntime>>().target().as_deref(),
        Some("main")
    );
    on_window_event(&w, &WindowEvent::Destroyed);
}

#[test]
fn a_clean_exit_ends_the_run_by_removing_the_running_marker() {
    let dir = tempfile::tempdir().unwrap();
    let life = Lifecycle::begin_with(
        Start {
            data_dir: dir.path(),
            process: "demo",
            mode: Mode::Normal,
            scope: InstanceScope::Main,
            app_version: "1.2.3",
        },
        4242,
        &|_| None,
    );
    let startup = Startup::builder().lifecycle(life).build().unwrap();
    let app = app(shell().startup(startup));
    assert_eq!(markers(dir.path()).len(), 1, "the run is marked as running");
    on_run_event(app.handle(), &RunEvent::Ready);
    assert_eq!(markers(dir.path()).len(), 1, "only the exit ends the run");
    on_run_event(app.handle(), &RunEvent::Exit);
    assert!(
        markers(dir.path()).is_empty(),
        "a clean exit leaves no marker"
    );
}

#[test]
fn run_events_without_the_window_layer_or_the_parts_are_harmless() {
    let app = app(Builder::new(identity()).in_memory().native_menu(false));
    assert!(app.try_state::<Windows<MockRuntime>>().is_none());
    on_run_event(app.handle(), &RunEvent::Ready);
    on_run_event(app.handle(), &RunEvent::Exit);
}

/// A custom step of an app.
struct Tour;

impl SetupItem for Tour {
    fn id(&self) -> &str {
        "tour"
    }
    fn title(&self) -> String {
        "Tour".into()
    }
    fn detail(&self) -> String {
        "A short tour.".into()
    }
    fn check(&self) -> crate::setup::Check {
        crate::setup::Check::Todo
    }
    fn act(&self) -> Result<(), String> {
        Ok(())
    }
}

struct Asks;

impl NotificationAccess for Asks {
    fn permission(&self) -> paddock_health::notify::os::Permission {
        paddock_health::notify::os::Permission::Unknown
    }
    fn request(&self) -> paddock_health::notify::os::Permission {
        paddock_health::notify::os::Permission::Granted
    }
    fn open_system_settings(&self) -> Result<(), String> {
        Ok(())
    }
    fn asks(&self) -> bool {
        true
    }
}

struct NoPitwall;

impl PitwallPort for NoPitwall {
    fn probe(&self) -> paddock_pitwall::LinkView {
        paddock_pitwall::LinkView::default()
    }
    fn connect(&self) -> Result<(), String> {
        Ok(())
    }
    fn start(&self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn the_first_run_checklist_has_the_steps_the_app_names_after_the_services() {
    use paddock_service::backend::{Target, WindowsRun};
    use paddock_service::manager::Manager;
    use paddock_service::spec::ServiceSpec;
    use paddock_service::testing::FakeSystem;
    let builder = shell()
        .service(|_| {
            let spec = ServiceSpec::builder("net.a.b", "A", "a").build().unwrap();
            let backend =
                WindowsRun::new(spec, Target::exe("C:\\a.exe"), Box::new(FakeSystem::new()));
            let manager = Manager::new(
                "A",
                Box::new(backend),
                Path::new("/nonexistent-paddock-test"),
                "net.a.b",
                "1",
                || false,
            );
            Ok(ServicePart::new(
                paddock_service::tauri::Setup::new(manager).checks(
                    std::time::Duration::ZERO,
                    std::time::Duration::ZERO,
                    0,
                ),
            ))
        })
        .first_run(|_| {
            FirstRun::new()
                .notifications(Arc::new(Asks))
                .pitwall(Arc::new(NoPitwall), paddock_pitwall::Caps::chat(1024))
                .item(Arc::new(Tour))
        });
    let app = app(builder);
    let list = app.state::<Arc<crate::setup::Checklist>>().inner().clone();
    let ids: Vec<String> = list.state().items.into_iter().map(|i| i.id).collect();
    assert_eq!(
        ids,
        ["service", "login", "notifications", "pitwall", "tour"]
    );
    // Without the part the checklist is not there at all.
    let plain = self::app(shell());
    assert!(plain.try_state::<Arc<crate::setup::Checklist>>().is_none());
}

#[test]
fn a_bad_first_run_step_fails_the_start_naming_the_checklist() {
    struct Bad;
    impl SetupItem for Bad {
        fn id(&self) -> &str {
            "Bad Id"
        }
        fn title(&self) -> String {
            String::new()
        }
        fn detail(&self) -> String {
            String::new()
        }
        fn check(&self) -> crate::setup::Check {
            crate::setup::Check::Todo
        }
        fn act(&self) -> Result<(), String> {
            Ok(())
        }
    }
    let result = mock_builder()
        .plugin(
            shell()
                .first_run(|_| FirstRun::new().item(Arc::new(Bad)))
                .build(),
        )
        .build(mock_context(noop_assets()));
    let err = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("the start must fail"),
    };
    assert!(err.contains("set-up checklist"), "{err}");
}

/// Waits until the work deferred so far has run (the deferred work runs in order, so a marker queued last arrives
/// after it; a hang guard only lost work reaches), then checks the main window exists.
fn wait_for_main(app: &tauri::App<MockRuntime>) {
    let (tx, rx) = std::sync::mpsc::channel();
    paddock_window::tauri::after_plugin_hook(app.handle(), move |_| {
        let _ = tx.send(());
    });
    rx.recv_timeout(std::time::Duration::from_secs(30))
        .expect("the deferred work ran");
    assert!(
        app.get_webview_window("main").is_some(),
        "the main window opened"
    );
}

/// A start that cannot open its window ends the app through the window layer's exit (code 1), never a panic; a start
/// that worked ends nothing.
#[test]
fn a_start_that_cannot_open_its_window_exits_with_code_1() {
    let exits = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = exits.clone();
    let app = app(Builder::new(identity())
        .in_memory()
        .native_menu(false)
        .windows(move |_| {
            Windows::builder(WindowConfig::new("Shell Demo"))
                .exit_app(move |_, code| seen.lock().unwrap().push(code))
                .build()
        }));
    started(app.handle(), Ok(()));
    assert!(exits.lock().unwrap().is_empty());
    started(
        app.handle(),
        Err(WindowError::new(
            paddock_window::WindowErrorCode::Failed,
            "no screen",
        )),
    );
    assert_eq!(*exits.lock().unwrap(), [1]);
}

/// A Dock click with no window open (macOS) opens the main window after the run-event hook, not inside it (Tauri holds
/// its plugin store there).
#[cfg(target_os = "macos")]
#[test]
fn a_dock_click_with_no_window_opens_the_main_window_after_the_hook() {
    // `RunEvent::Reopen` cannot be built outside Tauri (non-exhaustive): its branch calls this.
    let app = app(shell());
    let _scope = paddock_window::tauri::plugin_hook_scope();
    reopen_after_hook(app.handle());
    wait_for_main(&app);
}

/// A file opened from Finder with no window open (macOS) is handed to the window layer after the hook.
#[cfg(target_os = "macos")]
#[test]
fn a_file_opened_from_finder_opens_the_main_window_after_the_hook() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.log");
    std::fs::write(&file, "x").unwrap();
    let app = app(shell());
    let _scope = paddock_window::tauri::plugin_hook_scope();
    on_run_event(
        app.handle(),
        &RunEvent::Opened {
            urls: vec![tauri::Url::from_file_path(&file).unwrap()],
        },
    );
    wait_for_main(&app);
}

#[test]
fn the_default_settings_live_in_the_given_file_or_in_memory() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(crate::logging::SHELL_SETTINGS_FILE);
    let _ = settings_at(Some(file.clone()));
    let _ = settings_at(None);
    assert!(
        !dir.path().join(paddock_settings::FILE_NAME).exists(),
        "the app's own settings.json is never created"
    );
}

/// An app's own menu item (Dyno's "Quit and stop measuring") clicked with no window open runs the app's action and does
/// not bring the window back; other items keep the shell's policy.
#[test]
fn an_apps_menu_action_without_a_window_runs_instead_of_reopening() {
    let plain = app(Builder::new(identity()).in_memory().native_menu(false));
    let ran = Arc::new(std::sync::Mutex::new(0));
    let seen = ran.clone();
    let action: Arc<MenuQuit<MockRuntime>> = Arc::new(move |_| *seen.lock().unwrap() += 1);
    let actions = vec![("app.stop".to_string(), action)];
    menu_without_window_with(plain.handle(), "app.stop", None, &actions, |_| unreachable!("not a quit"));
    assert_eq!(*ran.lock().unwrap(), 1);
    menu_without_window_with(plain.handle(), "pd.app.quit", None, &actions, |_| ());
    assert_eq!(*ran.lock().unwrap(), 1, "Quit is not the app's action");
}
