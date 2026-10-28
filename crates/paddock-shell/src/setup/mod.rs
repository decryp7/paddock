//! The first-run set-up checklist (plan X19): the things an app asks once and then lets the user come back to.
//!
//! - [`Checklist`]: the steps in order, each one's own check, the user's answers (skipped, finished) in a `paddock-env`
//!   state file, and the actions run on a worker with the outcome sent to every window on [`EVENT`].
//! - [`SetupItem`]: one step. The shell brings four ([`items`]): the background service (`paddock-service`, plan X4),
//!   start at login (the `pd.login` setting), notification permission (the OS notifier's `Toaster::permission`, plan X8)
//!   and Pitwall pairing (`paddock-pitwall`'s `Link`, with the shared needs-AI shape). An app adds its own (Debrief's
//!   tour) by implementing the trait.
//! - Every step is skippable, the checklist can be finished, and it stays reachable later (Settings > Set-up and
//!   the Help menu). A step that has a problem always has a Repair that does not depend on the thing it repairs
//!   (rule 19). Nothing here blocks the IPC thread (rule 10): `setup_state` runs on the blocking pool and
//!   `setup_act`/`setup_repair` return at once.
//!
//! The app turns it on with [`Builder::first_run`](crate::Builder::first_run).

pub mod checklist;
pub mod items;
pub mod model;
pub mod tauri;

#[cfg(test)]
mod generated;
#[cfg(test)]
mod tests;

pub use checklist::{Checklist, Do, Emit, ListError, MAX_ID_LEN, MAX_ITEMS, Runner, SetupFile};
pub use items::{
    LinkPort, LoginItem, NotificationAccess, NotificationsItem, PitwallItem, PitwallPort,
    ServiceItem, ServicePort, ToasterAccess, ids, start_pitwall,
};
pub use model::{
    Check, ItemStatus, ItemView, SetupCommandError, SetupErrorCode, SetupItem, SetupState,
};

/// The event every window hears when the checklist changed (the payload is a [`SetupState`]).
pub const EVENT: &str = "paddock://setup";
/// The file of the user's answers, in the app's config folder.
pub const FILE_NAME: &str = "setup.json";

paddock_commands::command_table! {
    /// The commands of the set-up checklist. Every window may look; the main window and dialogs act.
    pub static COMMANDS;
    /// The checklist now (each step is asked on the blocking pool).
    Query [WINDOW] => tauri::setup_state,
    /// Runs a step's action; returns at once, the outcome arrives as an event.
    Mutation [MAIN | DIALOG] => tauri::setup_act,
    /// Runs a step's repair; returns at once, the outcome arrives as an event.
    Mutation [MAIN | DIALOG] => tauri::setup_repair,
    /// Skips a step for now, or takes it up again.
    Mutation [MAIN | DIALOG] => tauri::setup_skip,
    /// Finishes the checklist: it does not open by itself again.
    Mutation [MAIN | DIALOG] => tauri::setup_finish,
}

paddock_dialogs::dialogs! {
    /// The set-up checklist as its own window (ADR 0026 §3).
    pub static DIALOGS;
    "pd.setup" => Tool { width: Some(Px(560)), height: Some(520), commands: &[
        dialog_callable::setup_state,
        dialog_callable::setup_act,
        dialog_callable::setup_repair,
        dialog_callable::setup_skip,
        dialog_callable::setup_finish,
    ] },
}
