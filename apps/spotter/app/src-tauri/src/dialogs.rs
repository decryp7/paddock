//! Spotter's own dialog contracts (ADR 0026 §3). Only the Settings window is Spotter's: the shell's window as the app's
//! dialog, because its pages are the app's choice. About, What's new, Updates and the shared confirmation are the shell's
//! tables. The UI's `defineDialog` reads this table from the generated `src/app/generated/dialog-contracts.ts`.

use crate::dialog_callable as app;
use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// Spotter's dialogs, and the commands every one of its dialog windows may call (the start-up handshake).
    pub static DIALOGS, base &[shell::startup::window_pong, shell::startup::window_ready];
    // The settings commands and the commands Spotter's page of the window calls (general, mail, the page browser).
    "pd.settings" => Tool {
        width: Some(Px(880)),
        height: Some(640),
        commands: &[
            prefix(shell::settings::ALL, "settings_"),
            // The page shows the browser install's phase and Cancel from the window's own jobs list (`provideShellJobs`).
            shell::jobs::job_cancel,
            shell::jobs::jobs_snapshot,
            app::app_info,
            app::choose_folder,
            // Spotter's page for the tray popup's widgets (the chooser and its live preview).
            app::edit_popover,
            app::get_autostart,
            app::get_settings,
            app::install_renderer,
            app::mail_status,
            app::open,
            app::pause_notifications,
            app::popover_values,
            app::popover_view,
            app::renderer_status,
            app::save_settings,
            app::send_test_email,
            app::set_autostart,
            app::set_mail_password,
        ],
    },
}
