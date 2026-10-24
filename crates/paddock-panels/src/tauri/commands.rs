//! The commands of [`COMMANDS`](crate::COMMANDS). All are async (a window built from a synchronous command never
//! finishes loading on Windows) and return at once.

use super::{dock_back, focus, list, open, publish, set_keep_on_top};
use crate::wire::{GhostRequest, PanelError, PopOutBounds, PopOutInfo, PopOutRequest, SyncPublish};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Runtime, Window};

/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (put it behind `paddock_commands::tauri::Guard`).
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        panel_pop_out,
        panel_dock_back,
        panels_list,
        panel_focus,
        panel_keep_on_top,
        ui_sync_publish,
        drag_ghost_start,
        drag_ghost_show,
        drag_ghost_end,
        pointer_button_down
    ]
}

/// Pops a panel of the calling window out into a window of its own.
#[::tauri::command]
pub async fn panel_pop_out<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    request: PopOutRequest,
) -> Result<PopOutInfo, PanelError> {
    open(&app, window.label(), request)
}

/// Docks a pop-out back (the calling pop-out, or one of the caller's group by panel id).
#[::tauri::command]
pub async fn panel_dock_back<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    panel: Option<String>,
) -> Result<(), PanelError> {
    dock_back(&app, window.label(), panel.as_deref())
}

/// The pop-outs of the calling window's group.
#[::tauri::command]
pub async fn panels_list<R: Runtime>(app: AppHandle<R>, window: Window<R>) -> Vec<PopOutInfo> {
    list(&app, window.label())
}

/// Brings a popped-out panel's window to the front; false when it is not popped out.
#[::tauri::command]
pub async fn panel_focus<R: Runtime>(app: AppHandle<R>, window: Window<R>, panel: String) -> bool {
    focus(&app, window.label(), &panel)
}

/// Keeps the calling pop-out above its host window (or not).
#[::tauri::command]
pub async fn panel_keep_on_top<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    on: bool,
) -> Result<(), PanelError> {
    set_keep_on_top(&app, window.label(), on)
}

/// Forwards UI state to the other windows of the caller's host; answers how many got it.
#[::tauri::command]
pub async fn ui_sync_publish<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    sync: SyncPublish,
) -> Result<usize, PanelError> {
    publish(&app, window.label(), sync)
}

/// Starts a drag ghost for the calling window: `false` (and nothing happens) where it cannot follow the pointer.
#[::tauri::command]
pub async fn drag_ghost_start<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    request: GhostRequest,
) -> Result<bool, PanelError> {
    super::ghost::begin(&app, window.label(), request)
}

/// Shows the calling window's ghost under the pointer, or hides it.
#[::tauri::command]
pub async fn drag_ghost_show<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    visible: bool,
) -> Result<(), PanelError> {
    super::ghost::show_for(&app, window.label(), visible);
    Ok(())
}

/// Ends the calling window's drag: where the ghost was (logical px of its display), or `null`.
#[::tauri::command]
pub async fn drag_ghost_end<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
) -> Option<PopOutBounds> {
    super::end_for(&app, window.label())
}

/// Whether the primary mouse button is held anywhere on screen (`null` when the platform cannot tell).
#[::tauri::command]
pub async fn pointer_button_down() -> Option<bool> {
    super::native::primary_button_down()
}
