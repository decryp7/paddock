//! Dyno's own dialog contracts (ADR 0026 §3). Dyno's dialogs are the Settings window (the shell's window as the app's
//! dialog, because its pages are the app's choice), the alert-rule form, the two lookups and the lockers tool. About,
//! What's new, Updates and the shared confirmation are the shell's tables. The UI's `defineDialog` reads this table from
//! the generated `src/app/generated/dialog-contracts.ts`.
//!
//! Every engine request is the one `dyno` command (the agent re-validates each request, `commands::parse_request`), so a
//! dialog that reads or writes engine data names `dyno`; the commands that open a native file dialog stay with the main
//! window.

use crate::dialog_callable as app;
use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// Dyno's dialogs, and the commands every one of its dialog windows may call (the start-up handshake).
    pub static DIALOGS, base &[shell::startup::window_pong, shell::startup::window_ready, shell::diag::log_from_webview];
    // The settings commands, and the engine for Dyno's page of the window (its settings, the tray items, the plugins).
    // Dyno's page also edits the popup and menu-bar widget layouts (their editors read the layouts, the engine's choices and the
    // popup preview): those commands are open to dialogs in `COMMANDS` and must be named here too, or the window may not call them.
    "pd.settings" => Tool {
        width: Some(Px(880)),
        height: Some(700),
        commands: &[
            prefix(shell::settings::ALL, "settings_"),
            shell::dialogs::dialog_open,
            shell::mcp::mcp_get,
            shell::mcp::mcp_save,
            shell::mcp::mcp_revoke_key,
            shell::mcp::mcp_audit,
            shell::mcp::mcp_audit_clear,
            app::dyno,
            app::widget_layout,
            app::edit_widget_layout,
            app::widget_choices,
            app::popover_preview,
            app::popover_snapshot,
        ],
    },
    // Adding or changing an alert rule: validated by the engine on save.
    "dy.alert-rule" => Form(Modal) {
        width: Some(Medium),
        commands: &[app::dyno],
    },
    // What is locking a file or folder: a read-only list of the programs that hold it open.
    "dy.lockers" => Tool {
        width: Some(Px(560)),
        height: Some(480),
        commands: &[app::dyno],
    },
    // "Search the web about this": the engine builds the address, the browser opens on the click.
    "dy.web-search" => Form(Modal) {
        width: Some(Narrow),
        commands: &[app::dyno],
    },
    // "Search this file's hash on VirusTotal": the hash streams in, then the browser opens on the click.
    "dy.file-hash" => Viewer {
        width: Some(Wide),
        commands: &[app::dyno],
    },
}
