//! A native-feeling popover window for tray / menu-bar items: frameless, transparent, rounded, with the OS's own
//! translucent material behind it, anchored to the icon that opened it, as tall as its content, closed on blur and
//! Escape. Self-contained (it depends on `tauri` and `serde_json` only, plus the three small facts passed in
//! [`Style`]), so another app of the suite can copy this file.
//!
//! - macOS: transparent window, `NSVisualEffectView` popover material with a 12 px radius (Tauri window effects).
//!   Needs `macOSPrivateApi` (already on for the main window). The 1 px hairline border is the page's (an inset
//!   `box-shadow` on the root element); the window shadow is the system's.
//! - Windows 11: Acrylic behind a transparent window and `DWMWCP_ROUND` corners; older Windows: an opaque window.
//! - Linux: a transparent window with the page's own CSS radius; without a compositor it is simply square.
//!
//! The page learns what it is through `<html>` attributes set before it boots: `data-platform`, `data-vibrancy`,
//! `data-popover="true"` and `data-popover-open` (`true` once shown, `false` while closing). Fading and scaling are
//! CSS transitions on that attribute (about [`FADE_MS`] ms; none for `prefers-reduced-motion`, which the page
//! honours). The page reports its measured content height with the `popover_resize` command; Escape closes it.
//!
//! Geometry (placement, size limits, resize anchoring) is pure and unit-tested; the window code around it is thin.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime};

/// The popover's width in logical pixels.
pub const WIDTH: f64 = 360.0;
/// Height before the page has measured itself.
pub const DEFAULT_HEIGHT: f64 = 520.0;
/// Smallest and largest height a page may ask for.
pub const MIN_HEIGHT: f64 = 120.0;
/// See [`MIN_HEIGHT`].
pub const MAX_HEIGHT: f64 = 720.0;
/// The fade-out before the window is destroyed.
pub const FADE_MS: u64 = 120;
/// Corner radius of the native material (logical pixels).
pub const RADIUS: f64 = 12.0;

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left.
    pub x: f64,
    /// Top.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
}

/// Space between the icon and the popover, and the minimum margin to the screen edge.
const GAP: f64 = 6.0;

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < lo {
        lo
    } else {
        v.clamp(lo, hi)
    }
}

/// Whether the popover opens below the icon (a menu bar at the top) rather than above it (a taskbar at the bottom).
pub fn opens_below(icon: Rect, monitor: Rect) -> bool {
    icon.y + icon.h / 2.0 < monitor.y + monitor.h / 2.0
}

/// The top-left corner for a popover of `size` (physical pixels) next to the tray `icon` on `monitor`: centred under
/// (menu bar) or over (taskbar) the icon, and kept on the monitor.
pub fn position(icon: Rect, size: (f64, f64), monitor: Rect) -> (f64, f64) {
    let (w, h) = size;
    let y = if opens_below(icon, monitor) {
        icon.y + icon.h + GAP
    } else {
        icon.y - h - GAP
    };
    let x = icon.x + icon.w / 2.0 - w / 2.0;
    (
        clamp(x, monitor.x + GAP, monitor.x + monitor.w - w - GAP),
        clamp(y, monitor.y + GAP, monitor.y + monitor.h - h - GAP),
    )
}

/// The height (logical pixels) the page may have: what it asked for, within the limits and within 90 % of the monitor.
pub fn clamp_height(asked: f64, monitor_height_logical: f64) -> f64 {
    let max = MAX_HEIGHT.min((monitor_height_logical * 0.9).max(MIN_HEIGHT));
    if asked.is_finite() {
        asked.clamp(MIN_HEIGHT, max)
    } else {
        DEFAULT_HEIGHT.min(max)
    }
}

/// The new top edge after a resize: a popover under a menu bar keeps its top, one over a taskbar keeps its bottom
/// (so it grows upwards); either way it stays on the monitor.
pub fn resized_top(top: f64, old_h: f64, new_h: f64, below: bool, monitor: Rect) -> f64 {
    let y = if below { top } else { top + old_h - new_h };
    clamp(y, monitor.y + GAP, monitor.y + monitor.h - new_h - GAP)
}

/// A click on the icon right after the popover closed itself (it closes on blur, and the click that blurred it is this
/// click) must not open it again.
pub fn click_closes(since_closed: Option<Duration>) -> bool {
    since_closed.is_some_and(|d| d < Duration::from_millis(400))
}

/// Labels a page may be opened under: the main popover `tray` and `popover-<item>` with a plain item id.
pub fn is_popover_label(label: &str) -> bool {
    label == "tray" || label.strip_prefix("popover-").is_some_and(valid_item_id)
}

/// An item id: lowercase letters, digits and dashes, 1 to 32 characters (it becomes a window label and a route).
pub fn valid_item_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// What the page needs to know before it boots.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    /// `windows`, `macos` or `linux`.
    pub os: &'static str,
    /// `vibrancy`, `mica` or `none` (whether the OS draws a material behind the transparent window).
    pub vibrancy: &'static str,
}

/// The script run in a popover page before the app: the `<html>` attributes, the open flag and Escape to close.
pub fn init_script(style: Style) -> String {
    format!(
        "(function(){{var set=function(){{var d=document.documentElement;if(!d)return false;d.dataset.platform='{os}';d.dataset.vibrancy='{v}';d.dataset.popover='true';d.dataset.popoverOpen='false';return true;}};\
         if(!set()){{var o=new MutationObserver(function(){{if(set())o.disconnect();}});o.observe(document,{{childList:true}});}}\
         var show=function(){{requestAnimationFrame(function(){{document.documentElement.dataset.popoverOpen='true';}});}};\
         if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',show);else show();\
         document.addEventListener('keydown',function(e){{if(e.key==='Escape'){{window.__TAURI_INTERNALS__.invoke('popover_close');}}}});}})();",
        os = style.os,
        v = style.vibrancy
    )
}

/// Which popovers are open (so a resize knows how to anchor) and when each last closed itself.
#[derive(Default)]
pub struct State {
    below: Mutex<HashMap<String, bool>>,
    closed_at: Mutex<HashMap<String, Instant>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Registers the state; call once at start-up.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    app.manage(State::default());
}

fn monitor_bounds<R: Runtime>(app: &AppHandle<R>, near: Rect) -> (f64, Rect) {
    let m = app
        .monitor_from_point(near.x + near.w / 2.0, near.y + near.h / 2.0)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    match m {
        Some(m) => (
            m.scale_factor(),
            Rect {
                x: f64::from(m.position().x),
                y: f64::from(m.position().y),
                w: f64::from(m.size().width),
                h: f64::from(m.size().height),
            },
        ),
        None => (
            1.0,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
            },
        ),
    }
}

/// Opens the popover `label` showing `route` (e.g. `index.html#/tray`) next to `icon`, or closes it when it is open
/// (a click on the icon toggles). Returns an error text when the window cannot be built.
pub fn toggle<R: Runtime>(app: &AppHandle<R>, label: &str, route: &str, icon: Rect, style: Style) -> Result<(), String> {
    if !is_popover_label(label) {
        return Err(format!("{label} is not a popover label"));
    }
    if app.get_webview_window(label).is_some() {
        close(app, label);
        return Ok(());
    }
    let since = lock(&app.state::<State>().closed_at).get(label).map(Instant::elapsed);
    if click_closes(since) {
        return Ok(());
    }
    open(app, label, route, icon, style).map_err(|e| e.to_string())
}

fn open<R: Runtime>(app: &AppHandle<R>, label: &str, route: &str, icon: Rect, style: Style) -> tauri::Result<()> {
    let (scale, bounds) = monitor_bounds(app, icon);
    let (x, y) = position(icon, (WIDTH * scale, DEFAULT_HEIGHT * scale), bounds);
    lock(&app.state::<State>().below).insert(label.to_string(), opens_below(icon, bounds));
    let material = style.vibrancy != "none";
    let mut b = tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App(route.into()))
        .title("Dyno")
        .inner_size(WIDTH, DEFAULT_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .transparent(material || style.os == "linux")
        .initialization_script(init_script(style));
    if let Some(effects) = effects(style) {
        b = b.effects(effects);
    }
    let w = b.build()?;
    w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    #[cfg(windows)]
    round_corners(&w);
    let _ = w.set_focus();
    Ok(())
}

fn effects(style: Style) -> Option<tauri::utils::config::WindowEffectsConfig> {
    use tauri::utils::config::{Color, WindowEffectsConfig};
    use tauri::window::{Effect, EffectState};
    match (style.os, style.vibrancy) {
        ("macos", "vibrancy") => Some(WindowEffectsConfig {
            effects: vec![Effect::Popover],
            state: Some(EffectState::Active),
            radius: Some(RADIUS),
            color: None,
            ..Default::default()
        }),
        ("windows", "mica") => Some(WindowEffectsConfig {
            effects: vec![Effect::Acrylic],
            state: None,
            radius: None,
            // A faint tint so text stays readable over a busy wallpaper.
            color: Some(Color(24, 24, 28, 160)),
            ..Default::default()
        }),
        _ => None,
    }
}

/// Windows 11 rounds the corners of a frameless window only when asked (`DWMWA_WINDOW_CORNER_PREFERENCE`).
#[cfg(windows)]
#[allow(unsafe_code)]
fn round_corners<R: Runtime>(w: &tauri::WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE,
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

/// Resizes the popover to its content's height (the page measured it), anchored as it was opened. Returns the height
/// that was applied (logical pixels).
pub fn resize<R: Runtime>(app: &AppHandle<R>, label: &str, asked: f64) -> Result<f64, String> {
    if !is_popover_label(label) {
        return Err(format!("{label} is not a popover"));
    }
    let w = app.get_webview_window(label).ok_or("The popover is closed.")?;
    let scale = w.scale_factor().map_err(|e| e.to_string())?;
    let pos = w.outer_position().map_err(|e| e.to_string())?;
    let old_h = w.outer_size().map_err(|e| e.to_string())?.height as f64;
    let bounds = match w.current_monitor().ok().flatten() {
        Some(m) => Rect {
            x: f64::from(m.position().x),
            y: f64::from(m.position().y),
            w: f64::from(m.size().width),
            h: f64::from(m.size().height),
        },
        None => Rect {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        },
    };
    let new_logical = clamp_height(asked, bounds.h / scale);
    let new_h = new_logical * scale;
    if (new_h - old_h).abs() < 1.0 {
        return Ok(new_logical);
    }
    let below = lock(&app.state::<State>().below).get(label).copied().unwrap_or(true);
    let top = resized_top(f64::from(pos.y), old_h, new_h, below, bounds);
    w.set_size(LogicalSize::new(WIDTH, new_logical)).map_err(|e| e.to_string())?;
    w.set_position(PhysicalPosition::new(pos.x, top.round() as i32))
        .map_err(|e| e.to_string())?;
    Ok(new_logical)
}

/// Closes the popover with a short fade (the page animates `data-popover-open`), then destroys the window and
/// remembers the time (see [`click_closes`]).
pub fn close<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(w) = app.get_webview_window(label) else { return };
    lock(&app.state::<State>().closed_at).insert(label.to_string(), Instant::now());
    let _ = w.eval("document.documentElement.dataset.popoverOpen='false'");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(FADE_MS));
        let _ = w.destroy();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 2880.0,
        h: 1800.0,
    };

    fn icon(x: f64, y: f64) -> Rect {
        Rect { x, y, w: 44.0, h: 44.0 }
    }

    #[test]
    fn a_menu_bar_icon_opens_the_popover_below_it_centred() {
        let (x, y) = position(icon(2000.0, 0.0), (720.0, 1040.0), SCREEN);
        assert_eq!(y, 44.0 + GAP);
        assert_eq!(x, 2022.0 - 360.0);
    }

    #[test]
    fn a_taskbar_icon_opens_it_above() {
        let (_, y) = position(icon(2000.0, 1756.0), (720.0, 1040.0), SCREEN);
        assert_eq!(y, 1756.0 - 1040.0 - GAP);
        assert!(!opens_below(icon(2000.0, 1756.0), SCREEN));
    }

    #[test]
    fn it_never_leaves_the_monitor() {
        let (x, _) = position(icon(2850.0, 0.0), (720.0, 1040.0), SCREEN);
        assert_eq!(x, 2880.0 - 720.0 - GAP);
        let (x, _) = position(icon(0.0, 0.0), (720.0, 1040.0), SCREEN);
        assert_eq!(x, GAP);
        let tiny = Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 100.0,
        };
        assert_eq!(position(icon(10.0, 10.0), (720.0, 1040.0), tiny), (10.0 + GAP, 10.0 + GAP));
        // A second monitor to the left of the primary (negative coordinates).
        let left = Rect {
            x: -1920.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        let (x, y) = position(icon(-1900.0, 1036.0), (720.0, 600.0), left);
        assert_eq!((x, y), (-1920.0 + GAP, 1036.0 - 600.0 - GAP));
    }

    #[test]
    fn the_height_follows_the_content_within_limits() {
        assert_eq!(clamp_height(300.0, 1000.0), 300.0);
        assert_eq!(clamp_height(10.0, 1000.0), MIN_HEIGHT);
        assert_eq!(clamp_height(5000.0, 2000.0), MAX_HEIGHT);
        assert_eq!(clamp_height(5000.0, 600.0), 540.0, "at most 90 % of a small screen");
        assert_eq!(clamp_height(f64::NAN, 1000.0), DEFAULT_HEIGHT);
        assert_eq!(clamp_height(f64::INFINITY, 1000.0), DEFAULT_HEIGHT);
    }

    #[test]
    fn growing_keeps_the_anchored_edge() {
        // Under a menu bar the top stays; over a taskbar the bottom stays, so it grows upwards.
        assert_eq!(resized_top(50.0, 400.0, 600.0, true, SCREEN), 50.0);
        assert_eq!(resized_top(700.0, 400.0, 600.0, false, SCREEN), 500.0);
        // Never above the screen's top, never below its bottom.
        assert_eq!(resized_top(100.0, 400.0, 900.0, false, SCREEN), GAP);
        assert_eq!(resized_top(1500.0, 400.0, 600.0, true, SCREEN), 1800.0 - 600.0 - GAP);
    }

    #[test]
    fn the_click_that_blurred_the_popover_does_not_reopen_it() {
        assert!(click_closes(Some(Duration::from_millis(50))));
        assert!(!click_closes(Some(Duration::from_secs(2))));
        assert!(!click_closes(None));
    }

    #[test]
    fn only_known_labels_are_popovers() {
        assert!(is_popover_label("tray"));
        assert!(is_popover_label("popover-cpu-1"));
        for bad in [
            "main",
            "popover-",
            "popover-CPU",
            "popover-a b",
            "popover-../x",
            &format!("popover-{}", "a".repeat(33)),
        ] {
            assert!(!is_popover_label(bad), "{bad}");
        }
    }

    #[test]
    fn the_init_script_sets_the_attributes_and_closes_on_escape() {
        let s = init_script(Style {
            os: "macos",
            vibrancy: "vibrancy",
        });
        for needle in [
            "dataset.platform='macos'",
            "dataset.vibrancy='vibrancy'",
            "dataset.popover='true'",
            "popoverOpen",
            "'Escape'",
            "popover_close",
        ] {
            assert!(s.contains(needle), "{needle}");
        }
    }

    #[test]
    fn the_material_is_per_platform() {
        assert!(effects(Style {
            os: "macos",
            vibrancy: "vibrancy"
        })
        .is_some_and(|e| e.radius == Some(RADIUS)));
        assert!(effects(Style {
            os: "windows",
            vibrancy: "mica"
        })
        .is_some());
        assert!(
            effects(Style {
                os: "windows",
                vibrancy: "none"
            })
            .is_none(),
            "Windows 10: an opaque window"
        );
        assert!(effects(Style {
            os: "linux",
            vibrancy: "none"
        })
        .is_none());
    }
}
