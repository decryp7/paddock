//! The tray popover window, pure part: a frameless, transparent window with the OS's own material behind it, anchored
//! to the tray icon that opened it, as tall as its content (no empty gap), closed on blur and Escape, with a quick fade
//! (none with reduced motion, which the page honours). Ported from Dyno's `popover.rs`, per the Paddock lesson on tray
//! popups.
//!
//! - macOS: `NSVisualEffectView` popover material with a 12 px radius; the hairline border is the page's (an inset
//!   `box-shadow` on its root), the shadow the system's.
//! - Windows: Acrylic behind the transparent window and rounded corners (`DWMWCP_ROUND`, 8 px on Windows 11; Windows 10
//!   keeps square corners).
//! - Linux: a transparent window with the page's own CSS radius; without a compositor it is simply square.
//!
//! Labels are the shell's: `tray` for the icon's popover and `tray-<item>` for other items' (`paddock_commands`, class
//! `Tray`). The page learns what it is from `<html>` attributes set before it boots: `data-platform`, `data-material`,
//! `data-popover="true"` and `data-popover-open` (`true` once shown, `false` while closing). It reports its measured
//! content height with `tray_fit_height`; Escape calls `tray_close`.

use paddock_commands::Label;
use paddock_commands::labels::{TRAY_LABEL, TRAY_PREFIX, valid_tray_item};
use std::time::Duration;

/// The default width, logical px.
pub const DEFAULT_WIDTH: f64 = 360.0;
/// The height before the page has measured itself, logical px.
pub const DEFAULT_HEIGHT: f64 = 480.0;
/// The smallest height a page may ask for.
pub const MIN_HEIGHT: f64 = 120.0;
/// The largest height a page may ask for (and never more than [`SCREEN_SHARE`] of the monitor).
pub const MAX_HEIGHT: f64 = 720.0;
/// The share of the monitor's height a popover may take at most.
pub const SCREEN_SHARE: f64 = 0.9;
/// The narrowest and widest a popover may be.
pub const MIN_WIDTH: f64 = 240.0;
pub const MAX_WIDTH: f64 = 640.0;
/// The fade-out before the window is destroyed.
pub const FADE: Duration = Duration::from_millis(120);
/// The corner radius of the macOS material (logical px).
pub const RADIUS_MACOS: f64 = 12.0;
/// A click on the icon this soon after the popover closed itself does not reopen it.
pub const REOPEN_GUARD: Duration = Duration::from_millis(400);
/// A blur this soon after the popover was shown is not an outside click: on Windows the shell's notification area takes
/// the foreground back right after the tray click (or the foreground lock refuses `set_focus`), so the popover is
/// blurred before the user could have clicked elsewhere.
pub const BLUR_GRACE: Duration = Duration::from_millis(600);
/// Space between the icon and the popover, and the least margin to the screen edge (physical px).
pub const GAP: f64 = 6.0;

/// A rectangle in physical px (the tray icon, a monitor).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < lo { lo } else { v.clamp(lo, hi) }
}

/// The label of a popover: `tray` for `None`, `tray-<item>` for a valid item id.
pub fn label_for(item: Option<&str>) -> Option<String> {
    match item {
        None => Some(TRAY_LABEL.to_string()),
        Some(id) if valid_tray_item(id) => Some(format!("{TRAY_PREFIX}{id}")),
        Some(_) => None,
    }
}

/// Whether `label` is a tray popover's.
pub fn is_popover(label: &str) -> bool {
    matches!(Label::parse(label), Some(Label::Tray { .. }))
}

/// Whether the popover opens below the icon (a menu bar at the top) rather than above it (a taskbar at the bottom).
pub fn opens_below(icon: Rect, monitor: Rect) -> bool {
    icon.y + icon.h / 2.0 < monitor.y + monitor.h / 2.0
}

/// The top-left corner for a popover of `size` (physical px) at the tray `icon` on `monitor`: centred under (menu bar)
/// or over (taskbar) the icon, kept on the monitor.
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

/// Where a popover of `size` (physical px) goes next to `icon`, with whether it opens below it: on `monitor` (its work
/// area) when the platform says which; without one, below the icon on macOS (a menu bar) and above it elsewhere (a
/// taskbar), centred on it.
pub fn place(icon: Rect, size: (f64, f64), monitor: Option<Rect>, os: &str) -> (f64, f64, bool) {
    match monitor {
        Some(m) => {
            let (x, y) = position(icon, size, m);
            (x, y, opens_below(icon, m))
        }
        None => {
            let below = os == "macos";
            let x = icon.x + icon.w / 2.0 - size.0 / 2.0;
            let y = if below {
                icon.y + icon.h + GAP
            } else {
                icon.y - size.1 - GAP
            };
            (x, y, below)
        }
    }
}

/// [`resized_top`] on a known monitor; without one the anchored edge stays where it is.
pub fn refit_top(top: f64, old_h: f64, new_h: f64, below: bool, monitor: Option<Rect>) -> f64 {
    match monitor {
        Some(m) => resized_top(top, old_h, new_h, below, m),
        None if below => top,
        None => top + old_h - new_h,
    }
}

/// The height (logical px) the page gets: what it asked for, within the limits and within [`SCREEN_SHARE`] of the
/// monitor; [`DEFAULT_HEIGHT`] for a value that is not a number.
pub fn fit_height(asked: f64, monitor_height_logical: f64) -> f64 {
    let room = if monitor_height_logical.is_finite() && monitor_height_logical > 0.0 {
        monitor_height_logical * SCREEN_SHARE
    } else {
        MAX_HEIGHT
    };
    let max = MAX_HEIGHT.min(room.max(MIN_HEIGHT));
    if asked.is_finite() {
        asked.clamp(MIN_HEIGHT, max)
    } else {
        DEFAULT_HEIGHT.min(max)
    }
}

/// A width within [`MIN_WIDTH`]..=[`MAX_WIDTH`] ([`DEFAULT_WIDTH`] for a value that is not a number).
pub fn fit_width(asked: f64) -> f64 {
    if asked.is_finite() {
        asked.clamp(MIN_WIDTH, MAX_WIDTH)
    } else {
        DEFAULT_WIDTH
    }
}

/// The new top edge after a resize: a popover under a menu bar keeps its top, one over a taskbar keeps its bottom (it
/// grows upwards); either way it stays on the monitor.
pub fn resized_top(top: f64, old_h: f64, new_h: f64, below: bool, monitor: Rect) -> f64 {
    let y = if below { top } else { top + old_h - new_h };
    clamp(y, monitor.y + GAP, monitor.y + monitor.h - new_h - GAP)
}

/// Whether a click on the icon right after the popover closed itself must not open it again (it closes on blur, and
/// the click that blurred it is this click).
pub fn click_closes(since_closed: Option<Duration>) -> bool {
    since_closed.is_some_and(|d| d < REOPEN_GUARD)
}

/// What a blur (the popover lost the focus) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlurAction {
    /// An outside click: close.
    Close,
    /// Too soon after showing (see [`BLUR_GRACE`]): stay open and ask for the focus again.
    Keep,
}

/// What a blur `since_shown` after the popover was shown does, with the grace period `grace`.
pub fn blur_action(since_shown: Duration, grace: Duration) -> BlurAction {
    if since_shown < grace {
        BlurAction::Keep
    } else {
        BlurAction::Close
    }
}

/// The material the OS draws behind the transparent window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// macOS `NSVisualEffectView` (popover material).
    Vibrancy,
    /// Windows Acrylic.
    Acrylic,
    /// None: the page paints its own (opaque) background.
    None,
}

impl Material {
    /// The platform's material.
    pub fn for_os(os: &str) -> Material {
        match os {
            "macos" => Material::Vibrancy,
            "windows" => Material::Acrylic,
            _ => Material::None,
        }
    }

    /// The `data-material` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Material::Vibrancy => "vibrancy",
            Material::Acrylic => "acrylic",
            Material::None => "none",
        }
    }
}

/// The OS facts the page needs before it boots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// `std::env::consts::OS`: `macos`, `windows`, `linux`.
    pub os: &'static str,
    pub material: Material,
}

impl Style {
    /// This platform's style.
    pub fn current() -> Style {
        Style {
            os: std::env::consts::OS,
            material: Material::for_os(std::env::consts::OS),
        }
    }

    /// Whether the window is transparent (a material, or Linux with the page's own rounded background).
    pub fn transparent(&self) -> bool {
        self.material != Material::None || self.os == "linux"
    }
}

/// The OS identifier as the page sees it (a safe constant: letters only, so it can sit in the script).
fn safe(text: &str) -> String {
    text.chars().filter(char::is_ascii_lowercase).collect()
}

/// The script run in a popover page before the app: the `<html>` attributes, the open flag (set on the first frame, so
/// the page's CSS transition fades it in) and Escape to close.
pub fn init_script(style: Style) -> String {
    format!(
        "(function(){{var set=function(){{var d=document.documentElement;if(!d)return false;\
d.dataset.platform='{os}';d.dataset.material='{m}';d.dataset.popover='true';d.dataset.popoverOpen='false';return true;}};\
if(!set()){{var o=new MutationObserver(function(){{if(set())o.disconnect();}});o.observe(document,{{childList:true}});}}\
var show=function(){{requestAnimationFrame(function(){{document.documentElement.dataset.popoverOpen='true';}});}};\
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',show);else show();\
document.addEventListener('keydown',function(e){{if(e.key==='Escape'&&!e.defaultPrevented){{window.__TAURI_INTERNALS__.invoke('tray_close');}}}});}})();",
        os = safe(style.os),
        m = style.material.as_str()
    )
}

/// The script that starts the fade-out before the window is destroyed.
pub const CLOSING_SCRIPT: &str = "document.documentElement.dataset.popoverOpen='false'";

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
        Rect {
            x,
            y,
            w: 44.0,
            h: 44.0,
        }
    }

    #[test]
    fn a_blur_right_after_showing_keeps_the_popover_a_later_one_closes_it() {
        // The sequence Windows produces: shown, blurred 5 ms later (the shell took the foreground), focused again, then
        // the user clicks elsewhere.
        let g = BLUR_GRACE;
        assert_eq!(blur_action(Duration::from_millis(5), g), BlurAction::Keep);
        assert_eq!(blur_action(g - Duration::from_millis(1), g), BlurAction::Keep);
        assert_eq!(blur_action(g, g), BlurAction::Close);
        assert_eq!(blur_action(Duration::from_secs(30), g), BlurAction::Close);
        assert_eq!(blur_action(Duration::ZERO, Duration::ZERO), BlurAction::Close);
    }

    #[test]
    fn a_menu_bar_icon_opens_the_popover_below_it_centred() {
        let (x, y) = position(icon(2000.0, 0.0), (720.0, 1040.0), SCREEN);
        assert_eq!(y, 44.0 + GAP);
        assert_eq!(x, 2022.0 - 360.0);
        assert!(opens_below(icon(2000.0, 0.0), SCREEN));
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
        assert_eq!(
            position(icon(10.0, 10.0), (720.0, 1040.0), tiny),
            (10.0 + GAP, 10.0 + GAP)
        );
        // A second monitor left of the primary (negative coordinates), taskbar at the bottom.
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
        assert_eq!(fit_height(300.0, 1000.0), 300.0);
        assert_eq!(fit_height(10.0, 1000.0), MIN_HEIGHT);
        assert_eq!(fit_height(5000.0, 2000.0), MAX_HEIGHT);
        assert_eq!(
            fit_height(5000.0, 600.0),
            540.0,
            "at most 90 % of a small screen"
        );
        assert_eq!(fit_height(f64::NAN, 1000.0), DEFAULT_HEIGHT);
        assert_eq!(fit_height(f64::INFINITY, 400.0), 360.0);
        assert_eq!(fit_height(5000.0, f64::NAN), MAX_HEIGHT, "unknown monitor");
        assert_eq!(fit_height(5000.0, 50.0), MIN_HEIGHT);
        assert_eq!(fit_width(100.0), MIN_WIDTH);
        assert_eq!(fit_width(9000.0), MAX_WIDTH);
        assert_eq!(fit_width(f64::NAN), DEFAULT_WIDTH);
    }

    #[test]
    fn growing_keeps_the_anchored_edge() {
        assert_eq!(resized_top(50.0, 400.0, 600.0, true, SCREEN), 50.0);
        assert_eq!(resized_top(700.0, 400.0, 600.0, false, SCREEN), 500.0);
        assert_eq!(resized_top(100.0, 400.0, 900.0, false, SCREEN), GAP);
        assert_eq!(
            resized_top(1500.0, 400.0, 600.0, true, SCREEN),
            1800.0 - 600.0 - GAP
        );
    }

    #[test]
    fn without_a_monitor_the_popover_opens_where_the_platform_puts_its_tray() {
        let i = icon(1000.0, 0.0);
        assert_eq!(
            place(i, (720.0, 400.0), None, "macos"),
            (662.0, 44.0 + GAP, true)
        );
        assert_eq!(
            place(i, (720.0, 400.0), None, "windows"),
            (662.0, -400.0 - GAP, false)
        );
        assert_eq!(
            place(i, (720.0, 400.0), Some(SCREEN), "windows"),
            (662.0, 44.0 + GAP, true),
            "the monitor decides"
        );
        assert_eq!(refit_top(100.0, 400.0, 600.0, true, None), 100.0);
        assert_eq!(refit_top(700.0, 400.0, 600.0, false, None), 500.0);
        assert_eq!(refit_top(700.0, 400.0, 600.0, false, Some(SCREEN)), 500.0);
    }

    #[test]
    fn the_click_that_blurred_the_popover_does_not_reopen_it() {
        assert!(click_closes(Some(Duration::from_millis(50))));
        assert!(!click_closes(Some(Duration::from_secs(2))));
        assert!(!click_closes(None));
    }

    #[test]
    fn labels_are_the_shells_tray_labels() {
        assert_eq!(label_for(None).as_deref(), Some("tray"));
        assert_eq!(label_for(Some("cpu-1")).as_deref(), Some("tray-cpu-1"));
        assert_eq!(label_for(Some("CPU")), None);
        assert!(is_popover("tray") && is_popover("tray-cpu"));
        for bad in ["main", "tray-", "popover-x", "d-main-1"] {
            assert!(!is_popover(bad), "{bad}");
        }
    }

    #[test]
    fn the_init_script_sets_the_attributes_and_closes_on_escape() {
        let s = init_script(Style {
            os: "macos",
            material: Material::Vibrancy,
        });
        for needle in [
            "dataset.platform='macos'",
            "dataset.material='vibrancy'",
            "dataset.popover='true'",
            "popoverOpen",
            "'Escape'",
            "tray_close",
        ] {
            assert!(s.contains(needle), "{needle}");
        }
        let odd = init_script(Style {
            os: "x';alert(1)//",
            material: Material::None,
        });
        assert!(
            odd.contains("dataset.platform='xalert'"),
            "only letters reach the script"
        );
        assert!(CLOSING_SCRIPT.contains("popoverOpen='false'"));
    }

    #[test]
    fn the_material_is_per_platform() {
        assert_eq!(Material::for_os("macos"), Material::Vibrancy);
        assert_eq!(Material::for_os("windows"), Material::Acrylic);
        assert_eq!(Material::for_os("linux"), Material::None);
        assert_eq!(Material::Acrylic.as_str(), "acrylic");
        assert_eq!(Material::None.as_str(), "none");
        let linux = Style {
            os: "linux",
            material: Material::None,
        };
        assert!(
            linux.transparent(),
            "the page draws its own rounded background"
        );
        assert!(
            !Style {
                os: "freebsd",
                material: Material::None
            }
            .transparent()
        );
        assert_eq!(Style::current().os, std::env::consts::OS);
    }
}
