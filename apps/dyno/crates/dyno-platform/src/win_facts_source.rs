//! The Windows `FactsSource` (ADR 0033, WP-3): which fact comes from where, how each is timed and logged, and what it says when it
//! cannot be read. The OS calls sit behind [`WinSystem`] (`windows/facts.rs` is the real one), so every OS tests this part with
//! recorded answers and the mapping functions of `parse::win_facts`.
//!
//! A fact that fails is that fact's `Reading::unavailable` with the reason, never an error for the whole (the `FactsSource`
//! contract). Facts Dyno does not read on Windows by design say so and are declared in `SystemFacts::not_read_here`.

use crate::dev_folders::{scan, ScanLimits};
use crate::parse::win_facts::{self, DiskReadError, OsValues, Overlay, RawDisk};
use dyno_core::facts::{FactKind, SystemFacts};
use dyno_core::metrics::Reading;
use dyno_core::ports::FactsSource;
use std::path::PathBuf;
use std::time::Instant;

/// What the disks fact says when the storage provider refuses the caller (measured for a standard user in WP-0).
pub const DISKS_NEED_ADMIN: &str = "Disk health needs administrator readings on Windows.";
/// Pending updates are not asked of Windows Update: the search is slow and contacts Microsoft.
pub const NO_PENDING_UPDATES: &str =
    "Dyno does not ask Windows Update (the search is slow and contacts Microsoft). Settings > Windows Update shows them.";
/// Boot time is in a log Dyno does not read.
pub const NO_BOOT_TIME: &str = "Windows keeps boot times in a log Dyno does not read yet.";
/// `BackgroundAccessApplications` no longer holds one switch (measured on Windows 11 in WP-0, item 9).
pub const NO_BACKGROUND_SWITCH: &str = "Windows no longer keeps one switch for this.";

/// A restart key that Windows creates while a restart is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartKey {
    /// `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired`.
    UpdateRebootRequired,
    /// `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending`.
    ServicingRebootPending,
}

/// The effective power mode as the power API reports it, before the pure mapping.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PowerRead {
    /// The active scheme's GUID.
    pub scheme: Option<String>,
    /// The scheme's friendly name (localised).
    pub friendly_name: Option<String>,
    /// The effective overlay (Windows 11): not there, read, or the call failed.
    pub overlay: Overlay,
}

/// The OS calls the facts need. Every method answers for itself: an `Err` is the reason that one fact is unavailable.
pub trait WinSystem: Send {
    /// The `CurrentVersion` values that name the OS.
    fn os_values(&mut self) -> Result<OsValues, String>;
    /// The build number `RtlGetVersion` reports (it is not changed by an application manifest).
    fn api_build(&mut self) -> Option<u32>;
    /// Whether `key` exists; a key that is not there is `Ok(false)`.
    fn restart_key_exists(&mut self, key: RestartKey) -> Result<bool, String>;
    /// HKCU `Explorer\VisualEffects\VisualFXSetting` (`None` when the value is missing).
    fn visual_fx_setting(&mut self) -> Result<Option<u32>, String>;
    /// The active power scheme and effective overlay.
    fn power(&mut self) -> Result<PowerRead, String>;
    /// The physical disks of the storage provider.
    fn physical_disks(&mut self) -> Result<Vec<RawDisk>, DiskReadError>;
    /// The user's profile folder (`%USERPROFILE%`).
    fn home(&self) -> Option<PathBuf>;
}

/// The facts of a Windows machine, read through `S`.
pub struct WinFacts<S: WinSystem> {
    system: S,
}

impl<S: WinSystem> WinFacts<S> {
    /// Facts read through `system`.
    pub fn new(system: S) -> Self {
        WinFacts { system }
    }
}

/// Times one fact and logs how it ended (ER-27): the fact, the duration and the outcome, with the reason when it is unavailable.
/// A reason names the OS call or its error code, never a user's file names or values.
fn read_fact<T>(fact: &'static str, read: impl FnOnce() -> Reading<T>) -> Reading<T> {
    let started = Instant::now();
    let reading = read();
    let ms = started.elapsed().as_millis();
    match &reading {
        Reading::Available(_) => log::info!(target: "dyno::facts", "fact {fact}: read in {ms} ms"),
        Reading::Unavailable(why) => log::warn!(target: "dyno::facts", "fact {fact}: unavailable after {ms} ms: {why}"),
    }
    reading
}

impl<S: WinSystem> FactsSource for WinFacts<S> {
    fn facts(&mut self) -> SystemFacts {
        let whole = Instant::now();
        let started = Instant::now();
        let api_build = self.system.api_build();
        let os = match self.system.os_values() {
            Ok(v) => {
                if let (Some(api), Some(reg)) = (api_build, v.build) {
                    if api != reg {
                        log::warn!(target: "dyno::facts", "fact os: the registry says build {reg}, RtlGetVersion says {api}; using {api}");
                    }
                }
                let name = win_facts::win_os_name(&v, api_build);
                log::info!(target: "dyno::facts", "fact os: read in {} ms", started.elapsed().as_millis());
                name
            }
            Err(why) => {
                log::warn!(target: "dyno::facts", "fact os: the registry did not answer ({why}); naming it from the API build alone");
                win_facts::win_os_name(&OsValues::default(), api_build)
            }
        };
        let mut f = SystemFacts::unknown(os);

        f.disks = read_fact("disks", || match self.system.physical_disks() {
            Ok(rows) if rows.is_empty() => Reading::unavailable("Windows reported no physical disks."),
            Ok(rows) => Reading::Available(win_facts::disk_health(&rows)),
            Err(DiskReadError::AccessDenied) => Reading::unavailable(DISKS_NEED_ADMIN),
            Err(DiskReadError::Failed(code)) => {
                Reading::unavailable(format!("Windows did not answer the disk health query (error {code})."))
            }
        });
        f.restart_pending = read_fact("restart_pending", || {
            win_facts::restart_pending(
                self.system.restart_key_exists(RestartKey::UpdateRebootRequired),
                self.system.restart_key_exists(RestartKey::ServicingRebootPending),
            )
        });
        f.power_plan = read_fact("power_plan", || match self.system.power() {
            Ok(p) => {
                if let Some(unknown) = win_facts::unknown_overlay(&p.overlay) {
                    log::warn!(target: "dyno::facts", "power_plan: an overlay this Dyno does not know ({unknown}); the scheme alone is reported");
                }
                win_facts::effective_power_plan(p.scheme.as_deref(), p.friendly_name.as_deref(), &p.overlay)
            }
            Err(why) => Reading::unavailable(format!("Windows did not answer the power plan question ({why}).")),
        });
        f.visual_effects_full = read_fact("visual_effects_full", || win_facts::visual_effects(self.system.visual_fx_setting()));
        if let Some(home) = self.system.home() {
            let started = Instant::now();
            f.dev_folders = scan(&home, ScanLimits::default());
            log::info!(
                target: "dyno::facts",
                "fact dev_folders: {} found in {} ms",
                f.dev_folders.len(),
                started.elapsed().as_millis()
            );
        } else {
            log::warn!(target: "dyno::facts", "fact dev_folders: no profile folder, none listed");
        }

        f.declare_not_read_here(FactKind::PendingUpdates, NO_PENDING_UPDATES);
        f.declare_not_read_here(FactKind::LastBoot, NO_BOOT_TIME);
        f.declare_not_read_here(FactKind::BackgroundApps, NO_BACKGROUND_SWITCH);
        log::info!(target: "dyno::facts", "facts gathered in {} ms", whole.elapsed().as_millis());
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::facts::DiskHealthStatus;

    /// Recorded answers; nothing here touches this machine.
    #[derive(Default)]
    struct Recorded {
        os: Option<Result<OsValues, String>>,
        api_build: Option<u32>,
        update_key: Option<Result<bool, String>>,
        servicing_key: Option<Result<bool, String>>,
        fx: Option<Result<Option<u32>, String>>,
        power: Option<Result<PowerRead, String>>,
        disks: Option<Result<Vec<RawDisk>, DiskReadError>>,
        home: Option<PathBuf>,
    }

    impl WinSystem for Recorded {
        fn os_values(&mut self) -> Result<OsValues, String> {
            self.os.clone().unwrap_or_else(|| Err("no answer".into()))
        }
        fn api_build(&mut self) -> Option<u32> {
            self.api_build
        }
        fn restart_key_exists(&mut self, key: RestartKey) -> Result<bool, String> {
            let a = match key {
                RestartKey::UpdateRebootRequired => &self.update_key,
                RestartKey::ServicingRebootPending => &self.servicing_key,
            };
            a.clone().unwrap_or_else(|| Err("no answer".into()))
        }
        fn visual_fx_setting(&mut self) -> Result<Option<u32>, String> {
            self.fx.clone().unwrap_or_else(|| Err("no answer".into()))
        }
        fn power(&mut self) -> Result<PowerRead, String> {
            self.power.clone().unwrap_or_else(|| Err("no answer".into()))
        }
        fn physical_disks(&mut self) -> Result<Vec<RawDisk>, DiskReadError> {
            self.disks
                .clone()
                .unwrap_or_else(|| Err(DiskReadError::Failed("0x00000000".into())))
        }
        fn home(&self) -> Option<PathBuf> {
            self.home.clone()
        }
    }

    fn windows_11() -> Recorded {
        Recorded {
            os: Some(Ok(OsValues {
                product_name: Some("Windows 10 Pro".into()),
                display_version: Some("24H2".into()),
                installation_type: Some("Client".into()),
                build: Some(26100),
                ubr: Some(2314),
                ..Default::default()
            })),
            api_build: Some(26100),
            update_key: Some(Ok(false)),
            servicing_key: Some(Ok(false)),
            fx: Some(Ok(Some(2))),
            power: Some(Ok(PowerRead {
                scheme: Some("381b4222-f694-41f0-9685-ff5bb260df2e".into()),
                friendly_name: Some("Balanced".into()),
                overlay: Overlay::Read("961cc777-2547-4f9d-8174-7d86181b8a7a".into()),
            })),
            disks: Some(Ok(vec![RawDisk {
                friendly_name: Some("Model A".into()),
                health_status: Some(0),
            }])),
            home: None,
        }
    }

    #[test]
    fn every_fact_comes_from_its_reader() {
        let home = paddock_testkit::private_tempdir().unwrap();
        let mut sys = windows_11();
        sys.home = Some(home.path().to_path_buf());
        let f = WinFacts::new(sys).facts();
        assert_eq!(f.os, "Windows 11 Pro 24H2 (build 26100.2314)");
        assert_eq!(f.disks.value().unwrap()[0].status, DiskHealthStatus::Healthy);
        assert_eq!(f.restart_pending, Reading::Available(false));
        let plan = f.power_plan.value().unwrap();
        assert!(plan.power_saver && plan.name == "Balanced (Best power efficiency)");
        assert_eq!(f.visual_effects_full, Reading::Available(false));
        assert!(f.dev_folders.is_empty(), "an empty profile has none");
    }

    #[test]
    fn what_windows_never_reads_is_declared_by_design_and_nothing_is_left_unread_yet() {
        let f = WinFacts::new(windows_11()).facts();
        assert_eq!(
            f.not_read_here,
            [FactKind::PendingUpdates, FactKind::LastBoot, FactKind::BackgroundApps]
        );
        assert!(f.not_read_yet.is_empty(), "WP-3 leaves no fact waiting for a reader");
        assert_eq!(f.pending_updates, Reading::Unavailable(NO_PENDING_UPDATES.into()));
        assert_eq!(f.last_boot_secs, Reading::Unavailable(NO_BOOT_TIME.into()));
        assert_eq!(f.background_apps, Reading::Unavailable(NO_BACKGROUND_SWITCH.into()));
    }

    #[test]
    fn a_standard_user_who_may_not_read_disk_health_gets_the_administrator_sentence() {
        let mut sys = windows_11();
        sys.disks = Some(Err(DiskReadError::AccessDenied));
        let f = WinFacts::new(sys).facts();
        assert_eq!(
            f.disks,
            Reading::Unavailable("Disk health needs administrator readings on Windows.".into())
        );
        assert!(
            !f.not_read_here.contains(&FactKind::Disks),
            "a denied read is a failed read, so Health reports it as not checked"
        );
        assert!(!f.not_read_yet.contains(&FactKind::Disks));
        assert_eq!(f.restart_pending, Reading::Available(false), "the other facts are untouched");
    }

    #[test]
    fn disk_failures_other_than_denial_and_an_empty_list_say_so() {
        let mut sys = windows_11();
        sys.disks = Some(Err(DiskReadError::Failed("0x80041010".into())));
        let f = WinFacts::new(sys).facts();
        assert_eq!(
            f.disks,
            Reading::Unavailable("Windows did not answer the disk health query (error 0x80041010).".into())
        );
        let mut sys = windows_11();
        sys.disks = Some(Ok(vec![]));
        assert_eq!(
            WinFacts::new(sys).facts().disks,
            Reading::Unavailable("Windows reported no physical disks.".into())
        );
    }

    #[test]
    fn each_failed_fact_is_its_own_unavailable_reading() {
        let f = WinFacts::new(Recorded::default()).facts();
        assert_eq!(f.os, "Windows", "no registry and no API build: the plain name, never a fake one");
        assert!(f.disks.value().is_none() && f.restart_pending.value().is_none());
        assert!(f.power_plan.value().is_none() && f.visual_effects_full.value().is_none());
        assert!(f.power_plan.unavailable_reason().unwrap().contains("no answer"));
    }

    #[test]
    fn the_api_build_names_the_os_when_the_registry_is_stale_or_gone() {
        let mut sys = windows_11();
        sys.api_build = Some(22631);
        sys.os = Some(Ok(OsValues {
            product_name: Some("Windows 10 Home".into()),
            build: Some(19045),
            ..Default::default()
        }));
        assert_eq!(WinFacts::new(sys).facts().os, "Windows 11 Home (build 22631)");
        let sys = Recorded {
            api_build: Some(26100),
            ..Default::default()
        };
        assert_eq!(WinFacts::new(sys).facts().os, "Windows (build 26100)");
    }

    #[test]
    fn a_failed_overlay_call_is_unavailable_never_a_guess_of_no_overlay() {
        let mut sys = windows_11();
        sys.power = Some(Ok(PowerRead {
            scheme: Some("381b4222-f694-41f0-9685-ff5bb260df2e".into()),
            friendly_name: Some("Balanced".into()),
            overlay: Overlay::Failed(5),
        }));
        let f = WinFacts::new(sys).facts();
        assert_eq!(
            f.power_plan,
            Reading::Unavailable("Windows did not report the power mode (error 5).".into())
        );
        // A machine without the call (Windows 10) is the scheme alone, and that is a real answer.
        let mut sys = windows_11();
        sys.power = Some(Ok(PowerRead {
            scheme: Some("a1841308-3541-4fab-bc81-f71556f20b4a".into()),
            friendly_name: Some("Power saver".into()),
            overlay: Overlay::NotPresent,
        }));
        assert!(WinFacts::new(sys).facts().power_plan.value().unwrap().power_saver);
    }

    #[test]
    fn the_reader_has_no_state_between_gatherings() {
        let mut w = WinFacts::new(windows_11());
        assert_eq!(w.facts(), w.facts());
    }
}
