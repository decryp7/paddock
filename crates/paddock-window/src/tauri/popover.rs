//! The tray popover window: built frameless, transparent and always on top with the platform's material, anchored to
//! the tray icon on its monitor, fitted to the page's measured height, closed on blur and Escape with a short fade.
//! Ported from Dyno's `popover.rs`; the geometry is [`crate::popover`].

use super::manager::{apply_feel, lock};
use crate::feel::WebviewFeel;
use crate::popover::{
    CLOSING_SCRIPT, DEFAULT_HEIGHT, FADE, Material, Rect, Style, click_closes, fit_height,
    fit_width, init_script, is_popover, label_for, place, refit_top,
};
use crate::wire::{WindowError, WindowErrorCode};
use ::tauri::utils::config::{Color, WindowEffectsConfig};
use ::tauri::window::{Effect, EffectState};
use ::tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// What a popover shows.
#[derive(Debug, Clone, PartialEq)]
pub struct PopoverSpec {
    /// The route of the app's UI (`index.html#/tray`).
    pub route: String,
    /// The window title (screen readers and the window list of the OS read it).
    pub title: String,
    /// Logical px.
    pub width: f64,
}

impl PopoverSpec {
    pub fn new(route: &str, title: &str) -> PopoverSpec {
        PopoverSpec {
            route: route.to_string(),
            title: title.to_string(),
            width: crate::popover::DEFAULT_WIDTH,
        }
    }

    pub fn width(mut self, width: f64) -> Self {
        self.width = fit_width(width);
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct Open {
    below: bool,
    width: f64,
}

/// The popovers (managed state): their specs by label, the platform style, which are open (and how they are anchored),
/// and when each last closed.
pub struct Popovers {
    style: Style,
    specs: HashMap<String, PopoverSpec>,
    open: Mutex<HashMap<String, Open>>,
    closed_at: Mutex<HashMap<String, Instant>>,
}

impl Default for Popovers {
    fn default() -> Self {
        Popovers::new()
    }
}

impl Popovers {
    /// No popovers yet, this platform's style.
    pub fn new() -> Popovers {
        Popovers {
            style: Style::current(),
            specs: HashMap::new(),
            open: Mutex::new(HashMap::new()),
            closed_at: Mutex::new(HashMap::new()),
        }
    }

    /// Another platform style (an app that wants no material; tests, whose mock windows cannot take the platform's
    /// effects).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The popover `tray` (`item: None`, the icon's) or `tray-<item>`. An invalid item id is a bug: logged and left out.
    pub fn popover(mut self, item: Option<&str>, spec: PopoverSpec) -> Self {
        match label_for(item) {
            Some(label) => {
                self.specs.insert(label, spec);
            }
            None => {
                log::error!(Item = item.unwrap_or_default(); "Not a valid tray popover item id")
            }
        }
        self
    }

    /// Whether the popover of `item` (`None`: the icon's) has a spec: without one a tray click has nothing to open.
    pub fn has(&self, item: Option<&str>) -> bool {
        label_for(item).is_some_and(|label| self.specs.contains_key(&label))
    }

    /// Whether popover `label` is open.
    pub fn is_open(&self, label: &str) -> bool {
        lock(&self.open).contains_key(label)
    }
}

fn popovers<R: Runtime>(app: &AppHandle<R>) -> Result<::tauri::State<'_, Popovers>, WindowError> {
    app.try_state::<Popovers>().ok_or_else(|| {
        WindowError::new(
            WindowErrorCode::NotReady,
            "no tray popover is set up in this app (paddock_window::tauri::Popovers)",
        )
    })
}

/// The platform material for the builder: the macOS popover material with its radius, Windows Acrylic with a faint tint
/// (text stays readable over a busy wallpaper), none elsewhere.
pub(crate) fn effects(style: Style) -> Option<WindowEffectsConfig> {
    match style.material {
        Material::Vibrancy => Some(WindowEffectsConfig {
            effects: vec![Effect::Popover],
            state: Some(EffectState::Active),
            radius: Some(crate::popover::RADIUS_MACOS),
            color: None,
            ..Default::default()
        }),
        Material::Acrylic => Some(WindowEffectsConfig {
            effects: vec![Effect::Acrylic],
            state: None,
            radius: None,
            color: Some(Color(24, 24, 28, 160)),
            ..Default::default()
        }),
        Material::None => None,
    }
}

/// A monitor's work area in physical px.
pub(crate) fn work_rect(m: &::tauri::Monitor) -> Rect {
    let a = m.work_area();
    Rect {
        x: f64::from(a.position.x),
        y: f64::from(a.position.y),
        w: f64::from(a.size.width),
        h: f64::from(a.size.height),
    }
}

/// Opens popover `tray` (`item: None`) or `tray-<item>` anchored to `icon` (physical px), or closes it when it is
/// open (a click on the icon toggles). Returns whether it is open now. A click right after the popover closed itself
/// (the click that blurred it) does nothing.
pub fn toggle_popover<R: Runtime>(
    app: &AppHandle<R>,
    item: Option<&str>,
    icon: Rect,
) -> Result<bool, WindowError> {
    let label = label_for(item).ok_or_else(|| {
        WindowError::new(
            WindowErrorCode::NotAllowed,
            "not a valid tray popover item id",
        )
    })?;
    let pops = popovers(app)?;
    let spec = pops.specs.get(&label).cloned().ok_or_else(|| {
        WindowError::new(
            WindowErrorCode::NotAllowed,
            format!("no popover {label} is set up"),
        )
    })?;
    if pops.is_open(&label) {
        close_popover(app, &label);
        return Ok(false);
    }
    // The click that blurred it, or a click while it still fades out.
    let since = lock(&pops.closed_at).get(&label).map(Instant::elapsed);
    if click_closes(since) || app.get_webview_window(&label).is_some() {
        return Ok(false);
    }
    open(app, &pops, &label, &spec, icon)?;
    Ok(true)
}

fn open<R: Runtime>(
    app: &AppHandle<R>,
    pops: &Popovers,
    label: &str,
    spec: &PopoverSpec,
    icon: Rect,
) -> Result<(), WindowError> {
    super::plugin_hook::check_can_build(label)?;
    let style = pops.style;
    let feel = WebviewFeel::current();
    let b = WebviewWindowBuilder::new(app, label, WebviewUrl::App(spec.route.clone().into()))
        .title(&spec.title)
        .inner_size(spec.width, DEFAULT_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .visible(false)
        .focused(true)
        .transparent(style.transparent())
        .zoom_hotkeys_enabled(feel.zoom_hotkeys)
        .browser_extensions_enabled(feel.browser_extensions)
        .initialization_script(init_script(style));
    let w = super::native::with_material(b, style).build()?;
    apply_feel(&w, feel);
    let monitor = w
        .monitor_from_point(icon.x + icon.w / 2.0, icon.y + icon.h / 2.0)
        .ok()
        .flatten()
        .or_else(|| w.current_monitor().ok().flatten());
    let scale = monitor.as_ref().map_or(1.0, ::tauri::Monitor::scale_factor);
    let size = (spec.width * scale, DEFAULT_HEIGHT * scale);
    let (x, y, below) = place(icon, size, monitor.as_ref().map(work_rect), style.os);
    lock(&pops.open).insert(
        label.to_string(),
        Open {
            below,
            width: spec.width,
        },
    );
    let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    #[cfg(windows)]
    round_corners(&w);
    let _ = w.show();
    let _ = w.set_focus();
    Ok(())
}

/// Windows 11 rounds the corners of a frameless window only when asked (`DWMWA_WINDOW_CORNER_PREFERENCE`).
#[cfg(windows)]
#[allow(unsafe_code)]
fn round_corners<R: Runtime>(w: &WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmSetWindowAttribute,
    };
    let Ok(hwnd) = w.hwnd() else { return };
    let preference: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    // SAFETY: a valid window handle of this process and a pointer to a value that outlives the call; a failure (Windows
    // 10) only leaves the corners square.
    unsafe {
        let _ = DwmSetWindowAttribute(
            HWND(hwnd.0 as _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}

/// Fits popover `label` to its content's measured height (logical px), anchored as it was opened (under a menu bar the
/// top stays, over a taskbar the bottom). Returns the height applied.
pub fn fit_popover<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    asked: f64,
) -> Result<f64, WindowError> {
    if !is_popover(label) {
        return Err(WindowError::new(
            WindowErrorCode::NotAllowed,
            "this is not a tray popover",
        ));
    }
    let w = app
        .get_webview_window(label)
        .ok_or_else(|| WindowError::new(WindowErrorCode::Closed, "the popover is closed"))?;
    let pops = popovers(app)?;
    let open = lock(&pops.open).get(label).copied().unwrap_or(Open {
        below: true,
        width: crate::popover::DEFAULT_WIDTH,
    });
    let scale = w.scale_factor()?;
    let pos = w.outer_position()?;
    let old_h = f64::from(w.outer_size()?.height);
    let monitor = w.current_monitor().ok().flatten().map(|m| work_rect(&m));
    let logical = fit_height(asked, monitor.map_or(f64::NAN, |m| m.h / scale));
    let new_h = logical * scale;
    if (new_h - old_h).abs() >= 1.0 {
        let top = refit_top(f64::from(pos.y), old_h, new_h, open.below, monitor);
        let _ = w.set_size(LogicalSize::new(open.width, logical));
        let _ = w.set_position(PhysicalPosition::new(pos.x, top.round() as i32));
    }
    Ok(logical)
}

/// Closes popover `label` with a short fade (the page animates `data-popover-open`), then destroys the window off the
/// calling thread, and remembers when (see [`click_closes`]).
pub fn close_popover<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(w) = app.get_webview_window(label) else {
        return;
    };
    if let Some(p) = app.try_state::<Popovers>() {
        lock(&p.closed_at).insert(label.to_string(), Instant::now());
        lock(&p.open).remove(label);
    }
    let _ = w.eval(CLOSING_SCRIPT);
    destroy_later(w);
}

fn destroy_later<R: Runtime>(w: WebviewWindow<R>) {
    super::manager::spawn("paddock-popover-close", move || {
        std::thread::sleep(FADE);
        let _ = w.destroy();
    });
}

/// The popover's window events: it closes when it loses the focus, and is forgotten when destroyed. Returns whether
/// `label` is a popover (the event is handled).
pub(crate) fn on_window_event<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    event: &WindowEvent,
) -> bool {
    if !is_popover(label) {
        return false;
    }
    match event {
        WindowEvent::Focused(false) => close_popover(app, label),
        WindowEvent::Destroyed => {
            if let Some(p) = app.try_state::<Popovers>() {
                lock(&p.open).remove(label);
            }
        }
        _ => {}
    }
    true
}
