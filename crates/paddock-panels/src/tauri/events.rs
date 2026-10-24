//! The window events of the layer: a pop-out closing, moving and resizing, a host closing, the drag ending with its
//! window.

use super::ghost::{self, hide, release_page};
use super::popout::{notify, saved_of};
use super::{Panels, TARGET, native};
use crate::ghost::GHOST_LABEL;
use crate::wire::PanelsEvent;
use ::tauri::{AppHandle, Manager, Runtime, Window, WindowEvent};
use paddock_commands::labels::Label;

/// A pop-out window is gone: remember where it was, forget it and, unless its host is gone too or the app is quitting,
/// tell the host to dock the panel back.
pub(super) fn on_destroyed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(panels) = app.try_state::<Panels>() else {
        return;
    };
    let Some(entry) = panels.popouts.remove(label) else {
        return;
    };
    if let Some(last) = entry.last {
        panels.memory.put(entry.panel.view(), last);
    }
    panels.limiter.forget(label);
    log::info!(target: TARGET, Popout = label, Host = entry.host.as_str(); "Pop-out closed");
    if panels.exiting() || crate::tauri::window(app, &entry.host).is_none() {
        return;
    }
    notify(
        app,
        &entry.host,
        &PanelsEvent::Returned {
            panel: entry.panel.id(),
            label: label.to_string(),
        },
    );
    notify(app, &entry.host, &PanelsEvent::Changed);
}

/// The host is gone: close its pop-outs (without docking anything back). Returns how many there were.
pub fn close_all_of<R: Runtime>(app: &AppHandle<R>, host: &str) -> usize {
    let Some(panels) = app.try_state::<Panels>() else {
        return 0;
    };
    let labels = panels.popouts.of_host(host);
    for label in &labels {
        if let Some(entry) = panels.popouts.remove(label)
            && let Some(last) = entry.last
        {
            panels.memory.put(entry.panel.view(), last);
        }
        panels.limiter.forget(label);
        if let Some(w) = crate::tauri::window(app, label) {
            let _ = w.destroy();
        }
    }
    if !labels.is_empty() {
        log::info!(target: TARGET, Host = host, Popouts = labels.len() as u64; "Host window closed; its pop-outs closed too");
    }
    labels.len()
}

/// Hides or shows the host's pop-outs with it (the host went to the tray, or came back).
pub fn set_visible<R: Runtime>(app: &AppHandle<R>, host: &str, visible: bool) {
    let Some(panels) = app.try_state::<Panels>() else {
        return;
    };
    for label in panels.popouts.of_host(host) {
        if let Some(w) = crate::tauri::window(app, &label) {
            let _ = if visible { w.show() } else { w.hide() };
        }
    }
}

/// A pop-out moved or was resized: remember where it is and tell its host.
fn moved<R: Runtime>(window: &Window<R>, host: &str, label: &str) {
    let app = window.app_handle();
    if let Some(panels) = app.try_state::<Panels>() {
        panels.popouts.set_last(label, saved_of(window));
    }
    notify(app, host, &PanelsEvent::Changed);
}

/// A pop-out window is being moved (by its title bar or the shell's drag handle): tells its host where the cursor is, so
/// the host can show its dock guides and dock the panel when the window is dropped on one.
pub(super) fn dragged<R: Runtime>(app: &AppHandle<R>, host: &str, label: &str) {
    let Some(entry) = app.try_state::<Panels>().and_then(|p| p.popouts.get(label)) else {
        return;
    };
    let scale = crate::tauri::window(app, host)
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
    let Some((x, y)) = app
        .cursor_position()
        .ok()
        .and_then(|p| to_logical(p.x, p.y, scale))
    else {
        return;
    };
    notify(
        app,
        host,
        &PanelsEvent::Moved {
            panel: entry.panel.id(),
            label: label.to_string(),
            x,
            y,
        },
    );
}

/// A physical screen position in logical px; `None` for a nonsensical scale or position.
pub(crate) fn to_logical(x: f64, y: f64, scale: f64) -> Option<(f64, f64)> {
    (x.is_finite() && y.is_finite() && scale.is_finite() && scale > 0.0)
        .then(|| (x / scale, y / scale))
}

/// The layer's part of the app's window events: a pop-out's end, moves and resizes; the end of a host (whose pop-outs
/// close with it); and what ends a drag (the dragging window losing the focus with the button up, or going away).
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label();
    match (Label::parse(label), event) {
        (Some(Label::Popout { .. }), WindowEvent::Destroyed) => on_destroyed(app, label),
        (Some(Label::Popout { host, .. }), WindowEvent::Moved(_)) => {
            moved(window, host, label);
            dragged(app, host, label);
        }
        (Some(Label::Popout { host, .. }), WindowEvent::Resized(_)) => moved(window, host, label),
        (Some(Label::Main | Label::Window(_)), WindowEvent::Destroyed) => {
            close_all_of(app, label);
            ghost::end_for(app, label);
            end_ghost_without_windows(app, label);
        }
        _ => {}
    }
    match event {
        WindowEvent::Focused(false)
            if label != GHOST_LABEL && native::primary_button_down() != Some(true) =>
        {
            ghost::end_for(app, label);
        }
        WindowEvent::Destroyed if label == GHOST_LABEL => release_page(app),
        _ => {}
    }
}

/// With no application window left, the hidden ghost goes too, so it never keeps the app running.
pub(super) fn end_ghost_without_windows<R: Runtime>(app: &AppHandle<R>, gone: &str) {
    let others = app
        .webview_windows()
        .into_keys()
        .any(|l| l != gone && matches!(Label::parse(&l), Some(Label::Main | Label::Window(_))));
    if !others && let Some(w) = crate::tauri::window(app, GHOST_LABEL) {
        hide(app);
        let _ = w.destroy();
    }
}
