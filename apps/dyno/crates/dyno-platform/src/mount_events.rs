//! The OS's word that the mounts changed (a volume came or went), one adapter per OS, each on the calling thread:
//! macOS DiskArbitration callbacks on a run loop, Windows `WM_DEVICECHANGE` on a hidden window, Linux a poll on
//! `/proc/self/mountinfo` for `POLLPRI`. Each event is only a hint: the caller lists the mounts again. Events are folded to at
//! most one per second, and the arrivals reported while the adapter starts (macOS lists every present disk once) are dropped.

#![allow(unsafe_code)]
/// Calls `on_event("mount")` whenever the OS says the mounts changed, until `stopped()` is true. Returns at once on an OS with
/// no adapter or when the adapter cannot start (said in the log); the caller's periodic relist is the fallback then.
pub fn watch(stopped: &(dyn Fn() -> bool + Sync), on_event: &(dyn Fn(&'static str) + Sync)) {
    #[cfg(target_os = "macos")]
    macos::watch(stopped, on_event);
    #[cfg(target_os = "linux")]
    linux::watch(stopped, on_event);
    #[cfg(windows)]
    windows_impl::watch(stopped, on_event);
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (stopped, on_event);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::io::AsRawFd;

    pub fn watch(stopped: &(dyn Fn() -> bool + Sync), on_event: &(dyn Fn(&'static str) + Sync)) {
        let file = match std::fs::File::open("/proc/self/mountinfo") {
            Ok(f) => f,
            Err(e) => {
                log::warn!("mount events: cannot open mountinfo: {e}; mounts are listed on the timer only");
                return;
            }
        };
        let mut fds = [libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLPRI | libc::POLLERR,
            revents: 0,
        }];
        let mut first = true;
        while !stopped() {
            // SAFETY: `fds` is one valid pollfd; the timeout bounds the wait so `stopped` is looked at once a second.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 1, 1000) };
            if n > 0 {
                // The first return reports the state as it was opened on some kernels; it is not a change.
                if !std::mem::take(&mut first) {
                    on_event("mount");
                }
                // Reading the table re-arms the notification.
                let _ = std::fs::read("/proc/self/mountinfo");
            } else if n < 0 {
                log::warn!(
                    "mount events: poll failed: {}; mounts are listed on the timer only",
                    std::io::Error::last_os_error()
                );
                return;
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};

    type Ref = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: Ref;
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: u8) -> i32;
        fn CFRelease(cf: Ref);
    }

    #[link(name = "DiskArbitration", kind = "framework")]
    extern "C" {
        fn DASessionCreate(alloc: Ref) -> Ref;
        fn DASessionScheduleWithRunLoop(session: Ref, run_loop: Ref, mode: Ref);
        fn DARegisterDiskAppearedCallback(session: Ref, matching: Ref, callback: extern "C" fn(Ref, *mut c_void), context: *mut c_void);
        fn DARegisterDiskDisappearedCallback(session: Ref, matching: Ref, callback: extern "C" fn(Ref, *mut c_void), context: *mut c_void);
        fn DAUnregisterCallback(session: Ref, callback: *const c_void, context: *mut c_void);
    }

    extern "C" fn changed(_disk: Ref, context: *mut c_void) {
        // SAFETY: `context` is the address of the `AtomicBool` that `watch` keeps alive until the callbacks are unregistered.
        let flag = unsafe { &*(context as *const AtomicBool) };
        flag.store(true, Ordering::SeqCst);
    }

    pub fn watch(stopped: &(dyn Fn() -> bool + Sync), on_event: &(dyn Fn(&'static str) + Sync)) {
        let flag = AtomicBool::new(false);
        let context = &flag as *const AtomicBool as *mut c_void;
        // SAFETY: plain DiskArbitration calls on this thread; the session is released and the callbacks unregistered before
        // `flag` goes out of scope.
        unsafe {
            let session = DASessionCreate(std::ptr::null());
            if session.is_null() {
                log::warn!("mount events: no DiskArbitration session; mounts are listed on the timer only");
                return;
            }
            DARegisterDiskAppearedCallback(session, std::ptr::null(), changed, context);
            DARegisterDiskDisappearedCallback(session, std::ptr::null(), changed, context);
            DASessionScheduleWithRunLoop(session, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
            let mut first = true;
            while !stopped() {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, 0);
                let fired = flag.swap(false, Ordering::SeqCst);
                // The first pass reports every disk that is already there.
                if fired && !std::mem::take(&mut first) {
                    on_event("mount");
                }
                first = false;
            }
            DAUnregisterCallback(session, changed as *const c_void, context);
            DAUnregisterCallback(session, changed as *const c_void, context);
            CFRelease(session);
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW, RegisterClassW, TranslateMessage, MSG, PM_REMOVE,
        WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    };

    const WM_DEVICECHANGE: u32 = 0x0219;
    const DBT_DEVNODES_CHANGED: usize = 0x0007;
    const DBT_DEVICEARRIVAL: usize = 0x8000;
    const DBT_DEVICEREMOVECOMPLETE: usize = 0x8004;

    static CHANGED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == WM_DEVICECHANGE && matches!(wparam.0, DBT_DEVNODES_CHANGED | DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE) {
            CHANGED.store(true, Ordering::SeqCst);
        }
        // SAFETY: the default handling of a message this window does not care about.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    pub fn watch(stopped: &(dyn Fn() -> bool + Sync), on_event: &(dyn Fn(&'static str) + Sync)) {
        // SAFETY: an ordinary hidden top-level window (a message-only window gets no device broadcasts) created, pumped and
        // destroyed on this one thread.
        unsafe {
            let Ok(instance) = GetModuleHandleW(None) else {
                log::warn!("mount events: no module handle; mounts are listed on the timer only");
                return;
            };
            let class = w!("DynoMountEvents");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            // A second registration (an agent restarted in this process) fails harmlessly: the class is already there.
            RegisterClassW(&wc);
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Dyno mount events"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            ) else {
                log::warn!("mount events: no window for device changes; mounts are listed on the timer only");
                return;
            };
            let mut msg = MSG::default();
            while !stopped() {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if CHANGED.swap(false, Ordering::SeqCst) {
                    on_event("mount");
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let _ = DestroyWindow(hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// An adapter that is told to stop before it starts returns without a single event and leaves nothing running (it only reads).
    #[test]
    fn a_watcher_that_is_stopped_at_once_returns_without_events() {
        let events = AtomicUsize::new(0);
        watch(&|| true, &|_| {
            events.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(events.load(Ordering::SeqCst), 0);
    }
}
