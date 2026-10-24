//! Forwarding UI state between the windows of a host.

use super::popout::panels;
use crate::sync::{check_message, recipients};
use crate::wire::{PanelError, PanelErrorCode, SyncPublish, UI_SYNC_EVENT};
use ::tauri::{AppHandle, Emitter, Manager, Runtime};

/// Delivers `publish.message` from the window `sender` to the other windows of its host (see [`recipients`]): refused when
/// it is not a JSON object, larger than the limit, or sent faster than the window's rate. Returns how many windows got it.
pub fn publish<R: Runtime>(
    app: &AppHandle<R>,
    sender: &str,
    publish: SyncPublish,
) -> Result<usize, PanelError> {
    let panels = panels(app)?;
    check_message(&publish.message)?;
    if !panels.limiter.allow(sender, panels.now_ms()) {
        return Err(PanelError::new(
            PanelErrorCode::RateLimited,
            "this window shares its UI state too often; the update was dropped",
        ));
    }
    let windows = app.webview_windows();
    let to = recipients(sender, publish.scope, windows.keys().map(String::as_str));
    let mut delivered = 0;
    for label in &to {
        if app
            .emit_to(label.as_str(), UI_SYNC_EVENT, &publish.message)
            .is_ok()
        {
            delivered += 1;
        }
    }
    Ok(delivered)
}
