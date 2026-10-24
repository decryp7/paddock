//! The demo's own commands, behind its guard ([`crate::COMMANDS`]): the four commands of the shared `<pd-needs-ai>`
//! component (`paddock_pitwall::status::COMMANDS`) and the sample re-index. Each is async and returns at once (rule 10);
//! anything that talks to Pitwall runs on the blocking pool.

// `CommandError` is the family's one command error (plan X28); its size is its business, not these commands'.
#![allow(clippy::result_large_err)]

use crate::engine::{self, Engine};
use paddock_commands::CommandError;
use paddock_pitwall::{AiSnapshot, AiState, Link, LinkView};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

/// The app's Pitwall link, managed once at start.
pub struct Pitwall {
    pub link: Arc<Link>,
}

/// What the shared component draws for `view` (the demo has no engine of its own, so "use the built-in engine" is not
/// offered: rule 19).
pub fn snapshot_of(link: &Link, view: &LinkView) -> AiSnapshot {
    let app = link.app_name();
    let state = AiState::from_view(view, link.needs(), false);
    AiSnapshot {
        message: state.message(app),
        actions: state.actions(),
        pairing: view.pairing.clone(),
        built_in_available: false,
        built_in_for_session: false,
        app: app.to_string(),
        state,
    }
}

fn link_of<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<Link>, CommandError> {
    app.try_state::<Pitwall>()
        .map(|p| p.link.clone())
        .ok_or_else(|| CommandError::unavailable("Pitwall is not set up in this app."))
}

/// Tells every window the snapshot changed (`paddock://pitwall`).
pub fn announce<R: Runtime>(app: &AppHandle<R>, link: &Link) {
    let _ = app.emit(
        paddock_pitwall::status::EVENT,
        snapshot_of(link, &link.view()),
    );
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| CommandError::internal(format!("The Pitwall check failed unexpectedly: {e}")))
}

/// Looks at Pitwall now and answers what the component shows.
#[tauri::command]
pub async fn pitwall_state<R: Runtime>(app: AppHandle<R>) -> Result<AiSnapshot, CommandError> {
    let link = link_of(&app)?;
    blocking(move || {
        let view = link.probe();
        snapshot_of(&link, &view)
    })
    .await
}

/// The user chose to connect: starts the pairing; the code is in the answer.
#[tauri::command]
pub async fn pitwall_connect<R: Runtime>(app: AppHandle<R>) -> Result<AiSnapshot, CommandError> {
    let link = link_of(&app)?;
    blocking(move || match link.connect(true) {
        Ok(view) => Ok(snapshot_of(&link, &view)),
        Err(e) => Err(CommandError::failed(e.user_message(link.app_name()))),
    })
    .await?
}

/// Stops a waiting pairing; false when none waits.
#[tauri::command]
pub async fn pitwall_cancel_pairing<R: Runtime>(app: AppHandle<R>) -> Result<bool, CommandError> {
    let link = link_of(&app)?;
    blocking(move || link.cancel_pairing()).await
}

/// The demo has no engine of its own, so the repair is not offered; a call is refused with the reason.
#[tauri::command]
pub async fn pitwall_use_builtin<R: Runtime>(
    _app: AppHandle<R>,
) -> Result<AiSnapshot, CommandError> {
    Err(CommandError::unavailable(
        "The demo has no engine of its own to switch to.",
    ))
}

/// Starts the sample re-index and returns its job id at once; its progress goes out on `paddock://jobs`.
#[tauri::command]
pub async fn demo_reindex<R: Runtime>(
    app: AppHandle<R>,
    engine: State<'_, Engine>,
) -> Result<u64, CommandError> {
    let jobs = engine::registry(&app)
        .ok_or_else(|| CommandError::unavailable("The job registry is not set up."))?;
    let id = engine
        .reindex(&jobs)
        .map_err(|_| CommandError::unavailable("The demo is shutting down."))?;
    Ok(id.0)
}
