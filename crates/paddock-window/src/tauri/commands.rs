//! The commands of [`COMMANDS`](crate::COMMANDS). All are async (a window built from a synchronous command never
//! finishes loading on Windows) and return at once.

use super::instance::resolve_open;
use super::manager::{self, list, open_new, quit, resolve_close, reveal, state, top_level};
use super::popover::{close_popover, fit_popover};
use crate::close::clean_title;
use crate::launch::OpenRequest;
use crate::wire::{WindowError, WindowErrorCode, WindowInfo};
use crate::{CloseChoice, OpenChoice};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Runtime, WebviewWindow};

/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (put it behind `paddock_commands::tauri::Guard`).
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        window_new,
        window_close,
        windows_list,
        window_focus,
        window_set_title,
        window_resolve_close,
        window_take_pending,
        window_resolve_open,
        app_quit,
        tray_fit_height,
        tray_close
    ]
}

/// Opens a new, empty top-level window and focuses it.
#[::tauri::command]
pub async fn window_new<R: Runtime>(app: AppHandle<R>) -> Result<WindowInfo, WindowError> {
    let label = open_new(&app, OpenRequest::default())?;
    list(&app)
        .into_iter()
        .find(|w| w.label == label)
        .ok_or_else(|| WindowError::new(WindowErrorCode::Closed, "the window closed"))
}

/// Closes the calling window as its close button does (the close setting applies).
#[::tauri::command]
pub async fn window_close<R: Runtime>(window: WebviewWindow<R>) -> Result<(), WindowError> {
    Ok(window.close()?)
}

/// The top-level windows, `main` first.
#[::tauri::command]
pub async fn windows_list<R: Runtime>(app: AppHandle<R>) -> Vec<WindowInfo> {
    list(&app)
}

/// Shows, restores and focuses the top-level window `label`.
#[::tauri::command]
pub async fn window_focus<R: Runtime>(app: AppHandle<R>, label: String) -> Result<(), WindowError> {
    reveal(&app, &label)
}

/// Sets the calling window's title (one line, at most 200 characters; the app's name when empty).
#[::tauri::command]
pub async fn window_set_title<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    title: String,
) -> Result<(), WindowError> {
    let windows = state(&app)?;
    let title = clean_title(&title, &windows.config.title);
    if !windows.set_title(window.label(), &title) {
        return Ok(());
    }
    window.set_title(&title)?;
    manager::changed(&app);
    Ok(())
}

/// The answer to `paddock://window-close-requested`; with `remember` it becomes the setting.
#[::tauri::command]
pub async fn window_resolve_close<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    choice: CloseChoice,
    remember: bool,
) -> Result<(), WindowError> {
    resolve_close(&app, &window, choice, remember)
}

/// The files and links waiting for the calling window; a second call gets nothing.
#[::tauri::command]
pub async fn window_take_pending<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
) -> Result<OpenRequest, WindowError> {
    Ok(state(&app)?.take_pending(window.label()))
}

/// The answer to `paddock://window-open-ask`.
#[::tauri::command]
pub async fn window_resolve_open<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    choice: OpenChoice,
) -> Result<(), WindowError> {
    if !top_level(window.label()) {
        return Err(WindowError::new(
            WindowErrorCode::NotAllowed,
            "not a top-level window",
        ));
    }
    resolve_open(&app, window.label(), choice)
}

/// Quits the app: every window closes at once.
#[::tauri::command]
pub async fn app_quit<R: Runtime>(app: AppHandle<R>) -> Result<(), WindowError> {
    state(&app)?;
    quit(&app);
    Ok(())
}

/// The calling tray popover's measured content height (logical px); returns the height applied.
#[::tauri::command]
pub async fn tray_fit_height<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
    height: f64,
) -> Result<f64, WindowError> {
    fit_popover(&app, window.label(), height)
}

/// Closes the calling tray popover.
#[::tauri::command]
pub async fn tray_close<R: Runtime>(
    app: AppHandle<R>,
    window: WebviewWindow<R>,
) -> Result<(), WindowError> {
    if !crate::popover::is_popover(window.label()) {
        return Err(WindowError::new(
            WindowErrorCode::NotAllowed,
            "this is not a tray popover",
        ));
    }
    close_popover(&app, window.label());
    Ok(())
}
