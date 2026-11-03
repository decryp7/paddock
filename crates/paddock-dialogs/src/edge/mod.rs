//! The per-OS edges of the dialog size state machine (ADR 0043 §3 and §4): thin adapters that read what only the OS
//! knows (the window's DPI, its visible frame, its monitor's work area) in **physical px**, and turn it into the
//! logical [`Env`] that [`SizeState`](crate::size::SizeState) decides with. Every decision that can be made without a
//! window is a pure function here (compiled and tested on every OS); the Windows and Linux modules only call the OS
//! and feed these functions.
//!
//! * [`windows`] (Windows): `GetDpiForWindow`, `AdjustWindowRectExForDpi`, `DWMWA_EXTENDED_FRAME_BOUNDS`,
//!   `GetMonitorInfoW`, and a window subclass for `WM_SIZING`, `SC_SIZE`, `WM_GETDPISCALEDSIZE`, `WM_EXITSIZEMOVE`.
//! * [`linux`] (Linux): the frame as outer minus inner, the work area from the monitor, and the X11 or Wayland rules.

#[cfg(all(feature = "tauri", target_os = "linux"))]
pub mod linux;
#[cfg(all(feature = "tauri", windows))]
pub mod windows;

use crate::contract::Size;
use crate::size::{Env, physical};

/// Windows' base DPI: a scale of 1.0.
pub const BASE_DPI: u32 = 96;

/// Wayland has no work-area protocol: the work area is the monitor less this much, top and bottom, for panels (logical
/// px, ADR 0043 §3).
pub const WAYLAND_PANEL_MARGIN: f64 = 48.0;

/// A rectangle in physical px as the OS reports it (`RECT`: right and bottom are exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PhysRect {
    /// Width and height, never negative.
    pub fn size(self) -> (u32, u32) {
        (
            (i64::from(self.right) - i64::from(self.left)).max(0) as u32,
            (i64::from(self.bottom) - i64::from(self.top)).max(0) as u32,
        )
    }
}

/// What an edge read from the OS, physical px, for the window's current monitor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeMetrics {
    /// Device px per logical px (`dpi / 96` on Windows).
    pub scale: f64,
    /// The monitor's work area (size).
    pub work: (u32, u32),
    /// The visible frame: title bar and borders, without the invisible resize borders (outer minus inner).
    pub frame: (u32, u32),
}

impl EdgeMetrics {
    /// The logical [`Env`] of the state machine. A zero or non-finite scale is taken as 1.0 (an OS that failed to say).
    pub fn env(&self) -> Env {
        let s = if self.scale.is_finite() && self.scale > 0.0 { self.scale } else { 1.0 };
        let logical = |(w, h): (u32, u32)| Size {
            width: f64::from(w) / s,
            height: f64::from(h) / s,
        };
        Env {
            scale: s,
            work: logical(self.work),
            frame: logical(self.frame),
        }
    }
}

/// The scale factor of a DPI value (`0`, an API failure, counts as the base DPI).
pub fn scale_of_dpi(dpi: u32) -> f64 {
    f64::from(if dpi == 0 { BASE_DPI } else { dpi }) / f64::from(BASE_DPI)
}

/// `wParam` of a `WM_SYSCOMMAND` is a user resize when its command is `SC_SIZE` (the low four bits carry how the
/// command was started and are not part of it). `WM_ENTERSIZEMOVE` is deliberately not used: it is sent for a move too.
pub fn is_size_command(wparam: usize) -> bool {
    const SC_SIZE: usize = 0xF000;
    wparam & 0xFFF0 == SC_SIZE
}

/// The visible frame in physical px: the window's extended frame bounds (`DWMWA_EXTENDED_FRAME_BOUNDS`, which leave
/// out the invisible resize borders) less the client rectangle.
pub fn visible_frame(extended_bounds: PhysRect, client: (u32, u32)) -> (u32, u32) {
    let (w, h) = extended_bounds.size();
    (w.saturating_sub(client.0), h.saturating_sub(client.1))
}

/// The answer to `WM_GETDPISCALEDSIZE`: the outer size the window should have at the new DPI for the logical inner
/// size the state machine wants. `frame_at_new_dpi` is the **full** frame (invisible borders included) that
/// `AdjustWindowRectExForDpi` gives at that DPI, since `SetWindowPos` takes the outer rectangle.
pub fn dpi_scaled_outer(inner_logical: Size, new_dpi: u32, frame_at_new_dpi: (u32, u32)) -> (u32, u32) {
    let s = scale_of_dpi(new_dpi);
    (
        physical(inner_logical.width, s) + frame_at_new_dpi.0,
        physical(inner_logical.height, s) + frame_at_new_dpi.1,
    )
}

/// How a display server treats a dialog window's requests (Linux).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    /// A client cannot place a top-level window, and the compositor may configure it at any time.
    Wayland,
}

impl DisplayServer {
    /// From the environment the process runs in: `GDK_BACKEND` wins, then `XDG_SESSION_TYPE`, then
    /// `WAYLAND_DISPLAY`; anything else is X11.
    pub fn detect(gdk_backend: Option<&str>, session_type: Option<&str>, wayland_display: Option<&str>) -> Self {
        fn set(v: Option<&str>) -> Option<&str> {
            v.map(str::trim).filter(|v| !v.is_empty())
        }
        if let Some(b) = set(gdk_backend) {
            // `GDK_BACKEND=wayland,x11` is a preference list: the first is the one tried.
            return if b.split(',').next().is_some_and(|f| f.trim().eq_ignore_ascii_case("wayland")) {
                DisplayServer::Wayland
            } else {
                DisplayServer::X11
            };
        }
        if let Some(t) = set(session_type) {
            return if t.eq_ignore_ascii_case("wayland") { DisplayServer::Wayland } else { DisplayServer::X11 };
        }
        if set(wayland_display).is_some() { DisplayServer::Wayland } else { DisplayServer::X11 }
    }

    /// Whether `set_position` and `keep_on_monitor` do anything.
    pub fn can_place(self) -> bool {
        self == DisplayServer::X11
    }

    /// The work area (logical px) from the monitor's full size: Wayland has no work-area protocol, so panels are
    /// guessed with a margin top and bottom; X11 reports the real one.
    pub fn work_area(self, monitor_or_work: Size) -> Size {
        match self {
            DisplayServer::X11 => monitor_or_work,
            DisplayServer::Wayland => Size {
                width: monitor_or_work.width,
                height: (monitor_or_work.height - 2.0 * WAYLAND_PANEL_MARGIN).max(1.0),
            },
        }
    }
}

/// The metrics of a window from what a Linux toolkit reports, physical px: its outer and inner size (the frame is their
/// difference, `0` until the window is mapped and the window manager has framed it, so the glue reads it again on each
/// fit), its monitor's work area and its scale. On Wayland the work area is guessed (see [`DisplayServer::work_area`]).
pub fn metrics_for(
    server: DisplayServer,
    scale: f64,
    work: (u32, u32),
    outer: (u32, u32),
    inner: (u32, u32),
) -> EdgeMetrics {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let guessed = server.work_area(Size { width: f64::from(work.0) / s, height: f64::from(work.1) / s });
    EdgeMetrics {
        scale: s,
        work: (physical(guessed.width, s), physical(guessed.height, s)),
        frame: (outer.0.saturating_sub(inner.0), outer.1.saturating_sub(inner.1)),
    }
}

/// What a window system did with a size or constraint that was asked for (physical px, read back after the request's
/// `Resized`). A compositor may ignore a constraint: that is logged and the window stays usable, never an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The read-back equals the request (within one px a side).
    Honoured,
    /// Another size, not larger than the request: clamped by a maximum, a work area or a tile.
    Clamped,
    /// Another size, larger than the request: the window system grew it (a minimum the platform enforces).
    Enlarged,
}

/// Compares a request with the read-back size.
pub fn outcome(requested: (u32, u32), read_back: (u32, u32)) -> Outcome {
    let near = requested.0.abs_diff(read_back.0) <= 1 && requested.1.abs_diff(read_back.1) <= 1;
    if near {
        Outcome::Honoured
    } else if read_back.0 > requested.0 || read_back.1 > requested.1 {
        Outcome::Enlarged
    } else {
        Outcome::Clamped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: f64, height: f64) -> Size {
        Size { width, height }
    }

    #[test]
    fn dpi_is_a_scale_and_zero_is_the_base() {
        assert_eq!(scale_of_dpi(96), 1.0);
        assert_eq!(scale_of_dpi(120), 1.25);
        assert_eq!(scale_of_dpi(144), 1.5);
        assert_eq!(scale_of_dpi(0), 1.0);
    }

    #[test]
    fn only_sc_size_is_a_user_resize_whatever_started_it() {
        // SC_SIZE with the direction bits the OS adds for a border drag (0xF001..=0xF008) and the keyboard (0xF000).
        for w in 0xF000..=0xF008usize {
            assert!(is_size_command(w), "{w:#x}");
        }
        assert!(!is_size_command(0xF010), "SC_MOVE is a move");
        assert!(!is_size_command(0xF020), "SC_MINIMIZE");
        assert!(!is_size_command(0xF030), "SC_MAXIMIZE");
        assert!(!is_size_command(0), "an application's own command");
    }

    #[test]
    fn visible_frame_leaves_out_the_invisible_borders() {
        // A 1000x700 client at 150 %: DWM reports the visible outer rectangle; the invisible 8 px borders are not in it.
        let bounds = PhysRect { left: 100, top: 100, right: 1100 + 2, bottom: 800 + 31 + 2 };
        assert_eq!(visible_frame(bounds, (1000, 700)), (2, 33));
        // A client larger than the bounds (a stale read) is no frame, not an underflow.
        assert_eq!(visible_frame(bounds, (5000, 5000)), (0, 0));
    }

    #[test]
    fn the_scaled_size_is_the_ceiled_client_plus_the_full_frame_at_the_new_dpi() {
        // Inner 410.2 x 300.1 logical at 125 %: 513 x 376 physical, plus a 16 x 39 frame.
        assert_eq!(dpi_scaled_outer(size(410.2, 300.1), 120, (16, 39)), (513 + 16, 376 + 39));
        // At 100 % the same inner needs 411 x 301.
        assert_eq!(dpi_scaled_outer(size(410.2, 300.1), 96, (16, 39)), (411 + 16, 301 + 39));
    }

    #[test]
    fn metrics_become_a_logical_env_and_a_bad_scale_is_one() {
        let m = EdgeMetrics { scale: 1.5, work: (2880, 1560), frame: (24, 60) };
        let e = m.env();
        assert_eq!(e.scale, 1.5);
        assert_eq!(e.work, size(1920.0, 1040.0));
        assert_eq!(e.frame, size(16.0, 40.0));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(EdgeMetrics { scale: bad, ..m }.env().scale, 1.0);
        }
    }

    #[test]
    fn rect_size_never_goes_negative() {
        assert_eq!(PhysRect { left: 10, top: 10, right: 5, bottom: 30 }.size(), (0, 20));
    }

    #[test]
    fn the_display_server_comes_from_gdk_then_the_session_then_the_socket() {
        use DisplayServer::*;
        assert_eq!(DisplayServer::detect(None, None, None), X11);
        assert_eq!(DisplayServer::detect(None, Some("wayland"), None), Wayland);
        assert_eq!(DisplayServer::detect(None, Some("x11"), Some("wayland-0")), X11, "the session type is the stronger word");
        assert_eq!(DisplayServer::detect(None, None, Some("wayland-0")), Wayland);
        assert_eq!(DisplayServer::detect(None, Some(""), Some("")), X11);
        assert_eq!(DisplayServer::detect(Some("x11"), Some("wayland"), Some("wayland-0")), X11, "GDK_BACKEND wins");
        assert_eq!(DisplayServer::detect(Some("wayland,x11"), None, None), Wayland, "the first of a list");
        assert_eq!(DisplayServer::detect(Some("x11,wayland"), Some("wayland"), None), X11);
    }

    #[test]
    fn wayland_cannot_place_and_guesses_the_panels() {
        assert!(DisplayServer::X11.can_place());
        assert!(!DisplayServer::Wayland.can_place());
        let monitor = size(1920.0, 1080.0);
        assert_eq!(DisplayServer::X11.work_area(monitor), monitor);
        assert_eq!(DisplayServer::Wayland.work_area(monitor), size(1920.0, 984.0));
        assert_eq!(DisplayServer::Wayland.work_area(size(100.0, 50.0)).height, 1.0, "never zero");
    }

    #[test]
    fn linux_metrics_take_the_frame_from_outer_minus_inner_and_guess_wayland_panels() {
        let x11 = metrics_for(DisplayServer::X11, 2.0, (3840, 2160), (1030, 760), (1000, 700));
        assert_eq!(x11.frame, (30, 60));
        assert_eq!(x11.work, (3840, 2160));
        assert_eq!(x11.scale, 2.0);
        let wl = metrics_for(DisplayServer::Wayland, 2.0, (3840, 2160), (1000, 700), (1000, 700));
        assert_eq!(wl.frame, (0, 0), "an unframed window has none until it is mapped");
        assert_eq!(wl.work, (3840, 2160 - 192), "48 logical px top and bottom at scale 2");
        // An inner larger than the outer (a toolkit reporting before it is framed) is no frame, not an underflow.
        assert_eq!(metrics_for(DisplayServer::X11, 1.0, (800, 600), (500, 400), (510, 410)).frame, (0, 0));
    }

    #[test]
    fn a_request_is_honoured_clamped_or_enlarged() {
        assert_eq!(outcome((500, 400), (501, 399)), Outcome::Honoured);
        assert_eq!(outcome((500, 900), (500, 800)), Outcome::Clamped);
        assert_eq!(outcome((300, 200), (360, 200)), Outcome::Enlarged);
    }
}
