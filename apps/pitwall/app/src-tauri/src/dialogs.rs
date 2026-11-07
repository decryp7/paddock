//! Pitwall's own dialog contracts (ADR 0026 §3, the PW-D row of `docs/plans/debrief-1-2-api-dialogs.md`). Each is a real
//! OS window (rule 17) that the shell opens fitted to its content. Pitwall's dialogs decide and return their choice; the
//! window that opened them acts on it (creates the app, quits, installs the service), so no dialog calls one of Pitwall's
//! commands itself and each contract lists only the start-up handshake every dialog window needs. The shared confirmation
//! (`pd.confirm`), About, What's new and Updates are the shell's tables. The slots panel stays an in-app popover (an
//! anchored panel, rule 17). The one exception is the Settings window: the shell's window as Pitwall's tool, whose
//! Pitwall page reads and saves the settings, runs the service's jobs and clears the cache itself.

use crate::dialog_callable as app;
use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// Pitwall's dialogs, and the commands every one of its dialog windows may call (the start-up handshake).
    pub static DIALOGS, base &[shell::startup::window_pong, shell::startup::window_ready];
    // The Settings window: the shell's settings commands, and what Pitwall's page needs (its settings, presets of the
    // memory policy, the response cache, the overview for the network address it shows, the service and start at log-in,
    // the model update checks, its jobs) and `dialog_open` for the confirmation before a cache is cleared.
    "pd.settings" => Tool {
        width: Some(Px(880)),
        height: Some(700),
        commands: &[
            prefix(shell::settings::ALL, "settings_"),
            shell::dialogs::dialog_open,
            app::overview,
            // The Tray popup page: the layout, what the widgets show, the header, the rows each draws and one edit at a time.
            app::popover_view,
            app::popover_values,
            app::popover_header,
            app::popover_shape,
            app::edit_popover,
            app::settings,
            app::save_settings,
            app::apply_preset,
            app::cache_info,
            app::clear_cache,
            app::service_state,
            app::start_install_service,
            app::start_uninstall_service,
            app::start_repair_service,
            app::decline_service,
            app::open_service_login_items,
            app::get_autostart,
            app::set_autostart,
            app::update_check_status,
            app::update_notices,
            app::set_update_check,
            app::start_check_updates,
            app::app_job_cancel,
            app::app_job_stop_waiting,
            // Connected apps (ADR 0023 Decision 7): the apps, a pasted key (from `pw.mcp-key`), Refresh and Forget.
            app::connected_apps,
            app::set_app_key,
            app::connect_app,
            app::forget_app,
            app::revoke_tool_approval,
            app::open_connected_app,
        ],
    },
    // "New app": the name (Create app is disabled until it is valid); the window that opened it creates the app.
    "pw.create-app" => Form(Modal) {
        width: Some(Narrow),
    },
    // "Connect Dyno": the AI access key the user pastes; the Settings page that opened it keeps it (`set_app_key`).
    "pw.mcp-key" => Form(Modal) {
        width: Some(Narrow),
    },
    // The new key, shown once: Copy, then "I've saved it". No Cancel; Escape and the window's close do nothing.
    "pw.key-secret" => Decision {
        required_choice: true,
    },
    // "Quit Pitwall?": keep serving or stop serving, with "Don't ask again" in the body.
    "pw.quit" => Decision {
        width: Some(Medium),
    },
    // "Run Pitwall in the background?": Not now or Run in the background, with "Don't ask again" in the body; the install
    // then runs as a job in the main window.
    "pw.service-consent" => Decision {
        width: Some(Medium),
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four contracts are valid, have the kinds and widths the plan gives them, and open nothing of Pitwall's to a
    /// dialog (the opener acts on the choice).
    #[test]
    fn the_contracts_are_the_planned_ones() {
        let c = paddock_dialogs::Contracts::from_tables(&[&DIALOGS]).unwrap();
        for (id, width) in [
            ("pw.create-app", 400.0),
            ("pw.mcp-key", 400.0),
            ("pw.key-secret", 400.0),
            ("pw.quit", 560.0),
            ("pw.service-consent", 560.0),
        ] {
            assert_eq!(c.get(id).map(|r| r.width), Some(width), "{id}");
            for command in ["create_app", "quit_app", "start_install_service", "save_settings", "set_app_key"] {
                assert!(!c.allows(id, command), "{id} may not call {command}");
            }
        }
        for command in [
            "settings",
            "save_settings",
            "clear_cache",
            "start_install_service",
            "set_autostart",
            "connected_apps",
            "set_app_key",
            "connect_app",
            "forget_app",
            "revoke_tool_approval",
            "open_connected_app",
        ] {
            assert!(c.allows("pd.settings", command), "the Settings window may call {command}");
        }
        assert!(!c.allows("pd.settings", "delete_model") && !c.allows("pd.settings", "chat"));
        assert!(
            !c.allows("pd.settings", "quit_app") && !c.allows("pd.settings", "download_model"),
            "the Settings window neither quits nor downloads"
        );
        let secret = DIALOGS.entries().iter().find(|e| e.id == "pw.key-secret").unwrap();
        assert!(
            secret.required_choice,
            "the key is shown once: the dialog cannot be dismissed without the answer"
        );
    }
}
