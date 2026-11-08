//! Slow-changing facts on Linux. Everything that would need a heavy or privileged tool is an unavailable reading
//! with the reason: disk health (SMART) needs root and `smartctl`, pending updates would need the package manager
//! to refresh its lists; neither is run.

use super::paths::{home_dir, read_text, read_trim, HostPaths};
use super::tools::Tools;
use crate::dev_folders::{scan, ScanLimits};
use crate::parse::linux_proc::os_pretty_name;
use crate::parse::linux_tools::{boot_secs, cpufreq_plan, power_profile};
use dyno_core::facts::{FactKind, SystemFacts};
use dyno_core::metrics::Reading;
use dyno_core::ports::FactsSource;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How long `powerprofilesctl` and `systemd-analyze` may take.
const TOOL_TIMEOUT: Duration = Duration::from_secs(3);

/// Where the facts come from.
pub struct FactPaths {
    /// `/etc/os-release`.
    pub os_release: PathBuf,
    /// `/var/run/reboot-required`, the marker Debian-family systems create.
    pub reboot_marker: PathBuf,
    /// `/var/lib/dpkg`: its presence says the marker's absence means "no restart pending".
    pub dpkg_dir: PathBuf,
    /// The user's home folder (to find project folders).
    pub home: Option<PathBuf>,
}

impl FactPaths {
    /// This machine's.
    pub fn host() -> Self {
        FactPaths {
            os_release: PathBuf::from("/etc/os-release"),
            reboot_marker: PathBuf::from("/var/run/reboot-required"),
            dpkg_dir: PathBuf::from("/var/lib/dpkg"),
            home: home_dir(),
        }
    }
}

/// Facts for Linux.
pub struct LinuxFacts {
    sys: HostPaths,
    files: FactPaths,
    tools: Arc<dyn Tools>,
}

impl LinuxFacts {
    /// Facts read from the given places.
    pub fn new(sys: HostPaths, files: FactPaths, tools: Arc<dyn Tools>) -> Self {
        LinuxFacts { sys, files, tools }
    }

    fn power_plan(&self) -> Reading<dyno_core::facts::PowerPlan> {
        if let Ok(out) = self.tools.run("powerprofilesctl", &["get"], TOOL_TIMEOUT) {
            if let Some(plan) = out.ok.then(|| power_profile(&out.stdout)).flatten() {
                return Reading::Available(plan);
            }
        }
        let policy = self.sys.sys.join("devices/system/cpu/cpu0/cpufreq");
        let governor = read_trim(&policy.join("scaling_governor"));
        let driver = read_trim(&policy.join("scaling_driver")).unwrap_or_default();
        let preference = read_trim(&policy.join("energy_performance_preference"));
        match governor.and_then(|g| cpufreq_plan(&g, &driver, preference.as_deref())) {
            Some(plan) => Reading::Available(plan),
            None => Reading::unavailable("This computer reports no power profile (no power-profiles-daemon and no CPU frequency policy)"),
        }
    }

    fn restart_pending(&self) -> Reading<bool> {
        if self.files.reboot_marker.exists() {
            Reading::Available(true)
        } else if self.files.dpkg_dir.exists() {
            Reading::Available(false)
        } else {
            Reading::unavailable("This distribution leaves no restart-required marker to check")
        }
    }

    fn last_boot(&self) -> Reading<f32> {
        match self.tools.run("systemd-analyze", &["time"], TOOL_TIMEOUT) {
            Ok(out) if out.ok => match boot_secs(&out.stdout) {
                Some(s) => Reading::Available(s),
                None => Reading::unavailable("systemd has not finished starting up yet"),
            },
            Ok(_) | Err(_) => Reading::unavailable("Boot time needs systemd-analyze, which isn't available here"),
        }
    }
}

impl FactsSource for LinuxFacts {
    fn facts(&mut self) -> SystemFacts {
        let os = read_text(&self.files.os_release)
            .and_then(|t| os_pretty_name(&t))
            .unwrap_or_else(|| "Linux".to_string());
        let mut facts = SystemFacts::unknown(os);
        facts.disks = Reading::unavailable("Disk health (SMART) needs smartctl and administrator rights; Dyno doesn't run it");
        facts.restart_pending = self.restart_pending();
        facts.pending_updates =
            Reading::unavailable("Pending updates aren't checked: that would need the package manager to refresh its lists");
        facts.power_plan = self.power_plan();
        facts.visual_effects_full = Reading::unavailable("Desktop effects are set per desktop environment");
        facts.background_apps = Reading::unavailable("Linux has no background-apps list");
        facts.last_boot_secs = self.last_boot();
        facts.not_read_here = vec![
            FactKind::Disks,
            FactKind::PendingUpdates,
            FactKind::VisualEffects,
            FactKind::BackgroundApps,
        ];
        if let Some(home) = &self.files.home {
            facts.dev_folders = scan(home, ScanLimits::default());
        }
        facts
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools::fake::FakeTools;
    use super::super::tools::ToolOutput;
    use super::*;

    fn w(root: &std::path::Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn facts(root: &std::path::Path, tools: Arc<FakeTools>) -> LinuxFacts {
        LinuxFacts::new(
            HostPaths::under(root),
            FactPaths {
                os_release: root.join("etc/os-release"),
                reboot_marker: root.join("var/run/reboot-required"),
                dpkg_dir: root.join("var/lib/dpkg"),
                home: Some(root.join("home/me")),
            },
            tools,
        )
    }

    #[test]
    fn gathers_what_is_cheap_and_explains_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "etc/os-release", "NAME=Debian\nPRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\n");
        w(r, "var/run/reboot-required", "");
        w(r, "home/me/code/app/Cargo.toml", "");
        w(r, "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor", "powersave\n");
        w(r, "sys/devices/system/cpu/cpu0/cpufreq/scaling_driver", "acpi-cpufreq\n");
        let tools = Arc::new(FakeTools::with(
            "systemd-analyze",
            "Startup finished in 2.0s (firmware) + 1.0s (kernel) + 9.0s (userspace) = 12.0s\n",
        ));
        let f = facts(r, tools.clone()).facts();
        assert_eq!(f.os, "Debian GNU/Linux 12 (bookworm)");
        assert_eq!(f.restart_pending, Reading::Available(true));
        assert_eq!(f.power_plan.value().map(|p| p.power_saver), Some(true));
        assert_eq!(f.last_boot_secs, Reading::Available(10.0));
        assert_eq!(f.dev_folders.len(), 1);
        assert!(f.dev_folders[0].path.ends_with("code/app"));
        for unavailable in [
            &f.disks.value().is_none(),
            &f.pending_updates.value().is_none(),
            &f.visual_effects_full.value().is_none(),
        ] {
            assert!(unavailable);
        }
        // Nothing heavy is ever run.
        assert_eq!(tools.calls(), ["powerprofilesctl get", "systemd-analyze time"]);
    }

    #[test]
    fn power_profiles_daemon_wins_and_defaults_are_honest() {
        let d = tempfile::tempdir().unwrap();
        let tools = Arc::new(FakeTools::with("powerprofilesctl", "power-saver\n"));
        let f = facts(d.path(), tools).facts();
        assert_eq!(f.os, "Linux");
        assert!(f.power_plan.value().unwrap().power_saver);
        assert_eq!(f.restart_pending.value(), None, "no dpkg and no marker: unknown, not false");
        assert!(matches!(&f.last_boot_secs, Reading::Unavailable(m) if m.contains("systemd-analyze")));
        assert!(f.dev_folders.is_empty());
        std::fs::create_dir_all(d.path().join("var/lib/dpkg")).unwrap();
        assert_eq!(
            facts(d.path(), Arc::new(FakeTools::default())).facts().restart_pending,
            Reading::Available(false)
        );
        let failing = Arc::new(FakeTools::default());
        failing.answer(
            "powerprofilesctl",
            Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "daemon not running".into(),
            }),
        );
        assert!(matches!(facts(d.path(), failing).facts().power_plan, Reading::Unavailable(m) if m.contains("no power profile")));
    }
}
