//! The Tauri adapter of the checklist: it manages the [`Checklist`], sends every change to every window and serves the
//! commands. Without a checklist the commands answer `notSetUp` (degrade, don't fail).

use super::checklist::{Checklist, Do, ListError, Runner};
use super::model::{SetupCommandError, SetupItem, SetupState};
use super::{EVENT, FILE_NAME};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use std::path::PathBuf;
use std::sync::Arc;

/// Manages a checklist of `items` for the app; the answers live in `config_dir/setup.json` (`None`: in memory).
pub fn install<R: Runtime>(
    app: &AppHandle<R>,
    items: Vec<Arc<dyn SetupItem>>,
    config_dir: Option<PathBuf>,
) -> Result<Arc<Checklist>, ListError> {
    let handle = app.clone();
    let emit = Arc::new(move |state: &SetupState| {
        // A failed emit means the app is closing; nothing is left to tell.
        let _ = handle.emit(EVENT, state);
    });
    let run: Runner = Arc::new(|job| {
        std::thread::Builder::new()
            .name("paddock-setup".into())
            .spawn(job)
            .map(drop)
            .map_err(|e| e.to_string())
    });
    let list = Checklist::new(items, config_dir.map(|d| d.join(FILE_NAME)), emit, run)?;
    app.manage(list.clone());
    Ok(list)
}

fn checklist<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Checklist>, SetupCommandError> {
    app.try_state::<Arc<Checklist>>()
        .map(|s| s.inner().clone())
        .ok_or_else(SetupCommandError::not_set_up)
}

/// Where every step stands (each is asked on the blocking pool, with bounded helpers).
#[::tauri::command]
pub async fn setup_state<R: Runtime>(app: AppHandle<R>) -> Result<SetupState, SetupCommandError> {
    let list = checklist(&app)?;
    ::tauri::async_runtime::spawn_blocking(move || list.state())
        .await
        .map_err(|e| SetupCommandError::failed(&e.to_string()))
}

/// Runs a step's action; returns at once, the outcome arrives as an event.
#[::tauri::command]
pub async fn setup_act<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), SetupCommandError> {
    checklist(&app)?.request(&id, Do::Act)
}

/// Runs a step's repair; returns at once, the outcome arrives as an event.
#[::tauri::command]
pub async fn setup_repair<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<(), SetupCommandError> {
    checklist(&app)?.request(&id, Do::Repair)
}

/// Skips a step for now (`skipped`), or takes it up again.
#[::tauri::command]
pub async fn setup_skip<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    skipped: bool,
) -> Result<SetupState, SetupCommandError> {
    let list = checklist(&app)?;
    ::tauri::async_runtime::spawn_blocking(move || list.skip(&id, skipped))
        .await
        .map_err(|e| SetupCommandError::failed(&e.to_string()))?
}

/// Finishes the checklist: it does not open by itself again.
#[::tauri::command]
pub async fn setup_finish<R: Runtime>(app: AppHandle<R>) -> Result<SetupState, SetupCommandError> {
    let list = checklist(&app)?;
    ::tauri::async_runtime::spawn_blocking(move || list.finish())
        .await
        .map_err(|e| SetupCommandError::failed(&e.to_string()))?
}

/// The invoke handler of [`COMMANDS`](super::COMMANDS); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        setup_state,
        setup_act,
        setup_repair,
        setup_skip,
        setup_finish
    ]
}
