//! Where the tray dropdown opens, and when a click on the tray icon closes it instead (pure geometry, physical
//! pixels).

use std::time::Duration;

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

/// Space between the icon and the dropdown.
const GAP: f64 = 6.0;

/// The top-left corner for a dropdown of `size` next to the tray `icon` on `monitor`: centred under (menu bar at the
/// top) or over (taskbar at the bottom) the icon, whichever side of the screen the icon is on, and kept on the
/// monitor.
pub fn dropdown_position(icon: Rect, size: (f64, f64), monitor: Rect) -> (f64, f64) {
    let (w, h) = size;
    let icon_mid_y = icon.y + icon.h / 2.0;
    let below = icon_mid_y < monitor.y + monitor.h / 2.0;
    let y = if below { icon.y + icon.h + GAP } else { icon.y - h - GAP };
    let x = icon.x + icon.w / 2.0 - w / 2.0;
    let clamp = |v: f64, lo: f64, hi: f64| if hi < lo { lo } else { v.clamp(lo, hi) };
    (
        clamp(x, monitor.x + GAP, monitor.x + monitor.w - w - GAP),
        clamp(y, monitor.y + GAP, monitor.y + monitor.h - h - GAP),
    )
}

/// A click on the icon right after the dropdown closed itself (it hides on blur, and the click that blurred it is this
/// click) must not open it again.
pub fn click_closes(since_closed: Option<Duration>) -> bool {
    since_closed.is_some_and(|d| d < Duration::from_millis(400))
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
    fn a_menu_bar_icon_opens_the_dropdown_below_it_centred() {
        let (x, y) = dropdown_position(icon(2000.0, 0.0), (720.0, 1040.0), SCREEN);
        assert_eq!(y, 44.0 + GAP);
        assert_eq!(x, 2022.0 - 360.0);
    }

    #[test]
    fn a_taskbar_icon_opens_it_above() {
        let (_, y) = dropdown_position(icon(2000.0, 1756.0), (720.0, 1040.0), SCREEN);
        assert_eq!(y, 1756.0 - 1040.0 - GAP);
    }

    #[test]
    fn it_stays_on_the_monitor() {
        let (x, _) = dropdown_position(icon(2850.0, 0.0), (720.0, 1040.0), SCREEN);
        assert_eq!(x, 2880.0 - 720.0 - GAP);
        let (x, _) = dropdown_position(icon(0.0, 0.0), (720.0, 1040.0), SCREEN);
        assert_eq!(x, GAP);
        // A monitor smaller than the dropdown pins it to the corner instead of panicking.
        let tiny = Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 100.0,
        };
        assert_eq!(dropdown_position(icon(10.0, 10.0), (720.0, 1040.0), tiny), (10.0, 10.0));
    }

    #[test]
    fn the_click_that_blurred_the_dropdown_does_not_reopen_it() {
        assert!(click_closes(Some(Duration::from_millis(50))));
        assert!(!click_closes(Some(Duration::from_secs(2))));
        assert!(!click_closes(None));
    }
}
