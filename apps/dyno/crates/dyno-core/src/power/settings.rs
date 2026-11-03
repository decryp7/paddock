//! The power settings (ADR 0032 section 9). New keys are optional with defaults; unset thresholds fall back to them.

use serde::{Deserialize, Serialize};

/// Keep-awake settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeepAwakeSettings {
    /// The feature is available; it acts only when the user starts a hold.
    pub enabled: bool,
    /// Keep the display on too, by default (a hold can still say otherwise).
    pub display: bool,
    /// "While busy": how long every condition must be false before the hold ends, in seconds.
    pub grace_secs: u32,
    /// "While busy": CPU above this percentage counts as busy.
    pub cpu_pct: f32,
    /// "While busy": network above this many bytes per second counts as busy.
    pub net_bytes_per_s: u64,
    /// "While busy": disk above this many bytes per second counts as busy.
    pub disk_bytes_per_s: u64,
    /// Show a Keep-awake item in the tray (ADR 0028 item kind `awake`).
    pub tray_item: bool,
}

impl Default for KeepAwakeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            display: false,
            grace_secs: 120,
            cpu_pct: 50.0,
            net_bytes_per_s: 500 * 1024,
            disk_bytes_per_s: 5 * 1024 * 1024,
            tray_item: false,
        }
    }
}

/// All power settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PowerSettings {
    /// Keep awake.
    pub keep_awake: KeepAwakeSettings,
    /// The energy report.
    pub energy: super::energy::EnergySettings,
    /// Sleep, hibernate or shut down after the computer has been idle (off by default).
    pub idle: super::idle::IdleSettings,
    /// Sleep, hibernate or shut down at a time of day (off by default).
    pub timed: super::idle::TimedSettings,
}

impl PowerSettings {
    /// The settings with every value in its allowed range.
    pub fn normalised(mut self) -> Self {
        self.energy = self.energy.normalised();
        self.idle = self.idle.normalised();
        let k = &mut self.keep_awake;
        k.grace_secs = k.grace_secs.clamp(10, 3600);
        k.cpu_pct = if k.cpu_pct.is_finite() { k.cpu_pct.clamp(1.0, 100.0) } else { 50.0 };
        k.net_bytes_per_s = k.net_bytes_per_s.clamp(1024, 10 * 1024 * 1024 * 1024);
        k.disk_bytes_per_s = k.disk_bytes_per_s.clamp(1024, 10 * 1024 * 1024 * 1024);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_adrs() {
        let k = KeepAwakeSettings::default();
        assert!(k.enabled && !k.display && !k.tray_item);
        assert_eq!((k.grace_secs, k.cpu_pct), (120, 50.0));
        assert_eq!((k.net_bytes_per_s, k.disk_bytes_per_s), (512_000, 5_242_880));
    }

    #[test]
    fn missing_keys_take_their_defaults() {
        let p: PowerSettings = serde_json::from_str(r#"{"keepAwake":{"display":true}}"#).unwrap();
        assert!(p.keep_awake.display);
        assert_eq!(p.keep_awake.grace_secs, 120);
        assert_eq!(serde_json::from_str::<PowerSettings>("{}").unwrap(), PowerSettings::default());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let mut p = PowerSettings::default();
        p.keep_awake.grace_secs = 0;
        p.keep_awake.cpu_pct = f32::NAN;
        p.keep_awake.net_bytes_per_s = 0;
        p.keep_awake.disk_bytes_per_s = u64::MAX;
        let p = p.normalised();
        assert_eq!(p.keep_awake.grace_secs, 10);
        assert_eq!(p.keep_awake.cpu_pct, 50.0);
        assert_eq!(p.keep_awake.net_bytes_per_s, 1024);
        assert_eq!(p.keep_awake.disk_bytes_per_s, 10 * 1024 * 1024 * 1024);
        let mut q = PowerSettings::default();
        q.keep_awake.cpu_pct = 500.0;
        assert_eq!(q.normalised().keep_awake.cpu_pct, 100.0);
    }
}
