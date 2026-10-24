//! The Tauri adapter of `paddock-dialogs` (built with the `tauri` feature): dialogs as real OS windows (rule 17),
//! created by kind, placed on the parent's monitor and owned by the window that opened them.
//!
//! Set up: manage a [`Dialogs`] (from the app's contract file), list the four commands in the command table, hand
//! [`grant_of`] to the guard of `paddock-commands` and forward window events to [`on_window_event`]:
//!
//! ```ignore
//! paddock_commands::command_table! {
//!     static COMMANDS, macro handler;
//!     Mutation [MAIN | DIALOG] limit 9 * MIB => paddock_dialogs::tauri::dialog_open,
//!     Query [DIALOG] => paddock_dialogs::tauri::dialog_take_input,
//!     Mutation [DIALOG] limit 2 * MIB => paddock_dialogs::tauri::dialog_close,
//!     Quiet [DIALOG] => paddock_dialogs::tauri::dialog_fit_height,
//! }
//! tauri::Builder::default()
//!     .manage(paddock_dialogs::Dialogs::from_json(include_str!("../dialog-contracts.json"))?)
//!     .invoke_handler(Guard::new(&COMMANDS).dialog_grants(paddock_dialogs::tauri::grant_of).wrap(handler!()))
//!     .on_window_event(|w, e| paddock_dialogs::tauri::on_window_event(w, e))
//! ```
//!
//! * **Modal** dialogs disable their parent natively while they are open (owner window on Windows, transient window on
//!   Linux) and stay above it. On macOS the parent is not disabled natively: that begins a sheet on the parent, and a
//!   child window of a parent that carries a sheet is never key (the dialog showed greyed out and the app froze). There
//!   the UI's scrim blocks the parent.
//! * **Modeless** dialogs are owned too (above the parent, minimised with it) but the parent stays usable; on macOS they
//!   are independent windows ([`Parenting`](crate::Parenting)), since a child window there is confined to its parent's
//!   display.
//! * No dialog window has title-bar buttons; a close request from the OS reaches the dialog as its own Close/Cancel
//!   ([`CLOSE_REQUESTED_EVENT`]), and a required choice ignores it unless its page does not answer
//!   ([`page_unresponsive`]).
//!
//! Data never travels through window globals: the input is validated and parked at open (`dialog_take_input` hands it to
//! the dialog's own webview) and the result goes back through `dialog_close` as [`RESULT_EVENT`] to the opener.

use crate::contract::{DIALOG_MIN, Grant, Modality};
use crate::placement::{self, Bounds, Monitor, Placement, Rect, ScreenPosition};
use crate::registry::{CloseDecision, Dialogs, OpenDialog};
use crate::window::{
    CLOSE_REQUESTED_EVENT, CLOSED_EVENT, DialogClosed, DialogError, DialogErrorCode, DialogInfo,
    DialogRequest, DialogResult, DialogStart, MAX_RESULT_BYTES, RESULT_EVENT, WindowSpec,
    clean_title, disables_parent_natively, geometry, is_fitted, json_len, plan_fit, validate,
    window_config,
};
use ::tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
};
use paddock_commands::labels::{Label, resolve};
use serde_json::Value;
use std::time::Instant;

/// The log target of this crate.
const TARGET: &str = "paddock_dialogs";

/// The builder of a dialog window.
pub type Builder<'a, R> = WebviewWindowBuilder<'a, R, AppHandle<R>>;
type BuilderHook<R> = dyn for<'a> Fn(Builder<'a, R>) -> Builder<'a, R> + Send + Sync;
type BuiltHook<R> = dyn Fn(&WebviewWindow<R>) + Send + Sync;

/// The app's own steps in building a dialog window (its webview settings: zoom keys, browser extensions, dev
/// overrides). Optional managed state: without it dialog windows are built as they are.
pub struct WindowHooks<R: Runtime> {
    builder: Option<Box<BuilderHook<R>>>,
    built: Option<Box<BuiltHook<R>>>,
}

impl<R: Runtime> Default for WindowHooks<R> {
    fn default() -> Self {
        WindowHooks {
            builder: None,
            built: None,
        }
    }
}

impl<R: Runtime> WindowHooks<R> {
    /// No hooks.
    pub fn new() -> Self {
        Self::default()
    }

    /// `f` configures every dialog window's builder before it is built.
    pub fn builder(
        mut self,
        f: impl for<'a> Fn(Builder<'a, R>) -> Builder<'a, R> + Send + Sync + 'static,
    ) -> Self {
        self.builder = Some(Box::new(f));
        self
    }

    /// `f` runs on every dialog window right after it is built (what the builder cannot set).
    pub fn built(mut self, f: impl Fn(&WebviewWindow<R>) + Send + Sync + 'static) -> Self {
        self.built = Some(Box::new(f));
        self
    }
}

fn failed(e: impl std::fmt::Display) -> DialogError {
    DialogError::new(DialogErrorCode::Failed, e.to_string())
}

fn not_a_dialog() -> DialogError {
    DialogError::new(DialogErrorCode::NotADialog, "this is not a dialog window")
}

/// The grant of the dialog window `label` for the command guard:
/// `Guard::dialog_grants(paddock_dialogs::tauri::grant_of)`. `None` for a window that is not an open dialog.
pub fn grant_of<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Grant> {
    app.try_state::<Dialogs>()?.grant_of(label)
}

/// A window's outer rectangle in physical px.
fn outer_rect<R: Runtime>(w: &Window<R>) -> Option<Rect> {
    let pos = w.outer_position().ok()?;
    let size = w.outer_size().ok()?;
    Some(Rect {
        x: f64::from(pos.x),
        y: f64::from(pos.y),
        width: f64::from(size.width),
        height: f64::from(size.height),
    })
}

/// A monitor's work area and scale as the placement policy reads them.
pub fn monitor_rect(m: &::tauri::Monitor) -> Monitor {
    let a = m.work_area();
    Monitor {
        work: Rect {
            x: f64::from(a.position.x),
            y: f64::from(a.position.y),
            width: f64::from(a.size.width),
            height: f64::from(a.size.height),
        },
        scale: m.scale_factor(),
    }
}

/// The monitor of a window from what the platform reports: its current monitor, else the one its outer rectangle
/// overlaps most or is nearest to among `all`. Never the primary display unless the window is there.
fn pick_monitor(
    current: Option<::tauri::Monitor>,
    all: impl FnOnce() -> Vec<::tauri::Monitor>,
    outer: Option<Rect>,
) -> Option<(Monitor, String)> {
    let named = |m: &::tauri::Monitor| (monitor_rect(m), m.name().cloned().unwrap_or_default());
    if let Some(m) = current {
        return Some(named(&m));
    }
    let all = all();
    let rects: Vec<Monitor> = all.iter().map(monitor_rect).collect();
    let i = placement::monitor_of(&outer?, &rects)?;
    all.get(i).map(named)
}

/// The monitor `w` is on, with its name for the log.
pub fn monitor_of_window<R: Runtime>(w: &Window<R>) -> Option<(Monitor, String)> {
    pick_monitor(
        w.current_monitor().ok().flatten(),
        || w.available_monitors().unwrap_or_default(),
        outer_rect(w),
    )
}

/// A window's place in logical px of its monitor (outer position, inner size).
fn bounds_of<R: Runtime>(w: &Window<R>) -> Option<Bounds> {
    let scale = w.scale_factor().ok()?;
    let pos = w.outer_position().ok()?.to_logical::<f64>(scale);
    let size = w.inner_size().ok()?.to_logical::<f64>(scale);
    Some(Bounds {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
    })
}

fn dialogs<R: Runtime>(app: &AppHandle<R>) -> Result<::tauri::State<'_, Dialogs>, DialogError> {
    app.try_state::<Dialogs>()
        .ok_or_else(|| DialogError::new(DialogErrorCode::NotReady, "the dialogs are not ready yet"))
}

/// Opens the dialog `request.dialog_id` for the window `caller` (a top-level window, a pop-out or a dialog of the
/// shell's label scheme), or focuses the one it has open already.
pub fn open<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    request: DialogRequest,
) -> Result<DialogInfo, DialogError> {
    open_on(app, caller, request, std::env::consts::OS)
}

fn open_on<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    request: DialogRequest,
    os: &str,
) -> Result<DialogInfo, DialogError> {
    validate(&request)?;
    if Label::parse(caller).is_none() {
        return Err(DialogError::new(
            DialogErrorCode::NotADialog,
            "only the app's own windows open dialogs",
        ));
    }
    let host = resolve(caller).to_string();
    let dialogs = dialogs(app)?;
    // Only dialogs with a declared contract open; the contract (not the webview) decides kind, modality and floor.
    let contract = dialogs
        .contracts()
        .get(&request.dialog_id)
        .cloned()
        .ok_or_else(|| {
            DialogError::new(
                DialogErrorCode::Unknown,
                format!(
                    "this version of the app has no dialog {:?}",
                    request.dialog_id
                ),
            )
        })?;
    if request.instance.is_some() && !contract.instanced {
        return Err(DialogError::new(
            DialogErrorCode::Unknown,
            format!(
                "the dialog {} does not take an instance key",
                request.dialog_id
            ),
        ));
    }
    if let Some(existing) = dialogs.find(caller, &request.dialog_id, request.instance.as_deref()) {
        if let Some(w) = app.get_webview_window(&existing) {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        return Ok(DialogInfo {
            label: existing,
            dialog_id: request.dialog_id,
            existing: true,
        });
    }
    let parent = app
        .get_webview_window(caller)
        .or_else(|| app.get_webview_window(&host))
        .ok_or_else(|| DialogError::new(DialogErrorCode::Closed, "the window was closed"))?;
    let title = clean_title(&request.title);
    let mut entry = OpenDialog::new(&host, parent.label(), &request.dialog_id, &contract);
    entry.instance = request.instance.clone();
    entry.title = title.clone();
    entry.input = request.input.clone();
    let label = dialogs.reserve(entry)?;
    let (w, h, min_w, min_h) = geometry(&request, &contract);
    // The placement policy: on the parent's monitor, sized by the kind's rule.
    let parent_os = parent.as_ref().window();
    let monitor = monitor_of_window(&parent_os);
    let fitted = is_fitted(&request, contract.kind);
    let remembered = placement::memory_for(contract.kind, fitted, request.remembered, h);
    let place = placement::place(
        outer_rect(&parent_os),
        monitor.as_ref().map(|m| m.0),
        remembered,
        (w, h),
        (min_w, min_h),
    );
    log_placement(&request.dialog_id, monitor.as_ref(), &place);
    let spec = WindowSpec {
        route: dialogs.route(),
        label: &label,
        host: &host,
        dialog_id: &request.dialog_id,
        title: &title,
        modality: contract.modality,
        os,
    };
    if let Err(e) = build_window(app, &spec, &place, (min_w, min_h), &parent) {
        dialogs.remove(&label);
        return Err(DialogError::new(
            DialogErrorCode::Failed,
            format!("could not open the dialog window: {e}"),
        ));
    }
    if contract.modality == Modality::Modal && disables_parent_natively(os) {
        let _ = parent.set_enabled(false);
    }
    log::info!(
        target: TARGET,
        Dialog = request.dialog_id.as_str(), Host = host.as_str(), Window = label.as_str();
        "Dialog window opened"
    );
    Ok(DialogInfo {
        label,
        dialog_id: request.dialog_id,
        existing: false,
    })
}

/// The name and work area of a monitor for the log.
fn describe(monitor: Option<&(Monitor, String)>) -> (String, String) {
    match monitor {
        Some((m, name)) => {
            let w = &m.work;
            let work = format!("{},{} {}x{} @{}", w.x, w.y, w.width, w.height, m.scale);
            (name.clone(), work)
        }
        None => ("unknown".to_string(), "unknown".to_string()),
    }
}

/// Every placement is logged, so a user's log shows where a dialog went and why.
fn log_placement(dialog_id: &str, monitor: Option<&(Monitor, String)>, p: &Placement) {
    let (name, work) = describe(monitor);
    log::info!(
        target: TARGET,
        Dialog = dialog_id,
        Monitor = name.as_str(),
        WorkArea = work.as_str(),
        Placed = format!("{},{} {}x{} @{}", p.x, p.y, p.width, p.height, p.scale).as_str(),
        Remembered = p.remembered;
        "Dialog window placed"
    );
}

fn build_window<R: Runtime>(
    app: &AppHandle<R>,
    spec: &WindowSpec<'_>,
    place: &Placement,
    min: (f64, f64),
    parent: &WebviewWindow<R>,
) -> Result<(), DialogError> {
    let at = placement::screen_position(place, spec.os);
    let c = window_config(spec, &at, place, min);
    // A native title bar with the dialog's title: dialogs are ordinary OS windows.
    let builder = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(c.url.clone().into()))
        .title(c.title.clone())
        .inner_size(c.inner.0, c.inner.1)
        .position(c.position.0, c.position.1)
        .min_inner_size(c.min_inner.0, c.min_inner.1)
        .resizable(c.resizable)
        .closable(c.closable)
        .minimizable(c.minimizable)
        .maximizable(c.maximizable)
        .focused(true);
    let builder = if c.owned {
        owned_by(builder, parent)?
    } else {
        builder
    };
    let hooks = app.try_state::<WindowHooks<R>>();
    let builder = match hooks.as_ref().and_then(|h| h.builder.as_ref()) {
        Some(f) => f(builder),
        None => builder,
    };
    let window = builder.build().map_err(failed)?;
    if let Some(f) = hooks.as_ref().and_then(|h| h.built.as_ref()) {
        f(&window);
    }
    // Where logical coordinates are not one global space (Windows, Linux with mixed DPI), the builder converted them
    // with some monitor's scale: set the exact physical place (on the parent's monitor) and size now. Both values come
    // from a monitor rectangle (i32/u32) plus a clamped offset, so they fit.
    if let Some((x, y)) = at.physical {
        let _ = window.set_position(PhysicalPosition::new(x, y));
        let _ = window.set_size(PhysicalSize::new(
            place.width.round().max(1.0) as u32,
            place.height.round().max(1.0) as u32,
        ));
    }
    Ok(())
}

/// An owned window of `owner`: above it, minimised and hidden with it (Windows: owner; macOS: child; Linux: transient).
#[cfg(not(any(test, feature = "mock-runtime")))]
fn owned_by<'a, R: Runtime>(
    b: Builder<'a, R>,
    owner: &WebviewWindow<R>,
) -> Result<Builder<'a, R>, DialogError> {
    b.parent(owner).map_err(failed)
}

/// The mock runtime has no native windows to own (on Linux its `gtk_window` is unimplemented): tests check the
/// bookkeeping only. Other crates' tests enable it with the `mock-runtime` feature.
#[cfg(any(test, feature = "mock-runtime"))]
fn owned_by<'a, R: Runtime>(
    b: Builder<'a, R>,
    _owner: &WebviewWindow<R>,
) -> Result<Builder<'a, R>, DialogError> {
    Ok(b)
}

/// The user or the OS asked to close a dialog window (Cmd/Ctrl+W, the window manager): the request goes to the
/// dialog as its Close/Cancel (it decides: unsaved edits), and one that does not answer in time is closed anyway. A
/// required-choice modal ignores it. Returns whether the close may go ahead now.
pub fn close_requested<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    let Some(dialogs) = app.try_state::<Dialogs>() else {
        return true;
    };
    match dialogs.close_request(label, Instant::now()) {
        CloseDecision::Close => true,
        CloseDecision::Keep => false,
        CloseDecision::Ask => {
            let _ = app.emit_to(label, CLOSE_REQUESTED_EVENT, ());
            false
        }
    }
}

/// The window watchdog found a dialog's page not answering (`true`), or answering again. A required-choice dialog
/// whose page hangs can then be closed by the OS close (rule 19: the way out never depends on the hung page).
pub fn page_unresponsive<R: Runtime>(app: &AppHandle<R>, label: &str, unresponsive: bool) {
    if let Some(d) = app.try_state::<Dialogs>() {
        d.set_unresponsive(label, unresponsive);
    }
}

/// A dialog moved or was resized: remember where it is.
pub fn moved<R: Runtime>(window: &Window<R>) {
    if let Some(d) = window.app_handle().try_state::<Dialogs>() {
        d.set_bounds(window.label(), bounds_of(window));
    }
}

/// A dialog window is gone: forget it, re-enable and refocus its parent, tell the opener where it was.
pub fn on_destroyed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    on_destroyed_on(app, label, std::env::consts::OS);
}

fn on_destroyed_on<R: Runtime>(app: &AppHandle<R>, label: &str, os: &str) {
    let Some(dialogs) = app.try_state::<Dialogs>() else {
        return;
    };
    let Some(entry) = dialogs.remove(label) else {
        return;
    };
    log::info!(
        target: TARGET,
        Dialog = entry.dialog_id.as_str(), Host = entry.host.as_str(), Window = label;
        "Dialog window closed"
    );
    if dialogs.exiting() {
        return;
    }
    let Some(parent) = app.get_webview_window(&entry.parent) else {
        return;
    };
    if entry.modality == Modality::Modal
        && disables_parent_natively(os)
        && dialogs.modal_children_of(&entry.parent) == 0
    {
        let _ = parent.set_enabled(true);
    }
    let _ = parent.set_focus();
    let parent_os = parent.as_ref().window();
    let parent_at = parent_os
        .scale_factor()
        .ok()
        .and_then(|s| Some(parent_os.outer_position().ok()?.to_logical::<f64>(s)))
        .map(|p| (p.x, p.y));
    let remembered = entry
        .bounds
        .zip(parent_at)
        .and_then(|(b, p)| placement::remembered_of(b, p));
    let _ = app.emit_to(
        entry.parent.as_str(),
        CLOSED_EVENT,
        DialogClosed {
            label: label.to_string(),
            dialog_id: entry.dialog_id,
            remembered,
        },
    );
}

/// The host is gone: close its dialogs. Returns how many there were.
pub fn close_all_of<R: Runtime>(app: &AppHandle<R>, host: &str) -> usize {
    let Some(dialogs) = app.try_state::<Dialogs>() else {
        return 0;
    };
    let labels = dialogs.of_host(host);
    for label in &labels {
        dialogs.remove(label);
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.destroy();
        }
    }
    labels.len()
}

/// Hides or shows the host's dialogs with it (the host went to the tray, or came back).
pub fn set_visible<R: Runtime>(app: &AppHandle<R>, host: &str, visible: bool) {
    let Some(dialogs) = app.try_state::<Dialogs>() else {
        return;
    };
    for label in dialogs.of_host(host) {
        if let Some(w) = app.get_webview_window(&label) {
            let _ = if visible { w.show() } else { w.hide() };
        }
    }
}

/// The dialog layer's part of the app's window events: close requests, moves and resizes of dialogs, and the end of a
/// dialog or of a host (whose dialogs close with it).
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label();
    let parsed = Label::parse(label);
    let is_dialog = matches!(parsed, Some(Label::Dialog { .. }));
    match event {
        WindowEvent::CloseRequested { api, .. } if is_dialog => {
            if !close_requested(app, label) {
                api.prevent_close();
            }
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) if is_dialog => moved(window),
        WindowEvent::Destroyed if is_dialog => on_destroyed(app, label),
        WindowEvent::Destroyed if matches!(parsed, Some(Label::Main | Label::Window(_))) => {
            close_all_of(app, label);
        }
        _ => {}
    }
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (put it behind `paddock_commands::tauri::Guard` with
/// [`grant_of`] as its dialog grants).
pub fn invoke_handler<R: Runtime>()
-> impl Fn(::tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        dialog_open,
        dialog_take_input,
        dialog_close,
        dialog_fit_height
    ]
}

/// Opens a dialog window for the calling window. Async: a window must not be created from a synchronous command.
#[::tauri::command]
pub async fn dialog_open<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    request: DialogRequest,
) -> Result<DialogInfo, DialogError> {
    open(&app, window.label(), request)
}

/// The calling dialog's start data (its id, title, kind, modality and input). Only a dialog window has any.
#[::tauri::command]
pub async fn dialog_take_input<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
) -> Result<DialogStart, DialogError> {
    take_input(&app, window.label())
}

fn take_input<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<DialogStart, DialogError> {
    let entry = dialogs(app)?.get(label).ok_or_else(not_a_dialog)?;
    Ok(DialogStart {
        dialog_id: entry.dialog_id,
        title: entry.title,
        kind: entry.kind,
        modality: entry.modality,
        required_choice: entry.required_choice,
        input: entry.input,
    })
}

/// Closes the calling dialog, answering its opener with `result` (none when it was dismissed).
#[::tauri::command]
pub async fn dialog_close<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    result: Option<Value>,
) -> Result<(), DialogError> {
    close(&app, &window, result)
}

fn close<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    result: Option<Value>,
) -> Result<(), DialogError> {
    let label = window.label();
    let entry = dialogs(app)?.get(label).ok_or_else(not_a_dialog)?;
    if result
        .as_ref()
        .is_some_and(|r| json_len(r) > MAX_RESULT_BYTES)
    {
        return Err(DialogError::new(
            DialogErrorCode::Limit,
            format!("the dialog result is larger than {MAX_RESULT_BYTES} bytes"),
        ));
    }
    if result.is_some() {
        let _ = app.emit_to(
            entry.parent.as_str(),
            RESULT_EVENT,
            DialogResult {
                label: label.to_string(),
                dialog_id: entry.dialog_id,
                result,
            },
        );
    }
    window.destroy().map_err(failed)
}

/// Sizes the calling dialog window to its content: the dialog measured `content` px and asks for that height (the
/// width stays). A viewer only grows; the height is bounded by the dialog's own monitor, and a window that would hang
/// off its bottom moves up on that monitor.
#[::tauri::command]
pub async fn dialog_fit_height<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    content: f64,
) -> Result<(), DialogError> {
    let monitor = monitor_of_window(&window).map(|m| m.0);
    fit_height(&app, &window, content, monitor, std::env::consts::OS)
}

fn fit_height<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    content: f64,
    monitor: Option<Monitor>,
    os: &str,
) -> Result<(), DialogError> {
    let dialogs = dialogs(app)?;
    let entry = dialogs.get(window.label()).ok_or_else(not_a_dialog)?;
    let floor = dialogs
        .contracts()
        .get(&entry.dialog_id)
        .map_or(DIALOG_MIN.height, |c| c.floor().height);
    let scale = window.scale_factor().map_err(failed)?;
    let size = window
        .inner_size()
        .map_err(failed)?
        .to_logical::<f64>(scale);
    let outer = outer_rect(window);
    let plan = plan_fit(entry.kind, content, floor, size.height, outer, monitor, os)?;
    let Some(fit) = plan else {
        return Ok(());
    };
    window
        .set_size(LogicalSize::new(size.width, fit.height))
        .map_err(failed)?;
    if let Some(at) = fit.move_to {
        set_position(window, &at).map_err(failed)?;
    }
    log::info!(
        target: TARGET,
        Dialog = entry.dialog_id.as_str(), Content = content, Height = fit.height;
        "Dialog window fitted to its content"
    );
    Ok(())
}

/// Moves a window to `at`: the exact physical place where logical coordinates are not global, else points.
fn set_position<R: Runtime>(window: &Window<R>, at: &ScreenPosition) -> ::tauri::Result<()> {
    match at.physical {
        Some((x, y)) => window.set_position(PhysicalPosition::new(x, y)),
        None => window.set_position(LogicalPosition::new(at.logical.0, at.logical.1)),
    }
}

#[cfg(test)]
mod tests;
