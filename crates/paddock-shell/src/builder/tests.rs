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

/// Waits (with a hang guard only lost work reaches) until the main window exists.
fn wait_for_main(app: &tauri::App<MockRuntime>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while app.get_webview_window("main").is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the main window never opened"
        );
        std::thread::yield_now();
    }
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
    let report = StartReport::new(None);
    started(app.handle(), &report, Ok(()));
    assert!(exits.lock().unwrap().is_empty());
    started(
        app.handle(),
        &report,
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

/// A tray backend for a desktop without a tray.
struct NoTrayHere;

impl paddock_window::tauri::TrayBackend<MockRuntime> for NoTrayHere {
    fn create(
        &self,
        _: &AppHandle<MockRuntime>,
        _: &paddock_window::tray::TrayView,
    ) -> Result<(), String> {
        Err("no status-notifier host".into())
    }
    fn update(
        &self,
        _: &AppHandle<MockRuntime>,
        _: &paddock_window::tray::TrayView,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// The start's messages also reach the app's own log (its `start_log` hook): a desktop without a tray, and a start that
/// cannot open its window.
#[test]
fn the_start_messages_reach_the_apps_own_log() {
    let lines = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = lines.clone();
    let started_app = crate::testing::start(move || {
        Builder::new(identity())
            .in_memory()
            .native_menu(false)
            .windows(|_| Windows::builder(WindowConfig::new("Shell Demo")).build())
            .tray(|_| {
                paddock_window::tauri::Tray::new(NoTrayHere, |_| {
                    paddock_window::tray::TrayView::new("Shell Demo", vec![], "normal")
                })
            })
            .launch(LaunchArgs::default())
            .start_log(move |line| seen.lock().unwrap().push(line.to_string()))
            .add_to(mock_builder())
    });
    assert_eq!(
        started_app,
        Ok(crate::testing::Started {
            main_window: true,
            tray: false
        })
    );
    let got = lines.lock().unwrap().clone();
    assert!(got.iter().any(|l| l.contains("No tray icon")), "{got:?}");

    let exits = Arc::new(std::sync::Mutex::new(Vec::new()));
    let codes = exits.clone();
    let app = app(Builder::new(identity())
        .in_memory()
        .native_menu(false)
        .windows(move |_| {
            Windows::builder(WindowConfig::new("Shell Demo"))
                .exit_app(move |_, code| codes.lock().unwrap().push(code))
                .build()
        }));
    let heard = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let h = heard.clone();
    let report = StartReport::new(Some(Arc::new(move |line: &str| {
        h.lock().unwrap().push(line.to_string())
    })));
    started(
        app.handle(),
        &report,
        Err(WindowError::new(
            paddock_window::WindowErrorCode::Failed,
            "no screen",
        )),
    );
    assert_eq!(*exits.lock().unwrap(), [1]);
    let heard = heard.lock().unwrap().clone();
    assert!(
        heard
            .iter()
            .any(|l| l.contains("could not open its window") && l.contains("no screen")),
        "{heard:?}"
    );
}
