//! The Tauri adapter of `paddock-mcp` (built with the `tauri` feature; plan X22).
//!
//! - [`install`] manages the [`McpControl`] and sends its events to every window on [`EVENT`].
//! - [`COMMANDS`](crate::COMMANDS): `mcp_get` and `mcp_audit` (every window), `mcp_save`, `mcp_revoke_key` and `mcp_audit_clear`
//!   (main windows and the Settings dialog), and `mcp_issue_key`, which only the key dialog's window may call: the key is in the
//!   reply to that window alone, shown once, and in no state, event or log.

use crate::control::{AuditRow, IssuedKey, McpControl, McpError, McpEvent, McpSettings, McpView};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager as _, Runtime, State};
use std::sync::Arc;

/// The event every update is sent on.
pub const EVENT: &str = crate::EVENT_NAME;

/// Manages the control (a second call returns the managed one). `build` receives the event sender and builds the control
/// (and, with it, the server whose audit sink is [`McpControl::announcing_sink`]).
pub fn install<R: Runtime, E>(
    app: &AppHandle<R>,
    build: impl FnOnce(crate::control::Emit) -> Result<Arc<McpControl>, E>,
) -> Result<Arc<McpControl>, E> {
    if let Some(existing) = app.try_state::<Arc<McpControl>>() {
        return Ok(existing.inner().clone());
    }
    let handle = app.clone();
    let emit: crate::control::Emit = Arc::new(move |e: &McpEvent| {
        // A failed emit means the app is closing; nothing is left to tell.
        let _ = handle.emit(EVENT, e);
    });
    let control = build(emit)?;
    app.manage(control.clone());
    Ok(control)
}

type Managed<'a> = State<'a, Arc<McpControl>>;

/// The settings, the tools and the keys.
#[::tauri::command]
pub async fn mcp_get(state: Managed<'_>) -> Result<McpView, McpError> {
    let control = state.inner().clone();
    blocking(move || Ok(control.get())).await
}

/// Validates and saves the settings.
#[::tauri::command]
pub async fn mcp_save(state: Managed<'_>, settings: McpSettings) -> Result<McpView, McpError> {
    let control = state.inner().clone();
    blocking(move || control.save(settings)).await
}

/// Issues a client key; called by the key dialog's window only.
#[::tauri::command]
pub async fn mcp_issue_key(state: Managed<'_>, client: String) -> Result<IssuedKey, McpError> {
    let control = state.inner().clone();
    blocking(move || control.issue_key(&client)).await
}

/// Revokes a client key.
#[::tauri::command]
pub async fn mcp_revoke_key(state: Managed<'_>, id: u64) -> Result<McpView, McpError> {
    let control = state.inner().clone();
    blocking(move || control.revoke_key(id)).await
}

/// The recent calls, newest first.
#[::tauri::command]
pub async fn mcp_audit(state: Managed<'_>) -> Result<Vec<AuditRow>, McpError> {
    let control = state.inner().clone();
    blocking(move || Ok(control.audit())).await
}

/// Clears the audit log.
#[::tauri::command]
pub async fn mcp_audit_clear(state: Managed<'_>) -> Result<(), McpError> {
    let control = state.inner().clone();
    blocking(move || {
        control.clear_audit();
        Ok(())
    })
    .await
}

/// Runs quick, bounded work (a file) off the IPC thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, McpError> + Send + 'static,
) -> Result<T, McpError> {
    ::tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| Err(failed()))
}

pub(crate) fn failed() -> McpError {
    McpError {
        code: crate::control::ErrorCode::Failed,
        message: "The request could not be completed. Try again.".into(),
        retryable: true,
    }
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        mcp_get,
        mcp_save,
        mcp_issue_key,
        mcp_revoke_key,
        mcp_audit,
        mcp_audit_clear
    ]
}

#[cfg(test)]
mod tests;
