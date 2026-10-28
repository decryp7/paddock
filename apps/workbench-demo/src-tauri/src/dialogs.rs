//! The demo's dialog table (ADR 0026 §3): it has no dialogs of its own (the dock's layout dialogs are the workbench's
//! table), only the commands every one of its dialog windows may call: the start-up handshake and the panels' UI state
//! sync.

use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// No own dialogs; the base commands of its dialog windows.
    pub static DIALOGS, base &[
        shell::startup::window_ready,
        shell::startup::window_pong,
        paddock_panels::dialog_callable::ui_sync_publish,
    ];
}
