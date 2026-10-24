//! Where a pop-out opens: on any display, always inside that display's work area.
//!
//! Pure geometry in physical pixels (the policy of `paddock_dialogs::placement`, which this builds on), so it holds
//! across displays with different scale factors and for a display left of or above the primary one (negative
//! coordinates). The order of preference:
//!
//! 1. the place the UI asked for (a tab dropped from the drag ghost: logical px of the display it was dropped on, with
//!    that display's `scale`);
//! 2. the place remembered for the view (the last bounds of a pop-out of it);
//! 3. beside the host window: near its top-right corner, on the host's display;
//! 4. a default place.
//!
//! Whatever was chosen is then moved (and shrunk, when it is larger) into the work area of the display it overlaps most,
//! the nearest one when it overlaps none (a remembered place on a display that is gone), so a pop-out is always fully
//! visible and never on a display the user did not put it on.

use crate::wire::PopOutBounds;
use paddock_dialogs::placement::{Monitor, Placement, Rect, TITLE_BAR, monitor_of};
use paddock_window::bounds::{SavedBounds, inside};

/// The smallest pop-out (logical px).
pub const MIN_SIZE: (f64, f64) = (240.0, 160.0);
/// The largest side of a pop-out (logical px).
pub const MAX_SIDE: f64 = 16_384.0;
/// The size of a pop-out nothing else says (logical px).
pub const DEFAULT_SIZE: (f64, f64) = (560.0, 420.0);
/// How far inside the host's right edge, and below its top edge, a pop-out opens by default (logical px).
pub const HOST_INSET: (f64, f64) = (40.0, 80.0);
/// Where a pop-out opens when nothing at all is known (logical px).
pub const FALLBACK_AT: f64 = 100.0;

/// Which preference decided the place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The UI asked for it.
    Requested,
    /// The last bounds of a pop-out of the view.
    Remembered,
    /// Beside the host window.
    BesideHost,
    /// Nothing was known.
    Default,
}

/// A decided place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// Outer top-left corner, inner size and scale, physical px (see `Placement`).
    pub placement: Placement,
    pub source: Source,
}

/// Where the outer top-left corner comes from.
enum Origin {
    At((f64, f64)),
    Beside(Rect),
    Fallback,
}

fn finite(b: &PopOutBounds) -> bool {
    [b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite())
}

fn sane_scale(s: f64) -> Option<f64> {
    (s.is_finite() && s > 0.0 && s <= 16.0).then_some(s)
}

fn side(v: f64, min: f64) -> f64 {
    v.clamp(min, MAX_SIDE)
}

/// The squared distance (physical px) from a point to a rectangle; 0 inside.
fn distance2(r: &Rect, x: f64, y: f64) -> f64 {
    let dx = (r.x - x).max(x - (r.x + r.width)).max(0.0);
    let dy = (r.y - y).max(y - (r.y + r.height)).max(0.0);
    dx * dx + dy * dy
}

/// Logical coordinates without a scale: the physical position on each display is `logical * that display's scale`;
/// the display the point lands in (else nearest to) is taken, the host's display when several fit equally.
fn physical_of_logical(
    x: f64,
    y: f64,
    monitors: &[Monitor],
    prefer: Option<usize>,
) -> Option<(f64, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, m) in monitors.iter().enumerate() {
        let s = m.scale();
        let d = distance2(&m.work, x * s, y * s);
        let better = match best {
            None => true,
            Some((_, bd)) => d < bd || (d == bd && prefer == Some(i)),
        };
        if better {
            best = Some((i, d));
        }
    }
    let (i, _) = best?;
    let s = monitors.get(i)?.scale();
    Some((x * s, y * s))
}

/// Decides where a pop-out opens. `host` is the host window's outer rectangle (physical px), `monitors` the work areas.
pub fn place(
    wanted: Option<PopOutBounds>,
    remembered: Option<SavedBounds>,
    host: Option<Rect>,
    monitors: &[Monitor],
) -> Placed {
    let wanted = wanted.filter(finite);
    let host = host.filter(Rect::is_usable);
    let host_monitor = host.and_then(|h| monitor_of(&h, monitors));
    let one = Monitor::scale;
    let host_scale = host_monitor.and_then(|i| monitors.get(i)).map_or(1.0, one);
    // Size (logical px) and the outer top-left (physical px), by preference.
    let (source, (w, h), origin) = if let Some(b) = wanted {
        let size = (side(b.width, MIN_SIZE.0), side(b.height, MIN_SIZE.1));
        let at = match b.scale.and_then(sane_scale) {
            Some(s) => (b.x * s, b.y * s),
            None => physical_of_logical(b.x, b.y, monitors, host_monitor).unwrap_or((b.x, b.y)),
        };
        (Source::Requested, size, Origin::At(at))
    } else if let Some(r) = remembered.and_then(SavedBounds::sanitized) {
        let size = (side(r.width, MIN_SIZE.0), side(r.height, MIN_SIZE.1));
        (Source::Remembered, size, Origin::At((r.x, r.y)))
    } else if let Some(hr) = host {
        (Source::BesideHost, DEFAULT_SIZE, Origin::Beside(hr))
    } else {
        (Source::Default, DEFAULT_SIZE, Origin::Fallback)
    };
    let (x, y) = match origin {
        Origin::At(at) => at,
        Origin::Beside(hr) => (
            hr.x + hr.width - w * host_scale - HOST_INSET.0 * host_scale,
            hr.y + HOST_INSET.1 * host_scale,
        ),
        Origin::Fallback => (FALLBACK_AT * host_scale, FALLBACK_AT * host_scale),
    };
    let wide = Rect {
        x,
        y,
        width: w * host_scale,
        height: (h + TITLE_BAR) * host_scale,
    };
    let target = monitor_of(&wide, monitors).and_then(|i| monitors.get(i).copied());
    let placement = match target.filter(|m| m.work.is_usable()) {
        Some(m) => {
            let s = m.scale();
            let outer = Rect {
                x,
                y,
                width: w * s,
                height: (h + TITLE_BAR) * s,
            };
            let fit = inside(outer, &m.work);
            Placement {
                x: fit.x,
                y: fit.y,
                width: fit.width,
                height: (fit.height - TITLE_BAR * s).max(1.0),
                scale: s,
                remembered: source == Source::Remembered,
            }
        }
        None => Placement {
            x,
            y,
            width: w * host_scale,
            height: h * host_scale,
            scale: host_scale,
            remembered: source == Source::Remembered,
        },
    };
    Placed { placement, source }
}

/// What to remember of a pop-out at `bounds` (outer top-left physical px; inner size and scale of its display).
pub fn remember(x: f64, y: f64, width: f64, height: f64, scale: f64) -> Option<SavedBounds> {
    SavedBounds {
        x,
        y,
        width,
        height,
        scale,
        maximized: false,
        fullscreen: false,
    }
    .sanitized()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn mon(x: f64, y: f64, width: f64, height: f64, scale: f64) -> Monitor {
        Monitor {
            work: rect(x, y, width, height),
            scale,
        }
    }

    /// A 1080p display at 100% with a 40 px task bar, a 4K one at 200% to its right, and a 1440p one at 150% left of
    /// the first (negative coordinates).
    fn desk() -> Vec<Monitor> {
        vec![
            mon(0.0, 0.0, 1920.0, 1040.0, 1.0),
            mon(1920.0, 0.0, 3840.0, 2080.0, 2.0),
            mon(-2560.0, 0.0, 2560.0, 1400.0, 1.5),
        ]
    }

    fn wanted(x: f64, y: f64, width: f64, height: f64, scale: Option<f64>) -> Option<PopOutBounds> {
        Some(PopOutBounds {
            x,
            y,
            width,
            height,
            scale,
        })
    }

    #[test]
    fn a_requested_place_on_the_first_display_is_kept() {
        let p = place(
            wanted(100.0, 120.0, 560.0, 420.0, Some(1.0)),
            None,
            None,
            &desk(),
        );
        assert_eq!(p.source, Source::Requested);
        let pl = p.placement;
        assert_eq!(
            (pl.x, pl.y, pl.width, pl.height, pl.scale),
            (100.0, 120.0, 560.0, 420.0, 1.0)
        );
        assert!(!pl.remembered);
    }

    #[test]
    fn a_tab_dropped_on_a_high_dpi_display_opens_there_in_that_display_s_pixels() {
        // Logical (1100, 200) of the 200% display: physical (2200, 400), on the 4K display.
        let p = place(
            wanted(1100.0, 200.0, 560.0, 420.0, Some(2.0)),
            None,
            None,
            &desk(),
        )
        .placement;
        assert_eq!((p.x, p.y, p.scale), (2200.0, 400.0, 2.0));
        assert_eq!(
            (p.width, p.height),
            (1120.0, 840.0),
            "inner size in physical px of that display"
        );
        assert_eq!(p.logical_size(), (560.0, 420.0));
    }

    #[test]
    fn a_display_left_of_the_primary_one_has_negative_coordinates() {
        let p = place(
            wanted(-1500.0, 100.0, 800.0, 600.0, Some(1.5)),
            None,
            None,
            &desk(),
        )
        .placement;
        assert_eq!((p.x, p.y, p.scale), (-2250.0, 150.0, 1.5));
    }

    #[test]
    fn without_a_scale_the_display_the_point_lands_on_decides_and_a_tie_goes_to_the_host_display() {
        // Logical x 1100 is physical 1100 on the first display (inside it) and 2200 on the second (inside it).
        let on_first = rect(300.0, 100.0, 800.0, 600.0);
        let on_second = rect(3000.0, 100.0, 800.0, 600.0);
        let w = wanted(1100.0, 200.0, 400.0, 300.0, None);
        let a = place(w, None, Some(on_first), &desk()).placement;
        assert_eq!((a.x, a.scale), (1100.0, 1.0));
        let b = place(w, None, Some(on_second), &desk()).placement;
        assert_eq!((b.x, b.scale), (2200.0, 2.0));
        // A point that fits one display only.
        let c = place(
            wanted(2000.0, 100.0, 400.0, 300.0, None),
            None,
            None,
            &desk(),
        )
        .placement;
        assert_eq!(
            (c.x, c.scale),
            (4000.0, 2.0),
            "2000 logical is past the first display, on the second at 200%"
        );
    }

    #[test]
    fn a_place_off_every_display_moves_to_the_nearest_one_and_inside_its_work_area() {
        // Far right of everything.
        let p = place(
            wanted(90_000.0, 100.0, 560.0, 420.0, Some(1.0)),
            None,
            None,
            &desk(),
        )
        .placement;
        assert_eq!(p.scale, 2.0);
        assert!(p.x + p.width <= 1920.0 + 3840.0 && p.x >= 1920.0, "{p:?}");
        // Below the bottom edge of the first display: the title bar and all stay above the task bar.
        let q = place(
            wanted(100.0, 5000.0, 560.0, 420.0, Some(1.0)),
            None,
            None,
            &desk()[..1],
        )
        .placement;
        assert_eq!(q.scale, 1.0);
        assert!(q.y + q.height + TITLE_BAR <= 1040.0, "{q:?}");
        assert_eq!(q.x, 100.0);
    }

    #[test]
    fn a_pop_out_larger_than_the_work_area_is_shrunk_to_it() {
        let p = place(
            wanted(0.0, 0.0, 5000.0, 4000.0, Some(1.0)),
            None,
            None,
            &[mon(0.0, 0.0, 1280.0, 720.0, 1.0)],
        )
        .placement;
        assert_eq!((p.x, p.y, p.width), (0.0, 0.0, 1280.0));
        assert_eq!(p.height, 720.0 - TITLE_BAR);
    }

    #[test]
    fn sizes_are_bounded() {
        let p = place(wanted(0.0, 0.0, 10.0, 10.0, Some(1.0)), None, None, &desk()).placement;
        assert_eq!((p.width, p.height), MIN_SIZE);
        let q = place(wanted(0.0, 0.0, 1e9, 1e9, Some(1.0)), None, None, &[]).placement;
        assert_eq!(
            (q.width, q.height),
            (MAX_SIDE, MAX_SIDE),
            "no display to fit: only the bounds apply"
        );
    }

    #[test]
    fn a_remembered_place_is_used_when_nothing_is_asked_and_clamped_when_its_display_is_gone() {
        let saved = |x: f64| remember(x, 200.0, 640.0, 480.0, 1.0);
        let on = place(None, saved(300.0), None, &desk());
        assert_eq!(on.source, Source::Remembered);
        assert_eq!(
            (on.placement.x, on.placement.y, on.placement.width),
            (300.0, 200.0, 640.0)
        );
        assert!(on.placement.remembered);
        // It was saved on a display that has been unplugged: it moves onto the nearest remaining one.
        let one = [mon(0.0, 0.0, 1920.0, 1040.0, 1.0)];
        let gone = place(None, saved(6000.0), None, &one).placement;
        assert!(gone.x + gone.width <= 1920.0 && gone.x >= 0.0, "{gone:?}");
        // A request wins over the memory; damaged memory is ignored.
        let asked = place(
            wanted(10.0, 10.0, 300.0, 300.0, Some(1.0)),
            saved(300.0),
            None,
            &desk(),
        );
        assert_eq!(asked.source, Source::Requested);
        let bad = SavedBounds {
            x: f64::NAN,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            scale: 1.0,
            maximized: false,
            fullscreen: false,
        };
        assert_eq!(
            place(None, Some(bad), None, &desk()).source,
            Source::Default
        );
    }

    #[test]
    fn by_default_it_opens_beside_the_host_on_the_host_s_display() {
        // The host is on the 4K display at 200%.
        let host = rect(2000.0, 100.0, 2400.0, 1600.0);
        let p = place(None, None, Some(host), &desk());
        assert_eq!(p.source, Source::BesideHost);
        let pl = p.placement;
        assert_eq!(pl.scale, 2.0);
        assert_eq!(pl.x, 2000.0 + 2400.0 - 560.0 * 2.0 - 40.0 * 2.0);
        assert_eq!(pl.y, 100.0 + 80.0 * 2.0);
        // A host near the display's right edge: the pop-out stays inside.
        let wide = rect(1000.0, 0.0, 900.0, 700.0);
        let q = place(None, None, Some(wide), &[mon(0.0, 0.0, 1000.0, 700.0, 1.0)]).placement;
        assert!(
            q.x + q.width <= 1000.0 && q.y + q.height + TITLE_BAR <= 700.0,
            "{q:?}"
        );
    }

    #[test]
    fn with_nothing_known_it_opens_at_a_default_place() {
        let p = place(None, None, None, &[]);
        assert_eq!(p.source, Source::Default);
        let pl = p.placement;
        assert_eq!(
            (pl.x, pl.y, pl.width, pl.height, pl.scale),
            (100.0, 100.0, 560.0, 420.0, 1.0)
        );
        // An unusable host rectangle and non-finite bounds are ignored.
        let bad_host = rect(f64::NAN, 0.0, 10.0, 10.0);
        assert_eq!(
            place(
                wanted(f64::NAN, 0.0, 10.0, 10.0, None),
                None,
                Some(bad_host),
                &desk()
            )
            .source,
            Source::Default
        );
        // Nonsense scales are ignored: the display decides.
        let nonsense = place(
            wanted(100.0, 100.0, 400.0, 300.0, Some(-3.0)),
            None,
            None,
            &desk(),
        );
        assert_eq!(nonsense.placement.scale, 1.0);
    }

    #[test]
    fn remembering_refuses_nonsense() {
        assert!(remember(0.0, 0.0, 0.0, 10.0, 1.0).is_none());
        assert!(remember(f64::INFINITY, 0.0, 10.0, 10.0, 1.0).is_none());
        assert!(remember(1.0, 2.0, 300.0, 200.0, 2.0).is_some());
    }
}
