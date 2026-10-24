//! The Tauri adapter of `paddock-settings` (built with the `tauri` feature; ADR 0012 §3, plan S10).
//!
//! - [`manage`] puts the app's [`Settings`] in the app state, sends every change to every window as [`EVENT`]
//!   (`paddock://settings`, a [`SettingsChanged`]) and, when the file could not be used at start-up, posts one
//!   notification (cause [`PROBLEM_CAUSE`], with an "Open Settings" action) through `paddock-health`'s centre. Set up
//!   `paddock_health::tauri::init` first; without it the problem is only logged and shown in the Settings window.
//! - [`COMMANDS`] is the adapter's command table: `settings_overview` (every section with its values), `settings_set`
//!   (validate, apply, save and announce one section's changes) and `settings_reset` (a section or one field back to its
//!   default). They are async and return at once; the writes run on the blocking pool, never on the IPC or main thread
//!   (rule 10). [`handler`] is their guarded invoke handler and [`route`] puts it in front of the app's own.
//!
//! ```ignore
//! tauri::Builder::default()
//!     .setup(|app| {
//!         paddock_health::tauri::init(app.handle(), health_setup)?;     // first: the problem notice goes there
//!         let settings = paddock_settings::tauri::manage(app.handle(), Settings::open(config_dir.join("settings.json")));
//!         settings.register(Arc::new(paddock_settings::shell::appearance()))?;
//!         settings.register(paddock_settings::section(models_section(), |v| apply_models(v)))?;
//!         Ok(())
//!     })
//!     .invoke_handler(paddock_settings::tauri::route(
//!         paddock_settings::tauri::handler_with_grants(paddock_dialogs::tauri::grant_of),
//!         Guard::new(&APP_COMMANDS).wrap(app_handler!()),
//!     ))
//! ```

use crate::service::{
    ChangeSink, SetResult, Settings, SettingsChanged, SettingsCommandError, SettingsError,
    SettingsErrorCode, SettingsOverview,
};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use paddock_commands::DialogGrant;
use paddock_commands::tauri::Guard;
use paddock_health::notify::centre::{NewNotice, NoticeAction, NoticeScope, NoticeSeverity};
use serde_json::Value;
use std::sync::Arc;

/// The event every change that took effect is sent on, to every window; the payload is a [`SettingsChanged`].
pub const EVENT: &str = crate::EVENT_NAME;

/// The notification cause of a settings file that could not be used at start-up.
pub const PROBLEM_CAUSE: &str = "settings:file";

/// The notification source of the settings' messages.
pub const SOURCE: &str = "settings";

/// Sends every change to all webviews as [`EVENT`]. A failed emit (the app is closing) is dropped: windows that missed it
/// read `settings_overview`.
pub struct Emit<R: Runtime>(AppHandle<R>);

impl<R: Runtime> Emit<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Emit(app)
    }
}

impl<R: Runtime> ChangeSink for Emit<R> {
    fn changed(&self, change: &SettingsChanged) {
        let _ = self.0.emit(EVENT, change);
    }
}

/// The managed state the commands read.
pub struct ManagedSettings(Arc<Settings>);

impl ManagedSettings {
    pub fn settings(&self) -> &Arc<Settings> {
        &self.0
    }
}

/// Manages `settings` (its changes go to every window as [`EVENT`]), reports a start-up problem once and returns the
/// shared service for the app's registrations. A second call keeps the first service and returns it.
pub fn manage<R: Runtime>(app: &AppHandle<R>, settings: Settings) -> Arc<Settings> {
    if let Some(managed) = app.try_state::<ManagedSettings>() {
        return managed.settings().clone();
    }
    settings.set_sink(Arc::new(Emit::new(app.clone())));
    if let Some(problem) = settings.problem() {
        let notice = NewNotice::new(
            PROBLEM_CAUSE,
            SOURCE,
            NoticeSeverity::Warning,
            "Settings were reset to defaults",
            NoticeScope::App,
        )
        .body(problem.message.clone())
        .code("pd.settings.file-problem")
        .primary(NoticeAction::new(NoticeAction::OPEN_SETTINGS))
        .once();
        // `None` without a notification centre: the problem was logged when the file was read.
        let _ = paddock_health::tauri::post(app, None, notice, false);
    }
    let settings = Arc::new(settings);
    app.manage(ManagedSettings(settings.clone()));
    settings
}

fn not_set_up() -> SettingsCommandError {
    SettingsCommandError {
        code: SettingsErrorCode::NotSetUp,
        message: "Settings are not set up in this app.".into(),
        retryable: false,
    }
}

fn settings_of<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Settings>, SettingsCommandError> {
    app.try_state::<ManagedSettings>()
        .map(|m| m.settings().clone())
        .ok_or_else(not_set_up)
}

/// Runs a change on the blocking pool (it writes the file and calls the section's `apply`).
async fn change<R: Runtime>(
    app: AppHandle<R>,
    work: impl FnOnce(&Settings) -> Result<SetResult, SettingsError> + Send + 'static,
) -> Result<SetResult, SettingsCommandError> {
    let settings = settings_of(&app)?;
    match ::tauri::async_runtime::spawn_blocking(move || work(&settings)).await {
        Ok(result) => result.map_err(|e| SettingsCommandError::from(&e)),
        Err(e) => Err(SettingsCommandError {
            code: SettingsErrorCode::ApplyFailed,
            message: format!("The change failed unexpectedly: {e}"),
            retryable: false,
        }),
    }
}

/// Every section with its current values, the restart state and the start-up problem.
#[::tauri::command]
pub async fn settings_overview<R: Runtime>(
    app: AppHandle<R>,
) -> Result<SettingsOverview, SettingsCommandError> {
    Ok(settings_of(&app)?.overview())
}

/// Lays `values` over section `section`'s current values, validates, applies, saves and announces them. Refused values
/// come back as `errors` with `applied: false`.
#[::tauri::command]
pub async fn settings_set<R: Runtime>(
    app: AppHandle<R>,
    section: String,
    values: Value,
) -> Result<SetResult, SettingsCommandError> {
    change(app, move |s| s.set(&section, &values)).await
}

/// Puts section `section` back to its defaults, or only its field `key`.
#[::tauri::command]
pub async fn settings_reset<R: Runtime>(
    app: AppHandle<R>,
    section: String,
    key: Option<String>,
) -> Result<SetResult, SettingsCommandError> {
    change(app, move |s| s.reset(&section, key.as_deref())).await
}

paddock_commands::command_table! {
    /// The commands of the settings adapter. Every window may read; main windows and dialogs whose contract names them
    /// may change (the Settings window is a dialog).
    pub static COMMANDS, macro generated_handler;
    Query [WINDOW] => settings_overview,
    Mutation [MAIN | DIALOG] limit paddock_commands::MIB => settings_set,
    Mutation [MAIN | DIALOG] => settings_reset,
}

/// The invoke handler of [`COMMANDS`] without a guard: for a guard of the app's or the shell plugin's own (which folds
/// [`COMMANDS`] into its table).
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    generated_handler!()
}

/// The guarded invoke handler of [`COMMANDS`]. Dialog windows may call nothing of it; use [`handler_with_grants`] so the
/// dialogs whose contracts name a settings command (the Settings window) may call it.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS).wrap(generated_handler!())
}

/// As [`handler`], with the per-dialog contracts (ADR 0009): `lookup(app, label)` gives the grant of dialog `label`.
pub fn handler_with_grants<R: Runtime, G: DialogGrant + 'static>(
    lookup: impl Fn(&AppHandle<R>, &str) -> Option<G> + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&COMMANDS)
        .dialog_grants(lookup)
        .wrap(generated_handler!())
}

/// One invoke handler of two: the commands of [`COMMANDS`] go to `settings`, everything else to `other` (the app's).
pub fn route<R: Runtime>(
    settings: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
    other: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<R>| {
        if COMMANDS.spec(invoke.message.command()).is_some() {
            settings(invoke)
        } else {
            other(invoke)
        }
    }
}
