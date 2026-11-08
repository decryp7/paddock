//! The Windows edge of the dialog size state machine (ADR 0043 §3, §4 and D4, D5): physical px for the window's own
//! monitor and DPI, and the messages only the window procedure sees.
//!
//! * **Scale and frame.** `GetDpiForWindow` for the scale; the **full** frame for `SetWindowPos` from
//!   `AdjustWindowRectExForDpi` with the window's own styles at a given DPI; the **visible** frame (what the clamp to the
//!   work area uses) from `DWMWA_EXTENDED_FRAME_BOUNDS` less the client rectangle, because the full frame includes the
//!   invisible resize borders of Windows 10 and 11. The work area is `GetMonitorInfoW`'s `rcWork`.
//! * **User resize.** A `WM_SIZING` (sent only to a window the user is resizing) or a `WM_SYSCOMMAND` with `SC_SIZE`
//!   (keyboard size). Not `WM_ENTERSIZEMOVE`, which a move sends too.
//! * **DPI change.** `WM_GETDPISCALEDSIZE` is answered with the outer size the state machine wants at the new DPI, which
//!   becomes the suggested rectangle of `WM_DPICHANGED`; the posted refit then corrects Windows 10, where tao computes
//!   its own rectangle (D5).
//! * **Snap.** `WM_WINDOWPOSCHANGED` while `IsWindowArranged`: an Aero snap sends neither `WM_SIZING` nor `SC_SIZE`, and
//!   sends no size change at all when the kind's floor is wider than the snapped half (a viewer at 150 %).
//! * **Move end.** `WM_EXITSIZEMOVE` tells the state machine to check the clamp on the monitor the window ended on.
//!
//! The subclass is chained after tao's own (installed later, so it is called first and passes everything on). It must be
//! installed on the thread that owns the window, the main thread.

use super::{
    EdgeMetrics, PhysRect, dpi_scaled_outer, is_size_command, scale_of_dpi, visible_frame,
};
use crate::contract::Size;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GWL_STYLE, GetClientRect, GetWindowLongPtrW, IsWindowArranged, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_EXITSIZEMOVE, WM_GETDPISCALEDSIZE, WM_NCDESTROY, WM_SIZING, WM_SYSCOMMAND,
    WM_WINDOWPOSCHANGED,
};

/// The subclass id of this edge on a window ("PDLG").
const SUBCLASS_ID: usize = 0x5044_4C47;

/// What the window procedure tells the state machine's glue. Called on the window's thread inside the message, so an
/// implementation only posts work (`run_on_main_thread`) and answers `inner_for_dpi` from state it already holds.
pub trait WindowEvents {
    /// The user started to resize the window (`WM_SIZING`, `SC_SIZE`).
    fn user_resize_started(&self);
    /// A move or resize ended (`WM_EXITSIZEMOVE`).
    fn size_move_ended(&self);
    /// The window is arranged by the OS (an Aero snap: `WM_WINDOWPOSCHANGED` with `IsWindowArranged`), whether or not the
    /// snap changed its size.
    fn snapped(&self);
    /// The logical inner size the window should have at `dpi`, for `WM_GETDPISCALEDSIZE`; `None` leaves the OS's size.
    fn inner_for_dpi(&self, dpi: u32) -> Option<Size>;
}

/// Why an edge call failed (logged by the glue with the next step).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeError {
    /// The window has no native handle yet or any more.
    NoWindow,
    /// A Win32 call failed: its name and the error.
    Win32(&'static str, String),
}

impl std::fmt::Display for EdgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeError::NoWindow => f.write_str("the dialog window has no native handle"),
            EdgeError::Win32(call, e) => write!(f, "{call} failed: {e}"),
        }
    }
}

impl std::error::Error for EdgeError {}

/// Where the visible frame came from, for the fit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSource {
    /// `DWMWA_EXTENDED_FRAME_BOUNDS`.
    Dwm,
    /// DWM said nothing usable (composition off, window not yet shown): the full frame, which is never too small.
    FullFrame,
}

/// What the edge read for one window at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub dpi: u32,
    pub metrics: EdgeMetrics,
    /// The full frame (invisible resize borders included), physical px, at `dpi`.
    pub full_frame: (u32, u32),
    pub frame_source: FrameSource,
}

/// The window's own DPI (`0` when the call fails: the base DPI is then assumed).
pub fn dpi_of(hwnd: HWND) -> u32 {
    // SAFETY: a plain query; an invalid handle returns 0.
    unsafe { GetDpiForWindow(hwnd) }
}

/// The full frame of the window's own styles at `dpi`: what `SetWindowPos` adds to a client size.
pub fn full_frame_at(hwnd: HWND, dpi: u32) -> Result<(u32, u32), EdgeError> {
    // SAFETY: queries of the window's styles and a pure rectangle calculation.
    unsafe {
        let style = WINDOW_STYLE(GetWindowLongPtrW(hwnd, GWL_STYLE) as u32);
        let ex = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let mut r = RECT::default();
        AdjustWindowRectExForDpi(&mut r, style, false, ex, dpi.max(1))
            .map_err(|e| EdgeError::Win32("AdjustWindowRectExForDpi", e.to_string()))?;
        Ok(PhysRect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
        .size())
    }
}

fn client_size(hwnd: HWND) -> Result<(u32, u32), EdgeError> {
    let mut r = RECT::default();
    // SAFETY: `r` is a valid out parameter.
    unsafe { GetClientRect(hwnd, &mut r) }
        .map_err(|e| EdgeError::Win32("GetClientRect", e.to_string()))?;
    Ok(PhysRect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
    .size())
}

/// The work area of the monitor the window is on (nearest, if it is on none), physical px.
pub fn work_area_of(hwnd: HWND) -> Result<(u32, u32), EdgeError> {
    // SAFETY: `info` has its size set and outlives the call.
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(EdgeError::Win32(
                "GetMonitorInfoW",
                "no monitor information".into(),
            ));
        }
        let r = info.rcWork;
        Ok(PhysRect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
        .size())
    }
}

fn extended_frame_bounds(hwnd: HWND) -> Option<PhysRect> {
    let mut r = RECT::default();
    // SAFETY: `r` is a RECT and the size passed is its size.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut r).cast::<c_void>(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()?;
    let bounds = PhysRect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    };
    (bounds.size() != (0, 0)).then_some(bounds)
}

/// Reads the DPI, both frames and the work area of the window as it is now.
pub fn read(hwnd: HWND) -> Result<Reading, EdgeError> {
    let dpi = dpi_of(hwnd);
    let full = full_frame_at(hwnd, if dpi == 0 { super::BASE_DPI } else { dpi })?;
    let client = client_size(hwnd)?;
    let (frame, source) = match extended_frame_bounds(hwnd) {
        Some(b) => (visible_frame(b, client), FrameSource::Dwm),
        None => (full, FrameSource::FullFrame),
    };
    Ok(Reading {
        dpi,
        metrics: EdgeMetrics {
            scale: scale_of_dpi(dpi),
            work: work_area_of(hwnd)?,
            frame,
        },
        full_frame: full,
        frame_source: source,
    })
}

/// The native handle of a Tauri window.
pub fn hwnd_of<R: tauri::Runtime>(window: &tauri::Window<R>) -> Result<HWND, EdgeError> {
    window.hwnd().map_err(|_| EdgeError::NoWindow)
}

/// Installs the dialog subclass on `hwnd`; `events` lives until the window is destroyed. Call on the window's thread.
pub fn install(hwnd: HWND, events: Box<dyn WindowEvents>) -> Result<(), EdgeError> {
    let data = Box::into_raw(Box::new(events)) as usize;
    // SAFETY: `data` is a leaked `Box<Box<dyn WindowEvents>>`, freed by `subclass_proc` on `WM_NCDESTROY`.
    let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, data) };
    if ok.as_bool() {
        Ok(())
    } else {
        // SAFETY: the subclass was not installed, so nothing else owns `data`.
        drop(unsafe { Box::from_raw(data as *mut Box<dyn WindowEvents>) });
        Err(EdgeError::Win32(
            "SetWindowSubclass",
            "the call failed".into(),
        ))
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: `data` is the pointer `install` leaked; it stays valid until `WM_NCDESTROY` below.
    let events = unsafe { &*(data as *const Box<dyn WindowEvents>) };
    // A panic in the glue must never unwind into the window procedure: the message falls through to the next one.
    let handled = catch_unwind(AssertUnwindSafe(|| match msg {
        WM_SIZING => {
            events.user_resize_started();
            None
        }
        WM_SYSCOMMAND if is_size_command(wparam.0) => {
            events.user_resize_started();
            None
        }
        WM_EXITSIZEMOVE => {
            events.size_move_ended();
            None
        }
        WM_WINDOWPOSCHANGED => {
            // SAFETY: a plain query of the window handle the message came for.
            if unsafe { IsWindowArranged(hwnd) }.as_bool() {
                events.snapped();
            }
            None
        }
        WM_GETDPISCALEDSIZE => {
            let dpi = wparam.0 as u32;
            let inner = events.inner_for_dpi(dpi)?;
            let frame = full_frame_at(hwnd, dpi).ok()?;
            let (w, h) = dpi_scaled_outer(inner, dpi, frame);
            // SAFETY: for this message `lparam` points to the SIZE the OS wants filled.
            unsafe {
                *(lparam.0 as *mut SIZE) = SIZE {
                    cx: w as i32,
                    cy: h as i32,
                }
            };
            Some(LRESULT(1))
        }
        _ => None,
    }));
    if let Ok(Some(answer)) = handled {
        return answer;
    }
    if msg == WM_NCDESTROY {
        // SAFETY: the last message of the window: unhook, then free what `install` leaked.
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut Box<dyn WindowEvents>));
        }
        // SAFETY: the subclass chain continues without this link; `data` is not touched again.
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: passes the message on to the next subclass (tao's) and the window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    //! These create a message-only window (never on the desktop) and send it the messages the OS would.
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE, SendMessageW, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    #[derive(Default)]
    struct Log {
        resize: u32,
        ended: u32,
        snapped: u32,
    }

    struct Sink {
        log: Rc<RefCell<Log>>,
        inner: Option<Size>,
    }

    impl WindowEvents for Sink {
        fn user_resize_started(&self) {
            self.log.borrow_mut().resize += 1;
        }
        fn size_move_ended(&self) {
            self.log.borrow_mut().ended += 1;
        }
        fn snapped(&self) {
            self.log.borrow_mut().snapped += 1;
        }
        fn inner_for_dpi(&self, _dpi: u32) -> Option<Size> {
            self.inner
        }
    }

    fn window() -> HWND {
        // SAFETY: a message-only window of a built-in class, destroyed by the test.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("paddock-dialogs edge test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                400,
                300,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .expect("a message-only window")
    }

    fn sink(inner: Option<Size>) -> (Box<dyn WindowEvents>, Rc<RefCell<Log>>) {
        let log = Rc::new(RefCell::new(Log::default()));
        (
            Box::new(Sink {
                log: log.clone(),
                inner,
            }),
            log,
        )
    }

    #[test]
    fn the_full_frame_grows_with_the_dpi_it_is_asked_at() {
        let h = window();
        let at = |dpi| full_frame_at(h, dpi).unwrap();
        let (f96, f120, f144) = (at(96), at(120), at(144));
        assert!(
            f96.0 > 0 && f96.1 > f96.0,
            "a title bar makes it taller than wide: {f96:?}"
        );
        assert!(
            f120.1 > f96.1 && f144.1 > f120.1,
            "{f96:?} {f120:?} {f144:?}"
        );
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn a_reading_has_a_scale_a_work_area_and_a_frame_not_larger_than_the_full_one() {
        let h = window();
        let r = read(h).unwrap();
        assert!(r.metrics.scale >= 1.0);
        assert!(r.metrics.work.0 > 0 && r.metrics.work.1 > 0);
        assert!(
            r.metrics.frame.0 <= r.full_frame.0 && r.metrics.frame.1 <= r.full_frame.1,
            "{r:?}"
        );
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn wm_sizing_is_a_user_resize_and_the_end_of_a_move_is_reported() {
        // WM_SYSCOMMAND is not sent here: its default handling (SC_SIZE, SC_MOVE) starts a modal loop that never
        // returns on a message-only window (found in the VM run); `is_size_command` is tested in `edge`.
        let h = window();
        let (events, log) = sink(None);
        install(h, events).unwrap();
        unsafe {
            let mut r = RECT::default();
            SendMessageW(
                h,
                WM_SIZING,
                Some(WPARAM(1)),
                Some(LPARAM((&raw mut r) as isize)),
            );
        }
        assert_eq!(log.borrow().resize, 1);
        unsafe {
            SendMessageW(h, WM_EXITSIZEMOVE, None, None);
        }
        assert_eq!(log.borrow().ended, 1);
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn the_dpi_scaled_size_is_the_wanted_inner_at_the_new_dpi_plus_the_frame_there() {
        let h = window();
        let (events, _) = sink(Some(Size {
            width: 410.2,
            height: 300.1,
        }));
        install(h, events).unwrap();
        let mut size = SIZE::default();
        let answer = unsafe {
            SendMessageW(
                h,
                WM_GETDPISCALEDSIZE,
                Some(WPARAM(144)),
                Some(LPARAM((&raw mut size) as isize)),
            )
        };
        assert_eq!(answer.0, 1, "answered");
        let frame = full_frame_at(h, 144).unwrap();
        assert_eq!(
            (size.cx, size.cy),
            (616 + frame.0 as i32, 451 + frame.1 as i32)
        );
        unsafe { DestroyWindow(h).unwrap() };
    }

    #[test]
    fn without_a_wanted_size_the_os_keeps_its_own() {
        let h = window();
        let (events, _) = sink(None);
        install(h, events).unwrap();
        let mut size = SIZE { cx: 7, cy: 9 };
        unsafe {
            SendMessageW(
                h,
                WM_GETDPISCALEDSIZE,
                Some(WPARAM(144)),
                Some(LPARAM((&raw mut size) as isize)),
            )
        };
        assert_eq!((size.cx, size.cy), (7, 9), "untouched");
        unsafe { DestroyWindow(h).unwrap() };
    }
}
