//! User activity on macOS: seconds since the last input event (CoreGraphics) and the frontmost application
//! (`lsappinfo`).

use crate::parse::mac_tools::{parse_lsappinfo_front, parse_lsappinfo_pid};
use dyno_core::ports::UserActivity;
use dyno_core::process::Pid;
use std::time::Duration;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
}

/// kCGEventSourceStateCombinedSessionState.
const COMBINED_SESSION: i32 = 0;
/// kCGAnyInputEventType.
const ANY_INPUT: u32 = u32::MAX;

/// Idle time and foreground application.
pub struct MacActivity;

impl MacActivity {
    /// The reader.
    pub fn new() -> Self {
        MacActivity
    }
}

impl Default for MacActivity {
    fn default() -> Self {
        Self::new()
    }
}

impl UserActivity for MacActivity {
    fn idle_secs(&mut self) -> Option<u64> {
        // SAFETY: a pure query taking two integers; no pointers, no preconditions.
        let s = unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION, ANY_INPUT) };
        (s.is_finite() && s >= 0.0).then_some(s as u64)
    }

    fn foreground_pid(&mut self) -> Option<Pid> {
        let run = |args: &[&str]| {
            let mut cmd = crate::process::command("/usr/bin/lsappinfo");
            cmd.args(args);
            crate::process::output_text(cmd, Duration::from_secs(2)).ok()
        };
        let asn = parse_lsappinfo_front(&run(&["front"])?)?;
        parse_lsappinfo_pid(&run(&["info", "-only", "pid", &asn])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_time_is_a_small_number_or_unknown() {
        let idle = MacActivity::new().idle_secs();
        // Headless sessions may have no event source; when there is one the value is plausible (< 10 years).
        assert!(idle.is_none_or(|s| s < 315_360_000));
    }

    #[test]
    fn foreground_pid_never_panics() {
        let _ = MacActivity::new().foreground_pid();
    }
}
