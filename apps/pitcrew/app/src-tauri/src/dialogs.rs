//! Pitcrew's own dialog contracts (ADR 0026 §3): the confirmation and the stop-process question, the configuration diff, the
//! registration wizard and the Settings window (the shell's window as the app's dialog, because its pages are the app's
//! choice). About, What's new and Updates are the updater's table. The UI's `defineDialog` reads this table from the
//! generated `src/app/generated/dialog-contracts.ts`.

use crate::dialog_callable as app;
use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// Pitcrew's dialogs, and the commands every one of its dialog windows may call (the start-up handshake).
    pub static DIALOGS, base &[shell::startup::window_pong, shell::startup::window_ready];
    // "Please confirm" something that cannot be undone; the caller passes the words (a decision).
    "pc.confirm" => Decision { width: Some(Narrow) },
    // Stopping one process of a runner's tree.
    "pc.stop-process" => Decision { width: Some(Narrow) },
    // What saving a runner's configuration would change; the window is fitted to its content.
    "pc.config-diff" => Form(Modal) { width: Some(Wide) },
    // The registration wizard: registers the runner (a job) and follows it.
    "pc.register" => Form(Modal) {
        width: Some(Wide),
        commands: &[
            app::register_runner,
            app::runner_status,
            app::connection_state,
            app::platform_name,
            app::job_result,
            shell::jobs::jobs_snapshot,
            shell::jobs::job_cancel,
        ],
    },
    // The settings commands, and Pitcrew's own pages of the window (settings, service, channels, tokens, diagnostics).
    "pd.settings" => Tool {
        width: Some(Px(920)),
        height: Some(680),
        commands: &[
            prefix(shell::settings::ALL, "settings_"),
            shell::dialogs::dialog_open,
            app::get_settings,
            app::save_settings,
            app::runner_status,
            app::connection_state,
            app::platform_name,
            app::get_autostart,
            app::set_autostart,
            app::service_install_state,
            app::install_service,
            app::uninstall_service,
            app::repair_service,
            app::pitcrew_open_login_items,
            app::notification_channels_get,
            app::notification_channels_save,
            app::channel_secret_set,
            app::channel_secret_clear,
            app::channel_secret_present,
            app::channel_test,
            app::has_api_token,
            app::set_api_token,
            app::clear_api_token,
            app::export_diagnostics,
            app::open_url,
            app::open_page,
        ],
    },
}
