//! The native open and save dialogs through `tauri-plugin-dialog` (feature `dialog`), owned by the calling window.

use crate::{PickKind, PickRequest, Picker};
use ::tauri::{AppHandle, Manager, Runtime};
use std::path::PathBuf;
use tauri_plugin_dialog::{DialogExt, FilePath};

/// Shows the platform's dialogs. The app registers `tauri_plugin_dialog::init()`; without it nothing can be chosen (and the log
/// says so).
pub struct NativePicker<R: Runtime>(AppHandle<R>);

impl<R: Runtime> NativePicker<R> {
    /// A picker over `app`.
    pub fn new(app: AppHandle<R>) -> Self {
        NativePicker(app)
    }
}

fn paths(chosen: impl IntoIterator<Item = FilePath>) -> Vec<PathBuf> {
    chosen
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect()
}

impl<R: Runtime> Picker for NativePicker<R> {
    fn pick(&self, window: &str, request: &PickRequest) -> Vec<PathBuf> {
        if self
            .0
            .try_state::<tauri_plugin_dialog::Dialog<R>>()
            .is_none()
        {
            log::warn!(Window = window; "tauri-plugin-dialog is not registered; nothing can be chosen");
            return Vec::new();
        }
        let mut dialog = self.0.dialog().file();
        if let Some(w) = self.0.get_webview_window(window) {
            dialog = dialog.set_parent(&w);
        }
        if !request.title.is_empty() {
            dialog = dialog.set_title(&request.title);
        }
        for f in &request.filters {
            let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&f.name, &exts);
        }
        if let Some(n) = &request.default_name {
            dialog = dialog.set_file_name(n);
        }
        // Blocking is right here: this runs on the blocking pool, never on the IPC or main thread.
        match request.kind {
            PickKind::OpenFile => paths(dialog.blocking_pick_file()),
            PickKind::OpenFolder => paths(dialog.blocking_pick_folder()),
            PickKind::Save => paths(dialog.blocking_save_file()),
            PickKind::OpenFiles => paths(dialog.blocking_pick_files().unwrap_or_default()),
            PickKind::OpenFolders => paths(dialog.blocking_pick_folders().unwrap_or_default()),
        }
    }
}
