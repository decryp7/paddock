//! The Spotter app: tray icon, popover and window over the engine (ADR 0003), on the Paddock app shell (ADR 0012). The
//! shell plugin owns the frame around Spotter: windows, the tray popover window, the exit with its watchdog, dialogs,
//! notifications, service health, jobs, settings, About and updates. This crate adds Spotter's own commands
//! ([`COMMANDS`]), its services ([`health`]), its jobs on the shell's registry ([`jobs`]) and the engine's
//! start-up. The engine owns all watch state and pushes changes on `spotter://state`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod applog;
pub mod commands;
pub mod credentials;
pub mod dto;
pub mod health;
pub mod jobs;
pub mod lifecycle;
pub mod notifier;
pub mod state;
pub mod toast;
pub mod tray;

use paddock_shell::{AppIdentity, Builder};
use paddock_updater::tauri::UpdatesBuilder;
use paddock_window::tauri::{FixedPrefs, WindowConfig, Windows};
use paddock_window::{OnClose, OpenFilesIn};
use tauri::{Manager, Runtime};

/// The app's id (the shell's `AppIdentity`, its folders and its log target).
pub const APP_ID: &str = "spotter";
/// The product name (`tauri.conf.json` `productName`, the windows, the menus, About).
pub const APP_NAME: &str = "Spotter";
/// The app's version: the workspace's (`tauri.conf.json` repeats it for the bundles).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where the releases are (About's "Release notes", Updates' "Open the releases page").
pub const RELEASES_PAGE: &str = "https://dev.decryptology.net/decryp7/spotter/releases";
/// The feed the updater reads.
pub const UPDATE_FEED: &str = "https://dev.decryptology.net/decryp7/spotter/releases/download/latest/latest.json";
/// The generated dialog contracts and manifest, written by the UI's specs from its registry.
pub const DIALOG_CONTRACTS: &str = include_str!("../../dialog-contracts.json");
/// The manifest of the UI's contributions, compared by the tests.
pub const MANIFEST: &str = include_str!("../../paddock-manifest.json");

paddock_commands::command_table! {
    /// Spotter's own commands (the shell's are the plugin's, `plugin:paddock-shell|<command>`). Windows that are not
    /// `main` (the settings window, the tray popover) call only what is listed for them.
    pub static COMMANDS, macro handler;
    /// Facts about the app for the settings window.
    Query [WINDOW] => commands::app_info,
    /// The watches (the tray popover lists the unseen ones).
    Query [MAIN | TRAY] => commands::list_watches,
    /// One watch with its history.
    Query [MAIN] => commands::get_watch,
    /// The source kinds the wizard offers.
    Query [MAIN] => commands::source_kinds,
    /// Checks a draft without saving it (a job).
    Mutation [MAIN] limit 512 * paddock_commands::KIB => commands::test_source,
    /// Saves a watch (its secrets go to the credential store).
    Mutation [MAIN] limit 512 * paddock_commands::KIB => commands::save_watch,
    /// Deletes a watch.
    Mutation [MAIN] => commands::delete_watch,
    /// Pauses or resumes a watch.
    Mutation [MAIN] => commands::set_paused,
    /// Checks one watch or all of them (a job).
    Mutation [MAIN | TRAY] => commands::check_now,
    /// Marks a release as seen.
    Mutation [MAIN | TRAY] => commands::acknowledge,
    /// Marks the latest release of a product as seen.
    Mutation [MAIN | TRAY] => commands::acknowledge_product,
    /// Writes a pattern from an example and a selection.
    Query [MAIN] => commands::suggest_pattern,
    /// Tries a version scheme on a name.
    Query [MAIN] => commands::test_pattern,
    /// Describes a schedule.
    Query [MAIN] => commands::preview_schedule,
    /// The version patterns the helper starts from.
    Query [MAIN] => commands::version_schemes,
    /// Forgets that a release was seen.
    Mutation [MAIN] => commands::reset_acknowledgement,
    /// Records which version is installed.
    Mutation [MAIN] => commands::set_installed,
    /// Opens a release's location.
    Mutation [MAIN] => commands::open_release,
    /// Downloads a release (a job).
    Mutation [MAIN] => commands::download_release,
    /// The dashboard.
    Query [MAIN] => commands::dashboard,
    /// The event history.
    Query [MAIN] => commands::events,
    /// The settings.
    Query [WINDOW] => commands::get_settings,
    /// Saves the settings.
    Mutation [WINDOW] => commands::save_settings,
    /// Pauses notifications.
    Mutation [MAIN | DIALOG] => commands::pause_notifications,
    /// Whether Spotter starts at log-in.
    Query [WINDOW] => commands::get_autostart,
    /// Starts Spotter at log-in or not.
    Mutation [WINDOW] => commands::set_autostart,
    /// Opens the data, config or downloads folder or the logs.
    Mutation [WINDOW] => commands::open,
    /// The OS folder picker.
    Mutation [WINDOW] => commands::choose_folder,
    /// The OS program picker.
    Mutation [WINDOW] => commands::choose_program,
    /// The catalogue of watches to start from.
    Query [MAIN] => commands::presets,
    /// Feeds a page advertises.
    Mutation [MAIN] => commands::discover_feeds,
    /// Which browser renders JavaScript pages.
    Query [WINDOW] => commands::renderer_status,
    /// Installs the pinned browser (a job).
    Mutation [WINDOW] => commands::install_renderer,
    /// Builds drafts from a pasted list.
    Mutation [MAIN] limit 4 * paddock_commands::MIB => commands::import_list,
    /// Shows the main window, then a page.
    Mutation [MAIN | TRAY] => commands::show_main_window,
    /// The page asked for before the window could listen.
    Mutation [MAIN] => commands::pending_route,
    /// The picture of a watch.
    Quiet [MAIN | TRAY] => commands::watch_avatar,
    /// The picture of a catalogue entry.
    Quiet [MAIN] => commands::preset_avatar,
    /// The mark of a source kind.
    Quiet [MAIN | TRAY] => commands::kind_avatar,
    /// Whether e-mail can be sent.
    Query [WINDOW] => commands::mail_status,
    /// Stores the mail password.
    Mutation [WINDOW] => commands::set_mail_password,
    /// Sends a test message.
    Mutation [WINDOW] => commands::send_test_email,
}

/// Where the app runs: installed (the user's folders, real ticks) or in a test (nothing outside `dir`, no native menu
/// off the main thread, no update checks).
#[derive(Debug, Clone)]
pub enum Place {
    /// The installed app.
    Installed,
    /// A test: the updater and settings keep to `dir`.
    Test {
        /// The only folder the test touches.
        dir: std::path::PathBuf,
    },
}

/// The shell plugin with Spotter's parts.
pub fn shell<R: Runtime>(place: Place, hub: &health::Hub<R>) -> Builder<R> {
    let attach = hub.clone();
    let mut builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        .downloader(downloader())
        .app_commands(&COMMANDS)
        .dialog_contracts(DIALOG_CONTRACTS)
        .health(hub.services())
        .setup(move |app| {
            attach.attach(app);
            Ok(())
        });
    let test_dir = match &place {
        Place::Installed => None,
        Place::Test { dir } => Some(dir.clone()),
    };
    let updates_dir = test_dir.clone();
    builder = builder.updater(move |_| updater(updates_dir));
    builder = builder.windows(move |app| windows(app, test_dir.is_none()));
    match place {
        Place::Installed => builder,
        Place::Test { .. } => builder.in_memory().native_menu(false),
    }
}

/// The app's one downloader (segmented Range downloads, 4 connections per host across all downloads, 429 back-off,
/// resume, hash while writing, the free-space check): the updater's downloads and Spotter's own share it.
fn downloader() -> paddock_download::Downloader {
    let config = paddock_download::HttpConfig {
        user_agent: format!("Spotter/{VERSION}"),
        ..Default::default()
    };
    // The config is fixed and has no proxy, which is the only thing the constructor can refuse.
    paddock_download::Downloader::http(config).unwrap_or_else(|e| unreachable!("the fixed download config is valid: {e}"))
}

/// Updates, About and What's New. Without the CI's updater key the build cannot update itself and says so ("Updates
/// are not available in this build").
fn updater(test_dir: Option<std::path::PathBuf>) -> UpdatesBuilder {
    let b = UpdatesBuilder::new(APP_NAME)
        .channel("stable", "Stable", UPDATE_FEED)
        .public_key(option_env!("TAURI_UPDATER_PUBKEY"))
        .releases_page(RELEASES_PAGE)
        .homepage("https://dev.decryptology.net/decryp7/spotter")
        .changelog(include_str!("../../../CHANGELOG.md"))
        .legal("licence-mit", "MIT licence", include_str!("../../../LICENSE-MIT"))
        .legal("licence-apache", "Apache licence 2.0", include_str!("../../../LICENSE-APACHE"))
        .build_info(option_env!("SPOTTER_COMMIT"), option_env!("SPOTTER_BUILT"));
    match test_dir {
        None => b,
        Some(dir) => b.dirs(dir.join("config"), dir.join("cache")).background(false),
    }
}

/// The window layer: the main window from `tauri.conf.json` (`"create": false`), closing the last window keeps Spotter
/// in the tray (the shell quits when there is no tray), bounds remembered in the data folder when installed, and the
/// exit's teardown: jobs asked to stop and any browser a render holds ended, bounded by the exit watchdog.
fn windows<R: Runtime>(app: &tauri::AppHandle<R>, installed: bool) -> Windows<R> {
    let b = Windows::builder(WindowConfig::new(APP_NAME).size(1200.0, 780.0).min_size(760.0, 520.0))
        .prefs(std::sync::Arc::new(FixedPrefs::new(OnClose::Tray, OpenFilesIn::NewWindow)))
        .teardown("jobs and browsers", lifecycle::stop);
    match app.path().app_data_dir() {
        Ok(dir) if installed => b.bounds_file(dir.join("window-state.json")).build(),
        _ => b.build(),
    }
}

/// Spotter's commands behind the shell's guard: window classes, the per-dialog contracts and request limits.
pub fn invoke_handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(paddock_dialogs::tauri::grant_of)
        .wrap(handler!())
}

/// Starts the app.
pub fn run() {
    let cwd = std::env::current_dir().ok();
    let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    let hub = health::Hub::<tauri::Wry>::default();
    let setup_hub = hub.clone();
    let shell = shell::<tauri::Wry>(Place::Installed, &hub).setup(move |app| {
        // The engine opens before the event loop runs, so no command can arrive before it exists; nothing here waits
        // for the UI.
        let state = lifecycle::init(app, &setup_hub).map_err(|e| {
            applog::write(&format!("Spotter could not start: {e}"));
            e
        })?;
        app.manage(state);
        if !tray::create(app) {
            applog::write("no tray icon; closing the window will quit the app");
        }
        // Started at log-in (`--hidden`): stay in the tray without a window (only when there is a tray to come back from).
        paddock_window::tauri::start(app, &launch)?;
        Ok(())
    });
    let result = tauri::Builder::default()
        // First: a second launch shows the running app's window instead of starting another engine.
        .plugin(tauri_plugin_single_instance::init(paddock_window::tauri::second_instance))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(shell.build())
        .invoke_handler(invoke_handler())
        .on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated)
        .run(tauri::generate_context!());
    if let Err(e) = result {
        applog::write(&format!("Spotter could not start: {e}"));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod capability_tests {
    fn caps(src: &str) -> serde_json::Value {
        serde_json::from_str(src).unwrap()
    }

    fn granted(c: &serde_json::Value) -> Vec<&str> {
        c["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect()
    }

    /// The window has an overlay title bar on macOS: the page's drag regions only move it when the webview may start
    /// a window drag. Without the permission dragging fails silently.
    #[test]
    fn the_main_window_can_be_dragged_by_its_drag_regions() {
        let c = caps(include_str!("../capabilities/shell.json"));
        let granted = granted(&c);
        assert!(granted.contains(&"core:window:allow-start-dragging"), "{granted:?}");
        assert!(granted.contains(&"core:window:allow-internal-toggle-maximize"), "{granted:?}");
        assert!(c["windows"].as_array().unwrap().iter().any(|w| w == "main"));
    }

    /// The webview reaches the OS only through Spotter's own commands and the shell's sets: no other plugin permission,
    /// no wildcard, no remote content.
    #[test]
    fn the_capabilities_are_least_privilege() {
        for src in [
            include_str!("../capabilities/shell.json"),
            include_str!("../capabilities/dialog.json"),
            include_str!("../capabilities/tray.json"),
        ] {
            let c = caps(src);
            for p in granted(&c) {
                assert!(
                    p.starts_with("core:window:allow-") || p == "core:event:default" || p.starts_with("paddock-shell:"),
                    "unexpected permission {p}"
                );
                assert!(!p.contains('*'), "{p}");
            }
            assert!(c.get("remote").is_none(), "no remote content may use IPC");
        }
    }

    /// Each window class gets its own shell set; the popover has no dialog or main commands.
    #[test]
    fn each_window_class_has_its_shell_set() {
        let want = [
            (include_str!("../capabilities/shell.json"), "paddock-shell:main"),
            (include_str!("../capabilities/dialog.json"), "paddock-shell:dialog"),
            (include_str!("../capabilities/tray.json"), "paddock-shell:tray"),
        ];
        for (src, set) in want {
            assert!(granted(&caps(src)).contains(&set), "{set}");
        }
    }

    /// No `unsafe-eval`, no remote origin, nothing but the app's own scheme and IPC.
    #[test]
    fn the_csp_has_no_unsafe_eval_and_no_remote_origin() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = conf["app"]["security"]["csp"].as_str().unwrap();
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        assert!(!csp.contains("http://") || csp.contains("http://ipc.localhost"), "{csp}");
        assert!(!csp.contains("https:"), "{csp}");
        assert!(!csp.contains('*'), "{csp}");
        assert!(csp.contains("object-src 'none'") && csp.contains("frame-ancestors 'none'"));
    }

    /// The main window is built by the shell (`create: false`), so its remembered bounds and start in the tray apply.
    #[test]
    fn the_shell_builds_the_main_window() {
        for src in [
            include_str!("../tauri.conf.json"),
            include_str!("../tauri.macos.conf.json"),
            include_str!("../tauri.windows.conf.json"),
        ] {
            let conf: serde_json::Value = serde_json::from_str(src).unwrap();
            let main = conf["app"]["windows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|w| w["label"] == "main")
                .unwrap();
            assert_eq!(main["create"], false);
        }
    }

    /// The bundle step checks the built index.html first (a CSP nonce would break Angular's styles).
    #[test]
    fn the_build_checks_the_index_before_bundling() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(conf["build"]["beforeBuildCommand"].as_str().unwrap().contains("check-index.mjs"));
    }
}
