//! The Tauri adapter of `paddock-files` (built with the `tauri` feature; plan X31).
//!
//! - [`install`] manages the [`Files`] service. `paddock-shell` calls it from `Builder::files`.
//! - [`WindowGrants`]: a command argument resolved from the **calling window** (a dialog or pop-out resolves to its host),
//!   never from the webview, for the app's own commands: `grants.read_file(&raw, &["log"])?`.
//! - [`COMMANDS`](crate::COMMANDS): the pickers, the recent lists, reveal, open, the app folders and web addresses.
//! - [`on_window_event`]: files dropped on a window are granted to it and announced ([`DROPPED_EVENT`]); [`grant_command_line`]
//!   does the same for the command line and a second launch.
//! - Dialogs run on the blocking pool, never on the IPC or main thread (rule 10).

use crate::{
    Access, AppFolder, DROPPED_EVENT, Files, GrantSource, GrantedPath, PickRequest, RECENT_EVENT,
    RecentLists,
};
use ::tauri::ipc::{CommandArg, CommandItem, Invoke, InvokeError};
use ::tauri::{AppHandle, Emitter, Manager, Runtime, State, Window, WindowEvent};
use paddock_commands::CommandError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "dialog")]
mod native;
#[cfg(feature = "dialog")]
pub use native::NativePicker;

/// What the drop event carries: the granted paths.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedFiles {
    /// The paths that were granted to the window (files and folders).
    pub paths: Vec<String>,
}

/// Manages the service (once; a second call returns the first).
pub fn install<R: Runtime>(app: &AppHandle<R>, files: Files) -> Arc<Files> {
    if let Some(existing) = app.try_state::<Arc<Files>>() {
        return existing.inner().clone();
    }
    let files = Arc::new(files);
    app.manage(files.clone());
    files
}

/// The managed service, if the app set it up.
pub fn files<R: Runtime, M: Manager<R>>(manager: &M) -> Option<Arc<Files>> {
    manager.try_state::<Arc<Files>>().map(|f| f.inner().clone())
}

/// The grant key of a window: a dialog or pop-out uses its host's.
pub fn key(label: &str) -> &str {
    paddock_commands::labels::resolve(label)
}

/// The calling window's grants, as a command argument.
pub struct WindowGrants {
    files: Arc<Files>,
    window: String,
}

impl WindowGrants {
    /// The grants of the window labelled `label`.
    pub fn of(files: Arc<Files>, label: &str) -> WindowGrants {
        WindowGrants {
            files,
            window: key(label).to_string(),
        }
    }

    /// An existing file or folder the user chose, to read.
    pub fn read(&self, raw: &str, extensions: &[&str]) -> Result<GrantedPath, CommandError> {
        self.files
            .grants()
            .check(&self.window, raw, Access::Read, extensions)
    }

    /// An existing file the user chose, to read.
    pub fn read_file(&self, raw: &str, extensions: &[&str]) -> Result<GrantedPath, CommandError> {
        let g = self.read(raw, extensions)?;
        if !g.path().is_file() {
            return Err(CommandError::invalid(format!(
                "{} is not a file",
                g.path().display()
            )));
        }
        Ok(g)
    }

    /// An existing folder the user chose, to read.
    pub fn read_dir(&self, raw: &str) -> Result<GrantedPath, CommandError> {
        let g = self.read(raw, &[])?;
        if !g.path().is_dir() {
            return Err(CommandError::invalid(format!(
                "{} is not a folder",
                g.path().display()
            )));
        }
        Ok(g)
    }

    /// A file the user chose to write (it may not exist yet).
    pub fn write(&self, raw: &str, extensions: &[&str]) -> Result<GrantedPath, CommandError> {
        self.files
            .grants()
            .check(&self.window, raw, Access::Write, extensions)
    }

    /// A file the user chose to read, or to write.
    pub fn read_or_write(
        &self,
        raw: &str,
        extensions: &[&str],
    ) -> Result<GrantedPath, CommandError> {
        self.read(raw, extensions)
            .or_else(|e| self.write(raw, extensions).map_err(|_| e))
    }

    /// Grants a path to this window (the project file it opened or saved, a file the app produced).
    pub fn grant(
        &self,
        path: &std::path::Path,
        access: Access,
        source: GrantSource,
    ) -> Result<GrantedPath, CommandError> {
        self.files
            .grants()
            .grant(&self.window, path, access, source)
    }
}

impl<'de, R: Runtime> CommandArg<'de, R> for WindowGrants {
    fn from_command(command: CommandItem<'de, R>) -> Result<Self, InvokeError> {
        let webview = command.message.webview();
        let label = webview.window().label().to_string();
        match files(&webview) {
            Some(files) => Ok(WindowGrants::of(files, &label)),
            None => Err(InvokeError::from(CommandError::unavailable(
                "This app does not set up user files",
            ))),
        }
    }
}

/// Records an opened or saved path in a recent list (the granted path is the proof) and tells every window.
pub fn opened<R: Runtime>(
    app: &AppHandle<R>,
    list: &str,
    granted: &GrantedPath,
    name: Option<&str>,
) {
    if let Some(files) = files(app)
        && files.opened(list, granted, name)
    {
        announce(app, &files);
    }
}

fn announce<R: Runtime>(app: &AppHandle<R>, files: &Files) {
    // A failed emit means the app is closing; nothing is left to tell.
    let _ = app.emit(RECENT_EVENT, files.recents().list());
}

/// Grants command-line arguments (paths that resolve) to the window `label` for reading and announces them like a drop.
pub fn grant_command_line<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    args: &[String],
) -> Vec<GrantedPath> {
    let Some(files) = files(app) else {
        return Vec::new();
    };
    let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    files.grant_read(key(label), &paths, GrantSource::CommandLine)
}

/// The window event hook: files dropped on a window are granted to it and announced as [`DROPPED_EVENT`]; a closed window
/// forgets its grants.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let Some(files) = files(window) else { return };
    match event {
        WindowEvent::DragDrop(::tauri::DragDropEvent::Drop { paths, .. }) => {
            let granted = files.grant_read(key(window.label()), paths, GrantSource::Drop);
            if !granted.is_empty() {
                let payload = DroppedFiles {
                    paths: granted.iter().map(GrantedPath::text).collect(),
                };
                let _ = window.emit_to(window.label(), DROPPED_EVENT, payload);
            }
        }
        // A dialog or pop-out shares its host's grants: only a top-level window takes them with it.
        WindowEvent::Destroyed if key(window.label()) == window.label() => {
            files.grants().forget(window.label());
        }
        _ => {}
    }
}

type Managed<'a> = State<'a, Arc<Files>>;

fn joined<T>(r: Result<T, ::tauri::Error>) -> Result<T, CommandError> {
    r.map_err(|e| CommandError::internal(format!("the file operation did not finish: {e}")))
}

/// Shows a native dialog for the calling window and grants what the user chose to it; returns the chosen paths.
#[::tauri::command]
pub async fn files_pick<R: Runtime>(
    window: Window<R>,
    state: Managed<'_>,
    request: PickRequest,
) -> Result<Vec<String>, CommandError> {
    let files = state.inner().clone();
    let key = key(window.label()).to_string();
    let granted =
        joined(::tauri::async_runtime::spawn_blocking(move || files.pick(&key, request)).await)??;
    Ok(granted.iter().map(GrantedPath::text).collect())
}

/// The recent lists.
#[::tauri::command]
pub async fn files_recent_list(state: Managed<'_>) -> Result<RecentLists, CommandError> {
    Ok(state.recents().list())
}

/// Takes one path out of a recent list.
#[::tauri::command]
pub async fn files_recent_remove<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
    list: String,
    path: String,
) -> Result<(), CommandError> {
    if state.recents().remove(&list, &path) {
        announce(&app, &state);
    }
    Ok(())
}

/// Empties a recent list.
#[::tauri::command]
pub async fn files_recent_clear<R: Runtime>(
    app: AppHandle<R>,
    state: Managed<'_>,
    list: String,
) -> Result<(), CommandError> {
    if state.recents().clear(&list) {
        announce(&app, &state);
    }
    Ok(())
}

/// Grants a path of a recent list to the calling window again and returns its canonical form.
#[::tauri::command]
pub async fn files_recent_grant<R: Runtime>(
    window: Window<R>,
    state: Managed<'_>,
    list: String,
    path: String,
) -> Result<String, CommandError> {
    let files = state.inner().clone();
    let key = key(window.label()).to_string();
    let granted = joined(
        ::tauri::async_runtime::spawn_blocking(move || files.regrant(&key, &list, &path)).await,
    )??;
    Ok(granted.text())
}

/// Shows a granted path in the file manager.
#[::tauri::command]
pub async fn files_reveal<R: Runtime>(
    window: Window<R>,
    state: Managed<'_>,
    path: String,
) -> Result<(), CommandError> {
    let files = state.inner().clone();
    let key = key(window.label()).to_string();
    joined(::tauri::async_runtime::spawn_blocking(move || files.reveal(&key, &path)).await)?
}

/// Opens a granted path with the OS's program for it.
#[::tauri::command]
pub async fn files_open<R: Runtime>(
    window: Window<R>,
    state: Managed<'_>,
    path: String,
) -> Result<(), CommandError> {
    let files = state.inner().clone();
    let key = key(window.label()).to_string();
    joined(::tauri::async_runtime::spawn_blocking(move || files.open(&key, &path)).await)?
}

/// Shows one of the app's own folders in the file manager.
#[::tauri::command]
pub async fn files_open_app_folder(
    state: Managed<'_>,
    folder: AppFolder,
) -> Result<(), CommandError> {
    let files = state.inner().clone();
    joined(::tauri::async_runtime::spawn_blocking(move || files.open_app_folder(folder)).await)?
}

/// Opens a web address in the browser.
#[::tauri::command]
pub async fn files_open_url(state: Managed<'_>, url: String) -> Result<(), CommandError> {
    let files = state.inner().clone();
    joined(::tauri::async_runtime::spawn_blocking(move || files.open_url(&url)).await)?
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (every command); wrap it in the shell's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        files_pick,
        files_recent_list,
        files_recent_remove,
        files_recent_clear,
        files_recent_grant,
        files_reveal,
        files_open,
        files_open_app_folder,
        files_open_url
    ]
}

#[cfg(test)]
mod tests;
