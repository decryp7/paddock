//! The demo's own dialogs (ADR 0026 §3): one per kind, a modal form and a required choice, and the Settings window (the
//! shell's window as the app's dialog, since its pages are the app's choice). The UI's `defineDialog` reads them from the
//! generated `src/app/generated/dialog-contracts.ts`; the shell's own dialogs are the plugin's tables.

use crate::dialog_callable as demo;
use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// The demo's dialogs, and the commands every one of its dialog windows may call (the start-up handshake).
    pub static DIALOGS, base &[shell::startup::window_pong, shell::startup::window_ready];
    "sd.confirm-reset" => Decision,
    "sd.rename" => Form(Modal) { width: Some(Narrow) },
    "sd.about" => Viewer,
    "sd.inspector" => Tool { width: Some(Px(720)), height: Some(480), commands: &[demo::demo_status] },
    "sd.choose-style" => Decision { required_choice: true },
    // The settings commands, the Background service page's, the set-up page's, the packs page's, the Notification
    // channels page's (the secret is set in the secret dialog's window), Storage's, the AI access page's (the key is
    // issued in the key dialog's window), `dialog_open` (a page opens a dialog of its own) and the Language page's
    // whole-app relaunch.
    "pd.settings" => Tool {
        width: Some(Px(880)),
        height: Some(620),
        commands: &[
            prefix(shell::settings::ALL, "settings_"),
            prefix(shell::service::ALL, "service_"),
            shell::service::open_login_items,
            prefix(shell::setup::ALL, "setup_"),
            prefix(shell::packs::ALL, "packs_"),
            shell::channels::channels_get,
            shell::channels::channels_save,
            shell::channels::channels_clear_secret,
            shell::channels::channels_test,
            prefix(shell::storage::ALL, "storage_"),
            shell::mcp::mcp_get,
            shell::mcp::mcp_save,
            shell::mcp::mcp_revoke_key,
            shell::mcp::mcp_audit,
            shell::mcp::mcp_audit_clear,
            shell::dialogs::dialog_open,
            shell::startup::app_relaunch,
        ],
    },
}
