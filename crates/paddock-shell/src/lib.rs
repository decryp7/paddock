//! The Paddock app shell as one Tauri plugin (ADR 0012 §3, decision D2; plan S17).
//!
//! Adopting the shell is one line: the app builds the plugin with its parts and registers it. Every shell command is a
//! plugin command (`plugin:paddock-shell|health_snapshot`), so an app cannot forget one, and the one invoke handler is
//! the shell crates' handlers behind one `paddock-commands` guard over the composed table ([`COMMANDS`]): window
//! classes, per-dialog contracts, request limits.
//!
//! ```ignore
//! let shell = paddock_shell::Builder::new(AppIdentity::new("pitwall", "Pitwall", env!("CARGO_PKG_VERSION")))
//!     .app_commands(&app::COMMANDS)                         // the app's own table: contracts and manifest are checked against both
//!     .dialogs(&[&app::DIALOGS])                           // the app's own dialog table (ADR 0026 §3); the shell's are built in
//!     .health(vec![ServiceSpec::new("llm", "the model server").repair(restart)])
//!     .section(paddock_settings::section(models_section(), apply_models))
//!     .updater(|app| {                                      // says how it installs: .installer(..), or .no_install() for a notify-only build
//!         let key = option_env!("TAURI_UPDATER_PUBKEY");
//!         UpdatesBuilder::new("Pitwall").public_key(key).installer(Arc::new(PluginInstaller::new(app.clone(), key.unwrap_or_default())))
//!     })
//!     .windows(|_app| Windows::builder(WindowConfig::new("Pitwall")).build())
//!     .setup(|app| { /* the app's own state, after the shell's */ Ok(()) })
//!     .build();
//! tauri::Builder::default().plugin(shell) // ...
//! ```
//!
//! The app's capabilities grant the shell's set for each window class: `paddock-shell:main` for `main` and `w-*`,
//! `paddock-shell:popout` for `p-*`, `paddock-shell:dialog` for `d-*` and `paddock-shell:tray` for `tray*`
//! (`permissions/window-classes.toml`, generated from the table). Tauri's ACL is the coarse boundary; the guard is the
//! fine one (each dialog only what its contract declares).
//!
//! - [`COMMANDS`]: every shell crate's table folded into one (`command_table!` composition; a command two crates
//!   declare does not compile), plus the shell's own `shell_version`.
//! - [`Builder`]: the plugin with the shell's state (jobs, notification centre and health, settings with the shell's
//!   sections, dialogs, start-up watchdog, menu, windows, updater) and its hooks (page load, window events, run
//!   events).
//! - [`manifest`]: the Rust check of the app's `paddock-manifest.json` against the plugin and the app's table.
//! - [`VersionReply`]: what `shell_version` answers; the UI package refuses to start quietly on a different version.

mod builder;
mod commands;
pub mod logging;
pub mod manifest;
pub mod notifications;
pub mod permissions;
mod prefs;
pub mod setup;
#[cfg(feature = "mock-runtime")]
pub mod testing;

pub use builder::{AppIdentity, Builder, FirstRun, ServicePart};
pub use commands::{VersionReply, shell_version};
pub use prefs::SettingsPreferences;

/// The plugin's name: commands are `plugin:paddock-shell|<command>`, permissions `paddock-shell:<set>`.
pub const PLUGIN_NAME: &str = "paddock-shell";

/// This shell's version: the npm package `@decryp7/paddock-ui` carries the same one (lockstep).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

paddock_commands::command_table! {
    /// The shell's own commands.
    pub static OWN_COMMANDS;
    /// The plugin's version and the app's identity (the UI checks the version at start-up).
    Query [WINDOW | TRAY] => commands::shell_version,
}

paddock_commands::command_table! {
    /// Every command of the shell plugin: each shell crate's table folded into one, behind one guard.
    pub static COMMANDS = [
        OWN_COMMANDS,
        paddock_startup::COMMANDS,
        paddock_health::tauri::COMMANDS,
        paddock_dialogs::COMMANDS,
        paddock_diag::COMMANDS,
        paddock_files::COMMANDS,
        paddock_storage::COMMANDS,
        paddock_service::COMMANDS,
        setup::COMMANDS,
        paddock_packs::COMMANDS,
        paddock_channels::COMMANDS,
        paddock_mcp::COMMANDS,
        paddock_jobs::tauri::COMMANDS,
        paddock_settings::tauri::COMMANDS,
        paddock_updater::COMMANDS,
        paddock_window::COMMANDS,
        paddock_menu::COMMANDS,
    ];
}

paddock_dialogs::dialogs! {
    /// The shell's own dialogs (ADR 0026 §5): `pd.confirm`, the one shared confirmation. It calls nothing; it answers
    /// `true` or is dismissed, and the opener acts.
    pub static DIALOGS;
    "pd.confirm" => Decision { instanced: true },
}

/// The shell's dialog tables (ADR 0026 §3): every app's plugin registers them; an app adds its own with
/// [`Builder::dialogs`].
pub static SHELL_DIALOGS: [&paddock_dialogs::DialogTable; 7] = [
    &DIALOGS,
    &paddock_channels::DIALOGS,
    &paddock_mcp::DIALOGS,
    &paddock_storage::DIALOGS,
    &paddock_diag::DIALOGS,
    &paddock_updater::DIALOGS,
    &setup::DIALOGS,
];

/// The `dialog_callable` markers of every shell crate's table, by feature, for an app's own dialog table (ADR 0026 §3):
/// `use paddock_shell::dialog_commands as shell;` then `shell::settings::settings_set`.
pub mod dialog_commands {
    pub use crate::dialog_callable as shell;
    pub use crate::setup::dialog_callable as setup;
    pub use paddock_channels::dialog_callable as channels;
    pub use paddock_diag::dialog_callable as diag;
    pub use paddock_dialogs::dialog_callable as dialogs;
    pub use paddock_files::dialog_callable as files;
    pub use paddock_health::tauri::dialog_callable as health;
    pub use paddock_jobs::tauri::dialog_callable as jobs;
    pub use paddock_mcp::dialog_callable as mcp;
    pub use paddock_menu::dialog_callable as menu;
    pub use paddock_packs::dialog_callable as packs;
    pub use paddock_service::dialog_callable as service;
    pub use paddock_settings::tauri::dialog_callable as settings;
    pub use paddock_startup::dialog_callable as startup;
    pub use paddock_storage::dialog_callable as storage;
    pub use paddock_updater::dialog_callable as updater;
    pub use paddock_window::dialog_callable as window;
}

/// The command names, for the plugin's build step (generated from [`COMMANDS`]; a test fails when it is stale).
pub mod command_names {
    include!("command_names.rs");
}

#[cfg(test)]
mod tests;
