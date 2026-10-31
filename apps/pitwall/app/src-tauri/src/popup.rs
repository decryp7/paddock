//! The tray popup's widgets on the app's side (family ADR 0035): the person's layout, kept in `popup-widgets.json` in the
//! data folder, and the facts the live feed writes. What the widgets show, and every decision behind it, is
//! `pitwall_core::widgets`; this module stores, edits and publishes. Pure of Tauri: [`PopupState`] is tested without a window.
//!
//! The layout lives here, not in the server's settings, because it is a preference of this app (the server may run without
//! it) and the popup must draw its widgets while the server is down.

use crate::commands::{self, SharedClient};
use paddock_widgets::{Edit, Stored, WidgetError};
use pitwall_core::mgmt::{CommandError, Download, ErrorCode, LiveUpdate, Overview, PitwallEvent, UpdateCheckStatus};
use pitwall_core::updates::UpdateNotice;
use pitwall_core::widgets::{
    loading_shape, popover_header as header_of, popover_values as values_of, Facts, LoadingRows, PopoverCatalogue, PopoverHeader, PopoverValues,
    PopoverView, PopoverWidget, UpdatesFact,
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The layout's file in the data folder.
pub const FILE_NAME: &str = "popup-widgets.json";
/// The Tauri event both windows listen to (`tauri-backend.ts`): the layout, the values and the header, after any change.
pub const EVENT: &str = "pitwall://popover";
/// Largest layout file read.
const MAX_BYTES: u64 = 64 * 1024;
/// How many recent events are read after a connect (Recent errors keeps the errors among them).
const EVENTS_READ: u32 = 100;

/// Everything the popup and the Settings page draw, in one piece.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PopoverPush {
    /// The layout and its catalogue.
    pub view: PopoverView,
    /// What the widgets show.
    pub values: PopoverValues,
    /// The status pill and the footer menu.
    pub header: PopoverHeader,
}

/// Why an edit did not apply.
#[derive(Debug, PartialEq, Eq)]
pub enum EditError {
    /// The shared crate refused the edit (a limit, an unknown widget, a newer layout).
    Refused(String),
    /// The layout could not be saved; nothing changed.
    NotSaved(String),
}

impl From<EditError> for CommandError {
    fn from(e: EditError) -> CommandError {
        match e {
            EditError::Refused(m) => CommandError::new(ErrorCode::BadRequest, m),
            EditError::NotSaved(m) => CommandError::new(ErrorCode::Internal, m),
        }
    }
}

struct Inner {
    facts: Facts,
    layout: Stored<PopoverCatalogue>,
    last: Option<PopoverPush>,
}

/// The popup's layout and facts.
pub struct PopupState {
    /// The data folder; none: the layout is kept in memory only.
    dir: Option<PathBuf>,
    inner: Mutex<Inner>,
}

/// Reads a layout file; anything unusable gives the defaults (the shared crate reads a damaged layout leniently).
fn load_layout(dir: &std::path::Path) -> Stored<PopoverCatalogue> {
    let path = dir.join(FILE_NAME);
    std::fs::metadata(&path)
        .ok()
        .filter(|m| m.len() <= MAX_BYTES)
        .and_then(|_| std::fs::read_to_string(&path).ok())
        .and_then(|t| serde_json::from_str::<Stored<PopoverCatalogue>>(&t).ok())
        .unwrap_or_default()
}

/// Writes the layout atomically (a temporary file, then a rename).
fn save_layout(dir: &std::path::Path, layout: &Stored<PopoverCatalogue>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(FILE_NAME);
    let tmp = dir.join(format!("{FILE_NAME}.new"));
    let text = serde_json::to_string_pretty(layout).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Now, in milliseconds since the epoch (the edge: everything else takes the time as an argument).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl PopupState {
    /// The state over the layout saved in `dir` (the defaults when there is none or it is unusable).
    pub fn load(dir: Option<PathBuf>) -> PopupState {
        let layout = dir.as_deref().map(load_layout).unwrap_or_default();
        PopupState {
            dir,
            inner: Mutex::new(Inner {
                facts: Facts::new(),
                layout,
                last: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The layout as the chooser draws it.
    pub fn view(&self) -> PopoverView {
        paddock_widgets::view(&self.lock().layout)
    }

    /// What the widgets show at `now_ms`.
    pub fn values(&self, now_ms: i64) -> PopoverValues {
        let g = self.lock();
        values_of(g.layout.layout(), &g.facts, now_ms)
    }

    /// The status pill and the footer menu.
    pub fn header(&self) -> PopoverHeader {
        header_of(&self.lock().facts)
    }

    /// How many rows each kind draws.
    pub fn shape(&self) -> Vec<LoadingRows> {
        loading_shape()
    }

    /// Changes the facts.
    pub fn with_facts<R>(&self, f: impl FnOnce(&mut Facts) -> R) -> R {
        f(&mut self.lock().facts)
    }

    /// Applies one edit: validated by the shared crate, saved, and only then kept; a refused or unsaved edit changes nothing.
    pub fn edit(&self, edit: Edit<PopoverWidget>) -> Result<PopoverView, EditError> {
        let mut g = self.lock();
        let mut next = g.layout.clone();
        next.edit(edit).map_err(|e: WidgetError| EditError::Refused(e.to_string()))?;
        if let Some(dir) = &self.dir {
            save_layout(dir, &next).map_err(EditError::NotSaved)?;
        }
        g.layout = next;
        Ok(paddock_widgets::view(&g.layout))
    }

    /// Everything the windows draw, now.
    pub fn snapshot(&self, now_ms: i64) -> PopoverPush {
        let g = self.lock();
        PopoverPush {
            view: paddock_widgets::view(&g.layout),
            values: values_of(g.layout.layout(), &g.facts, now_ms),
            header: header_of(&g.facts),
        }
    }

    /// The snapshot when it differs from the last one published (and remembers it), else nothing: the live feed ticks
    /// every second, a window needs to hear only what changed.
    pub fn changed(&self, now_ms: i64) -> Option<PopoverPush> {
        let push = self.snapshot(now_ms);
        let mut g = self.lock();
        if g.last.as_ref() == Some(&push) {
            return None;
        }
        g.last = Some(push.clone());
        Some(push)
    }
}

/// The shared state the Tauri app manages.
pub type SharedPopup = Arc<PopupState>;

/// Tells every window what changed since the last time, if anything.
pub fn publish<R: Runtime>(app: &AppHandle<R>) {
    let Some(popup) = app.try_state::<SharedPopup>() else { return };
    if let Some(push) = popup.changed(now_ms()) {
        let _ = app.emit(EVENT, push);
    }
}

/// Reads what the live feed does not carry (downloads, recent events, model update notices) and publishes. Failures leave
/// the facts as they were: the widgets say "not checked yet" rather than guess.
pub async fn refresh_extras<R: Runtime>(app: AppHandle<R>, client: SharedClient) {
    let Some(popup) = app.try_state::<SharedPopup>().map(|p| p.inner().clone()) else { return };
    if let Ok(v) = commands::forward(client.as_ref(), "downloads", serde_json::Value::Null).await {
        if let Ok(list) = serde_json::from_value::<Vec<Download>>(v) {
            popup.with_facts(|f| f.set_downloads(&list, now_ms()));
        }
    }
    if let Ok(v) = commands::forward(client.as_ref(), "events", serde_json::json!({ "limit": EVENTS_READ })).await {
        if let Ok(list) = serde_json::from_value::<Vec<PitwallEvent>>(v) {
            popup.with_facts(|f| f.seed_events(&list, now_ms()));
        }
    }
    refresh_updates(&popup, client.as_ref()).await;
    publish(&app);
}

/// Reads the model update notices and the check's state.
pub async fn refresh_updates(popup: &PopupState, client: &dyn crate::server_client::ServerClient) {
    let notices = commands::forward(client, "update_notices", serde_json::Value::Null).await;
    let status = commands::forward(client, "update_check_status", serde_json::Value::Null).await;
    let (Ok(notices), Ok(status)) = (notices, status) else { return };
    let (Ok(notices), Ok(status)) = (
        serde_json::from_value::<Vec<UpdateNotice>>(notices),
        serde_json::from_value::<UpdateCheckStatus>(status),
    ) else {
        return;
    };
    let fact = UpdatesFact {
        count: u32::try_from(notices.len()).unwrap_or(u32::MAX),
        summary: notices.first().map(|n| n.summary.clone()),
        last_checked_ms: status.last_checked_at.map(|t| i64::try_from(t).unwrap_or(i64::MAX)),
    };
    popup.with_facts(|f| f.set_updates(fact, now_ms()));
}

/// Whether a pushed update means the model update notices may have changed.
pub fn touches_updates(u: &LiveUpdate) -> bool {
    matches!(u, LiveUpdate::Event { event } if event.kind == pitwall_core::events::EventKind::ModelUpdate)
}

/// Feeds a snapshot to the facts.
pub fn seed(popup: &PopupState, overview: &Overview) {
    popup.with_facts(|f| f.seed(overview, now_ms()));
}

#[cfg(test)]
#[path = "popup_tests.rs"]
mod tests;

// ---- commands -------------------------------------------------------------------------------------------------------

type Out<T> = Result<T, CommandError>;

/// `popover_view`: the tray popup's layout as the chooser draws it.
#[tauri::command]
pub async fn popover_view(state: tauri::State<'_, SharedPopup>) -> Out<PopoverView> {
    Ok(state.view())
}

/// `popover_values`: what the popup's widgets show now.
#[tauri::command]
pub async fn popover_values(state: tauri::State<'_, SharedPopup>) -> Out<PopoverValues> {
    Ok(state.values(now_ms()))
}

/// `popover_header`: the status pill and which footer items work now.
#[tauri::command]
pub async fn popover_header(state: tauri::State<'_, SharedPopup>) -> Out<PopoverHeader> {
    Ok(state.header())
}

/// `popover_shape`: the rows each widget draws, so a loading twin has the height of a ready widget.
#[tauri::command]
pub async fn popover_shape(state: tauri::State<'_, SharedPopup>) -> Out<Vec<LoadingRows>> {
    Ok(state.shape())
}

/// `edit_popover`: one edit of the popup's layout (the shared crate validates it and keeps one level of undo). Every window
/// hears the result.
#[tauri::command]
pub async fn edit_popover<R: Runtime>(app: AppHandle<R>, state: tauri::State<'_, SharedPopup>, edit: Edit<PopoverWidget>) -> Out<PopoverView> {
    let view = state.edit(edit)?;
    if let Some(push) = state.changed(now_ms()) {
        let _ = app.emit(EVENT, push);
    }
    Ok(view)
}
