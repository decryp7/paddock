//! The operating system's own memory-pressure and foreground signals (ADR 0006), read every few seconds by the
//! release loop and kept in a small shared cell, so a reading never blocks a request or the dashboard.
//! What the readings mean is `pitwall_core::pressure`; this module only fetches them:
//!
//! | OS | Memory pressure | Foreground (yield) |
//! |---|---|---|
//! | Linux | PSI, `/proc/pressure/memory` | not detected (no portable way) |
//! | macOS | `sysctl kern.memorystatus_vm_pressure_level` | `pmset -g assertions` display-sleep assertion |
//! | Windows | memory resource notification | `SHQueryUserNotificationState` (full screen, presentation, busy) |
//!
//! A source that is missing or fails reads as "normal"; Pitwall then relies on the available-RAM percentage.

use pitwall_core::pressure::{Level, Signals};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static LEVEL: AtomicU8 = AtomicU8::new(0);
static FOREGROUND: AtomicBool = AtomicBool::new(false);

/// The latest readings.
pub fn current() -> Signals {
    Signals {
        level: match LEVEL.load(Ordering::Relaxed) {
            1 => Level::Warn,
            2 => Level::Critical,
            _ => Level::Normal,
        },
        foreground_busy: FOREGROUND.load(Ordering::Relaxed),
    }
}

/// Stores readings (the poller, and tests).
pub fn store(s: Signals) {
    LEVEL.store(
        match s.level {
            Level::Normal => 0,
            Level::Warn => 1,
            Level::Critical => 2,
        },
        Ordering::Relaxed,
    );
    FOREGROUND.store(s.foreground_busy, Ordering::Relaxed);
}

/// Reads the OS now and stores the result.
pub async fn refresh() -> Signals {
    let s = read().await;
    store(s);
    s
}

#[cfg(target_os = "linux")]
async fn read() -> Signals {
    let level = match tokio::fs::read_to_string("/proc/pressure/memory").await {
        Ok(t) => pitwall_core::pressure::parse_psi(&t).unwrap_or_default(),
        Err(_) => Level::Normal, // kernels without PSI (or containers hiding it)
    };
    Signals {
        level,
        foreground_busy: false,
    }
}

#[cfg(target_os = "macos")]
async fn read() -> Signals {
    use crate::process;
    use std::time::Duration;
    async fn run(program: &str, args: &[&str]) -> Option<String> {
        let mut c = process::command(program, None);
        c.args(args);
        match tokio::time::timeout(Duration::from_secs(3), c.output()).await {
            Ok(Ok(o)) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
            _ => None,
        }
    }
    let level = run("/usr/sbin/sysctl", &["-n", "kern.memorystatus_vm_pressure_level"])
        .await
        .and_then(|t| pitwall_core::pressure::parse_macos_level(&t))
        .unwrap_or_default();
    let foreground_busy = run("/usr/bin/pmset", &["-g", "assertions"])
        .await
        .is_some_and(|t| pitwall_core::pressure::parse_pmset_display_busy(&t));
    Signals { level, foreground_busy }
}

#[cfg(windows)]
async fn read() -> Signals {
    tokio::task::spawn_blocking(windows_impl::read).await.unwrap_or_default()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
async fn read() -> Signals {
    Signals::default()
}

#[cfg(windows)]
mod windows_impl {
    //! Two small Win32 queries, each with its `unsafe` call justified.
    #![allow(unsafe_code)]

    use pitwall_core::pressure::{Level, Signals};
    use windows::core::BOOL;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Memory::{
        CreateMemoryResourceNotification, LowMemoryResourceNotification, QueryMemoryResourceNotification,
    };
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};

    fn low_memory() -> bool {
        // SAFETY: the handle is created here, only queried with a valid out-pointer to a BOOL on our stack, and closed
        // before returning; nothing else holds it.
        unsafe {
            let Ok(h) = CreateMemoryResourceNotification(LowMemoryResourceNotification) else {
                return false;
            };
            let mut state = BOOL(0);
            let low = QueryMemoryResourceNotification(h, &mut state).is_ok() && state.as_bool();
            let _ = CloseHandle(h);
            low
        }
    }

    fn foreground_busy() -> bool {
        // SAFETY: takes no pointers; returns the state of the calling user's session (the server runs in it).
        unsafe {
            SHQueryUserNotificationState().is_ok_and(|s| s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE)
        }
    }

    pub fn read() -> Signals {
        Signals {
            level: if low_memory() { Level::Warn } else { Level::Normal },
            foreground_busy: foreground_busy(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn readings_are_stored_and_the_os_can_be_read_on_this_machine() {
        let before = current();
        store(Signals {
            level: Level::Critical,
            foreground_busy: true,
        });
        assert_eq!(current().level, Level::Critical);
        assert!(current().foreground_busy);
        store(Signals {
            level: Level::Warn,
            foreground_busy: false,
        });
        assert_eq!(current().level, Level::Warn);
        store(Signals::default());
        assert_eq!(current(), Signals::default());
        // Reading never fails, whatever the OS offers (a missing source reads as normal).
        let _ = refresh().await;
        store(before);
    }
}
