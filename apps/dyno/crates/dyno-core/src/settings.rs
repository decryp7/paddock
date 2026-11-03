//! The user's settings: versioned, every key optional (defaults fill the gaps), damaged files fall back to defaults.

use crate::alerts::AlertSettings;
use crate::history::Retention;
use crate::widgets::{DashboardCatalogue, MenuBarCatalogue, PopoverCatalogue};
use paddock_widgets::Stored;
use serde::{Deserialize, Serialize};

/// The settings schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// The largest settings file Dyno reads.
pub const MAX_SETTINGS_BYTES: usize = 256 * 1024;

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
    /// Start Dyno at login.
    pub launch_at_login: bool,
    /// Sampling.
    pub sampling: Sampling,
    /// History retention.
    pub retention: Retention,
    /// Alerts.
    pub alerts: AlertSettings,
    /// Local web API.
    pub web_api: WebApi,
    /// The dashboard's widgets and presets. Edited only through `EditWidgetLayout` (a saved `Settings` never replaces it).
    #[serde(with = "crate::widgets::stored_wire")]
    pub dashboard: Stored<DashboardCatalogue>,
    /// The menu-bar / tray items. Edited only through `EditWidgetLayout`.
    #[serde(with = "crate::widgets::stored_wire")]
    pub menu_bar: Stored<MenuBarCatalogue>,
    /// The tray popup's widgets. Edited only through `EditWidgetLayout`.
    #[serde(with = "crate::widgets::stored_wire")]
    pub popover: Stored<PopoverCatalogue>,
    /// Alarm behaviour (quiet hours, chatter detection, retention).
    pub alarms: crate::alarms::AlarmConfig,
    /// Keep the last ten minutes at one-second resolution for captures (backs off when Dyno's CPU budget is at risk).
    pub flight_recorder: bool,
    /// "Look up this IP": the https template the browser is sent to (Dyno never fetches).
    pub ip_lookup: crate::ip_lookup::IpLookupSettings,
    /// Alarms for new exposed listeners and new remote hosts (both off by default).
    pub network_alerts: crate::network_alerts::NetworkAlerts,
    /// The user's own alert rules (none by default).
    pub custom_alerts: Vec<crate::alarms::custom::CustomAlertRule>,
    /// Desktop notifications for new alarms (quiet hours and muted rules still apply).
    pub notifications: bool,
    /// Pinned performance counters and saved counter sets.
    pub counters: crate::counters::CounterSettings,
    /// "Is this normal?" baselines: learning period and alarms.
    pub baselines: crate::baseline::BaselineSettings,
    /// Alarms for hardware findings (throttling episodes, disks and batteries near the end of their life).
    pub hardware_alarms: HardwareAlarms,
    /// New and unusual programs: learn a baseline and list what is new (observe-only).
    pub unusual: crate::unusual::UnusualSettings,
    /// The disk map's limits.
    pub disk: crate::disk::settings::DiskSettings,
    /// Long operations run as jobs: how many at once.
    pub jobs: crate::jobs::JobSettings,
    /// Maintenance windows: when Dyno's heavier background work may run (off: any time).
    pub maintenance: crate::extras::maintenance::MaintenanceSettings,
    /// Backup health checks: how stale is too stale, and where to find the tools.
    pub backup: crate::backup::BackupSettings,
}

/// Which hardware findings may raise an alarm (the pages always show them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HardwareAlarms {
    /// A throttling episode that lasts (heat, power limit or battery saver).
    pub throttling: bool,
    /// A disk or battery with less than a year left, at medium confidence or better.
    pub lifespan: bool,
}

impl Default for HardwareAlarms {
    fn default() -> Self {
        HardwareAlarms {
            throttling: true,
            lifespan: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SCHEMA_VERSION,
            launch_at_login: false,
            sampling: Sampling::default(),
            retention: Retention::default(),
            alerts: AlertSettings::default(),
            web_api: WebApi::default(),
            dashboard: Default::default(),
            menu_bar: Default::default(),
            popover: Default::default(),
            alarms: Default::default(),
            flight_recorder: true,
            ip_lookup: Default::default(),
            network_alerts: Default::default(),
            custom_alerts: Vec::new(),
            notifications: true,
            counters: Default::default(),
            baselines: Default::default(),
            hardware_alarms: Default::default(),
            unusual: Default::default(),
            disk: Default::default(),
            jobs: Default::default(),
            maintenance: Default::default(),
            backup: Default::default(),
        }
    }
}

impl Settings {
    /// The settings with every value in its allowed range (what the agent stores and uses).
    pub fn normalised(mut self) -> Settings {
        self.version = SCHEMA_VERSION;
        self.sampling.interval_ms = self.sampling.interval_ms.clamp(500, 10_000);
        self.sampling.process_interval_ms = self
            .sampling
            .process_interval_ms
            .clamp(1_000, 30_000)
            .max(self.sampling.interval_ms);
        self.retention = self.retention.clamped();
        self.alerts = self.alerts.clamped();
        self.baselines = self.baselines.clamped();
        self.web_api.port = self.web_api.port.max(1024);
        self.alarms = self.alarms.clamped();
        self.ip_lookup = self.ip_lookup.normalised();
        self.custom_alerts = crate::alarms::custom::normalise_rules(self.custom_alerts);
        self.counters = self.counters.normalised();
        self.unusual = self.unusual.clamped();
        self.disk = self.disk.clamped();
        self.jobs = self.jobs.clamped();
        // A damaged value falls back to its default rather than blocking the whole file.
        self.maintenance.windows.truncate(crate::extras::maintenance::MAX_WINDOWS);
        if self.maintenance.validate().is_err() {
            self.maintenance = Default::default();
        }
        if self.backup.validate().is_err() {
            self.backup = Default::default();
        }
        self
    }

    /// What the user must fix before these settings are saved (the lenient [`Settings::normalised`] would silently reset
    /// it): a maintenance window or a backup override that does not make sense.
    pub fn check_extras(&self) -> Result<(), String> {
        self.maintenance.validate()?;
        self.backup.validate()
    }
}

/// Reads a settings file: unknown keys are ignored, missing ones defaulted, a damaged or newer file falls back to the
/// defaults with a warning (the agent keeps the damaged file aside).
pub fn parse(text: &str) -> (Settings, Option<String>) {
    if text.len() > MAX_SETTINGS_BYTES {
        return (
            Settings::default(),
            Some("The settings file is too large; using the defaults.".into()),
        );
    }
    match serde_json::from_str::<Settings>(text) {
        Ok(s) if s.version > SCHEMA_VERSION => (
            Settings::default(),
            Some(format!(
                "The settings were saved by a newer Dyno (version {}); using the defaults.",
                s.version
            )),
        ),
        Ok(s) => (s.normalised(), None),
        Err(e) => (
            Settings::default(),
            Some(format!("The settings file is damaged ({e}); using the defaults.")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_take_defaults() {
        let (s, w) = parse(r#"{"launchAtLogin":true,"sampling":{"intervalMs":2000},"futureKey":1}"#);
        assert!(w.is_none());
        assert!(s.launch_at_login);
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
        s.web_api.port = 80;
        let n = s.normalised();
        assert_eq!((n.sampling.interval_ms, n.sampling.process_interval_ms), (500, 30_000));
        assert_eq!(n.web_api.port, 1024);
        let j = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(j["webApi"]["port"], 47_800);
    }

    #[test]
    fn the_three_layouts_are_optional_and_default() {
        let (s, w) = parse(r#"{"launchAtLogin":true}"#);
        assert!(w.is_none());
        assert_eq!(s.dashboard, Stored::default());
        assert_eq!(s.menu_bar, Stored::default());
        assert_eq!(s.popover, Stored::default());
        assert!(s.dashboard.notes().is_empty() && s.menu_bar.notes().is_empty() && s.popover.notes().is_empty());
        let j = serde_json::to_value(Settings::default()).unwrap();
        assert!(j["dashboard"]["presets"].is_array());
        assert_eq!(j["menuBar"]["placements"][0]["widget"]["verticalBars"]["metric"], "cpuTotal");
        assert_eq!(j["popover"]["placements"][0]["widget"], "activeAlarms");
    }

    #[test]
    fn the_old_tray_keys_and_the_old_dashboard_shape_are_not_read() {
        // Dyno 1.0 is unreleased: `tray` and `trayItems` are dropped outright; an old-shape dashboard gives the defaults with
        // a note and is kept (written back unchanged) until the person edits the dashboard.
        let old = r#"{"tray":{"metrics":["cpu"],"style":"bars"},"trayItems":{"version":1,"items":[]},
            "dashboard":{"version":1,"widgets":[{"id":"cpu","kind":"graph","metric":"cpuTotal"}],"presets":[]}}"#;
        let (s, w) = parse(old);
        assert!(w.is_none());
        assert_eq!(s.menu_bar, Stored::default());
        assert_eq!(s.dashboard.layout(), Stored::<DashboardCatalogue>::default().layout());
        assert_eq!(s.dashboard.notes().len(), 1);
        let back = serde_json::to_value(&s).unwrap();
        assert!(back.get("tray").is_none() && back.get("trayItems").is_none());
        assert_eq!(back["dashboard"]["widgets"][0]["id"], "cpu");
    }

    #[test]
    fn settings_survive_postcard_with_their_layouts() {
        let mut s = Settings::default();
        s.popover
            .edit(paddock_widgets::Edit::Remove {
                id: paddock_widgets::PlacementId::new("cpu-memory").unwrap(),
            })
            .unwrap();
        let back: Settings = postcard::from_bytes(&postcard::to_stdvec(&s).unwrap()).unwrap();
        assert_eq!(back.popover.layout().placements, s.popover.layout().placements);
        assert_eq!(back.dashboard.layout().placements, s.dashboard.layout().placements);
        assert_eq!(back.menu_bar.layout().placements, s.menu_bar.layout().placements);
    }

    #[test]
    fn disk_settings_are_optional_clamped_and_camel_case() {
        let (s, w) = parse(r#"{"theme":"dark"}"#);
        assert!(w.is_none());
        assert_eq!(s.disk, crate::disk::settings::DiskSettings::default());
        let (s, _) = parse(r#"{"disk":{"scanFolders":1,"notableFileMib":2}}"#);
        assert_eq!((s.disk.scan_folders, s.disk.notable_file_mib), (10_000, 2));
        let j = serde_json::to_value(Settings::default()).unwrap();
        // The schema of the two parallelism and cost sections (rule 51): these keys, these defaults, these shapes.
        assert_eq!(
            j["disk"],
            serde_json::json!({
                "scanFolders": 2_000_000,
                "notableFileMib": 10,
                "readThreads": "auto",
                "finderFiles": 500_000,
                "finderDepth": 48,
                "finderHashGib": 64,
                "savedMaps": {"enabled": true, "maxMb": 1024, "refreshOnOpen": "off"}
            })
        );
        assert_eq!(j["jobs"], serde_json::json!({"workers": "auto"}));
        let (s, _) = parse(
            r#"{"disk":{"readThreads":{"fixed":99},"finderFiles":1,"finderDepth":0,"finderHashGib":0},"jobs":{"workers":{"fixed":0}}}"#,
        );
        assert_eq!(
            (
                s.disk.read_threads,
                s.disk.finder_files,
                s.disk.finder_depth,
                s.disk.finder_hash_gib,
                s.jobs.workers
            ),
            (crate::threads::Threads::Fixed(16), 1_000, 1, 1, crate::threads::Threads::Fixed(1)),
            "the engine validates what the UI sends"
        );
        // A damaged value falls back to the defaults with a warning rather than failing the whole file.
        let (s, w) = parse(r#"{"disk":{"readThreads":"many"}}"#);
        assert!(w.is_some() || s.disk.read_threads == crate::threads::Threads::Auto);
    }

    #[test]
    fn unusual_settings_are_optional_and_clamped() {
        let (s, w) = parse(r#"{"launchAtLogin":true}"#);
        assert!(w.is_none());
        assert_eq!(s.unusual, crate::unusual::UnusualSettings::default());
        let (s, _) = parse(r#"{"unusual":{"learningDays":500}}"#);
        assert_eq!(s.unusual.learning_days, 60);
        assert!(s.unusual.enabled);
        let j = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(j["unusual"]["learningDays"], 7);
    }

    #[test]
    fn extras_settings_are_optional_validated_and_reset_when_damaged() {
        let (s, w) = parse(r#"{"launchAtLogin":true}"#);
        assert!(w.is_none());
        assert!(!s.maintenance.enabled && s.backup.alert_when_stale && s.backup.stale_after_hours == 48);
        let (s, _) = parse(r#"{"maintenance":{"enabled":true,"windows":[]},"backup":{"staleAfterHours":0}}"#);
        assert_eq!(s.maintenance, Default::default());
        assert_eq!(s.backup, Default::default());
        let mut s = Settings::default();
        assert!(s.check_extras().is_ok());
        s.maintenance.enabled = true;
        assert!(s.check_extras().unwrap_err().contains("Add a window"));
        s.maintenance.enabled = false;
        s.backup.stale_after_hours = 5000;
        assert!(s.check_extras().is_err());
    }
}
