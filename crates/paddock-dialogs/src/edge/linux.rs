//! The Linux edge of the dialog size state machine (ADR 0043 §3, §4 and D4, D6), through tao, GTK and wry. It degrades
//! honestly: what an X11 window manager or a Wayland compositor may ignore is logged, never an error.
//!
//! * **Scale.** The window's scale factor (GDK's, integer or fractional as the toolkit reports it) from the monitor it
//!   is on.
//! * **Frame.** GTK gives no frame API: outer minus inner after the window is mapped, read again on each fit
//!   ([`super::metrics_for`]). Before the map it is `0`; the state machine corrects it when the frame arrives
//!   (`SizeState::on_moved` with the new [`Env`](crate::size::Env)).
//! * **X11.** The work area is the monitor's. No OS signal tells a user resize from ours, so an unexplained `Resized`
//!   after every pending request has arrived is a user resize ([`INFER_USER_RESIZE`]).
//! * **Wayland.** A client cannot place a top-level window: `set_position` and `keep_on_monitor` have no effect
//!   ([`can_place`]) and the dialog relies on the compositor. There is no work-area protocol, so the work area is the
//!   monitor less a margin for panels. A compositor may configure the window at any time (tiling, maximise, a size it
//!   prefers) and may ignore a minimum or maximum; such a configure is a user resize (D6), and [`super::outcome`] says
//!   what became of a request for the log. A minimum is set on the toolkit (`set_min_inner_size`), which both server
//!   kinds honour as a hint; the maximum is the clamp in the plan, never a window-system maximum a tiler would ignore.

use super::{DisplayServer, EdgeMetrics, metrics_for};
use tauri::{Runtime, Window};

/// Linux gives no resize signal: the state machine infers a user resize from an unexplained `Resized`.
pub const INFER_USER_RESIZE: bool = true;

/// The display server this process runs under, from its environment.
pub fn display_server() -> DisplayServer {
    let var = |k: &str| std::env::var(k).ok();
    DisplayServer::detect(
        var("GDK_BACKEND").as_deref(),
        var("XDG_SESSION_TYPE").as_deref(),
        var("WAYLAND_DISPLAY").as_deref(),
    )
}

/// Whether moving a window has any effect here.
pub fn can_place() -> bool {
    display_server().can_place()
}

/// Reads the scale, frame and work area of `window` as they are now; `None` when the window has no monitor yet.
pub fn read<R: Runtime>(window: &Window<R>) -> Option<EdgeMetrics> {
    let monitor = window.current_monitor().ok().flatten()?;
    let work = monitor.work_area().size;
    let outer = window.outer_size().ok()?;
    let inner = window.inner_size().ok()?;
    Some(metrics_for(
        display_server(),
        window
            .scale_factor()
            .ok()
            .unwrap_or_else(|| monitor.scale_factor()),
        (work.width, work.height),
        (outer.width, outer.height),
        (inner.width, inner.height),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_infers_the_user_from_an_unexplained_resize() {
        const { assert!(INFER_USER_RESIZE) };
    }

    #[test]
    fn the_server_of_this_process_is_one_of_the_two() {
        let s = display_server();
        assert_eq!(s.can_place(), can_place());
        assert!(matches!(s, DisplayServer::X11 | DisplayServer::Wayland));
    }
}
