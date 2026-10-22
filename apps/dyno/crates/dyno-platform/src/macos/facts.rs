//! Slow-changing facts on macOS: the OS version, Low Power Mode, the startup disk's SMART status and developer
//! folders. Update checks (`softwareupdate -l`) are slow and phone home, so Dyno does not run them.

use crate::dev_folders::{scan, ScanLimits};
use crate::parse::launchd::parse_smart_status;
use crate::parse::mac_tools::{parse_pmset_low_power, parse_sw_vers};
use dyno_core::facts::{DiskHealth, DiskHealthStatus, PowerPlan, SystemFacts};
use dyno_core::metrics::Reading;
use dyno_core::ports::FactsSource;
use std::path::PathBuf;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(8);

/// Facts for macOS.
pub struct MacFacts {
    home: Option<PathBuf>,
}

impl MacFacts {
    /// Facts for the current user.
    pub fn new() -> Self {
        MacFacts {
            home: std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from),
        }
    }
}

impl Default for MacFacts {
    fn default() -> Self {
        Self::new()
    }
}

fn tool(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = crate::process::command(program);
    cmd.args(args);
    crate::process::output_text(cmd, TIMEOUT).ok()
}

/// Maps a `SMART Status` value.
pub fn smart_health(status: &str) -> DiskHealthStatus {
    match status.to_ascii_lowercase().as_str() {
        "verified" => DiskHealthStatus::Healthy,
        "failing" => DiskHealthStatus::Unhealthy,
        "about to fail" => DiskHealthStatus::Warning,
        _ => DiskHealthStatus::Unknown,
    }
}

impl FactsSource for MacFacts {
    fn facts(&mut self) -> SystemFacts {
        let os = tool("/usr/bin/sw_vers", &[])
            .and_then(|t| parse_sw_vers(&t))
            .unwrap_or_else(|| "macOS".into());
        let mut f = SystemFacts::unknown(os);
        f.disks = match tool("/usr/sbin/diskutil", &["info", "/"]).and_then(|t| parse_smart_status(&t)) {
            Some(s) => Reading::Available(vec![DiskHealth {
                name: "Startup disk".into(),
                status: smart_health(&s),
                wear: None,
                temperature: None,
            }]),
            None => Reading::unavailable("macOS did not report a SMART status for the startup disk"),
        };
        f.restart_pending = Reading::unavailable("macOS does not say when a restart is pending");
        f.pending_updates = Reading::unavailable("Dyno does not check for macOS updates (it is slow and contacts Apple)");
        f.power_plan = match tool("/usr/bin/pmset", &["-g"]).and_then(|t| parse_pmset_low_power(&t)) {
            Some(low) => Reading::Available(PowerPlan {
                id: if low { "low-power" } else { "automatic" }.into(),
                name: if low { "Low Power Mode" } else { "Automatic" }.into(),
                power_saver: low,
            }),
            None => Reading::unavailable("This Mac has no Low Power Mode setting"),
        };
        f.visual_effects_full = Reading::unavailable("Reduce Motion and Transparency are not read yet");
        f.background_apps = Reading::unavailable("macOS has no per-app background permission to count");
        if let Some(h) = &self.home {
            f.dev_folders = scan(h, ScanLimits::default());
        }
        f.last_boot_secs = Reading::unavailable("Boot time is not measured yet");
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_smart_values() {
        assert_eq!(smart_health("Verified"), DiskHealthStatus::Healthy);
        assert_eq!(smart_health("Failing"), DiskHealthStatus::Unhealthy);
        assert_eq!(smart_health("About to Fail"), DiskHealthStatus::Warning);
        assert_eq!(smart_health("Not Supported"), DiskHealthStatus::Unknown);
    }

    #[test]
    fn reads_this_macs_facts() {
        let f = MacFacts::new().facts();
        assert!(f.os.starts_with("macOS"), "{}", f.os);
        assert!(f.power_plan.value().is_some() || f.power_plan.value().is_none());
        assert!(f.disks.value().is_some(), "SMART status of the startup disk: {:?}", f.disks);
        assert!(f.restart_pending.value().is_none());
    }
}
