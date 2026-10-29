//! The Pitwall app: tray icon and window over the Pitwall server. It holds no model or request state of its own:
//! everything comes from the server through [`server_client::ServerClient`] (the management API on 127.0.0.1) and its
//! live feed. Closing the window never stops the server; the server runs as an OS service (see `service_install`).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app_commands;
pub mod app_menu;
pub mod app_state;
pub mod commands;
pub mod credentials;
pub mod jobs;
pub mod live;
pub mod native_feel;
pub mod quit;
pub mod server_client;
pub mod service_install;
pub mod service_repair;
pub mod shell;
pub mod tray;
pub mod tray_model;
pub mod updater;
pub mod window;

use commands::SharedClient;
use pitwall_core::mgmt::{LiveUpdate, Overview};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tray_model::{TrayAction, TrayModel};
use window::Lifecycle;

/// The tray model, shared by the live feed (writer) and the tray menu (reader).
type SharedModel = Arc<Mutex<TrayModel>>;

/// Feeds the live events to the UI and the tray.
struct AppSink {
    app: AppHandle<Wry>,
    model: SharedModel,
}

impl AppSink {
    fn redraw(&self) {
        let view = self.model.lock().unwrap_or_else(|p| p.into_inner()).view();
        let drawn = self.app.state::<tray::Drawn>();
        tray::update(&self.app, &drawn, &view);
    }
}

impl live::LiveSink for AppSink {
    fn seed(&mut self, overview: &Overview) {
        self.model.lock().unwrap_or_else(|p| p.into_inner()).seed(overview);
        self.redraw();
    }

    fn update(&mut self, raw: &serde_json::Value, parsed: Option<&LiveUpdate>) {
        let _ = self.app.emit(live::EVENT, raw);
        if let Some(u) = parsed {
            self.model.lock().unwrap_or_else(|p| p.into_inner()).apply(u);
            self.redraw();
        }
    }

    fn disconnected(&mut self) {
        self.model.lock().unwrap_or_else(|p| p.into_inner()).set_connected(false);
        self.redraw();
    }
}

/// What the tray's menu does.
fn tray_action(app: &AppHandle<Wry>, action: TrayAction) {
    match action {
        TrayAction::Open => {
            shell::handle(app, Lifecycle::TrayOpen);
        }
        // What quitting does about serving follows the setting; the service itself stays registered either way.
        TrayAction::Quit | TrayAction::QuitAndStop => {
            let app = app.clone();
            let forced = (action == TrayAction::QuitAndStop).then_some(quit::QuitChoice::StopServing);
            tauri::async_runtime::spawn(async move { quit::request(&app, forced).await });
        }
        TrayAction::ToggleServing | TrayAction::ReleaseMemory => {
            let client = app.state::<SharedClient>().inner().clone();
            let paused = app.state::<SharedModel>().lock().unwrap_or_else(|p| p.into_inner()).paused();
            tauri::async_runtime::spawn(async move {
                let result = match action {
                    TrayAction::ToggleServing => {
                        commands::forward(client.as_ref(), "set_serving", serde_json::json!({ "paused": !paused })).await
                    }
                    _ => commands::forward(client.as_ref(), "release_memory", serde_json::Value::Null).await,
                };
                if let Err(e) = result {
                    eprintln!("Pitwall: the tray action failed ({:?})", e.code);
                }
            });
        }
    }
}

/// Starts the app.
// The deprecated blocking commands stay registered until the UI has moved to the jobs (WP-3 of Pitwall 1.1).
#[allow(deprecated)]
pub fn run() {
    let data_dir = app_state::data_dir().unwrap_or_default();
    let keys = Arc::new(credentials::StoredAdminKey::new(
        Box::new(credentials::OsKeyStore),
        data_dir.join("admin.key"),
    ));
    let client: SharedClient = Arc::new(server_client::LoopbackClient::new(data_dir, keys));
    let model: SharedModel = Arc::new(Mutex::new(TrayModel::new()));
    let builder = tauri::Builder::default();
    // Must be the first plugin. Debug builds skip it so a development run does not collide with an installed Pitwall.
    #[cfg(all(not(debug_assertions), any(target_os = "macos", windows, target_os = "linux")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        if let Some(event) = window::second_launch(&args) {
            shell::handle(app, event);
        }
    }));
    let result = builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![window::HIDDEN_ARG]),
        ))
        .plugin(updater::plugin())
        .on_menu_event(app_menu::on_event)
        .manage(client.clone())
        .manage(model.clone())
        .manage(Arc::new(commands::Streams::default()))
        .manage(Arc::new(jobs::Jobs::default()))
        .manage(tray::Drawn::default())
        .manage(quit::PendingAsk::default())
        .manage(updater::Pending::default())
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::history,
            commands::usage_series,
            commands::app_detail,
            commands::presets,
            commands::create_preset,
            commands::update_preset,
            commands::delete_preset,
            commands::resolve_preset,
            commands::events,
            commands::set_serving,
            commands::release_memory,
            commands::cache_info,
            commands::clear_cache,
            commands::apply_preset,
            commands::cancel_job,
            commands::local_models,
            commands::search_models,
            commands::repo_files,
            commands::download_model,
            commands::downloads,
            commands::cancel_download,
            commands::delete_model,
            commands::load_model,
            commands::unload_model,
            commands::pin_model,
            commands::set_model_idle,
            commands::update_check_status,
            commands::update_notices,
            commands::check_updates_now,
            commands::set_update_check,
            commands::dismiss_update,
            commands::set_model_slots,
            commands::model_fit,
            commands::disk_usage,
            commands::engines,
            commands::install_build,
            commands::retry_engine_setup,
            commands::remove_build,
            commands::select_build,
            commands::rollback_build,
            commands::engine_pins,
            commands::set_global_pin,
            commands::set_model_pin,
            commands::build_capabilities,
            commands::packs,
            commands::remove_leftover,
            commands::apps,
            commands::create_app,
            commands::delete_app,
            commands::create_key,
            commands::revoke_key,
            commands::set_permissions,
            commands::approve_pairing,
            commands::deny_pairing,
            commands::pairings,
            commands::requests,
            commands::request_content,
            commands::chat,
            commands::chat_cancel,
            commands::benchmarks,
            commands::run_benchmark,
            commands::settings,
            commands::save_settings,
            quit::quit_app,
            quit::take_quit_ask,
            app_commands::app_update_status,
            app_commands::check_app_update,
            app_commands::install_app_update,
            app_commands::service_state,
            app_commands::install_service,
            app_commands::uninstall_service,
            app_commands::decline_service,
            app_commands::open_login_items,
            app_commands::repair_service,
            app_commands::get_autostart,
            app_commands::set_autostart,
            app_commands::start_load_model,
            app_commands::start_benchmark,
            app_commands::start_install_service,
            app_commands::start_uninstall_service,
            app_commands::start_app_update,
            app_commands::start_repair_service,
            app_commands::start_check_updates,
            app_commands::job_cancel,
            app_commands::job_stop_waiting,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            // After an update launchd may refuse the registered service (a rebuilt app has a new code identity on
            // macOS): register it again in the background, so the server comes back without the user's help.
            #[cfg(target_os = "macos")]
            {
                let client = app.state::<SharedClient>().inner().clone();
                std::thread::spawn(move || {
                    let answers =
                        || tauri::async_runtime::block_on(commands::forward(client.as_ref(), "overview", serde_json::Value::Null)).is_ok();
                    // The service may only fail later (it restarts itself onto a new binary when idle), so keep
                    // checking while it does not answer, a bounded number of times per run of the app.
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
            // Opening Pitwall after it was quit with "stop serving" resumes serving.
            {
                let client = app.state::<SharedClient>().inner().clone();
                tauri::async_runtime::spawn(async move { quit::resume_if_stopped(client.as_ref()).await });
            }
            #[cfg(target_os = "macos")]
            {
                match app_menu::build(&handle) {
                    Ok(menu) => {
                        let _ = app.set_menu(menu);
                    }
                    Err(e) => eprintln!("Pitwall: the app menu could not be built ({e})"),
                }
            }
            let view = model.lock().unwrap_or_else(|p| p.into_inner()).view();
            if let Err(e) = tray::create(&handle, &view, tray_action) {
                // Linux without a status-notifier host or libayatana-appindicator: the window is then the only way
                // in, so closing it must exit (see `window::react`).
                if cfg!(target_os = "linux") {
                    eprintln!("Pitwall: no tray icon ({e}); closing the window will quit the app");
                    app.manage(tray::NoTray);
                } else {
                    return Err(e.into());
                }
            }
            if let Some(w) = app.get_webview_window(window::MAIN) {
                native_feel::apply_to_existing(&w);
            }
            // The live feed: UI events and the tray, reconnecting on its own.
            let client = app.state::<SharedClient>().inner().clone();
            let sink_app = handle.clone();
            let sink_model = app.state::<SharedModel>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let mut sink = AppSink {
                    app: sink_app,
                    model: sink_model,
                };
                live::run(client, &mut sink).await;
            });
            // Started at log-in: stay in the tray, without a webview (only when there is a tray to come back from).
            let args: Vec<String> = std::env::args().collect();
            if window::started_hidden(&args) {
                shell::handle(&handle, Lifecycle::StartedHidden);
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match result {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Pitwall could not start: {e}");
            std::process::exit(1);
        }
    };
    // Closing the window destroys it and Pitwall stays in the tray (a hidden webview could come back blank on macOS and
    // kept its memory); Open in the tray, or the Dock icon on macOS, creates it again. Quit exits.
    app.run(|app, event| match event {
        tauri::RunEvent::ExitRequested { code, api, .. }
            if shell::handle(app, Lifecycle::ExitRequested { code }) == window::Reaction::KeepRunning =>
        {
            api.prevent_exit();
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            shell::handle(app, Lifecycle::DockReopen);
        }
        _ => {}
    });
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

    /// Least privilege: the webview gets events and window dragging, nothing from the file system, shell, opener,
    /// updater, autostart or network plugins (the UI goes through Pitwall's own commands).
    #[test]
    fn the_webview_is_granted_no_plugin_or_wildcard_permissions() {
        let (_, granted) = granted();
        for p in &granted {
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
