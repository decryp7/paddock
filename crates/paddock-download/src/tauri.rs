//! The Tauri adapter of `paddock-download` (feature `tauri`): one shared [`Downloader`] in the app state, so the per-host caps
//! hold across every download the app's commands start.
//!
//! ```ignore
//! .setup(|app| {
//!     paddock_download::tauri::manage(app.handle(), Downloader::http(HttpConfig::default())?);
//!     Ok(())
//! })
//! // in a command: let d = paddock_download::tauri::downloader(&app).ok_or(...)?; register a job, submit it, return its id
//! ```

use crate::Downloader;
use ::tauri::{AppHandle, Manager, Runtime};
use std::sync::Arc;

/// The managed state.
pub struct Downloads(pub Arc<Downloader>);

/// Puts `downloader` into the app state (once; a second call keeps the first) and returns the shared one.
pub fn manage<R: Runtime>(app: &AppHandle<R>, downloader: Downloader) -> Arc<Downloader> {
    let d = Arc::new(downloader);
    if app.manage(Downloads(d.clone())) {
        d
    } else {
        app.state::<Downloads>().0.clone()
    }
}

/// The shared downloader, or `None` when the app did not call [`manage`].
pub fn downloader<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<Downloader>> {
    app.try_state::<Downloads>().map(|s| s.0.clone())
}
