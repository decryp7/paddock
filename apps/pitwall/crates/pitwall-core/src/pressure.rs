//! Reading the operating system's own memory-pressure and foreground signals (ADR 0006). The server reads the raw
//! text or flag from the OS (Linux PSI, macOS `sysctl`/`pmset`, Windows notifications) and these pure functions decide
//! what it means; the result feeds [`crate::memory::System`].

use crate::memory::System;

/// How hard the OS says memory is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub enum Level {
    /// Nothing to report.
    #[default]
    Normal,
    /// The OS is reclaiming memory (macOS warn, Windows low-memory notification, Linux PSI "some" stalls).
    Warn,
    /// Processes are stalling on memory (macOS critical, Linux PSI "full" stalls).
    Critical,
}

/// What the OS reports about memory and the user's foreground work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals {
    /// Memory pressure.
    pub level: Level,
    /// A full-screen app, game or presentation has the foreground.
    pub foreground_busy: bool,
}

impl Signals {
    /// Puts the signals into a memory reading.
    pub fn apply(self, mut sys: System) -> System {
        sys.os_pressure = self.level >= Level::Warn;
        sys.os_critical = self.level == Level::Critical;
        sys.foreground_busy = self.foreground_busy;
        sys
    }
}

/// PSI "some" 10-second average (percent of time at least one task stalled) from which pressure counts as `Warn`.
pub const PSI_SOME_WARN: f64 = 10.0;
/// PSI "full" 10-second average (all tasks stalled) from which pressure counts as `Critical`.
pub const PSI_FULL_CRITICAL: f64 = 5.0;

fn psi_avg10(line: &str) -> Option<f64> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix("avg10="))
        .and_then(|v| v.parse().ok())
}

/// Linux `/proc/pressure/memory`:
/// `some avg10=0.00 avg60=0.00 avg300=0.00 total=0` and a `full` line. `None` when the text is not PSI.
pub fn parse_psi(text: &str) -> Option<Level> {
    let mut some = None;
    let mut full = None;
    for line in text.lines().take(8) {
        if line.starts_with("some ") {
            some = psi_avg10(line);
        } else if line.starts_with("full ") {
            full = psi_avg10(line);
        }
    }
    let some = some?;
    Some(if full.unwrap_or(0.0) >= PSI_FULL_CRITICAL {
        Level::Critical
    } else if some >= PSI_SOME_WARN {
        Level::Warn
    } else {
        Level::Normal
    })
}

/// macOS `sysctl -n kern.memorystatus_vm_pressure_level`: 1 normal, 2 warning, 4 critical.
pub fn parse_macos_level(text: &str) -> Option<Level> {
    match text.trim().parse::<u32>().ok()? {
        1 => Some(Level::Normal),
        2 => Some(Level::Warn),
        4 => Some(Level::Critical),
        _ => None,
    }
}

/// macOS `pmset -g assertions`: full-screen video, games and presentations hold a "prevent display sleep" assertion.
/// True when `PreventUserIdleDisplaySleep` (or the older `NoDisplaySleepAssertion`) is currently 1.
pub fn parse_pmset_display_busy(text: &str) -> bool {
    text.lines().take(60).any(|l| {
        let mut w = l.split_whitespace();
        matches!(
            (w.next(), w.next()),
            (Some("PreventUserIdleDisplaySleep" | "NoDisplaySleepAssertion"), Some("1"))
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Policy;

    const CALM: &str = "some avg10=0.00 avg60=0.00 avg300=0.00 total=12345\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";

    #[test]
    fn psi_levels_follow_the_thresholds() {
        assert_eq!(parse_psi(CALM), Some(Level::Normal));
        assert_eq!(
            parse_psi("some avg10=9.99 avg60=1 avg300=1 total=1\nfull avg10=0.00 avg60=0 avg300=0 total=0"),
            Some(Level::Normal)
        );
        assert_eq!(
            parse_psi("some avg10=10.00 avg60=1 avg300=1 total=1\nfull avg10=0.00 avg60=0 avg300=0 total=0"),
            Some(Level::Warn)
        );
        assert_eq!(
            parse_psi("some avg10=12 avg60=1 avg300=1 total=1\nfull avg10=5.00 avg60=0 avg300=0 total=0"),
            Some(Level::Critical)
        );
        assert_eq!(
            parse_psi("some avg10=1.00 avg60=1 avg300=1 total=1"),
            Some(Level::Normal),
            "no full line"
        );
    }

    #[test]
    fn psi_garbage_is_not_pressure() {
        assert_eq!(parse_psi(""), None);
        assert_eq!(parse_psi("nonsense"), None);
        assert_eq!(parse_psi("some avg10=abc avg60=1"), None);
        assert_eq!(parse_psi("full avg10=50.0 avg60=1"), None, "needs the some line");
    }

    #[test]
    fn macos_levels() {
        assert_eq!(parse_macos_level("1\n"), Some(Level::Normal));
        assert_eq!(parse_macos_level(" 2 "), Some(Level::Warn));
        assert_eq!(parse_macos_level("4"), Some(Level::Critical));
        assert_eq!(parse_macos_level("3"), None);
        assert_eq!(parse_macos_level("x"), None);
    }

    #[test]
    fn display_assertions_mean_a_busy_foreground() {
        let idle = "Assertion status system-wide:\n   PreventUserIdleDisplaySleep    0\n   PreventUserIdleSystemSleep     1\n";
        let busy = "Assertion status system-wide:\n   PreventUserIdleDisplaySleep    1\n   PreventUserIdleSystemSleep     1\n";
        assert!(!parse_pmset_display_busy(idle));
        assert!(parse_pmset_display_busy(busy));
        assert!(parse_pmset_display_busy("   NoDisplaySleepAssertion    1"));
        assert!(!parse_pmset_display_busy(""));
    }

    #[test]
    fn signals_land_in_the_memory_reading() {
        let base = System {
            total: 100,
            available: 90,
            ..System::default()
        };
        let policy = Policy::default();
        assert!(!Signals::default().apply(base).under_pressure(&policy));
        let warn = Signals {
            level: Level::Warn,
            foreground_busy: true,
        }
        .apply(base);
        assert!(warn.os_pressure && !warn.os_critical && warn.foreground_busy && warn.under_pressure(&policy));
        let crit = Signals {
            level: Level::Critical,
            foreground_busy: false,
        }
        .apply(base);
        assert!(crit.os_pressure && crit.os_critical && !crit.foreground_busy);
    }
}
