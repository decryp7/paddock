//! The Dyno app: tray icon, window and tray dropdown over the Dyno agent (ADR 0002). It measures nothing itself:
//! [`link::AgentLink`] finds (or starts) the agent, forwards the webview's requests and turns the agent's pushes into
//! events, and [`tray`] draws the icon from them.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod commands;
pub mod dto;
pub mod link;
pub mod logging;
pub mod login;
pub mod native_feel;
pub mod placement;
pub mod shell;
pub mod tray;
pub mod tray_model;
pub mod updater;
pub mod window;

use link::{AgentLink, ProcessLauncher};
use std::sync::Arc;
use tauri::Manager;

/// Starts the app.
pub fn run() {
    let background = std::env::args().any(|a| a == "--background");
    let result = tauri::Builder::default()
        // Must be the first plugin: a second launch asks the running app to show its window and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::show_main(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(updater::plugin())
        .invoke_handler(tauri::generate_handler![
            commands::dyno,
            commands::app_info,
            commands::tray_open_main,
            commands::tray_quit,
        ])
        .on_window_event(|win, event| match event {
            tauri::WindowEvent::Destroyed => {
                if let Some(link) = win.app_handle().try_state::<Arc<AgentLink>>() {
                    link.drop_window(win.label());
                }
            }
            tauri::WindowEvent::Focused(false) if win.label() == tray::DROPDOWN => tray::dropdown_blurred(win.app_handle()),
            _ => {}
        })
        .setup(move |app| {
            let dirs = dyno_agent::paths::Dirs::current();
            logging::init(&dirs.logs());
            logging::info(&format!(
                "Dyno {} starting (data folder: {})",
                env!("CARGO_PKG_VERSION"),
                dirs.root().display()
            ));
            let shell = shell::Shell::new(app.handle().clone());
            let launcher = ProcessLauncher::new(login::agent_program);
            let link = AgentLink::new(dirs, env!("CARGO_PKG_VERSION"), shell.clone(), Box::new(launcher));
            app.manage(shell);
            app.manage(link.clone());
            if let Err(e) = tray::create(app.handle()) {
                // Linux without a status-notifier host or libayatana-appindicator: the window is then the only way in,
                // so closing it must exit (see `window::keeps_running`).
                if cfg!(target_os = "linux") {
                    logging::info(&format!("no tray icon ({e}); closing the window will quit the app"));
                    app.manage(window::NoTray);
                } else {
                    return Err(e.into());
                }
            }
            link.spawn()?;
            if !window::start_hidden(background, window::has_tray(app.handle())) {
                window::show_main(app.handle());
            }
            // After an update launchd may refuse the registered agent (a rebuilt app has a new code identity on
            // macOS): register it again in the background, so measuring resumes without the user's help.
            let repair = link.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                match login::platform_service().repair(repair.connected()) {
                    Ok(true) => logging::info("registered the agent service again (the OS refused the old registration)"),
                    Ok(false) => {}
                    Err(e) => logging::info(&format!("the agent service could not be registered again: {e}")),
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match result {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Dyno could not start: {e}");
            std::process::exit(1);
        }
    };
    // Closing the window destroys it and Dyno stays in the tray (a hidden webview could come back blank on macOS and
    // kept its memory); Open in the tray, or the app's Dock icon on macOS, builds it again. Quit exits, and then stops
    // the agent this app started unless a service keeps it (see `link::stop_agent_on_quit`).
    app.run(|app, event| match event {
        tauri::RunEvent::ExitRequested { code, api, .. } if window::keeps_running(code, window::has_tray(app)) => api.prevent_exit(),
        tauri::RunEvent::Exit => {
            if let Some(link) = app.try_state::<Arc<AgentLink>>() {
                link.quit();
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => window::show_main(app),
        _ => {}
    });
}

#[cfg(test)]
mod capability_tests {
    /// The window has an overlay title bar on macOS (and no native one where decorations are off): the page's drag
    /// regions only move it when the webview may start a window drag. Without the permission dragging fails silently.
    #[test]
    fn the_main_window_can_be_dragged_by_its_drag_regions() {
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let granted: Vec<&str> = caps["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()).collect();
        assert!(granted.contains(&"core:window:allow-start-dragging"), "{granted:?}");
        assert!(granted.contains(&"core:window:allow-internal-toggle-maximize"), "{granted:?}");
        assert!(caps["windows"].as_array().unwrap().iter().any(|w| w == "main"));
    }

    /// Least privilege: only the core event/window permissions above; no plugin, fs, shell or opener permission.
    #[test]
    fn the_capability_grants_no_plugin_commands() {
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        for p in caps["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()) {
            assert!(p.starts_with("core:event:") || p.starts_with("core:window:allow-"), "{p}");
            assert!(!p.contains('*'), "{p}");
        }
    }
}
