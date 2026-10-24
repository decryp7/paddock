//! The user's choices for OS notifications: which severities notify, quiet hours, and the rate limits. One value per
//! app, shared by every window through [`SharedOsSettings`], so a change in Settings applies to the next notification
//! at once. The settings section itself (`pd.notifications`) lives in `paddock-shell`, which depends on the settings
//! service; this crate stays below it.

use super::time::QuietHours;
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use std::sync::{Arc, RwLock};

/// The seconds one cause stays quiet after its toast.
pub const COOLDOWN_SECS: RangeInclusive<u32> = 0..=3600;
/// The most toasts in one window of time.
pub const BURST: RangeInclusive<u32> = 1..=50;
/// The length of that window, in seconds.
pub const BURST_WINDOW_SECS: RangeInclusive<u32> = 10..=3600;

/// Which messages notify through the OS (Debrief's `NotificationLevel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OsLevel {
    /// Never.
    Off,
    /// Things that went wrong or need the user: warnings, errors and critical failures.
    #[default]
    Alerts,
    /// Also finished work and information.
    All,
}

/// The policy's inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OsSettings {
    /// Which severities notify.
    pub level: OsLevel,
    /// When toasts wait (local time); the messages are still in the notification centre.
    pub quiet_hours: Option<QuietHours>,
    /// Whether a critical failure still notifies inside quiet hours.
    pub critical_in_quiet_hours: bool,
    /// A cause that notified stays quiet for this long (seconds).
    pub cooldown_secs: u32,
    /// At most this many toasts...
    pub burst: u32,
    /// ...in this many seconds, whatever their causes.
    pub burst_window_secs: u32,
}

impl Default for OsSettings {
    fn default() -> OsSettings {
        OsSettings {
            level: OsLevel::default(),
            quiet_hours: None,
            critical_in_quiet_hours: true,
            cooldown_secs: 30,
            burst: 5,
            burst_window_secs: 300,
        }
    }
}

impl OsSettings {
    /// The same with every number inside its range (a settings file edited by hand cannot disable the limits).
    pub fn clamped(mut self) -> OsSettings {
        let fit = |v: u32, r: &RangeInclusive<u32>| v.clamp(*r.start(), *r.end());
        self.cooldown_secs = fit(self.cooldown_secs, &COOLDOWN_SECS);
        self.burst = fit(self.burst, &BURST);
        self.burst_window_secs = fit(self.burst_window_secs, &BURST_WINDOW_SECS);
        self
    }
}

/// The settings every window and thread reads: cheap to clone, changed in one place.
#[derive(Debug, Clone, Default)]
pub struct SharedOsSettings(Arc<RwLock<OsSettings>>);

impl SharedOsSettings {
    /// A handle holding `settings` (clamped).
    pub fn new(settings: OsSettings) -> SharedOsSettings {
        SharedOsSettings(Arc::new(RwLock::new(settings.clamped())))
    }

    /// The current settings.
    pub fn get(&self) -> OsSettings {
        *self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Replaces them (clamped); every clone sees the change.
    pub fn set(&self, settings: OsSettings) {
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = settings.clamped();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_debriefs_alerts_with_30_seconds_and_5_in_5_minutes() {
        let d = OsSettings::default();
        assert_eq!(
            (d.level, d.cooldown_secs, d.burst, d.burst_window_secs),
            (OsLevel::Alerts, 30, 5, 300)
        );
        assert!(d.quiet_hours.is_none() && d.critical_in_quiet_hours);
        assert_eq!(d.clamped(), d);
    }

    #[test]
    fn numbers_outside_their_range_are_pulled_in() {
        let wild = OsSettings {
            cooldown_secs: u32::MAX,
            burst: 0,
            burst_window_secs: 1,
            ..OsSettings::default()
        }
        .clamped();
        assert_eq!(
            (wild.cooldown_secs, wild.burst, wild.burst_window_secs),
            (3600, 1, 10)
        );
    }

    #[test]
    fn missing_keys_take_the_defaults_and_levels_use_camel_case() {
        let s: OsSettings =
            serde_json::from_str(r#"{"level":"all","quietHours":{"start":"22:00","end":"07:00"}}"#)
                .unwrap();
        assert_eq!(s.level, OsLevel::All);
        assert!(s.quiet_hours.is_some());
        assert_eq!(s.burst, 5);
        assert_eq!(
            serde_json::from_str::<OsSettings>("{}").unwrap(),
            OsSettings::default()
        );
        assert!(serde_json::from_str::<OsSettings>(r#"{"level":"loud"}"#).is_err());
    }

    #[test]
    fn a_change_through_one_handle_reaches_every_clone_and_is_clamped() {
        let a = SharedOsSettings::default();
        let b = a.clone();
        a.set(OsSettings {
            level: OsLevel::Off,
            burst: 999,
            ..OsSettings::default()
        });
        assert_eq!((b.get().level, b.get().burst), (OsLevel::Off, 50));
        assert_eq!(
            SharedOsSettings::new(OsSettings {
                burst: 0,
                ..OsSettings::default()
            })
            .get()
            .burst,
            1
        );
    }

    #[test]
    fn a_poisoned_lock_still_reads_and_writes() {
        let s = SharedOsSettings::default();
        let inner = s.0.clone();
        let _ = std::thread::spawn(move || {
            let _guard = inner.write().unwrap();
            panic!("poison");
        })
        .join();
        s.set(OsSettings {
            level: OsLevel::All,
            ..OsSettings::default()
        });
        assert_eq!(s.get().level, OsLevel::All);
    }
}
