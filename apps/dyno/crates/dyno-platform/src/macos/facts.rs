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
    run: Tool,
}

/// Runs one system program and returns its text, or nothing when it fails or times out.
type Tool = fn(&str, &[&str]) -> Option<String>;

impl MacFacts {
    /// Facts for the current user.
    pub fn new() -> Self {
        MacFacts {
            home: std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from),
            run: tool,
        }
    }

    /// Facts read through `run` and from `home` instead of the real programs and home folder (tests).
    #[cfg(test)]
    fn with(home: Option<PathBuf>, run: Tool) -> Self {
        MacFacts { home, run }
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
        let os = (self.run)("/usr/bin/sw_vers", &[])
            .and_then(|t| parse_sw_vers(&t))
            .unwrap_or_else(|| "macOS".into());
        let mut f = SystemFacts::unknown(os);
        f.disks = match (self.run)("/usr/sbin/diskutil", &["info", "/"]).and_then(|t| parse_smart_status(&t)) {
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
        f.power_plan = match (self.run)("/usr/bin/pmset", &["-g"]).and_then(|t| parse_pmset_low_power(&t)) {
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

    /// Canned answers in the shape of the real programs' output; the test never runs them or reads the real home folder.
    fn canned(program: &str, _args: &[&str]) -> Option<String> {
        match program {
            "/usr/bin/sw_vers" => Some("ProductName:\t\tmacOS\nProductVersion:\t\t15.1\nBuildVersion:\t\t24B83\n".into()),
            "/usr/sbin/diskutil" => Some("   Device Identifier:         disk3s1s1\n   SMART Status:             Verified\n".into()),
            "/usr/bin/pmset" => Some("System-wide power settings:\n lowpowermode         0\n".into()),
            _ => None,
        }
    }

    #[test]
    fn reads_the_facts_from_the_programs_output_without_touching_this_mac() {
        let home = tempfile::tempdir().unwrap();
        let f = MacFacts::with(Some(home.path().to_path_buf()), canned).facts();
        assert!(f.os.starts_with("macOS"), "{}", f.os);
        assert_eq!(f.power_plan.value().map(|p| p.power_saver), Some(false));
        let disks = f.disks.value().unwrap();
        assert_eq!(disks[0].status, DiskHealthStatus::Healthy);
        assert!(f.restart_pending.value().is_none());
    }

    #[test]
    fn missing_programs_degrade_to_unavailable_readings() {
        let f = MacFacts::with(None, |_, _| None).facts();
        assert_eq!(f.os, "macOS");
        assert!(f.disks.value().is_none() && f.power_plan.value().is_none());
    }
}
