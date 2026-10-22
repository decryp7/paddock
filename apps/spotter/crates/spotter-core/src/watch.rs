//! Watches: what to monitor and how. A [`WatchSpec`] is the stored (and edited) form; [`Watch`] is the parsed one.

use crate::action::Action;
use crate::limits::{MAX_ACTIONS, MAX_NAME_CHARS};
use crate::notify::NotificationPolicy;
use crate::pattern::{NamePattern, PatternSyntax};
use crate::release::ReleaseFilter;
use crate::schedule::clamp_interval;
use crate::source::{PluginKinds, Source, SourceConfig};
use crate::version::VersionScheme;
use serde::{Deserialize, Serialize};

/// A watch's stable id: 1 to 40 of `a-z`, `0-9`, `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WatchId(String);

impl WatchId {
    /// Parses an id.
    pub fn parse(s: &str) -> Result<WatchId, String> {
        if (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            Ok(WatchId(s.to_string()))
        } else {
            Err(format!("\"{s}\" is not a watch id."))
        }
    }

    /// The id from random bytes (the app passes random bytes; the core does not generate randomness).
    pub fn from_random(bytes: [u8; 8]) -> WatchId {
        WatchId(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for WatchId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        WatchId::parse(&s)
    }
}

impl From<WatchId> for String {
    fn from(id: WatchId) -> String {
        id.0
    }
}

impl std::fmt::Display for WatchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn default_scheme() -> String {
    "semver".into()
}

fn default_interval() -> u32 {
    300
}

/// A watch as stored in `spotter.toml` and as the edit form sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchSpec {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Where releases come from.
    pub source: SourceConfig,
    /// The version scheme id (`semver`, `numeric`, `build-number`, `date`, `custom`).
    #[serde(default = "default_scheme")]
    pub versions: String,
    /// The pattern of a custom scheme.
    #[serde(default)]
    pub version_pattern: Option<String>,
    /// Keep pre-releases.
    #[serde(default)]
    pub include_prereleases: bool,
    /// Only releases whose name matches this glob (empty for all).
    #[serde(default)]
    pub name_filter: String,
    /// Seconds between checks.
    #[serde(default = "default_interval")]
    pub interval_secs: u32,
    /// When to notify.
    #[serde(default)]
    pub notify: NotificationPolicy,
    /// What to do with a new release.
    #[serde(default)]
    pub actions: Vec<Action>,
    /// Not checked while paused.
    #[serde(default)]
    pub paused: bool,
}

/// A parsed watch.
#[derive(Debug, Clone, PartialEq)]
pub struct Watch {
    /// Stable id.
    pub id: WatchId,
    /// Display name.
    pub name: String,
    /// The source.
    pub source: Source,
    /// The version scheme.
    pub scheme: VersionScheme,
    /// Which releases count.
    pub filter: ReleaseFilter,
    /// Seconds between checks (clamped to the allowed range).
    pub interval_secs: u32,
    /// When to notify.
    pub notify: NotificationPolicy,
    /// What to do with a new release.
    pub actions: Vec<Action>,
    /// Not checked while paused.
    pub paused: bool,
    /// The stored form.
    pub spec: WatchSpec,
}

impl Watch {
    /// Parses a spec; the error lists every problem.
    pub fn parse(spec: &WatchSpec, plugins: &dyn PluginKinds) -> Result<Watch, Vec<String>> {
        let mut problems = Vec::new();
        let id = WatchId::parse(&spec.id).map_err(|e| problems.push(e)).ok();
        let name = spec.name.trim().to_string();
        if name.is_empty() {
            problems.push("Give the watch a name.".into());
        } else if name.chars().count() > MAX_NAME_CHARS {
            problems.push(format!("The name is too long (at most {MAX_NAME_CHARS} characters)."));
        }
        let source = Source::parse(&spec.source, plugins).map_err(|e| problems.extend(e)).ok();
        let scheme = VersionScheme::parse(&spec.versions, spec.version_pattern.as_deref())
            .map_err(|e| problems.push(e))
            .ok();
        let name_filter = NamePattern::new(PatternSyntax::Glob, &spec.name_filter)
            .map_err(|e| problems.push(e))
            .ok();
        if spec.actions.len() > MAX_ACTIONS {
            problems.push(format!("A watch can have at most {MAX_ACTIONS} actions."));
        }
        problems.extend(spec.actions.iter().filter_map(|a| a.check().err()));
        match (id, source, scheme, name_filter) {
            (Some(id), Some(source), Some(scheme), Some(name_filter)) if problems.is_empty() => Ok(Watch {
                id,
                name,
                source,
                scheme,
                filter: ReleaseFilter {
                    include_prereleases: spec.include_prereleases,
                    name: name_filter,
                },
                interval_secs: clamp_interval(spec.interval_secs),
                notify: spec.notify,
                actions: spec.actions.clone(),
                paused: spec.paused,
                spec: spec.clone(),
            }),
            _ => Err(problems),
        }
    }
}

/// The colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Follow the system.
    #[default]
    System,
    /// Light.
    Light,
    /// Dark.
    Dark,
}

fn yes() -> bool {
    true
}

/// App-wide settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Colour theme.
    #[serde(default)]
    pub theme: Theme,
    /// Notifications at all.
    #[serde(default = "yes")]
    pub notifications: bool,
    /// Notifications are paused until then (milliseconds since the epoch); from the dashboard's quick action.
    #[serde(default)]
    pub notifications_paused_until: Option<i64>,
    /// Check for Spotter's own updates automatically (off by default; a manual check always works).
    #[serde(default)]
    pub check_app_updates: bool,
    /// Where downloads go when an action names no folder, and the dashboard's "open downloads".
    #[serde(default)]
    pub downloads_folder: Option<String>,
    /// The check interval new watches start with.
    #[serde(default = "default_interval")]
    pub default_interval_secs: u32,
    /// Plugins that are installed but switched off.
    #[serde(default)]
    pub disabled_plugins: Vec<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            theme: Theme::System,
            notifications: true,
            notifications_paused_until: None,
            check_app_updates: false,
            downloads_folder: None,
            default_interval_secs: default_interval(),
            disabled_plugins: Vec::new(),
        }
    }
}

impl Settings {
    /// Whether notifications go out at `now_ms`.
    pub fn notifications_active(&self, now_ms: i64) -> bool {
        self.notifications && self.notifications_paused_until.is_none_or(|until| now_ms >= until)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::notify::NotifyMode;

    pub(crate) fn spec(id: &str) -> WatchSpec {
        WatchSpec {
            id: id.into(),
            name: "Nightly".into(),
            source: SourceConfig::new("folder", &[("path", "/drops")]),
            versions: "semver".into(),
            version_pattern: None,
            include_prereleases: false,
            name_filter: String::new(),
            interval_secs: 60,
            notify: NotificationPolicy::default(),
            actions: Vec::new(),
            paused: false,
        }
    }

    #[test]
    fn ids_parse() {
        assert!(WatchId::parse("abc-123").is_ok());
        for bad in ["", "ABC", "a b", &"a".repeat(41)] {
            assert!(WatchId::parse(bad).is_err());
        }
        let id = WatchId::from_random([0, 1, 2, 255, 4, 5, 6, 7]);
        assert_eq!(id.as_str(), "000102ff04050607");
        assert_eq!(id.to_string(), "000102ff04050607");
        assert_eq!(serde_json::from_str::<WatchId>("\"x1\"").unwrap().as_str(), "x1");
        assert!(serde_json::from_str::<WatchId>("\"X\"").is_err());
        assert_eq!(String::from(id.clone()), id.as_str());
    }

    #[test]
    fn specs_parse_into_watches() {
        let w = Watch::parse(&spec("w1"), &()).unwrap();
        assert_eq!((w.name.as_str(), w.interval_secs, w.scheme.id()), ("Nightly", 60, "semver"));
        let mut s = spec("w1");
        s.interval_secs = 1;
        s.name_filter = "*.zip".into();
        s.notify.mode = NotifyMode::Off;
        let w = Watch::parse(&s, &()).unwrap();
        assert_eq!(w.interval_secs, crate::limits::MIN_INTERVAL_SECS);
        assert!(!w.filter.name.is_any());
        assert_eq!(w.spec, s);
    }

    #[test]
    fn every_problem_is_listed() {
        let mut s = spec("BAD ID");
        s.name = " ".into();
        s.source = SourceConfig::new("folder", &[]);
        s.versions = "custom".into();
        s.actions = vec![Action::Download {
            folder: "".into(),
            verify: true,
        }];
        let problems = Watch::parse(&s, &()).unwrap_err();
        assert_eq!(problems.len(), 5, "{problems:?}");
        let mut long = spec("x");
        long.name = "n".repeat(MAX_NAME_CHARS + 1);
        long.actions = vec![Action::OpenLocation; MAX_ACTIONS + 1];
        assert_eq!(Watch::parse(&long, &()).unwrap_err().len(), 2);
    }

    #[test]
    fn settings_default_and_pause() {
        let s: Settings = toml::from_str("").unwrap();
        assert_eq!(s, Settings::default());
        assert!(s.notifications_active(0));
        let paused = Settings {
            notifications_paused_until: Some(100),
            ..Settings::default()
        };
        assert!(!paused.notifications_active(99) && paused.notifications_active(100));
        let off = Settings {
            notifications: false,
            ..Settings::default()
        };
        assert!(!off.notifications_active(0));
        let themed: Settings = toml::from_str("theme = \"dark\"").unwrap();
        assert_eq!(themed.theme, Theme::Dark);
    }
}
