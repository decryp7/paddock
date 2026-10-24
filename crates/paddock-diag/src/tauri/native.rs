//! The native save dialog through `tauri-plugin-dialog` (feature `dialog`), owned by the dialog window and centred on it.

use super::SavePicker;
use ::tauri::{AppHandle, Manager, Runtime};
use std::path::PathBuf;
use tauri_plugin_dialog::DialogExt;

/// Asks where to save with the platform's save dialog. The app registers `tauri_plugin_dialog::init()`; without it nothing
/// can be chosen and the save ends as dismissed (and says so in the log).
pub struct NativePicker<R: Runtime>(AppHandle<R>);

impl<R: Runtime> NativePicker<R> {
    /// A picker over `app`.
    pub fn new(app: AppHandle<R>) -> Self {
        NativePicker(app)
    }
}

impl<R: Runtime> SavePicker for NativePicker<R> {
    fn pick(&self, window: &str, suggested_name: &str) -> Option<PathBuf> {
        if self
            .0
            .try_state::<tauri_plugin_dialog::Dialog<R>>()
            .is_none()
        {
            log::warn!(Window = window; "tauri-plugin-dialog is not registered; the report cannot be saved");
            return None;
        }
        let mut dialog = self
            .0
            .dialog()
            .file()
            .set_title("Save the problem report")
            .set_file_name(suggested_name)
            .add_filter("Zip archive", &["zip"]);
        if let Some(w) = self.0.get_webview_window(window) {
            dialog = dialog.set_parent(&w);
        }
        // Blocking is right here: this runs on the save thread, never on the IPC or main thread.
        dialog.blocking_save_file().and_then(|p| p.into_path().ok())
    }
}
