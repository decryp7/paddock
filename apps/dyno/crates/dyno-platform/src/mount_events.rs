//! The OS's word that the mounts changed (a volume came or went), one adapter per OS, each on the calling thread:
//! macOS DiskArbitration callbacks on a run loop, Windows `WM_DEVICECHANGE` on a hidden window, Linux a poll on
//! `/proc/self/mountinfo` for `POLLPRI`. Each event is only a hint: the caller lists the mounts again. Events are folded to at
//! most one per second. macOS reports a disk's volume path changing (the real mount and unmount signal; "disk appeared" fires before
//! the volume is mounted and an unmount without an eject fires no disappearance), and the arrivals reported while the adapter
//! starts (every present disk once) are dropped. Linux compares the mountinfo content, so no change is missed or invented.

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

/// Tells whether the mount table changed: remembers a hash of its text and compares the next reading. The first reading only
/// sets the baseline. A wake-up of `poll` is a hint, the content decides (current kernels signal a real change on the first
/// `POLLPRI` after open, older ones also at open).
#[derive(Debug, Default)]
pub struct TableWatch(Option<u64>);

impl TableWatch {
    /// Reads `table` (the content of `/proc/self/mountinfo`); true when it differs from the one read before.
    pub fn changed(&mut self, table: &[u8]) -> bool {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        table.hash(&mut h);
        let now = h.finish();
        self.0.replace(now).is_some_and(|before| before != now)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::TableWatch;
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
        let mut table = TableWatch::default();
        table.changed(&std::fs::read("/proc/self/mountinfo").unwrap_or_default());
        while !stopped() {
            // SAFETY: `fds` is one valid pollfd; the timeout bounds the wait so `stopped` is looked at once a second.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 1, 1000) };
            if n > 0 {
                // Reading the table re-arms the notification; its content says whether anything changed.
                if table.changed(&std::fs::read("/proc/self/mountinfo").unwrap_or_default()) {
                    on_event("mount");
                }
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

/// How long the macOS loop waits when `CFRunLoopRunInMode` returned at once for want of anything to run (`kCFRunLoopRunFinished`),
/// instead of spinning; `None` for the ordinary returns (timed out, handled a source, stopped).
pub fn run_loop_pause(result: i32) -> Option<std::time::Duration> {
    const FINISHED: i32 = 1;
    (result == FINISHED).then_some(std::time::Duration::from_millis(500))
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};

    type Ref = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: Ref;
        static kCFTypeArrayCallBacks: u8;
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: u8) -> i32;
        fn CFArrayCreate(alloc: Ref, values: *const Ref, count: isize, callbacks: *const u8) -> Ref;
        fn CFRelease(cf: Ref);
    }

    #[link(name = "DiskArbitration", kind = "framework")]
    extern "C" {
        static kDADiskDescriptionVolumePathKey: Ref;
        fn DASessionCreate(alloc: Ref) -> Ref;
        fn DASessionScheduleWithRunLoop(session: Ref, run_loop: Ref, mode: Ref);
        fn DASessionUnscheduleFromRunLoop(session: Ref, run_loop: Ref, mode: Ref);
        fn DARegisterDiskAppearedCallback(session: Ref, matching: Ref, callback: extern "C" fn(Ref, *mut c_void), context: *mut c_void);
        fn DARegisterDiskDisappearedCallback(session: Ref, matching: Ref, callback: extern "C" fn(Ref, *mut c_void), context: *mut c_void);
        fn DARegisterDiskDescriptionChangedCallback(
            session: Ref,
            matching: Ref,
            watch: Ref,
            callback: extern "C" fn(Ref, Ref, *mut c_void),
            context: *mut c_void,
        );
        fn DAUnregisterCallback(session: Ref, callback: *const c_void, context: *mut c_void);
    }

    fn raise(context: *mut c_void) {
        // SAFETY: `context` is the address of the `AtomicBool` that `watch` keeps alive until the callbacks are unregistered.
        let flag = unsafe { &*(context as *const AtomicBool) };
        flag.store(true, Ordering::SeqCst);
    }

    extern "C" fn changed(_disk: Ref, context: *mut c_void) {
        raise(context);
    }

    /// The volume path of a disk changed: it was mounted or unmounted (a disk that appears is not mounted yet, and an unmount
    /// without an eject makes no disk disappear).
    extern "C" fn path_changed(_disk: Ref, _keys: Ref, context: *mut c_void) {
        raise(context);
    }

    pub fn watch(stopped: &(dyn Fn() -> bool + Sync), on_event: &(dyn Fn(&'static str) + Sync)) {
        let flag = AtomicBool::new(false);
        let context = &flag as *const AtomicBool as *mut c_void;
        // SAFETY: plain DiskArbitration calls on this thread; the callbacks are unregistered, the session unscheduled and released,
        // and the key array released before `flag` goes out of scope.
        unsafe {
            let session = DASessionCreate(std::ptr::null());
            if session.is_null() {
                log::warn!("mount events: no DiskArbitration session; mounts are listed on the timer only");
                return;
            }
            let keys = [kDADiskDescriptionVolumePathKey];
            let watch_keys = CFArrayCreate(std::ptr::null(), keys.as_ptr(), 1, &kCFTypeArrayCallBacks);
            DARegisterDiskAppearedCallback(session, std::ptr::null(), changed, context);
            DARegisterDiskDisappearedCallback(session, std::ptr::null(), changed, context);
            if !watch_keys.is_null() {
                DARegisterDiskDescriptionChangedCallback(session, std::ptr::null(), watch_keys, path_changed, context);
            }
            let run_loop = CFRunLoopGetCurrent();
            DASessionScheduleWithRunLoop(session, run_loop, kCFRunLoopDefaultMode);
            let mut first = true;
            while !stopped() {
                let result = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 1.0, 0);
                let fired = flag.swap(false, Ordering::SeqCst);
                // The first pass reports every disk that is already there.
                if fired && !std::mem::take(&mut first) {
                    on_event("mount");
                }
                first = false;
                if let Some(pause) = super::run_loop_pause(result) {
                    std::thread::sleep(pause);
                }
            }
            DAUnregisterCallback(session, changed as *const c_void, context);
            DAUnregisterCallback(session, changed as *const c_void, context);
            DAUnregisterCallback(session, path_changed as *const c_void, context);
            DASessionUnscheduleFromRunLoop(session, run_loop, kCFRunLoopDefaultMode);
            if !watch_keys.is_null() {
                CFRelease(watch_keys);
            }
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

    /// The mountinfo watcher reports a changed table, including the very first change after the baseline, and nothing else.
    #[test]
    fn a_table_watch_reports_every_change_from_the_baseline_on() {
        let mut t = TableWatch::default();
        assert!(!t.changed(b"/ rw\n"), "the first reading is the baseline");
        assert!(!t.changed(b"/ rw\n"), "the same table is no change");
        assert!(t.changed(b"/ rw\n/Volumes/usb rw\n"), "the first real change is not dropped");
        assert!(t.changed(b"/ rw\n"), "an unmount is a change");
    }

    /// An idle macOS run loop (nothing to run) is waited on, not spun on.
    #[test]
    fn a_finished_run_loop_is_paused_and_a_timeout_is_not() {
        assert_eq!(run_loop_pause(1), Some(std::time::Duration::from_millis(500)));
        for ordinary in [2, 3, 4] {
            assert_eq!(run_loop_pause(ordinary), None);
        }
    }

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
