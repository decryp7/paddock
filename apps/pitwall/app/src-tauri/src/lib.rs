//! The Pitwall app: tray icon and window over the Pitwall server, on the Paddock app shell (ADR 0012, ADR 0029). It holds
//! no model or request state of its own: everything comes from the server through [`server_client::ServerClient`] (the
//! management API on 127.0.0.1) and its live feed. Closing the window never stops the server; the server runs as an OS
//! service (see `service_install`).
//!
//! The shell plugin owns the frame around Pitwall: the start (the tray, then the main window, after every plugin is set
//! up), the windows and their bounds, the exit with its watchdog and the guard in front of the commands. This crate adds
//! Pitwall's commands ([`COMMANDS`]), its state and live feed ([`shell`]'s set-up), the tray's model and actions
//! ([`tray`]) and quitting with the "When you quit Pitwall" choice ([`quit`]).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app_commands;
pub mod app_state;
pub mod commands;
pub mod credentials;
pub mod dialogs;
pub mod jobs;
pub mod live;
pub mod quit;
pub mod server_client;
pub mod service_install;
pub mod service_repair;
pub mod tray;
pub mod tray_model;
pub mod updater;

use commands::SharedClient;
use paddock_shell::{AppIdentity, Builder};
use paddock_window::tauri::{FixedPrefs, WindowConfig, Windows};
use paddock_window::{OnClose, OpenFilesIn};
use pitwall_core::mgmt::{LiveUpdate, Overview};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tray_model::TrayModel;

/// The app's id (the shell's `AppIdentity`, its folders and its log target).
pub const APP_ID: &str = "pitwall";
/// The product name (`tauri.conf.json` `productName`, the windows, the menus).
pub const APP_NAME: &str = "Pitwall";
/// The app's version: the workspace's (`tauri.conf.json` repeats it for the bundles).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long a quit may take before the shell's exit watchdog ends the process: the server's own bound for stopping
/// serving (`stop_serving` times out at 30 s in `commands::timeout_for`) plus time to close the windows.
pub const EXIT_STALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(40);

/// The tray's facts, shared by the live feed (which writes them) and the tray's model (which reads them).
pub type SharedModel = Arc<Mutex<TrayModel>>;

paddock_commands::command_table! {
    /// Pitwall's own commands (the shell's are the plugin's, `plugin:paddock-shell|<command>`), each with the windows that
    /// may call it: `WINDOW` is the main window and its dialogs (a dialog is narrowed further to its contract), `TRAY` the
    /// tray popover. Names and argument shapes are pinned by the UI's `tauri-backend.ts`.
    pub static COMMANDS, macro handler;
    Query [WINDOW | TRAY] => commands::overview,
    Query [WINDOW] => commands::history,
    Query [WINDOW] => commands::usage_series,
    Query [WINDOW] => commands::app_detail,
    Query [WINDOW] => commands::presets,
    Mutation [WINDOW] => commands::create_preset,
    Mutation [WINDOW] => commands::update_preset,
    Mutation [WINDOW] => commands::delete_preset,
    Query [WINDOW] => commands::resolve_preset,
    Query [WINDOW] => commands::events,
    Mutation [WINDOW | TRAY] => commands::set_serving,
    Mutation [WINDOW | TRAY] => commands::release_memory,
    Query [WINDOW] => commands::cache_info,
    Mutation [WINDOW] => commands::clear_cache,
    Mutation [WINDOW] => commands::apply_preset,
    Mutation [WINDOW] => commands::cancel_job,
    Query [WINDOW] => commands::local_models,
    Query [WINDOW] => commands::search_models,
    Query [WINDOW] => commands::repo_files,
    Mutation [WINDOW] => commands::download_model,
    Query [WINDOW] => commands::downloads,
    Mutation [WINDOW] => commands::cancel_download,
    Mutation [WINDOW] => commands::delete_model,
    Mutation [WINDOW] => commands::unload_model,
    Mutation [WINDOW] => commands::pin_model,
    Mutation [WINDOW] => commands::set_model_idle,
    Query [WINDOW | TRAY] => commands::update_check_status,
    Query [WINDOW | TRAY] => commands::update_notices,
    Mutation [WINDOW] => commands::set_update_check,
    Mutation [WINDOW] => commands::dismiss_update,
    Mutation [WINDOW] => commands::set_model_slots,
    Query [WINDOW] => commands::model_fit,
    Query [WINDOW] => commands::disk_usage,
    Query [WINDOW] => commands::engines,
    Mutation [WINDOW] => commands::install_build,
    Mutation [WINDOW] => commands::retry_engine_setup,
    Mutation [WINDOW] => commands::remove_build,
    Mutation [WINDOW] => commands::select_build,
    Mutation [WINDOW] => commands::rollback_build,
    Query [WINDOW] => commands::engine_pins,
    Mutation [WINDOW] => commands::set_global_pin,
    Mutation [WINDOW] => commands::set_model_pin,
    Query [WINDOW] => commands::build_capabilities,
    Query [WINDOW] => commands::packs,
    Mutation [WINDOW] => commands::remove_leftover,
    Query [WINDOW] => commands::apps,
    Mutation [WINDOW] => commands::create_app,
    Mutation [WINDOW] => commands::delete_app,
    Mutation [WINDOW] => commands::create_key,
    Mutation [WINDOW] => commands::revoke_key,
    Mutation [WINDOW] => commands::set_permissions,
    Mutation [WINDOW | TRAY] => commands::approve_pairing,
    Mutation [WINDOW | TRAY] => commands::deny_pairing,
    Query [WINDOW | TRAY] => commands::pairings,
    Query [WINDOW] => commands::requests,
    Query [WINDOW] => commands::request_content,
    Mutation [WINDOW] limit 9 * paddock_commands::MIB => commands::chat,
    Mutation [WINDOW] => commands::chat_cancel,
    Query [WINDOW] => commands::benchmarks,
    Query [WINDOW] => commands::settings,
    Mutation [WINDOW] => commands::save_settings,
    Mutation [WINDOW] => quit::quit_app,
    Query [WINDOW] => quit::take_quit_ask,
    Query [WINDOW | TRAY] => app_commands::app_update_status,
    Mutation [WINDOW] => app_commands::check_app_update,
    Query [WINDOW] => app_commands::service_state,
    Mutation [WINDOW] => app_commands::decline_service,
    Mutation [WINDOW] => app_commands::open_service_login_items,
    Query [WINDOW] => app_commands::get_autostart,
    Mutation [WINDOW] => app_commands::set_autostart,
    Mutation [WINDOW] => app_commands::start_load_model,
    Mutation [WINDOW] => app_commands::start_benchmark,
    Mutation [WINDOW] => app_commands::start_install_service,
    Mutation [WINDOW] => app_commands::start_uninstall_service,
    Mutation [WINDOW] => app_commands::start_app_update,
    Mutation [WINDOW] => app_commands::start_repair_service,
    Mutation [WINDOW] => app_commands::start_check_updates,
    Mutation [WINDOW] => app_commands::app_job_cancel,
    Quiet [WINDOW] => app_commands::app_job_stop_waiting,
}

/// Where the app runs: installed (the user's folders, the OS credential store) or in a test (nothing outside `dir`, no
/// native menu, no OS credential store, no service repair).
#[derive(Debug, Clone)]
pub enum Place {
    /// The installed app.
    Installed,
    /// A test: the app's data folder is `dir`.
    Test {
        /// The only folder the test touches.
        dir: std::path::PathBuf,
    },
}

/// The client of the server, its offline knowledge and the tray's facts, for `place`.
fn state(place: &Place) -> (SharedClient, Arc<updater::OfflineKnown>, SharedModel) {
    let (data_dir, store): (std::path::PathBuf, Box<dyn credentials::KeyStore>) = match place {
        Place::Installed => (app_state::data_dir().unwrap_or_default(), Box::new(credentials::OsKeyStore)),
        Place::Test { dir } => (dir.clone(), Box::new(credentials::NoKeyStore)),
    };
    let keys = Arc::new(credentials::StoredAdminKey::new(store, data_dir.join("admin.key")));
    let known_offline = Arc::new(updater::OfflineKnown::default());
    let client: SharedClient = Arc::new(updater::Observing::new(
        Arc::new(server_client::LoopbackClient::new(data_dir, keys)),
        known_offline.clone(),
    ));
    (client, known_offline, Arc::new(Mutex::new(TrayModel::new())))
}

/// The shell plugin with Pitwall's parts: its commands, its state and the live feed (set-up: state only, no window; the
/// shell opens the tray and the window after every plugin, ADR 0029) and the window layer.
pub fn shell<R: Runtime>(place: Place) -> Builder<R> {
    let installed = matches!(place, Place::Installed);
    let (client, known_offline, model) = state(&place);
    let builder = Builder::new(AppIdentity::new(APP_ID, APP_NAME, VERSION))
        .app_commands(&COMMANDS)
        .dialogs(&[&dialogs::DIALOGS])
        .setup(move |app| {
            app.manage(client);
            app.manage(known_offline);
            app.manage(model);
            app.manage(Arc::new(commands::Streams::default()));
            app.manage(Arc::new(jobs::Jobs::default()));
            app.manage(quit::PendingAsk::default());
            app.manage(updater::Pending::default());
            pitwall_setup(app, installed);
            Ok(())
        })
        .windows(move |app| windows(app, installed));
    if installed {
        builder
    } else {
        builder.in_memory().native_menu(false)
    }
}

/// The window layer: the main window from `tauri.conf.json` (`"create": false`); closing it destroys the webview and
/// Pitwall stays in the tray (a hidden webview could come back blank on macOS and kept its memory); the bounds are
/// remembered in the data folder when installed; a quit that stalls ends after [`EXIT_STALL_LIMIT`].
fn windows<R: Runtime>(app: &AppHandle<R>, installed: bool) -> Windows<R> {
    let config = WindowConfig::new(APP_NAME)
        .size(1200.0, 780.0)
        .min_size(860.0, 560.0)
        .exit_stall_limit(EXIT_STALL_LIMIT);
    let b = Windows::builder(config).prefs(Arc::new(FixedPrefs::new(OnClose::Tray, OpenFilesIn::NewWindow)));
    match app.path().app_data_dir() {
        Ok(dir) if installed => b.bounds_file(dir.join("window-state.json")).build(),
        _ => b.build(),
    }
}

/// Pitwall's commands behind the shell's guard: window classes and request limits.
pub fn invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    paddock_commands::tauri::Guard::new(&COMMANDS)
        .dialog_grants(paddock_dialogs::tauri::grant_of)
        .wrap(handler!())
}

/// Pitwall on `builder` (after the plugins already on it): the shell plugin `shell`, the tray drawn by `tray_backend`, the
/// main window for `launch` (none for `--hidden` with a tray). The start `run` uses (it adds Pitwall's guarded commands)
/// and the start test composes.
pub fn app<R: Runtime>(
    builder: tauri::Builder<R>,
    shell: Builder<R>,
    launch: paddock_window::launch::LaunchArgs,
    tray_backend: impl paddock_window::tauri::TrayBackend<R>,
) -> tauri::Builder<R> {
    // The shell opens both after every plugin is set up (ADR 0029: a window built inside a plugin hook deadlocks).
    shell.tray(move |_| tray::tray(tray_backend)).launch(launch).add_to(builder)
}

/// Where the live feed goes: the UI's event and the tray.
struct AppSink<R: Runtime> {
    app: AppHandle<R>,
    model: SharedModel,
}

impl<R: Runtime> live::LiveSink for AppSink<R> {
    fn seed(&mut self, overview: &Overview) {
        self.model.lock().unwrap_or_else(|p| p.into_inner()).seed(overview);
        paddock_window::tauri::refresh_tray(&self.app);
    }

    fn update(&mut self, raw: &serde_json::Value, parsed: Option<&LiveUpdate>) {
        let _ = self.app.emit(live::EVENT, raw);
        if let Some(u) = parsed {
            self.model.lock().unwrap_or_else(|p| p.into_inner()).apply(u);
            paddock_window::tauri::refresh_tray(&self.app);
        }
    }

    fn disconnected(&mut self) {
        self.model.lock().unwrap_or_else(|p| p.into_inner()).set_connected(false);
        paddock_window::tauri::refresh_tray(&self.app);
    }
}

/// Pitwall's own set-up, after its state is managed: the live feed (UI events and the tray, reconnecting on its own),
/// resuming serving after a "stop serving" quit and, installed on macOS, repairing a service registration launchd refuses.
fn pitwall_setup<R: Runtime>(app: &AppHandle<R>, installed: bool) {
    let client = app.state::<SharedClient>().inner().clone();
    let sink = AppSink {
        app: app.clone(),
        model: app.state::<SharedModel>().inner().clone(),
    };
    tauri::async_runtime::spawn(async move {
        let mut sink = sink;
        live::run(client, &mut sink).await;
    });
    if !installed {
        return;
    }
    // Opening Pitwall after it was quit with "stop serving" resumes serving.
    let client = app.state::<SharedClient>().inner().clone();
    tauri::async_runtime::spawn(async move { quit::resume_if_stopped(client.as_ref()).await });
    // After an update launchd may refuse the registered service (a rebuilt app has a new code identity on macOS):
    // register it again in the background, so the server comes back without the user's help.
    #[cfg(target_os = "macos")]
    {
        let client = app.state::<SharedClient>().inner().clone();
        std::thread::spawn(move || {
            let answers =
                || tauri::async_runtime::block_on(commands::forward(client.as_ref(), "overview", serde_json::Value::Null)).is_ok();
            // The service may only fail later (it restarts itself onto a new binary when idle), so keep checking while it
            // does not answer, a bounded number of times per run of the app.
            for attempt in 0..service_install::REPAIR_ATTEMPTS {
                std::thread::sleep(service_install::repair_delay(attempt));
                match service_install::repair_registration(&answers) {
                    Some(service_repair::RepairOutcome::Recovered | service_repair::RepairOutcome::NeedsApproval) => break,
                    // Failed: the next check tries again (bounded by REPAIR_ATTEMPTS); nothing to repair: keep watching.
                    Some(service_repair::RepairOutcome::Failed(_) | service_repair::RepairOutcome::Cancelled) | None => {}
                }
            }
        });
    }
}

/// Starts the app.
pub fn run() {
    // First: the shared crates' log lines (the start's error among them) reach Pitwall's daily log file (paddock-log).
    let _log = paddock_shell::logging::install(&AppIdentity::new(APP_ID, APP_NAME, VERSION));
    let cwd = std::env::current_dir().ok();
    let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &[]);
    let builder = tauri::Builder::default();
    // First: a second launch shows the running app's window instead of starting another one (release builds; a login
    // start with `--hidden` changes nothing).
    #[cfg(all(not(debug_assertions), any(target_os = "macos", windows, target_os = "linux")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(paddock_window::tauri::second_instance));
    let builder = builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![paddock_window::launch::HIDDEN_FLAG]),
        ))
        .plugin(updater::plugin());
    let builder = app(builder, shell(Place::Installed), launch, tray::native()).invoke_handler(invoke_handler());
    // A web content process termination hook exists on macOS and iOS only (Tauri 2 has none for WebView2 or WebKitGTK).
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = builder.on_web_content_process_terminate(paddock_startup::tauri::web_content_terminated);
    if let Err(e) = builder.run(tauri::generate_context!()) {
        eprintln!("Pitwall could not start: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod capability_tests {
    fn granted() -> (serde_json::Value, Vec<String>) {
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let granted = caps["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str().map(str::to_string))
            .collect();
        (caps, granted)
    }

    /// The window has an overlay title bar on macOS (and no native one where decorations are off): the page's drag
    /// regions only move it when the webview may start a window drag. Without the permission dragging fails silently.
    #[test]
    fn the_main_window_can_be_dragged_by_its_drag_regions() {
        let (caps, granted) = granted();
        assert!(granted.iter().any(|p| p == "core:window:allow-start-dragging"), "{granted:?}");
        assert!(
            granted.iter().any(|p| p == "core:window:allow-internal-toggle-maximize"),
            "{granted:?}"
        );
        assert!(caps["windows"].as_array().unwrap().iter().any(|w| w == "main"));
    }

    /// Least privilege: the webview gets events, window dragging and the shell's main set, nothing from the file system,
    /// shell, opener, updater, autostart or network plugins (the UI goes through Pitwall's own commands).
    #[test]
    fn the_webview_is_granted_no_plugin_or_wildcard_permissions() {
        let (_, granted) = granted();
        assert!(granted.iter().any(|p| p == "paddock-shell:main"), "{granted:?}");
        for p in &granted {
            if p == "paddock-shell:main" {
                continue;
            }
            assert!(p.starts_with("core:"), "{p}");
            assert!(!p.contains('*') && !p.ends_with(":default") || p == "core:event:default", "{p}");
            for banned in ["fs:", "shell:", "http:", "updater:", "autostart:", "opener:", "process:"] {
                assert!(!p.starts_with(banned), "{p}");
            }
        }
    }

    /// The Content-Security-Policy never allows `unsafe-eval` or a remote origin.
    #[test]
    fn the_csp_has_no_unsafe_eval_and_no_remote_origin() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = v["app"]["security"]["csp"].as_str().unwrap();
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        assert!(
            csp.matches("http://").count() == csp.matches("http://ipc.localhost").count(),
            "{csp}"
        );
        assert!(!csp.contains("https:") && !csp.contains("ws:") && !csp.contains(" *"), "{csp}");
        assert!(csp.contains("object-src 'none'") && csp.contains("frame-ancestors 'none'"));
    }

    /// macOS needs `macOSPrivateApi` in the shared configuration and the matching cargo feature, and an ad-hoc signing
    /// identity so SMAppService accepts the bundle (docs/suite-lessons.md).
    #[test]
    fn macos_private_api_and_ad_hoc_signing_are_configured() {
        let shared: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(shared["app"]["macOSPrivateApi"], true);
        let mac: serde_json::Value = serde_json::from_str(include_str!("../tauri.macos.conf.json")).unwrap();
        assert_eq!(mac["app"]["macOSPrivateApi"], true);
        assert_eq!(mac["bundle"]["macOS"]["signingIdentity"], "-");
        assert!(include_str!("../Cargo.toml").contains("\"macos-private-api\""));
        let agent = mac["bundle"]["macOS"]["files"].as_object().unwrap();
        assert!(agent
            .keys()
            .any(|k| k == "Library/LaunchAgents/net.decryptology.pitwall.service.plist"));
    }

    /// The main window is opaque on every OS, like every family window (user, 2026-10-30): no transparent window and
    /// no Mica or vibrancy behind it.
    #[test]
    fn the_main_window_is_opaque_on_every_os() {
        for (os, src) in [
            ("shared", include_str!("../tauri.conf.json")),
            ("macos", include_str!("../tauri.macos.conf.json")),
            ("windows", include_str!("../tauri.windows.conf.json")),
            ("linux", include_str!("../tauri.linux.conf.json")),
        ] {
            let v: serde_json::Value = serde_json::from_str(src).unwrap();
            for w in v["app"]["windows"].as_array().into_iter().flatten() {
                assert_ne!(w["transparent"], true, "{os}: {w}");
                assert!(w.get("windowEffects").is_none(), "{os}: {w}");
            }
        }
    }

    /// The launch agent restarts the server (KeepAlive), runs the bundled program and marks it supervised.
    #[test]
    fn the_launch_agent_plist_keeps_the_bundled_server_alive() {
        let plist = include_str!("../macos/net.decryptology.pitwall.service.plist");
        assert!(plist.contains("<key>BundleProgram</key>\n    <string>Contents/MacOS/pitwall-server</string>"));
        assert!(plist.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(plist.contains("<key>PITWALL_SERVICE</key>"));
        assert!(plist.contains("<string>net.decryptology.pitwall.service</string>"));
    }

    /// The installers: NSIS for Windows, dmg and app for macOS, deb and AppImage for Linux, with the server inside.
    #[test]
    fn the_installers_and_the_sidecar_are_configured() {
        let shared: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(shared["bundle"]["externalBin"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b == "binaries/pitwall-server"));
        let win: serde_json::Value = serde_json::from_str(include_str!("../tauri.windows.conf.json")).unwrap();
        assert!(win["bundle"]["targets"].as_array().unwrap().iter().any(|t| t == "nsis"));
        let lin: serde_json::Value = serde_json::from_str(include_str!("../tauri.linux.conf.json")).unwrap();
        let targets = lin["bundle"]["targets"].as_array().unwrap();
        assert!(targets.iter().any(|t| t == "deb") && targets.iter().any(|t| t == "appimage"));
        assert!(shared["bundle"]["targets"].as_array().unwrap().iter().any(|t| t == "dmg"));
        let hooks = include_str!("../windows/installer-hooks.nsh");
        assert!(hooks.contains("pitwall-server.exe\" install") && hooks.contains("pitwall-server.exe\" uninstall"));
    }
}
