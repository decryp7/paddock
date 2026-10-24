//! The Tauri adapter of `paddock-channels` (built with the `tauri` feature; plan X17).
//!
//! - [`install`] manages the [`ChannelsControl`] and sends its events to every window on [`EVENT`].
//! - [`COMMANDS`](crate::COMMANDS): `channels_get` (every window), `channels_save`, `channels_clear_secret` and `channels_test`
//!   (main windows and the Settings dialog), and `channels_set_secret`, which only the secret dialog's window and the Settings
//!   dialog may call: the secret goes from that window straight to the credential store and is never part of any state or event.
//! - `channels_test` returns at once with a test id; the outcome arrives as a [`ChannelsEvent::Test`] (rule 10).

use crate::config::{ChannelConfig, Settings};
use crate::control::{ChannelsControl, ChannelsError, ChannelsEvent, ChannelsView, TestStarted};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager as _, Runtime, State};
use std::sync::Arc;

/// The event every update is sent on.
pub const EVENT: &str = crate::EVENT_NAME;

/// Manages the control (a second call returns the managed one).
pub fn install<R: Runtime, E>(
    app: &AppHandle<R>,
    build: impl FnOnce(crate::control::Emit) -> Result<Arc<ChannelsControl>, E>,
) -> Result<Arc<ChannelsControl>, E> {
    if let Some(existing) = app.try_state::<Arc<ChannelsControl>>() {
        return Ok(existing.inner().clone());
    }
    let handle = app.clone();
    let emit: crate::control::Emit = Arc::new(move |e: &ChannelsEvent| {
        // A failed emit means the app is closing; nothing is left to tell.
        let _ = handle.emit(EVENT, e);
    });
    let control = build(emit)?;
    app.manage(control.clone());
    Ok(control)
}

type Managed<'a> = State<'a, Arc<ChannelsControl>>;

/// The settings and which secrets are stored.
#[::tauri::command]
pub async fn channels_get(state: Managed<'_>) -> Result<ChannelsView, ChannelsError> {
    let control = state.inner().clone();
    blocking(move || control.get()).await
}

/// Validates and saves the settings.
#[::tauri::command]
pub async fn channels_save(
    state: Managed<'_>,
    settings: Settings,
) -> Result<ChannelsView, ChannelsError> {
    let control = state.inner().clone();
    blocking(move || control.save(settings)).await
}

/// Stores a channel's token or password; called by the secret dialog's window only.
#[::tauri::command]
pub async fn channels_set_secret(
    state: Managed<'_>,
    id: String,
    value: String,
) -> Result<(), ChannelsError> {
    let control = state.inner().clone();
    blocking(move || control.set_secret(&id, &value)).await
}

/// Removes a channel's token or password.
#[::tauri::command]
pub async fn channels_clear_secret(state: Managed<'_>, id: String) -> Result<(), ChannelsError> {
    let control = state.inner().clone();
    blocking(move || control.clear_secret(&id)).await
}

/// Starts a test send; returns at once, the outcome arrives as an event.
#[::tauri::command]
pub async fn channels_test(
    state: Managed<'_>,
    channel: ChannelConfig,
) -> Result<TestStarted, ChannelsError> {
    state.test(channel).map(|test_id| TestStarted { test_id })
}

/// Runs quick, bounded work (a file, the credential store) off the IPC thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ChannelsError> + Send + 'static,
) -> Result<T, ChannelsError> {
    ::tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| Err(failed()))
}

pub(crate) fn failed() -> ChannelsError {
    ChannelsError {
        code: crate::control::ErrorCode::Failed,
        message: "The request could not be completed. Try again.".into(),
        retryable: true,
    }
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        channels_get,
        channels_save,
        channels_set_secret,
        channels_clear_secret,
        channels_test
    ]
}

#[cfg(test)]
mod tests;
