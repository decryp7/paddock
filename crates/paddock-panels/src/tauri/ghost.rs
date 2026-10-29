//! The drag ghost window: created hidden on the first drag and reused, moved by a tracker thread, never left behind.

use super::popout::panels;
use super::{Panels, native};
use crate::ghost::geometry::{PhysRect, Placement, Screen};
use crate::ghost::request::Ghost;
use crate::ghost::{
    GHOST_LABEL, GHOST_PAGE, MAX_DRAG, RELEASED_TICKS, Tick, Visibility, content_script,
    positioning_supported,
};
use crate::wire::{GhostRequest, PanelError, PanelErrorCode, PopOutBounds};
use ::tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, Window};
use std::time::Duration;

/// Can windows be placed on screen here? Not on Wayland (unless GTK runs on XWayland).
pub fn positioning_supported_here() -> bool {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let gdk = std::env::var("GDK_BACKEND").ok();
    positioning_supported(std::env::consts::OS, wayland.as_deref(), gdk.as_deref())
}

/// The displays as the platform reports them, physical px.
pub(super) fn screens_from(all: &[::tauri::Monitor]) -> Vec<Screen> {
    let rect = |p: ::tauri::PhysicalPosition<i32>, s: ::tauri::PhysicalSize<u32>| PhysRect {
        x: p.x,
        y: p.y,
        width: s.width,
        height: s.height,
    };
    all.iter()
        .map(|m| Screen {
            bounds: rect(*m.position(), *m.size()),
            work: rect(m.work_area().position, m.work_area().size),
            scale: m.scale_factor(),
        })
        .collect()
}

/// The displays as `host` window sees them (none when the window is gone).
pub(super) fn screens<R: Runtime>(app: &AppHandle<R>, host: &str) -> Vec<Screen> {
    crate::tauri::window(app, host)
        .and_then(|w| w.available_monitors().ok())
        .map(|all| screens_from(&all))
        .unwrap_or_default()
}

fn cursor<R: Runtime>(app: &AppHandle<R>) -> Option<(f64, f64)> {
    app.cursor_position().ok().map(|p| (p.x, p.y))
}

/// Hands the page its content again (also after the page reloads).
pub(super) fn push_content<R: Runtime>(app: &AppHandle<R>) {
    let Some(panels) = app.try_state::<Panels>() else {
        return;
    };
    let content = panels.ghost().content().cloned();
    let Some(content) = content else { return };
    if let (Some(webview), Some(script)) = (
        app.get_webview_window(GHOST_LABEL),
        content_script(&content),
    ) {
        let _ = webview.eval(script).inspect_err(native::content_lost);
    }
}

/// The page loaded (or the ghost window went away).
pub(super) fn release_page<R: Runtime>(app: &AppHandle<R>) {
    if let Some(p) = app.try_state::<Panels>() {
        p.ghost().reset();
    }
}

/// The ghost window, created hidden on first use.
fn ensure_window<R: Runtime>(
    app: &AppHandle<R>,
    size: crate::ghost::geometry::GhostSize,
) -> ::tauri::Result<Window<R>> {
    if let Some(w) = crate::tauri::window(app, GHOST_LABEL) {
        return Ok(w);
    }
    if paddock_commands::plugin_hook::in_plugin_hook() {
        return Err(::tauri::Error::Io(std::io::Error::other(format!(
            "the window \"{GHOST_LABEL}\" {}",
            paddock_commands::plugin_hook::INSIDE_PLUGIN_HOOK
        ))));
    }
    let builder = WebviewWindowBuilder::new(app, GHOST_LABEL, WebviewUrl::App(GHOST_PAGE.into()))
        .title("Drag preview")
        .inner_size(size.width, size.height)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .on_page_load(native::on_page_load);
    let window = builder.build()?.as_ref().window();
    native::make_click_through(&window);
    Ok(window)
}

/// The ghost's page finished loading: it can take content now (and again after a reload).
pub(super) fn page_loaded<R: Runtime>(app: &AppHandle<R>) {
    if let Some(p) = app.try_state::<Panels>() {
        p.ghost().set_loaded(true);
    }
    push_content(app);
}

fn apply<R: Runtime>(app: &AppHandle<R>, p: Placement) {
    let Some(w) = crate::tauri::window(app, GHOST_LABEL) else {
        return;
    };
    let size = ::tauri::PhysicalSize::new(p.rect.width, p.rect.height);
    if w.outer_size().ok() != Some(size) {
        let _ = w.set_size(size);
    }
    let _ = w.set_position(::tauri::PhysicalPosition::new(p.rect.x, p.rect.y));
}

pub(super) fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = crate::tauri::window(app, GHOST_LABEL) {
        native::show_without_focus(&w);
    }
}

pub(super) fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = crate::tauri::window(app, GHOST_LABEL) {
        native::hide(&w);
    }
}

/// Ends `host`'s session (if it has one) and hides the ghost; where the ghost was, in logical px of its display.
pub fn end_for<R: Runtime>(app: &AppHandle<R>, host: &str) -> Option<PopOutBounds> {
    let placed = app.try_state::<Panels>()?.ghost().finish(host);
    hide(app);
    placed.map(|p| p.logical())
}

/// Starts a drag ghost for `host` from the UI's request: `false` where it cannot follow the pointer.
pub(super) fn begin<R: Runtime>(
    app: &AppHandle<R>,
    host: &str,
    request: GhostRequest,
) -> Result<bool, PanelError> {
    let ghost = request
        .parse()
        .map_err(|m| PanelError::new(PanelErrorCode::Invalid, m))?;
    begin_on(app, host, ghost, positioning_supported_here())
}

/// Starts `host`'s session on these displays (none: no ghost).
pub(super) fn start<R: Runtime>(
    app: &AppHandle<R>,
    host: &str,
    ghost: Ghost,
    screens: Vec<Screen>,
) -> Result<bool, PanelError> {
    let panels = panels(app)?;
    if screens.is_empty() {
        return Ok(false);
    }
    let made = ensure_window(app, ghost.size).inspect_err(native::ghost_unavailable);
    if made.is_err() {
        return Ok(false);
    }
    hide(app);
    let (id, loaded) = panels
        .ghost()
        .begin(host, ghost.content, ghost.size, screens);
    if loaded {
        push_content(app);
    }
    native::spawn_tracker(app.clone(), id).map_err(|e| {
        panels.ghost().finish(host);
        PanelError::new(PanelErrorCode::Failed, e.to_string())
    })?;
    Ok(true)
}

/// Shows the calling window's ghost under the pointer, or hides it.
pub(super) fn show_for<R: Runtime>(app: &AppHandle<R>, host: &str, visible: bool) {
    let Some(panels) = app.try_state::<Panels>() else {
        return;
    };
    let at = cursor(app);
    let step = panels.ghost().set_visible(host, visible, at);
    match step {
        Visibility::Show(p) => {
            apply(app, p);
            show(app);
        }
        Visibility::Hide => hide(app),
        Visibility::Ignore => {}
    }
}

/// One step of the tracker of session `id`: `up` ticks with the primary button up, `elapsed` since the start. Returns
/// whether to keep following.
pub(super) fn step<R: Runtime>(app: &AppHandle<R>, id: u64, up: u32, elapsed: Duration) -> bool {
    let Some(panels) = app.try_state::<Panels>() else {
        return false;
    };
    let step = panels
        .ghost()
        .tick(id, cursor(app), up >= RELEASED_TICKS, elapsed > MAX_DRAG);
    match step {
        Tick::Idle => true,
        Tick::Move(p) => {
            apply(app, p);
            true
        }
        Tick::Hide => {
            hide(app);
            false
        }
        Tick::Stop => false,
    }
}

/// [`begin`] where the platform does (not) let an app place its windows.
pub(super) fn begin_on<R: Runtime>(
    app: &AppHandle<R>,
    host: &str,
    ghost: Ghost,
    supported: bool,
) -> Result<bool, PanelError> {
    panels(app)?;
    if !supported {
        return Ok(false);
    }
    start(app, host, ghost, screens(app, host))
}
