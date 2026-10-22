//! The UI ↔ app wire contract (camelCase JSON), mirroring `app/src/app/core/api.types.ts`. Change both in the same
//! commit. Conversions from the core's types live here, apart from the commands that use them.

use serde::{Deserialize, Serialize};
use spotter_core::action::Action;
use spotter_core::dashboard::{self, NewItem, WatchNumbers};
use spotter_core::journal::{Event, EventKind};
use spotter_core::notify::{NotificationPolicy, NotifyMode};
use spotter_core::release::Release;
use spotter_core::source::SourceConfig;
use spotter_core::state::{CheckRecord, KnownRelease, WatchStatus};
use spotter_core::time::{ClockTime, QuietHours};
use spotter_core::watch::{Settings, Theme, WatchSpec};
use spotter_plugin_api::{FieldKind, FieldSpec, SourceKindSpec};
use std::collections::BTreeMap;

/// `WatchStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatusDto {
    /// `pending`, `ok`, `noMatch`, `unreachable`, `authNeeded`, `pluginError`, `invalid`, `failed`.
    pub kind: &'static str,
    /// Why, for failures.
    pub message: Option<String>,
}

impl From<&WatchStatus> for WatchStatusDto {
    fn from(s: &WatchStatus) -> Self {
        let (kind, message) = match s {
            WatchStatus::Pending => ("pending", None),
            WatchStatus::Ok => ("ok", None),
            WatchStatus::NoMatch => ("noMatch", None),
            WatchStatus::Unreachable(m) => ("unreachable", Some(m.clone())),
            WatchStatus::AuthNeeded(m) => ("authNeeded", Some(m.clone())),
            WatchStatus::PluginError(m) => ("pluginError", Some(m.clone())),
            WatchStatus::Invalid(m) => ("invalid", Some(m.clone())),
            WatchStatus::Failed(m) => ("failed", Some(m.clone())),
        };
        WatchStatusDto { kind, message }
    }
}

/// `Release`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDto {
    /// Version text.
    pub version: String,
    /// Pre-release label present.
    pub prerelease: bool,
    /// Name.
    pub name: String,
    /// URL or path.
    pub location: String,
    /// Published.
    pub published_at_ms: Option<i64>,
    /// First seen.
    pub first_seen_ms: i64,
    /// Size.
    pub size: Option<u64>,
    /// SHA-256 hex.
    pub sha256: Option<String>,
}

impl ReleaseDto {
    /// From a release and when it was first seen.
    pub fn new(r: &Release, first_seen_ms: i64) -> ReleaseDto {
        ReleaseDto {
            version: r.version.text.clone(),
            prerelease: r.version.is_prerelease(),
            name: r.name.clone(),
            location: r.location.clone(),
            published_at_ms: r.published_at.map(|t| t.0),
            first_seen_ms,
            size: r.size,
            sha256: r.sha256.map(|d| d.to_hex()),
        }
    }
}

impl From<&KnownRelease> for ReleaseDto {
    fn from(k: &KnownRelease) -> Self {
        ReleaseDto::new(&k.release, k.first_seen.0)
    }
}

/// Quiet hours (`HH:MM`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuietHoursDto {
    /// Start.
    pub start: String,
    /// End.
    pub end: String,
}

/// `NotificationPolicy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyDto {
    /// `every-new`, `newer-than-installed`, `off`.
    pub mode: String,
    /// Repeat until acknowledged.
    pub nag: bool,
    /// Quiet hours.
    pub quiet_hours: Option<QuietHoursDto>,
}

/// An action (same JSON shape as the core's).
pub type ActionDto = Action;

/// The source part of a draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDto {
    /// Kind id.
    pub kind: String,
    /// Values.
    pub values: BTreeMap<String, String>,
}

/// `WatchDraft`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchDraftDto {
    /// `None` for a new watch.
    pub id: Option<String>,
    /// Name.
    pub name: String,
    /// Source.
    pub source: SourceDto,
    /// Scheme id.
    pub versions: String,
    /// Custom scheme pattern.
    pub version_pattern: Option<String>,
    /// Keep pre-releases.
    pub include_prereleases: bool,
    /// Name glob.
    pub name_filter: String,
    /// Interval.
    pub interval_secs: u32,
    /// Notifications.
    pub notify: NotifyDto,
    /// Actions.
    pub actions: Vec<ActionDto>,
    /// Paused.
    pub paused: bool,
}

impl WatchDraftDto {
    /// The stored form, with `id`. The error lists every problem.
    pub fn to_spec(&self, id: String) -> Result<WatchSpec, Vec<String>> {
        let mode = match self.notify.mode.as_str() {
            "every-new" => NotifyMode::EveryNew,
            "newer-than-installed" => NotifyMode::NewerThanInstalled,
            "off" => NotifyMode::Off,
            other => return Err(vec![format!("\"{other}\" is not a notification mode.")]),
        };
        let quiet_hours = match &self.notify.quiet_hours {
            None => None,
            Some(q) => Some(QuietHours {
                start: ClockTime::try_from(q.start.clone()).map_err(|e| vec![e])?,
                end: ClockTime::try_from(q.end.clone()).map_err(|e| vec![e])?,
            }),
        };
        Ok(WatchSpec {
            id,
            name: self.name.clone(),
            source: SourceConfig {
                kind: self.source.kind.clone(),
                values: self.source.values.iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect(),
            },
            versions: self.versions.clone(),
            version_pattern: self.version_pattern.clone().filter(|p| !p.trim().is_empty()),
            include_prereleases: self.include_prereleases,
            name_filter: self.name_filter.clone(),
            interval_secs: self.interval_secs,
            notify: NotificationPolicy { mode, nag: self.notify.nag, quiet_hours },
            actions: self.actions.clone(),
            paused: self.paused,
        })
    }
}

impl From<&WatchSpec> for WatchDraftDto {
    fn from(s: &WatchSpec) -> Self {
        WatchDraftDto {
            id: Some(s.id.clone()),
            name: s.name.clone(),
            source: SourceDto {
                kind: s.source.kind.clone(),
                values: s.source.values.clone(),
            },
            versions: s.versions.clone(),
            version_pattern: s.version_pattern.clone(),
            include_prereleases: s.include_prereleases,
            name_filter: s.name_filter.clone(),
            interval_secs: s.interval_secs,
            notify: NotifyDto {
                mode: match s.notify.mode {
                    NotifyMode::EveryNew => "every-new",
                    NotifyMode::NewerThanInstalled => "newer-than-installed",
                    NotifyMode::Off => "off",
                }
                .into(),
                nag: s.notify.nag,
                quiet_hours: s.notify.quiet_hours.map(|q| QuietHoursDto {
                    start: q.start.into(),
                    end: q.end.into(),
                }),
            },
            actions: s.actions.clone(),
            paused: s.paused,
        }
    }
}

/// `WatchSummary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchSummaryDto {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Kind id.
    pub source_kind: String,
    /// Kind label.
    pub source_label: String,
    /// Where it looks.
    pub source_description: String,
    /// Status.
    pub status: WatchStatusDto,
    /// Paused.
    pub paused: bool,
    /// A check is running.
    pub checking: bool,
    /// Latest.
    pub latest: Option<ReleaseDto>,
    /// Acknowledged version.
    pub acknowledged: Option<String>,
    /// Installed version.
    pub installed: Option<String>,
    /// Latest newer than acknowledged.
    pub unseen: bool,
    /// Latest newer than installed.
    pub newer_than_installed: Option<bool>,
    /// Last check.
    pub last_check_ms: Option<i64>,
    /// Next check.
    pub next_check_ms: Option<i64>,
    /// Failures in a row.
    pub failures: u32,
    /// Interval.
    pub interval_secs: u32,
}

/// `CheckRecord`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRecordDto {
    /// When.
    pub at_ms: i64,
    /// Duration.
    pub took_ms: u64,
    /// Error.
    pub error: Option<String>,
    /// Found.
    pub found: usize,
}

impl From<&CheckRecord> for CheckRecordDto {
    fn from(c: &CheckRecord) -> Self {
        CheckRecordDto {
            at_ms: c.at.0,
            took_ms: c.took_ms,
            error: c.error.clone(),
            found: c.found,
        }
    }
}

/// `WatchDetail`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchDetailDto {
    /// Summary.
    pub summary: WatchSummaryDto,
    /// Stored form.
    pub spec: WatchDraftDto,
    /// History.
    pub history: Vec<ReleaseDto>,
    /// Checks.
    pub checks: Vec<CheckRecordDto>,
    /// Secret keys with a stored value.
    pub secrets_set: Vec<String>,
}

/// `FieldSpec`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldSpecDto {
    /// Key.
    pub key: String,
    /// Label.
    pub label: String,
    /// Help.
    pub help: String,
    /// Kind.
    pub kind: &'static str,
    /// Required.
    pub required: bool,
    /// Default.
    pub default: String,
    /// Placeholder.
    pub placeholder: String,
    /// Integer minimum.
    pub min: Option<i64>,
    /// Integer maximum.
    pub max: Option<i64>,
    /// Choice options.
    pub options: Vec<OptionDto>,
}

/// A choice option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptionDto {
    /// Value.
    pub value: String,
    /// Label.
    pub label: String,
}

impl From<&FieldSpec> for FieldSpecDto {
    fn from(f: &FieldSpec) -> Self {
        let (kind, min, max, options) = match &f.kind {
            FieldKind::Text => ("text", None, None, Vec::new()),
            FieldKind::Url => ("url", None, None, Vec::new()),
            FieldKind::Folder => ("folder", None, None, Vec::new()),
            FieldKind::Secret => ("secret", None, None, Vec::new()),
            FieldKind::Integer { min, max } => ("integer", Some(*min), Some(*max), Vec::new()),
            FieldKind::Toggle => ("toggle", None, None, Vec::new()),
            FieldKind::Choice(o) => (
                "choice",
                None,
                None,
                o.iter()
                    .map(|o| OptionDto {
                        value: o.value.clone(),
                        label: o.label.clone(),
                    })
                    .collect(),
            ),
        };
        FieldSpecDto {
            key: f.key.clone(),
            label: f.label.clone(),
            help: f.help.clone(),
            kind,
            required: f.required,
            default: f.default.clone(),
            placeholder: f.placeholder.clone(),
            min,
            max,
            options,
        }
    }
}

/// `SourceKind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceKindDto {
    /// Id.
    pub id: String,
    /// Label.
    pub label: String,
    /// Description.
    pub description: String,
    /// Group.
    pub group: &'static str,
    /// Plugin name.
    pub plugin: Option<String>,
    /// Form.
    pub fields: Vec<FieldSpecDto>,
}

impl SourceKindDto {
    /// A built-in kind.
    pub fn builtin(k: &SourceKindSpec) -> SourceKindDto {
        let group = match k.id.as_str() {
            "folder" => "files",
            "web-directory" | "feed" | "json-api" => "web",
            "github" | "gitea" | "gitlab" => "forge",
            _ => "registry",
        };
        SourceKindDto {
            id: k.id.clone(),
            label: k.label.clone(),
            description: k.description.clone(),
            group,
            plugin: None,
            fields: k.fields.iter().map(FieldSpecDto::from).collect(),
        }
    }

    /// A plugin's kind with its full id.
    pub fn plugin(full_id: String, plugin_name: String, k: &SourceKindSpec) -> SourceKindDto {
        SourceKindDto {
            id: full_id,
            label: k.label.clone(),
            description: k.description.clone(),
            group: "plugin",
            plugin: Some(plugin_name),
            fields: k.fields.iter().map(FieldSpecDto::from).collect(),
        }
    }
}

/// `SourcePreview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourcePreviewDto {
    /// Draft problems.
    pub problems: Vec<String>,
    /// The check's failure.
    pub error: Option<WatchStatusDto>,
    /// Releases.
    pub releases: Vec<ReleaseDto>,
    /// Unparsed names.
    pub without_version: Vec<String>,
    /// Skipped pre-releases.
    pub prereleases_skipped: usize,
    /// Skipped names.
    pub names_skipped: usize,
    /// Duration.
    pub took_ms: u64,
}

/// `WhatsNewItem`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNewDto {
    /// Watch id.
    pub watch_id: String,
    /// Watch name.
    pub watch_name: String,
    /// Kind id.
    pub source_kind: String,
    /// Release.
    pub release: ReleaseDto,
    /// Released.
    pub released_at_ms: i64,
    /// Newer than installed.
    pub newer_than_installed: Option<bool>,
    /// Unseen.
    pub unseen: bool,
}

impl WhatsNewDto {
    /// From the core's item, with the watch's name and kind.
    pub fn new(n: &NewItem, name: String, kind: String, first_seen_ms: i64) -> WhatsNewDto {
        WhatsNewDto {
            watch_id: n.watch.to_string(),
            watch_name: name,
            source_kind: kind,
            release: ReleaseDto::new(&n.release, first_seen_ms),
            released_at_ms: n.released_at.0,
            newer_than_installed: n.newer_than_installed,
            unseen: n.unseen,
        }
    }
}

/// `WatchNumbers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchNumbersDto {
    /// Watch id.
    pub watch_id: String,
    /// Average interval.
    pub average_interval_ms: Option<i64>,
    /// Since last.
    pub since_last_ms: Option<i64>,
    /// Releases per bucket.
    pub releases: Vec<u32>,
    /// Failed checks per bucket.
    pub failed_checks: Vec<u32>,
    /// Checks per bucket.
    pub checks: Vec<u32>,
    /// Sparkline.
    pub sparkline: Vec<u32>,
}

impl From<&WatchNumbers> for WatchNumbersDto {
    fn from(w: &WatchNumbers) -> Self {
        WatchNumbersDto {
            watch_id: w.watch.to_string(),
            average_interval_ms: w.average_interval_ms,
            since_last_ms: w.since_last_ms,
            releases: w.releases.clone(),
            failed_checks: w.failed_checks.clone(),
            checks: w.checks.clone(),
            sparkline: w.sparkline.clone(),
        }
    }
}

/// `DownloadBucket`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DownloadBucketDto {
    /// Worked.
    pub count: u32,
    /// Failed.
    pub failed: u32,
    /// Bytes.
    pub bytes: u64,
}

impl From<&dashboard::DownloadBucket> for DownloadBucketDto {
    fn from(d: &dashboard::DownloadBucket) -> Self {
        DownloadBucketDto {
            count: d.count,
            failed: d.failed,
            bytes: d.bytes,
        }
    }
}

/// `Dashboard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardDto {
    /// Range.
    pub range: String,
    /// Buckets.
    pub buckets_ms: Vec<i64>,
    /// What's new.
    pub whats_new: Vec<WhatsNewDto>,
    /// Per watch.
    pub watches: Vec<WatchNumbersDto>,
    /// Downloads.
    pub downloads: Vec<DownloadBucketDto>,
    /// Cost.
    pub compute_micros: u64,
}

/// `AppEvent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppEventDto {
    /// When.
    pub at_ms: i64,
    /// Kind.
    pub kind: &'static str,
    /// Watch id.
    pub watch_id: Option<String>,
    /// Watch name.
    pub watch_name: Option<String>,
    /// Version.
    pub version: Option<String>,
    /// Name.
    pub name: Option<String>,
    /// Message.
    pub message: Option<String>,
    /// Path.
    pub path: Option<String>,
    /// Size.
    pub size: Option<u64>,
    /// Verified.
    pub verified: Option<bool>,
    /// Action.
    pub action: Option<String>,
    /// Plugin.
    pub plugin: Option<String>,
}

impl AppEventDto {
    /// From a journal event and its watch's name.
    pub fn new(e: &Event, watch_name: Option<String>) -> AppEventDto {
        let mut d = AppEventDto {
            at_ms: e.at.0,
            watch_id: e.watch.as_ref().map(|w| w.to_string()),
            watch_name,
            ..AppEventDto::default()
        };
        match &e.kind {
            EventKind::NewRelease { version, name } => {
                d.kind = "new-release";
                d.version = Some(version.clone());
                d.name = Some(name.clone());
            }
            EventKind::CheckFailed { message } => {
                d.kind = "check-failed";
                d.message = Some(message.clone());
            }
            EventKind::Recovered => d.kind = "recovered",
            EventKind::DownloadFinished { version, path, size, verified } => {
                d.kind = "download-finished";
                d.version = Some(version.clone());
                d.path = Some(path.clone());
                d.size = Some(*size);
                d.verified = Some(*verified);
            }
            EventKind::ActionFailed { action, message } => {
                d.kind = "action-failed";
                d.action = Some(action.clone());
                d.message = Some(message.clone());
            }
            EventKind::PluginIssue { plugin, message } => {
                d.kind = "plugin-issue";
                d.plugin = Some(plugin.clone());
                d.message = Some(message.clone());
            }
        }
        d
    }
}

/// `Settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// `system`, `light`, `dark`.
    pub theme: String,
    /// Notifications.
    pub notifications: bool,
    /// Paused until.
    pub notifications_paused_until_ms: Option<i64>,
    /// Automatic update checks.
    pub check_app_updates: bool,
    /// Downloads folder.
    pub downloads_folder: Option<String>,
    /// Default interval.
    pub default_interval_secs: u32,
    /// Disabled plugins.
    pub disabled_plugins: Vec<String>,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        SettingsDto {
            theme: match s.theme {
                Theme::System => "system",
                Theme::Light => "light",
                Theme::Dark => "dark",
            }
            .into(),
            notifications: s.notifications,
            notifications_paused_until_ms: s.notifications_paused_until,
            check_app_updates: s.check_app_updates,
            downloads_folder: s.downloads_folder.clone(),
            default_interval_secs: s.default_interval_secs,
            disabled_plugins: s.disabled_plugins.clone(),
        }
    }
}

impl SettingsDto {
    /// The core's settings; the error says what is wrong.
    pub fn to_settings(&self) -> Result<Settings, String> {
        let theme = match self.theme.as_str() {
            "system" => Theme::System,
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            other => return Err(format!("\"{other}\" is not a theme.")),
        };
        Ok(Settings {
            theme,
            notifications: self.notifications,
            notifications_paused_until: self.notifications_paused_until_ms,
            check_app_updates: self.check_app_updates,
            downloads_folder: self.downloads_folder.clone().filter(|f| !f.trim().is_empty()),
            default_interval_secs: spotter_core::schedule::clamp_interval(self.default_interval_secs),
            disabled_plugins: self.disabled_plugins.clone(),
        })
    }
}

/// `PluginInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoDto {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Version.
    pub version: Option<String>,
    /// Folder.
    pub dir: String,
    /// Kinds.
    pub kinds: Vec<KindRef>,
    /// Why not usable.
    pub problem: Option<String>,
    /// Disabled.
    pub disabled: bool,
    /// Quarantined.
    pub quarantined: bool,
    /// Last problem.
    pub last_problem: Option<String>,
    /// Recent strikes.
    pub strikes: usize,
}

/// A kind reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KindRef {
    /// Full id.
    pub id: String,
    /// Label.
    pub label: String,
}

impl From<&spotter_plugin_host::PluginStatus> for PluginInfoDto {
    fn from(s: &spotter_plugin_host::PluginStatus) -> Self {
        let i = &s.installed;
        let m = i.manifest.as_ref();
        PluginInfoDto {
            id: i.id.clone(),
            name: m.map_or_else(|| i.id.clone(), |m| m.name.clone()),
            version: m.map(|m| m.version.clone()),
            dir: i.dir.display().to_string(),
            kinds: m
                .map(|m| {
                    m.kinds
                        .iter()
                        .map(|k| KindRef {
                            id: format!("{}/{}", i.id, k.id),
                            label: k.label.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            problem: i.problem.clone(),
            disabled: s.disabled,
            quarantined: s.health.quarantined,
            last_problem: s.health.last_problem.clone(),
            strikes: s.health.strikes.len(),
        }
    }
}

/// `AppInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    /// Version.
    pub version: String,
    /// `spotter.toml`.
    pub config_path: String,
    /// Logs.
    pub logs_dir: String,
    /// Plugins.
    pub plugins_dir: String,
    /// Downloads.
    pub downloads_dir: String,
    /// Load warnings.
    pub warnings: Vec<String>,
    /// Migrated from Release Monitor at this start.
    pub migrated_from_legacy: bool,
    /// `windows`, `macos`, `linux`.
    pub platform: &'static str,
}

/// `AppUpdateStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatusDto {
    /// Whether this build can update itself.
    pub available: bool,
    /// Why not.
    pub reason: Option<String>,
    /// Version.
    pub current_version: String,
}

/// `AppUpdateCheck`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateCheckDto {
    /// A newer version exists.
    pub update_available: bool,
    /// Its version.
    pub version: Option<String>,
    /// Its notes.
    pub notes: Option<String>,
}

/// `StatePush`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatePushDto {
    /// Every watch.
    pub watches: Vec<WatchSummaryDto>,
}

/// A command error (`CommandError` in api.types.ts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct CommandError {
    /// `badRequest`, `notFound`, `invalid`, `unavailable`, `internal`.
    pub code: &'static str,
    /// For the user.
    pub message: String,
    /// Every problem, for `invalid`.
    pub problems: Vec<String>,
}

impl CommandError {
    /// A bad argument.
    pub fn bad_request(message: impl Into<String>) -> CommandError {
        CommandError {
            code: "badRequest",
            message: message.into(),
            problems: Vec::new(),
        }
    }

    /// Something that does not exist.
    pub fn not_found(message: impl Into<String>) -> CommandError {
        CommandError {
            code: "notFound",
            message: message.into(),
            problems: Vec::new(),
        }
    }

    /// Input with problems.
    pub fn invalid(problems: Vec<String>) -> CommandError {
        CommandError {
            code: "invalid",
            message: problems.first().cloned().unwrap_or_else(|| "The input is not valid.".into()),
            problems,
        }
    }

    /// Something not available here or now.
    pub fn unavailable(message: impl Into<String>) -> CommandError {
        CommandError {
            code: "unavailable",
            message: message.into(),
            problems: Vec::new(),
        }
    }

    /// Anything else.
    pub fn internal(message: impl Into<String>) -> CommandError {
        CommandError {
            code: "internal",
            message: message.into(),
            problems: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn draft() -> WatchDraftDto {
        serde_json::from_value(json!({
            "id": null, "name": "N", "source": {"kind": "folder", "values": {"path": "/d", "pattern": " "}},
            "versions": "semver", "versionPattern": " ", "includePrereleases": false, "nameFilter": "",
            "intervalSecs": 60, "notify": {"mode": "newer-than-installed", "nag": true, "quietHours": {"start": "22:00", "end": "07:00"}},
            "actions": [{"kind": "download", "folder": "/x", "verify": true}, {"kind": "run-command", "program": "/bin/x", "args": ["{path}"]}],
            "paused": false
        }))
        .unwrap()
    }

    #[test]
    fn drafts_round_trip_through_specs() {
        let spec = draft().to_spec("w1".into()).unwrap();
        assert_eq!(spec.source.values.len(), 1, "empty values are dropped");
        assert_eq!(spec.version_pattern, None);
        assert_eq!(spec.notify.mode, NotifyMode::NewerThanInstalled);
        let back = WatchDraftDto::from(&spec);
        assert_eq!(back.id.as_deref(), Some("w1"));
        assert_eq!(back.notify.quiet_hours.as_ref().unwrap().start, "22:00");
        let json = serde_json::to_value(&back).unwrap();
        assert_eq!(json["actions"][1]["kind"], "run-command");
        assert_eq!(json["intervalSecs"], 60);
        let mut bad = draft();
        bad.notify.mode = "loud".into();
        assert!(bad.to_spec("x".into()).is_err());
        let mut bad = draft();
        bad.notify.quiet_hours = Some(QuietHoursDto {
            start: "25:00".into(),
            end: "01:00".into(),
        });
        assert!(bad.to_spec("x".into()).is_err());
        let mut off = draft();
        off.notify.mode = "off".into();
        off.notify.quiet_hours = None;
        assert_eq!(WatchDraftDto::from(&off.to_spec("x".into()).unwrap()).notify.mode, "off");
        off.notify.mode = "every-new".into();
        assert_eq!(WatchDraftDto::from(&off.to_spec("x".into()).unwrap()).notify.mode, "every-new");
    }

    #[test]
    fn statuses_settings_and_errors_serialise_as_the_ui_expects() {
        let all = [
            WatchStatus::Pending,
            WatchStatus::Ok,
            WatchStatus::NoMatch,
            WatchStatus::Unreachable("m".into()),
            WatchStatus::AuthNeeded("m".into()),
            WatchStatus::PluginError("m".into()),
            WatchStatus::Invalid("m".into()),
            WatchStatus::Failed("m".into()),
        ];
        let kinds: Vec<&str> = all.iter().map(|s| WatchStatusDto::from(s).kind).collect();
        assert_eq!(kinds, ["pending", "ok", "noMatch", "unreachable", "authNeeded", "pluginError", "invalid", "failed"]);
        for theme in [Theme::System, Theme::Light, Theme::Dark] {
            let s = Settings { theme, ..Settings::default() };
            assert_eq!(SettingsDto::from(&s).to_settings().unwrap(), s);
        }
        let mut bad = SettingsDto::from(&Settings::default());
        bad.theme = "pink".into();
        assert!(bad.to_settings().is_err());
        let json = serde_json::to_value(CommandError::invalid(vec!["a".into(), "b".into()])).unwrap();
        assert_eq!(json, json!({"code": "invalid", "message": "a", "problems": ["a", "b"]}));
        assert_eq!(CommandError::invalid(vec![]).message, "The input is not valid.");
        for e in [
            CommandError::bad_request("x"),
            CommandError::not_found("x"),
            CommandError::unavailable("x"),
            CommandError::internal("x"),
        ] {
            assert_eq!(e.to_string(), "x");
        }
    }

    #[test]
    fn fields_and_kinds_map_for_the_form() {
        let kinds: Vec<SourceKindDto> = spotter_core::source::catalog::builtin_kinds().iter().map(SourceKindDto::builtin).collect();
        let groups: Vec<&str> = kinds.iter().map(|k| k.group).collect();
        assert!(groups.contains(&"files") && groups.contains(&"forge") && groups.contains(&"registry") && groups.contains(&"web"));
        let f = FieldSpecDto::from(&FieldSpec::new("n", "N", FieldKind::Integer { min: 1, max: 5 }));
        assert_eq!((f.kind, f.min, f.max), ("integer", Some(1), Some(5)));
        for (kind, name) in [
            (FieldKind::Text, "text"),
            (FieldKind::Url, "url"),
            (FieldKind::Folder, "folder"),
            (FieldKind::Secret, "secret"),
            (FieldKind::Toggle, "toggle"),
        ] {
            assert_eq!(FieldSpecDto::from(&FieldSpec::new("k", "K", kind)).kind, name);
        }
        let p = SourceKindDto::plugin("p/k".into(), "P".into(), &SourceKindSpec::new("k", "K", "d"));
        assert_eq!((p.group, p.plugin.as_deref()), ("plugin", Some("P")));
    }

    #[test]
    fn events_map_every_kind() {
        let id = spotter_core::watch::WatchId::parse("w").unwrap();
        let kinds = [
            EventKind::NewRelease {
                version: "1".into(),
                name: "n".into(),
            },
            EventKind::CheckFailed { message: "m".into() },
            EventKind::Recovered,
            EventKind::DownloadFinished {
                version: "1".into(),
                path: "/p".into(),
                size: 3,
                verified: true,
            },
            EventKind::ActionFailed {
                action: "download".into(),
                message: "m".into(),
            },
            EventKind::PluginIssue {
                plugin: "p".into(),
                message: "m".into(),
            },
        ];
        let names: Vec<&str> = kinds
            .into_iter()
            .map(|kind| {
                AppEventDto::new(
                    &Event {
                        at: spotter_core::time::Timestamp(1),
                        watch: Some(id.clone()),
                        kind,
                    },
                    Some("W".into()),
                )
                .kind
            })
            .collect();
        assert_eq!(names, ["new-release", "check-failed", "recovered", "download-finished", "action-failed", "plugin-issue"]);
    }
}
