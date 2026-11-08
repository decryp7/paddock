//! User activity: time since the last input (GetLastInputInfo) and the foreground window's process.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use dyno_core::ports::UserActivity;
use dyno_core::process::Pid;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// [`UserActivity`] for Windows.
pub struct WinActivity;

/// Milliseconds from `last_input` to `now` on the 32-bit tick counter (which wraps every 49.7 days).
pub fn idle_millis(now: u32, last_input: u32) -> u32 {
    now.wrapping_sub(last_input)
}

impl UserActivity for WinActivity {
    fn idle_secs(&mut self) -> Option<u64> {
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is a valid LASTINPUTINFO with cbSize set.
        let ok = unsafe { GetLastInputInfo(&mut info) }.as_bool();
        if !ok {
            return None;
        }
        // SAFETY: no arguments.
        let now = unsafe { GetTickCount() };
        Some(u64::from(idle_millis(now, info.dwTime)) / 1000)
    }

    fn foreground_pid(&mut self) -> Option<Pid> {
        // SAFETY: no arguments; a null window means none is in the foreground.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        // SAFETY: `hwnd` came from GetForegroundWindow (a stale one just yields 0); `pid` is a valid out-pointer.
        let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        // A zero thread id is the call's failure: the window is gone, there is no process to name.
        (thread != 0 && pid != 0).then_some(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_time_survives_tick_wraparound() {
        assert_eq!(idle_millis(5_000, 2_000), 3_000);
        assert_eq!(idle_millis(100, u32::MAX - 99), 200);
    }

    #[test]
    fn reads_without_panicking() {
        let mut a = WinActivity;
        // A service session has no input desktop: both may be None, but never panic.
        let _ = a.idle_secs();
        let _ = a.foreground_pid();
    }
}
