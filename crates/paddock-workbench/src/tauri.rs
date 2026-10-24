//! The Tauri adapter: the managed [`LayoutStore`] and the layout commands (plan W2a, W7).
//!
//! ```ignore
//! app.manage(paddock_workbench::tauri::Layouts::open(dirs.config()));
//! // served by the workbench plugin, or in an app's own table: `paddock_workbench::tauri::invoke_handler()`
//! ```

use crate::layout::{FILE_NAME, LayoutError, LayoutStore};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Manager, Runtime};
use paddock_commands::CommandError;
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::Arc;

/// The managed store.
#[derive(Clone)]
pub struct Layouts(pub Arc<LayoutStore>);

impl Layouts {
    /// Opens `<config>/workbench-layout.json` (never fails; a problem is logged once).
    pub fn open(config_dir: &Path) -> Layouts {
        let store = LayoutStore::open(config_dir.join(FILE_NAME));
        if let Some(problem) = store.problem() {
            log::warn!(target: "paddock_workbench", "workbench layouts: {problem}");
        }
        Layouts(Arc::new(store))
    }
}

/// The error of a refused request, in the one envelope (`paddock_commands::CommandError`).
pub fn to_command_error(e: LayoutError) -> CommandError {
    match e {
        LayoutError::BadKey => CommandError::invalid(e.to_string()).field("key"),
        LayoutError::NotAnObject => CommandError::invalid(e.to_string()).field("value"),
        LayoutError::TooLarge | LayoutError::Full => CommandError::limit(e.to_string()),
        LayoutError::Write(_) => CommandError::io(e.to_string()),
    }
}

fn not_set_up() -> Refusal {
    Box::new(CommandError::internal(
        "The workbench layouts are not set up (manage paddock_workbench::tauri::Layouts).",
    ))
}

/// A refusal on the wire (boxed: the envelope is large and every command returns it).
pub type Refusal = Box<CommandError>;

/// Runs store work on the blocking pool (file I/O never on the IPC thread, rule 10).
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, LayoutError> + Send + 'static,
) -> Result<T, Refusal> {
    let joined = ::tauri::async_runtime::spawn_blocking(work).await;
    joined
        .unwrap_or_else(|e| Err(LayoutError::Write(e.to_string())))
        .map_err(|e| Box::new(to_command_error(e)))
}

/// `layout_get`: the value under `key`, or null.
#[::tauri::command]
pub async fn layout_get<R: Runtime>(
    app: AppHandle<R>,
    key: String,
) -> Result<Option<Map<String, Value>>, Refusal> {
    let store = app
        .try_state::<Layouts>()
        .ok_or_else(not_set_up)?
        .inner()
        .0
        .clone();
    blocking(move || store.get(&key)).await
}

/// `layout_set`: saves `value` under `key` (`null` deletes it).
#[::tauri::command]
pub async fn layout_set<R: Runtime>(
    app: AppHandle<R>,
    key: String,
    value: Option<Value>,
) -> Result<(), Refusal> {
    let store = app
        .try_state::<Layouts>()
        .ok_or_else(not_set_up)?
        .inner()
        .0
        .clone();
    blocking(move || store.set(&key, value)).await
}

/// The unguarded handler of [`COMMANDS`](crate::COMMANDS) (the plugin wraps it in the guard).
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![layout_get, layout_set]
}

#[cfg(test)]
mod tests;
