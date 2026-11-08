//! The OS calls behind the tray diagnostics (ER-27): ControlCenter's and Explorer's pid and start time, the window server's list of
//! on-screen windows and its menu bar. Thin: every judgement is in a pure module ([`crate::tray_shown`]).
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
pub use mac::context;
#[cfg(target_os = "macos")]
pub use mac::{control_center_pid, on_screen_windows};
#[cfg(windows)]
pub use win::context;

/// Elsewhere there is nothing to add to the log.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn context() -> String {
    String::new()
}

#[cfg(target_os = "macos")]
mod mac {
    use crate::tray_shown::Window;
    use objc2::rc::Retained;
    use objc2_app_kit::NSRunningApplication;
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// When process `pid` started (seconds since the epoch), from the kernel (`proc_pidinfo`) (`launchDate` is empty for
    /// ControlCenter, which is not a registered GUI app).
    fn start_secs(pid: i32) -> Option<f64> {
        // Safety: an all-zero `proc_bsdinfo` is a valid value to be overwritten; the size passed is its size.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
        (got == size && info.pbi_start_tvsec > 0).then(|| info.pbi_start_tvsec as f64 + info.pbi_start_tvusec as f64 / 1e6)
    }

    /// What the log needs to explain a missing item: whether the session has a menu bar at all, ControlCenter's pid and start time
    /// (seconds since the epoch) and how long after it Dyno started.
    pub fn context() -> String {
        let bar = on_screen_windows().map_or_else(
            |e| format!("menu bar unknown ({e})"),
            |w| crate::tray_shown::menubar(&w, std::process::id() as i32).describe(),
        );
        let own = start_secs(std::process::id() as i32);
        let cc = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str("com.apple.controlcenter"));
        let cc = cc.firstObject();
        let cc_text = match &cc {
            Some(c) => {
                let start = start_secs(c.processIdentifier());
                format!(
                    "ControlCenter pid {} started {} (epoch s)",
                    c.processIdentifier(),
                    start.map_or_else(|| "unknown".to_string(), |s| format!("{s:.0}"))
                )
            }
            None => "ControlCenter not found".to_string(),
        };
        let after = match (own, cc.as_ref().and_then(|c| start_secs(c.processIdentifier()))) {
            (Some(o), Some(c)) => format!(", Dyno started {:.0} s after ControlCenter", o - c),
            _ => String::new(),
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        format!("{bar}; {cc_text}{after}; now {now} (epoch s)")
    }

    fn string_of(dict: &NSDictionary, key: &str) -> Option<String> {
        let v = dict.objectForKey(&NSString::from_str(key))?;
        v.downcast::<NSString>().ok().map(|s| s.to_string())
    }

    fn number_of(dict: &NSDictionary, key: &str) -> Option<f64> {
        let v = dict.objectForKey(&NSString::from_str(key))?;
        v.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
    }

    /// ControlCenter's process id (it draws every app's status items).
    pub fn control_center_pid() -> Option<i32> {
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str("com.apple.controlcenter"))
            .firstObject()
            .map(|c| c.processIdentifier())
    }

    /// The window server's list of on-screen windows: owner pid, name (when readable) and size.
    pub fn on_screen_windows() -> Result<Vec<Window>, String> {
        let list = CGWindowListCopyWindowInfo(CGWindowListOption::OptionOnScreenOnly, 0).ok_or("the window list is empty or refused")?;
        let raw = objc2_core_foundation::CFRetained::into_raw(list)
            .as_ptr()
            .cast::<NSArray<NSDictionary>>();
        // Safety: CFArrayRef is toll-free bridged to NSArray, and the list owns +1 reference that `from_raw` takes over.
        let array: Retained<NSArray<NSDictionary>> = unsafe { Retained::from_raw(raw) }.ok_or("null window list")?;
        let mut out = Vec::new();
        for i in 0..array.count() {
            let dict = array.objectAtIndex(i);
            let Some(owner_pid) = number_of(&dict, "kCGWindowOwnerPID") else {
                continue;
            };
            let bounds = dict
                .objectForKey(&NSString::from_str("kCGWindowBounds"))
                .and_then(|b| b.downcast::<NSDictionary>().ok());
            let (width, height) = bounds.map_or((0.0, 0.0), |b| {
                (number_of(&b, "Width").unwrap_or(0.0), number_of(&b, "Height").unwrap_or(0.0))
            });
            out.push(Window {
                owner_pid: owner_pid as i32,
                layer: number_of(&dict, "kCGWindowLayer").unwrap_or(0.0) as i32,
                name: string_of(&dict, "kCGWindowName"),
                width,
                height,
            });
        }
        Ok(out)
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

    /// FILETIME (100 ns since 1601) to seconds since the Unix epoch.
    fn epoch(ft: FILETIME) -> f64 {
        let t = (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime);
        t as f64 / 1e7 - 11_644_473_600.0
    }

    fn creation(process: windows_sys::Win32::Foundation::HANDLE) -> Option<f64> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
        // Safety: `process` is a valid handle for the call and the four out-pointers are live locals.
        let ok = unsafe { GetProcessTimes(process, &mut created, &mut exit, &mut kernel, &mut user) };
        (ok != 0).then(|| epoch(created))
    }

    /// Explorer's pid and start time (seconds since the epoch) and Dyno's, for the log: the icons are added to Explorer's taskbar,
    /// so a start before Explorer is ready shows here.
    pub fn context() -> String {
        // Safety: a handle to this process needs no closing; the class name is a null-terminated wide string that outlives the call.
        let own = creation(unsafe { GetCurrentProcess() });
        let class: Vec<u16> = "Shell_TrayWnd\0".encode_utf16().collect();
        let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        if hwnd.is_null() {
            return "Explorer's taskbar window (Shell_TrayWnd) does not exist yet".to_string();
        }
        let mut pid = 0u32;
        // Safety: `hwnd` came from FindWindowW a moment ago; `pid` is a live local.
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        // Safety: a query-only handle to the process that owns the taskbar, closed below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        let started = if process.is_null() {
            None
        } else {
            let t = creation(process);
            // Safety: the handle was opened above and is closed once.
            unsafe { CloseHandle(process) };
            t
        };
        let after = match (own, started) {
            (Some(o), Some(e)) => format!(", Dyno started {:.0} s after Explorer", o - e),
            _ => String::new(),
        };
        format!(
            "taskbar window owned by pid {pid}, started {} (epoch s){after}",
            started.map_or_else(|| "unknown".to_string(), |s| format!("{s:.0}"))
        )
    }
}
