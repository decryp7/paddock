//! The OS calls behind the tray diagnostics and the macOS start activation (ER-27): who is in front, whether Dyno has been active,
//! ControlCenter's and Explorer's pid and start time, the window server's list of on-screen windows. Thin: every judgement is
//! in a pure module ([`crate::tray_focus`], [`crate::tray_shown`]).
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
pub use mac::{context, on_screen_windows, NativeFocus};
#[cfg(windows)]
pub use win::context;

/// Elsewhere there is nothing to add to the log.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn context() -> String {
    String::new()
}

#[cfg(target_os = "macos")]
mod mac {
    use crate::tray::on_main;
    use crate::tray_focus::{FocusPort, Front, Method};
    use crate::tray_shown::Window;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    use objc2_foundation::{MainThreadMarker, NSArray, NSDictionary, NSNumber, NSString};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tauri::{AppHandle, Wry};

    fn epoch_secs(app: &NSRunningApplication) -> Option<f64> {
        app.launchDate().map(|d| d.timeIntervalSince1970())
    }

    /// Dyno's activity and ControlCenter's identity, for the log: whether Dyno is the active app now, ControlCenter's pid and start
    /// time (seconds since the epoch) and how long after it Dyno started. Main thread only.
    pub fn context() -> String {
        let Some(mtm) = MainThreadMarker::new() else {
            return "context unavailable: not on the main thread".to_string();
        };
        let active = NSApplication::sharedApplication(mtm).isActive();
        let own = NSRunningApplication::runningApplicationWithProcessIdentifier(std::process::id() as i32).and_then(|a| epoch_secs(&a));
        let cc = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str("com.apple.controlcenter"));
        let cc = cc.firstObject();
        let cc_text = match &cc {
            Some(c) => {
                let start = epoch_secs(c);
                format!(
                    "ControlCenter pid {} started {} (epoch s)",
                    c.processIdentifier(),
                    start.map_or_else(|| "unknown".to_string(), |s| format!("{s:.0}"))
                )
            }
            None => "ControlCenter not found".to_string(),
        };
        let after = match (own, cc.as_ref().and_then(|c| epoch_secs(c))) {
            (Some(o), Some(c)) => format!(", Dyno started {:.0} s after ControlCenter", o - c),
            _ => String::new(),
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        format!("Dyno active now {active}; {cc_text}{after}; now {now} (epoch s)")
    }

    fn string_of(dict: &NSDictionary, key: &str) -> Option<String> {
        let v = dict.objectForKey(&NSString::from_str(key))?;
        v.downcast::<NSString>().ok().map(|s| s.to_string())
    }

    fn number_of(dict: &NSDictionary, key: &str) -> Option<f64> {
        let v = dict.objectForKey(&NSString::from_str(key))?;
        v.downcast::<NSNumber>().ok().map(|n| n.doubleValue())
    }

    /// The window server's list of on-screen windows: owner, name (when readable) and size.
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
            let Some(owner) = string_of(&dict, "kCGWindowOwnerName") else {
                continue;
            };
            let bounds = dict
                .objectForKey(&NSString::from_str("kCGWindowBounds"))
                .and_then(|b| b.downcast::<NSDictionary>().ok());
            let (width, height) = bounds.map_or((0.0, 0.0), |b| {
                (number_of(&b, "Width").unwrap_or(0.0), number_of(&b, "Height").unwrap_or(0.0))
            });
            out.push(Window {
                owner,
                name: string_of(&dict, "kCGWindowName"),
                width,
                height,
            });
        }
        Ok(out)
    }

    /// The Apple event `activate` to Dyno itself, sent by `osascript` (a program with a timeout, never a window): macOS honours it
    /// where the cooperative call is refused for a background launch (measured on macOS 26.6.2).
    fn apple_event_activate() -> Result<(), String> {
        let script = format!("tell application id \"{}\" to activate", crate::tray_identity::BUNDLE_ID);
        let mut cmd = paddock_proc::command("/usr/bin/osascript");
        cmd.args(["-e", &script]);
        let out =
            paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(std::time::Duration::from_secs(5))).map_err(|e| e.to_string())?;
        if out.success() {
            Ok(())
        } else {
            Err(format!("osascript {:?}: {}", out.outcome, out.stderr_text().trim()))
        }
    }

    /// [`FocusPort`] over AppKit: every call hops to the main thread (the activation sequence runs on its own thread).
    pub struct NativeFocus {
        /// The app.
        pub app: AppHandle<Wry>,
    }

    impl NativeFocus {
        fn main<T: Send + 'static>(&self, f: impl FnOnce(MainThreadMarker) -> T + Send + 'static) -> Result<T, String> {
            on_main(&self.app, move |_| {
                MainThreadMarker::new().map(f).ok_or_else(|| "not on the main thread".to_string())
            })?
        }
    }

    impl FocusPort for NativeFocus {
        fn frontmost(&mut self) -> Option<Front> {
            self.main(|_| {
                let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
                let pid = front.processIdentifier();
                (pid != std::process::id() as i32).then(|| Front {
                    pid,
                    name: front.localizedName().map_or_else(|| "unnamed".to_string(), |n| n.to_string()),
                })
            })
            .ok()
            .flatten()
        }

        fn is_active(&mut self) -> bool {
            self.main(|mtm| NSApplication::sharedApplication(mtm).isActive()).unwrap_or(false)
        }

        fn activate(&mut self, how: Method) -> Result<(), String> {
            match how {
                Method::Cooperative => self.main(|mtm| NSApplication::sharedApplication(mtm).activate()),
                Method::Forced =>
                {
                    #[allow(deprecated)]
                    self.main(|mtm| NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true))
                }
                Method::AppleEvent => apple_event_activate(),
            }
        }

        fn give_back(&mut self, to: &Front) -> Result<(), String> {
            let pid = to.pid;
            self.main(move |mtm| {
                let other = NSRunningApplication::runningApplicationWithProcessIdentifier(pid).ok_or("that app has quit")?;
                // macOS 14+: Dyno yields its activation to the other app, which then activates (the cooperative way back).
                NSApplication::sharedApplication(mtm).yieldActivationToApplication(&other);
                if other.activateWithOptions(NSApplicationActivationOptions::empty()) {
                    Ok(())
                } else {
                    Err("the app refused to activate".to_string())
                }
            })?
        }

        fn wait(&mut self, ms: u64) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
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
