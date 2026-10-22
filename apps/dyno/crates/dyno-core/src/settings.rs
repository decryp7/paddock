//! The user's settings: versioned, every key optional (defaults fill the gaps), damaged files fall back to defaults.

use crate::alerts::AlertSettings;
use crate::cleanup::{CleanupSchedule, DeleteMode};
use crate::history::Retention;
use serde::{Deserialize, Serialize};

/// The settings schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// The largest settings file Dyno reads.
pub const MAX_SETTINGS_BYTES: usize = 256 * 1024;

/// Colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow the OS.
    #[default]
    System,
    /// Dark.
    Dark,
    /// Light.
    Light,
}

/// A metric the tray icon can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrayMetric {
    /// CPU.
    Cpu,
    /// Memory.
    Memory,
    /// Disk activity.
    Disk,
    /// Network activity.
    Network,
    /// GPU.
    Gpu,
}

/// How the tray icon draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TrayStyle {
    /// Vertical bars, one per metric.
    #[default]
    Bars,
    /// A small line graph of the first metric.
    Graph,
}

/// Sampling intervals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sampling {
    /// System sensors (500–10 000 ms).
    pub interval_ms: u32,
    /// The process table (1 000–30 000 ms, ≥ `interval_ms`).
    pub process_interval_ms: u32,
}

impl Default for Sampling {
    fn default() -> Self {
        Sampling {
            interval_ms: 1_000,
            process_interval_ms: 2_000,
        }
    }
}

/// Tray icon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tray {
    /// Which metrics (1–3).
    pub metrics: Vec<TrayMetric>,
    /// Style.
    pub style: TrayStyle,
}

impl Default for Tray {
    fn default() -> Self {
        Tray {
            metrics: vec![TrayMetric::Cpu, TrayMetric::Memory],
            style: TrayStyle::Bars,
        }
    }
}

/// Cleanup preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CleanupSettings {
    /// Scheduled runs.
    pub schedule: CleanupSchedule,
    /// Categories scheduled runs include (the user enabled them).
    pub categories: Vec<String>,
    /// Where deleted items go.
    pub mode: DeleteMode,
    /// Projects untouched this many days offer their build output (7–365).
    pub dev_project_age_days: u32,
}

impl Default for CleanupSettings {
    fn default() -> Self {
        CleanupSettings {
            schedule: CleanupSchedule::Off,
            categories: Vec::new(),
            mode: DeleteMode::RecycleBin,
            dev_project_age_days: 30,
        }
    }
}

/// The optional local web page / API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebApi {
    /// Off by default.
    pub enabled: bool,
    /// Port (1024–65 535).
    pub port: u16,
    /// Listen on the LAN (opt-in; requires TLS and the access key).
    pub lan: bool,
}

impl Default for WebApi {
    fn default() -> Self {
        WebApi {
            enabled: false,
            port: 47_800,
            lan: false,
        }
    }
}

/// All settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Schema version.
    pub version: u32,
    /// Theme.
    pub theme: Theme,
    /// Start Dyno at login.
    pub launch_at_login: bool,
    /// Sampling.
    pub sampling: Sampling,
    /// History retention.
    pub retention: Retention,
    /// Tray icon.
    pub tray: Tray,
    /// Alerts.
    pub alerts: AlertSettings,
    /// Cleanup.
    pub cleanup: CleanupSettings,
    /// Local web API.
    pub web_api: WebApi,
    /// The dashboard's widgets and presets.
    pub dashboard: crate::widgets::DashboardLayout,
    /// The tray / menu-bar items.
    pub tray_items: crate::widgets::TrayItems,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SCHEMA_VERSION,
            theme: Theme::System,
            launch_at_login: false,
            sampling: Sampling::default(),
            retention: Retention::default(),
            tray: Tray::default(),
            alerts: AlertSettings::default(),
            cleanup: CleanupSettings::default(),
            web_api: WebApi::default(),
            dashboard: Default::default(),
            tray_items: Default::default(),
        }
    }
}

impl Settings {
    /// The settings with every value in its allowed range (what the agent stores and uses).
    pub fn normalised(mut self) -> Settings {
        self.version = SCHEMA_VERSION;
        self.sampling.interval_ms = self.sampling.interval_ms.clamp(500, 10_000);
        self.sampling.process_interval_ms = self.sampling.process_interval_ms.clamp(1_000, 30_000).max(self.sampling.interval_ms);
        self.retention = self.retention.clamped();
        self.tray.metrics.dedup();
        self.tray.metrics.truncate(3);
        if self.tray.metrics.is_empty() {
            self.tray.metrics = Tray::default().metrics;
        }
        self.alerts = self.alerts.clamped();
        self.cleanup.dev_project_age_days = self.cleanup.dev_project_age_days.clamp(7, 365);
        self.cleanup.categories.retain(|c| !c.is_empty() && c.len() <= 64);
        self.cleanup.categories.truncate(100);
        self.web_api.port = self.web_api.port.max(1024);
        self.dashboard = self.dashboard.normalised();
        self.tray_items = self.tray_items.normalised();
        self
    }
}

/// Reads a settings file: unknown keys are ignored, missing ones defaulted, a damaged or newer file falls back to the
/// defaults with a warning (the agent keeps the damaged file aside).
pub fn parse(text: &str) -> (Settings, Option<String>) {
    if text.len() > MAX_SETTINGS_BYTES {
        return (Settings::default(), Some("The settings file is too large; using the defaults.".into()));
    }
    match serde_json::from_str::<Settings>(text) {
        Ok(s) if s.version > SCHEMA_VERSION => (
            Settings::default(),
            Some(format!("The settings were saved by a newer Dyno (version {}); using the defaults.", s.version)),
        ),
        Ok(s) => (s.normalised(), None),
        Err(e) => (Settings::default(), Some(format!("The settings file is damaged ({e}); using the defaults."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_take_defaults() {
        let (s, w) = parse(r#"{"theme":"dark","sampling":{"intervalMs":2000},"futureKey":1}"#);
        assert!(w.is_none());
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.sampling.interval_ms, 2_000);
        assert_eq!(s.sampling.process_interval_ms, 2_000);
        assert_eq!(s.retention, Retention::default());
        assert_eq!(parse("{}").0, Settings::default());
    }

    #[test]
    fn damage_and_newer_versions_fall_back() {
        let (s, w) = parse("{not json");
        assert_eq!(s, Settings::default());
        assert!(w.unwrap().contains("damaged"));
        let (_, w) = parse(r#"{"version":99}"#);
        assert!(w.unwrap().contains("newer"));
        let (_, w) = parse(&" ".repeat(MAX_SETTINGS_BYTES + 1));
        assert!(w.unwrap().contains("too large"));
    }

    #[test]
    fn values_are_clamped() {
        let mut s = Settings::default();
        s.sampling = Sampling {
            interval_ms: 1,
            process_interval_ms: 999_999,
        };
        s.tray.metrics = vec![TrayMetric::Cpu, TrayMetric::Cpu, TrayMetric::Disk, TrayMetric::Gpu, TrayMetric::Network];
        s.cleanup.dev_project_age_days = 1;
        s.cleanup.categories = vec![String::new(), "userTemp".into()];
        s.web_api.port = 80;
        let n = s.normalised();
        assert_eq!((n.sampling.interval_ms, n.sampling.process_interval_ms), (500, 30_000));
        assert_eq!(n.tray.metrics, vec![TrayMetric::Cpu, TrayMetric::Disk, TrayMetric::Gpu]);
        assert_eq!(n.cleanup.dev_project_age_days, 7);
        assert_eq!(n.cleanup.categories, vec!["userTemp".to_string()]);
        assert_eq!(n.web_api.port, 1024);
        let mut e = Settings::default();
        e.tray.metrics.clear();
        assert_eq!(e.normalised().tray.metrics, Tray::default().metrics);
        let j = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(j["webApi"]["port"], 47_800);
        assert_eq!(j["cleanup"]["mode"], "recycleBin");
    }

    #[test]
    fn dashboard_and_tray_items_are_normalised_and_optional() {
        // Old files without the new keys still load with the defaults.
        let (s, w) = parse(r#"{"theme":"dark"}"#);
        assert!(w.is_none());
        assert_eq!(s.dashboard, crate::widgets::DashboardLayout::default());
        assert_eq!(s.tray_items, crate::widgets::TrayItems::default());
        let mut s = Settings::default();
        s.dashboard.widgets.clear();
        s.tray_items.items.clear();
        let n = s.normalised();
        assert!(!n.dashboard.widgets.is_empty());
        assert_eq!(n.tray_items.items.len(), 2);
        let j = serde_json::to_value(Settings::default()).unwrap();
        assert!(j["dashboard"]["presets"].is_array());
        assert_eq!(j["trayItems"]["items"][0]["metric"], "cpuTotal");
    }
}
