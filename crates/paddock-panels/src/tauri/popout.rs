//! Opening, listing, focusing, docking back and rebuilding pop-out windows.

use super::{Builder, Panels, TARGET, WindowHooks, host_of, is_popout, native};
use crate::panel::{Panel, popout_url};
use crate::place::{self, MIN_SIZE, Placed};
use crate::registry::PopOut;
use crate::wire::{
    PANELS_EVENT, PanelError, PanelErrorCode, PanelsEvent, PopOutBounds, PopOutInfo, PopOutRequest,
};
use ::tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, Window,
};
use paddock_commands::labels::Label;
use paddock_dialogs::placement::{self, Monitor, Rect};
use paddock_window::bounds::SavedBounds;
use paddock_window::close::clean_title;

pub(super) fn panels<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<::tauri::State<'_, Panels>, PanelError> {
    app.try_state::<Panels>()
        .ok_or_else(|| PanelError::new(PanelErrorCode::NotReady, "the panels are not ready yet"))
}

fn failed(e: impl std::fmt::Display) -> PanelError {
    PanelError::new(PanelErrorCode::Failed, e.to_string())
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

/// The work areas and scales of the displays, as the placement reads them.
pub fn monitors_from(all: &[::tauri::Monitor]) -> Vec<Monitor> {
    all.iter()
        .map(paddock_dialogs::tauri::monitor_rect)
        .collect()
}

/// A window's place now: logical place and size on its display, with that display's scale.
fn bounds_of<R: Runtime>(w: &Window<R>) -> Option<PopOutBounds> {
    let scale = w.scale_factor().ok()?;
    let pos = w.outer_position().ok()?.to_logical::<f64>(scale);
    let size = w.inner_size().ok()?.to_logical::<f64>(scale);
    Some(PopOutBounds {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        scale: Some(scale),
    })
}

/// What to remember of a window: outer top-left (physical px), inner size (logical px) and the display's scale.
pub(super) fn saved_of<R: Runtime>(w: &Window<R>) -> Option<SavedBounds> {
    let scale = w.scale_factor().ok()?;
    let pos = w.outer_position().ok()?;
    let size = w.inner_size().ok()?.to_logical::<f64>(scale);
    place::remember(
        f64::from(pos.x),
        f64::from(pos.y),
        size.width,
        size.height,
        scale,
    )
}

fn info<R: Runtime>(app: &AppHandle<R>, label: &str, e: &PopOut) -> PopOutInfo {
    let window = crate::tauri::window(app, label);
    PopOutInfo {
        label: label.to_string(),
        host: e.host.clone(),
        panel: e.panel.id(),
        title: e.title.clone(),
        bounds: window.as_ref().and_then(bounds_of),
        monitor: window
            .and_then(|w| w.current_monitor().ok().flatten())
            .and_then(|m| m.name().cloned()),
        keep_on_top: e.keep_on_top,
    }
}

pub(super) fn notify<R: Runtime>(app: &AppHandle<R>, host: &str, event: &PanelsEvent) {
    let _ = app.emit_to(host, PANELS_EVENT, event);
}

fn focus_window<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if let Some(w) = crate::tauri::window(app, label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// The window title: "<title> - <host title>".
pub(super) fn window_title(title: &str, host_title: &str) -> String {
    if host_title.is_empty() {
        title.to_string()
    } else {
        clean_title(&format!("{title} - {host_title}"), title)
    }
}

/// Pops `request.panel` of the window `caller` out into a window of its own, or focuses the one that already shows it. The
/// panel stays where it is when this fails.
pub fn open<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    request: PopOutRequest,
) -> Result<PopOutInfo, PanelError> {
    open_on(app, caller, request, std::env::consts::OS)
}

pub(super) fn open_on<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    request: PopOutRequest,
    os: &str,
) -> Result<PopOutInfo, PanelError> {
    let panels = panels(app)?;
    let host = match Label::parse(caller) {
        Some(Label::Main | Label::Window(_)) => caller,
        Some(_) => {
            return Err(PanelError::new(
                PanelErrorCode::NotAllowed,
                "only an application window pops panels out",
            ));
        }
        None => {
            return Err(PanelError::new(
                PanelErrorCode::NotAllowed,
                "not a window of the application",
            ));
        }
    };
    let panel = Panel::parse(&request.panel).map_err(|p| {
        PanelError::new(
            PanelErrorCode::Invalid,
            format!(
                "{:?}: {p}",
                request.panel.chars().take(64).collect::<String>()
            ),
        )
    })?;
    let host_window = app
        .get_webview_window(host)
        .ok_or_else(|| PanelError::new(PanelErrorCode::Closed, "the window was closed"))?;
    if let Some(existing) = panels.popouts.find(host, &panel) {
        focus_window(app, &existing);
        let entry = panels
            .popouts
            .get(&existing)
            .ok_or_else(|| PanelError::new(PanelErrorCode::Closed, "the pop-out was closed"))?;
        return Ok(info(app, &existing, &entry));
    }
    let keep_on_top = request.keep_on_top.unwrap_or(false);
    let title = clean_title(request.title.as_deref().unwrap_or(""), &panel.id());
    let host_os = host_window.as_ref().window();
    let host_title = host_window.title().unwrap_or_default();
    let label = panels.popouts.reserve(PopOut {
        host: host.to_string(),
        panel: panel.clone(),
        title: title.clone(),
        keep_on_top,
        last: None,
    })?;
    let monitors = monitors_from(&host_os.available_monitors().unwrap_or_default());
    let placed = place::place(
        request.bounds,
        panels.memory.get(panel.view()),
        outer_rect(&host_os),
        &monitors,
    );
    let spec = Spec {
        label: &label,
        host,
        panel: &panel,
        title: &window_title(&title, &host_title),
        keep_on_top,
        os,
    };
    if let Err(e) = build_window(app, &spec, &placed, &host_window) {
        panels.popouts.remove(&label);
        return Err(failed(format!(
            "could not open a window for the panel: {e}"
        )));
    }
    log::info!(target: TARGET, Popout = label.as_str(), Host = host, Panel = panel.id().as_str(); "Panel popped out");
    notify(app, host, &PanelsEvent::Changed);
    let entry = panels
        .popouts
        .get(&label)
        .ok_or_else(|| PanelError::new(PanelErrorCode::Closed, "the pop-out was closed"))?;
    Ok(info(app, &label, &entry))
}

struct Spec<'a> {
    label: &'a str,
    host: &'a str,
    panel: &'a Panel,
    title: &'a str,
    keep_on_top: bool,
    os: &'a str,
}

/// Builds the pop-out window. With `keep_on_top` it is an owned window of the host: it stays above its owner, goes behind
/// other apps together with it, and minimises and hides with it. Never a system-wide always-on-top window.
fn build_window<R: Runtime>(
    app: &AppHandle<R>,
    spec: &Spec<'_>,
    placed: &Placed,
    host: &WebviewWindow<R>,
) -> Result<(), PanelError> {
    let panels = panels(app)?;
    let p = placed.placement;
    let at = placement::screen_position(&p, spec.os);
    let (w, h) = p.logical_size();
    let url = popout_url(panels.route(), spec.panel, spec.host, spec.label);
    let builder: Builder<'_, R> =
        WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(url.into()))
            .title(spec.title)
            .inner_size(w, h)
            .position(at.logical.0, at.logical.1)
            .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
            .resizable(true)
            .focused(true);
    let builder = if spec.keep_on_top {
        owned_by(builder, host)?
    } else {
        builder
    };
    let builder = native::frameless(builder);
    let hooks = app.try_state::<WindowHooks<R>>();
    let builder = match hooks.as_ref().and_then(|h| h.builder.as_ref()) {
        Some(f) => f(builder),
        None => builder,
    };
    let window = builder.build().map_err(failed)?;
    if let Some(f) = hooks.as_ref().and_then(|h| h.built.as_ref()) {
        f(&window);
    }
    // Where logical coordinates are not one global space (Windows, Linux with mixed DPI) the builder converted them with
    // some display's scale: set the exact physical place and size now.
    if let Some((x, y)) = at.physical {
        let _ = window.set_position(PhysicalPosition::new(x, y));
        let _ = window.set_size(PhysicalSize::new(
            p.width.round().max(1.0) as u32,
            p.height.round().max(1.0) as u32,
        ));
    }
    Ok(())
}

#[cfg(not(any(test, feature = "mock-runtime")))]
fn owned_by<'a, R: Runtime>(
    b: Builder<'a, R>,
    owner: &WebviewWindow<R>,
) -> Result<Builder<'a, R>, PanelError> {
    b.parent(owner).map_err(failed)
}

/// The mock runtime has no native windows to own (on Linux its `gtk_window` is unimplemented): tests check the
/// bookkeeping only.
#[cfg(any(test, feature = "mock-runtime"))]
fn owned_by<'a, R: Runtime>(
    b: Builder<'a, R>,
    _owner: &WebviewWindow<R>,
) -> Result<Builder<'a, R>, PanelError> {
    Ok(b)
}

/// The pop-outs of the group `caller` belongs to (the host and its pop-outs see the same list).
pub fn list<R: Runtime>(app: &AppHandle<R>, caller: &str) -> Vec<PopOutInfo> {
    let Some(panels) = app.try_state::<Panels>() else {
        return Vec::new();
    };
    let host = host_of(caller);
    panels
        .popouts
        .of_host(host)
        .into_iter()
        .filter_map(|l| panels.popouts.get(&l).map(|e| info(app, &l, &e)))
        .collect()
}

/// The label of the pop-out of `caller`'s group named by `panel`, or the caller itself when it is a pop-out and names none.
fn target<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    panel: Option<&str>,
) -> Result<String, PanelError> {
    let panels = panels(app)?;
    match (is_popout(caller), panel) {
        (true, None) => Ok(caller.to_string()),
        (false, None) => Err(PanelError::new(
            PanelErrorCode::Invalid,
            "name the panel to dock back",
        )),
        (_, Some(id)) => {
            let panel = Panel::parse(id)
                .map_err(|p| PanelError::new(PanelErrorCode::Invalid, p.to_string()))?;
            panels.popouts.find(host_of(caller), &panel).ok_or_else(|| {
                PanelError::new(PanelErrorCode::Closed, "that panel is not popped out")
            })
        }
    }
}

/// Closes a pop-out; its panel docks back into the host (the host hears it when the window is destroyed).
pub fn dock_back<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    panel: Option<&str>,
) -> Result<(), PanelError> {
    let label = target(app, caller, panel)?;
    let w = crate::tauri::window(app, &label)
        .ok_or_else(|| PanelError::new(PanelErrorCode::Closed, "the pop-out was already closed"))?;
    w.close().map_err(failed)
}

/// Brings a popped-out panel's window to the front; false when it is not popped out.
pub fn focus<R: Runtime>(app: &AppHandle<R>, caller: &str, panel: &str) -> bool {
    match target(app, caller, Some(panel)) {
        Ok(label) => {
            focus_window(app, &label);
            true
        }
        Err(_) => false,
    }
}

/// Keeps the calling pop-out above its host window (or not). An owner cannot change on a live window, so the pop-out is
/// rebuilt in place: a new window at the same place for the same panel, then the old one is dropped without docking
/// anything back.
pub fn set_keep_on_top<R: Runtime>(
    app: &AppHandle<R>,
    caller: &str,
    on: bool,
) -> Result<(), PanelError> {
    let panels = panels(app)?;
    if !is_popout(caller) {
        return Err(PanelError::new(
            PanelErrorCode::NotAllowed,
            "only a pop-out window can stay on top of its window",
        ));
    }
    let entry = panels
        .popouts
        .get(caller)
        .ok_or_else(|| PanelError::new(PanelErrorCode::Closed, "the pop-out was closed"))?;
    if entry.keep_on_top == on {
        return Ok(());
    }
    let old = crate::tauri::window(app, caller)
        .ok_or_else(|| PanelError::new(PanelErrorCode::Closed, "the pop-out was closed"))?;
    let host = app
        .get_webview_window(&entry.host)
        .ok_or_else(|| PanelError::new(PanelErrorCode::Closed, "the window was closed"))?;
    let here = saved_of(&old);
    let host_os = host.as_ref().window();
    let monitors = monitors_from(&host_os.available_monitors().unwrap_or_default());
    let placed = place::place(None, here, outer_rect(&host_os), &monitors);
    let fresh = panels.popouts.reserve_replacing(PopOut {
        keep_on_top: on,
        ..entry.clone()
    });
    let spec = Spec {
        label: &fresh,
        host: &entry.host,
        panel: &entry.panel,
        title: &window_title(&entry.title, &host.title().unwrap_or_default()),
        keep_on_top: on,
        os: std::env::consts::OS,
    };
    if let Err(e) = build_window(app, &spec, &placed, &host) {
        panels.popouts.remove(&fresh);
        return Err(failed(format!("could not rebuild the window: {e}")));
    }
    // Forget the old one first: its destruction must not dock the panel back.
    panels.popouts.remove(caller);
    let _ = old.destroy();
    notify(app, &entry.host, &PanelsEvent::Changed);
    Ok(())
}
