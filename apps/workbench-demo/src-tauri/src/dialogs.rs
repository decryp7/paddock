//! The demo's dialog table (ADR 0026 §3): it has no dialogs of its own (the dock's layout dialogs are the workbench's
//! table), only the commands every one of its dialog windows may call (the panels' UI state sync).

paddock_dialogs::dialogs! {
    /// No own dialogs; the base commands of its dialog windows.
    pub static DIALOGS, base &[paddock_panels::dialog_callable::ui_sync_publish];
}
