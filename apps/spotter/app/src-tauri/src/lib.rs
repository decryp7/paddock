//! The Spotter app: tray icon, popover and window over the engine (ADR 0003), on the Paddock app shell (ADR 0012). The
//! shell plugin owns the frame around Spotter: windows, the tray popover window, the exit with its watchdog, dialogs,
//! notifications, service health, jobs, settings, About and updates. This crate adds Spotter's own commands
//! ([`COMMANDS`]), its services ([`health`]), its jobs on the shell's registry ([`jobs`]) and the engine's
//! start-up. The engine owns all watch state and pushes changes on `spotter://state`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod applog;
pub mod commands;
pub mod credentials;
pub mod dialogs;
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
use std::sync::Arc;
use tauri::{Manager, Runtime};

/// The app's id (the shell's `AppIdentity`, its folders and its log target).
pub const APP_ID: &str = "spotter";
/// The product name (`tauri.conf.json` `productName`, the windows, the menus, About).
pub const APP_NAME: &str = "Spotter";
/// The app's version: the workspace's (`tauri.conf.json` repeats it for the bundles).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where the releases are (About's "Release notes", Updates' "Open the releases page").
pub const RELEASES_PAGE: &str = "https://dev.decryptology.net/decryp7/paddock/releases?q=spotter-v";
/// The feed the updater reads.
pub const UPDATE_FEED: &str = "https://dev.decryptology.net/decryp7/paddock/releases/download/spotter-latest/latest.json";
/// The feed the updater reads: [`UPDATE_FEED`], except in a `-test.N` build made with `SPOTTER_TEST_UPDATE_FEED` set (the
/// VM update test, built with a throw-away key; the family key never signs such a build). The override must be https.
pub fn update_feed(version: &str, test_feed: Option<&'static str>) -> &'static str {
    match test_feed {
        Some(url) if version.contains("-test.") && url.starts_with("https://") => url,
        _ => UPDATE_FEED,
    }
}
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
    let engine_hub = hub.clone();
    let seams = match &place {
        Place::Installed => lifecycle::Seams::installed(),
        Place::Test { dir } => lifecycle::Seams::test(dir),
    };
    let Net {
        downloader,
        proxy,
        problem,
    } = net(&place);
    hub.note_proxy_problem(problem);
    let mut builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        .downloader(downloader)
        .app_commands(&COMMANDS)
        .dialogs(&[&dialogs::DIALOGS])
        .health(hub.services())
        .setup(move |app| {
            app.manage(ProxyChoice(proxy.clone()));
            attach.attach(app);
            Ok(())
        })
        // The engine opens before the event loop runs, so no command can arrive before it exists; nothing here waits for
        // the UI and nothing opens a window (the shell opens the tray and the window after every plugin, ADR 0029).
        .setup(move |app| {
            let state = lifecycle::init(app, &engine_hub, &seams).map_err(|e| {
                (seams.note)(&format!("Spotter could not start: {e}"));
                e
            })?;
            app.manage(state);
            Ok(())
        });
    let test_dir = match &place {
        Place::Installed => None,
        Place::Test { dir } => Some(dir.clone()),
    };
    let updates_dir = test_dir.clone();
    builder = builder.updater(move |app| updater(updates_dir, app));
    builder = builder.windows(move |app| windows(app, test_dir.is_none()));
    match place {
        Place::Installed => builder,
        // No start guard: a test's app never reaches `RunEvent::Ready`, so it would tick on.
        Place::Test { .. } => builder.in_memory().native_menu(false).start_guard(None),
    }
}

/// How long the system's proxy settings may take to read (the OS lookups poll `scutil`, `reg` or `netsh`) before the
/// downloader starts without them.
const PROXY_DETECT_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

fn http_config(proxy: paddock_download::ProxyMode) -> paddock_download::HttpConfig {
    paddock_download::HttpConfig {
        user_agent: format!("Spotter/{VERSION}"),
        proxy,
        ..Default::default()
    }
}

/// The app's network: its one downloader (segmented Range downloads, 4 connections per host across all downloads, 429
/// back-off, resume, hash while writing, the free-space check; the updater's downloads and Spotter's own share it), the
/// proxy decision it was built with (the metadata client takes the same one) and, when the system's proxy settings could
/// not be used, why.
pub struct Net {
    /// The shared downloader.
    pub downloader: paddock_download::Downloader,
    /// Which proxy, if any, each request goes through.
    pub proxy: Arc<paddock_download::ProxyRules>,
    /// Why the system's proxy settings are not in use (`None`: they are, or there are none).
    pub problem: Option<String>,
}

/// The state the shell's setup puts in the app: the proxy decision for the metadata client.
pub struct ProxyChoice(pub Arc<paddock_download::ProxyRules>);

/// Builds the [`Net`]. `detect` reads the proxy settings (the system's in the app; none in tests, rule 7). It runs on its
/// own thread and may take at most `budget`; a setting Spotter cannot use (a `socks5://` address, say) or a lookup that
/// hangs must not stop the app from starting, so the downloader then connects directly and `problem` says why (shown as a
/// degraded connection in the status bar).
pub fn net_with<D>(detect: D, budget: std::time::Duration) -> Net
where
    D: FnOnce() -> Result<paddock_download::ProxyRules, paddock_download::DownloadError> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(detect());
    });
    let why = match rx.recv_timeout(budget) {
        Ok(Ok(rules)) => {
            let rules = Arc::new(rules);
            match paddock_download::Downloader::http(http_config(paddock_download::ProxyMode::Rules((*rules).clone()))) {
                Ok(downloader) => {
                    return Net {
                        downloader,
                        proxy: rules,
                        problem: None,
                    }
                }
                Err(e) => e.to_string(),
            }
        }
        Ok(Err(e)) => e.to_string(),
        Err(_) => "reading the system's proxy settings took too long".to_string(),
    };
    log::warn!("the system proxy settings cannot be used, downloads go direct: {why}");
    // Direct has no proxy to refuse, which is the only thing the constructor can reject.
    let downloader = paddock_download::Downloader::http(http_config(paddock_download::ProxyMode::Direct))
        .unwrap_or_else(|e| unreachable!("the fixed direct download config is valid: {e}"));
    Net {
        downloader,
        proxy: Arc::new(paddock_download::ProxyRules::none()),
        problem: Some(why),
    }
}

fn net(place: &Place) -> Net {
    match place {
        Place::Installed => net_with(paddock_download::ProxyRules::detect, PROXY_DETECT_BUDGET),
        Place::Test { .. } => net_with(|| Ok(paddock_download::ProxyRules::none()), PROXY_DETECT_BUDGET),
    }
}

/// The legal documents About lists, compiled into the binary: the third-party notices (generated by `scripts/notices.sh spotter`
/// from the Rust crates and npm packages Spotter ships; CI fails when they are stale) and Spotter's own two licences.
const LEGAL: [(&str, &str, &str); 3] = [
    (
        "notices",
        "Third-party notices",
        include_str!("../resources/THIRD_PARTY_NOTICES.md"),
    ),
    ("licence-mit", "MIT licence", include_str!("../../../LICENSE-MIT")),
    ("licence-apache", "Apache licence 2.0", include_str!("../../../LICENSE-APACHE")),
];

/// Updates, About and What's New. Without the CI's updater key the build cannot update itself and says so ("Updates
/// are not available in this build").
///
/// The installer is `tauri-plugin-updater` (registered in [`run`]) with the same key the updater verifies with: without
/// it an update is found but never installed.
fn updater<R: Runtime>(test_dir: Option<std::path::PathBuf>, app: &tauri::AppHandle<R>) -> UpdatesBuilder {
    let key = option_env!("TAURI_UPDATER_PUBKEY");
    let b = LEGAL
        .iter()
        .fold(UpdatesBuilder::new(APP_NAME), |b, (id, title, text)| b.legal(id, title, *text));
    let b = b
        .channel("stable", "Stable", update_feed(VERSION, option_env!("SPOTTER_TEST_UPDATE_FEED")))
        .public_key(key)
        .installer(Arc::new(paddock_updater::tauri::PluginInstaller::new(
            app.clone(),
            key.unwrap_or_default(),
        )))
        .releases_page(RELEASES_PAGE)
        .homepage("https://dev.decryptology.net/decryp7/paddock")
        .changelog(include_str!("../../../CHANGELOG.md"))
        .build_info(option_env!("PADDOCK_BUILD_COMMIT"), option_env!("PADDOCK_BUILD_DATE"));
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

/// Spotter on `builder` (after the plugins already on it): the shell plugin `shell`, the tray drawn by `tray_backend`,
/// the main window for `launch` (the shell opens both after every plugin is set up) and Spotter's guarded commands. The
/// start `run` uses and the start test composes.
pub fn app<R: Runtime>(
    builder: tauri::Builder<R>,
    shell: Builder<R>,
    launch: paddock_window::launch::LaunchArgs,
    tray_backend: impl paddock_window::tauri::TrayBackend<R>,
) -> tauri::Builder<R> {
    shell
        .tray(move |app| tray::tray(app, tray_backend))
        .launch(launch)
        .add_to(builder)
        .invoke_handler(invoke_handler())
}

/// Starts the app.
pub fn run() {
    // First: the shared crates' log lines (the start's error among them) reach Spotter's daily log file (paddock-log).
    let _log = paddock_shell::logging::install(&AppIdentity::new(APP_ID, APP_NAME, VERSION));
    let cwd = std::env::current_dir().ok();
    let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    let hub = health::Hub::<tauri::Wry>::default();
    let shell = shell::<tauri::Wry>(Place::Installed, &hub);
    let builder = tauri::Builder::default()
        // First: a second launch shows the running app's window instead of starting another engine.
        .plugin(tauri_plugin_single_instance::init(paddock_window::tauri::second_instance))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ));
    let builder = app(builder, shell, launch, tray::native());
    // A web content process termination hook exists on macOS and iOS only (Tauri 2 has none for WebView2 or WebKitGTK).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = builder.on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated);
    let result = builder.run(tauri::generate_context!());
    if let Err(e) = result {
        applog::write(&format!("Spotter could not start: {e}"));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod updater_tests {
    use super::*;

    /// About lists the third-party notices first and they name what Spotter ships: a Rust crate, an npm package of the UI and
    /// the licence text each is under (the attribution Apache-2.0 and MIT require).
    #[test]
    fn about_lists_the_third_party_notices_with_known_dependencies() {
        let (id, title, text) = LEGAL[0];
        assert_eq!((id, title), ("notices", "Third-party notices"));
        for known in ["- tauri ", "- serde ", "- rxjs ", "- tslib "] {
            assert!(text.contains(known), "the notices do not list `{}`", known.trim());
        }
        assert!(
            text.contains("## Licence texts") && text.contains("Permission is hereby granted"),
            "the licence texts are included"
        );
        let ids: Vec<_> = LEGAL.iter().map(|d| d.0).collect();
        assert_eq!(
            ids,
            ["notices", "licence-mit", "licence-apache"],
            "ids are unique and the notices stay listed"
        );
    }

    /// A release build never reads a feed from its build environment; only a `-test.N` build (the VM update test, made with
    /// a throw-away key) may, and only over https, so a stray variable cannot point a shipped copy somewhere else.
    #[test]
    fn only_a_test_build_may_take_its_feed_from_the_build() {
        let other = Some("https://192.168.64.1:8089/latest.json");
        assert_eq!(update_feed("1.0.0", other), UPDATE_FEED, "a release build ignores the override");
        assert_eq!(update_feed("1.0.0-rc.1", other), UPDATE_FEED, "so does a release candidate");
        assert_eq!(update_feed("1.0.0-test.1", other), other.unwrap());
        assert_eq!(update_feed("1.0.0-test.1", None), UPDATE_FEED);
        assert_eq!(update_feed("1.0.0-test.1", Some("http://192.168.64.1/latest.json")), UPDATE_FEED, "https only");
        assert_eq!(update_feed("1.0.0-test.1", Some("")), UPDATE_FEED);
    }

    #[test]
    fn the_built_app_has_an_installer_so_a_found_update_can_be_installed() {
        let app = tauri::test::mock_app();
        assert!(updater(None, app.handle()).has_installer());
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
