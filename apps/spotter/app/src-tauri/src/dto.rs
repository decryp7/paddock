//! The UI <-> app wire contract (camelCase JSON), mirroring `app/src/app/core/api.types.ts`. Change both in the same
//! commit. The engine's types become DTOs here, apart from the commands that use them. Inputs are parsed into core
//! types once, at this edge ([`WatchDraftDto::into_spec`], [`SettingsDto::into_settings`]).

use serde::{Deserialize, Serialize};
use spotter_core::action::Action;
use spotter_core::extract::{GroupSpan, Selection};
use spotter_core::filter::VersionFilter;
use spotter_core::notify::{DigestMode, NotificationChannel, NotificationPolicy, NotifyMode};
use spotter_core::pack::{Preset, SchemeDef};
use spotter_core::release::LatestBy;
use spotter_core::schedule::{Schedule, SchedulePreview};
use spotter_core::schema::{FieldKind, FieldSpec, SourceKindSpec};
use spotter_core::source::web_page::DiscoveredFeed;
use spotter_core::source::SourceConfig;
use spotter_core::state::{CheckRecord, WatchStatus};
use spotter_core::time::QuietHours;
use spotter_core::tracking::{ProductSetting, ProductsSpec, Tracking};
use spotter_core::watch::{Settings, Theme, WatchSpec};
use spotter_engine::error::EngineError;
use spotter_engine::preview::PatternTest;
use spotter_engine::view::{self, EventView, ReleaseView};
use std::collections::BTreeMap;

pub use spotter_render::RendererStatus as RendererStatusDto;

/// `WatchStatus`: `kind` is `pending`, `ok`, `noMatch`, `unreachable`, `authNeeded`, `needsYou`, `invalid` or `failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatusDto {
    /// The status kind.
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
            WatchStatus::NeedsYou(m) => ("needsYou", Some(m.clone())),
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
    /// The version as the scheme shows it.
    pub version: String,
    /// A pre-release.
    pub prerelease: bool,
    /// Display name.
    pub name: String,
    /// A URL or a path.
    pub location: String,
    /// When it was published.
    pub published_at_ms: Option<i64>,
    /// When Spotter first found it.
    pub first_seen_ms: i64,
    /// Size in bytes.
    pub size: Option<u64>,
    /// SHA-256 in hex.
    pub sha256: Option<String>,
    /// The product (`id` group).
    pub product: Option<String>,
    /// The channel label.
    pub channel: String,
    /// Other named groups of the pattern.
    pub fields: BTreeMap<String, String>,
    /// Every file of the release, the primary one first.
    pub artifacts: Vec<ArtifactDto>,
    /// The same file name came back with new contents.
    pub updated_in_place: bool,
}

/// `Artifact`: one file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDto {
    /// File name.
    pub name: String,
    /// Where it is.
    pub location: String,
    /// Extension.
    pub kind: String,
    /// Variant.
    pub variant: String,
    /// Size in bytes.
    pub size: Option<u64>,
    /// Last change.
    pub modified_ms: Option<i64>,
    /// Creation time, where reported.
    pub created_ms: Option<i64>,
    /// The checksum file next to it.
    pub checksum_file: Option<String>,
}

impl From<&view::ArtifactView> for ArtifactDto {
    fn from(a: &view::ArtifactView) -> Self {
        ArtifactDto {
            name: a.name.clone(),
            location: a.location.clone(),
            kind: a.kind.clone(),
            variant: a.variant.clone(),
            size: a.size,
            modified_ms: a.modified_ms,
            created_ms: a.created_ms,
            checksum_file: a.checksum_file.clone(),
        }
    }
}

impl From<&ReleaseView> for ReleaseDto {
    fn from(r: &ReleaseView) -> Self {
        ReleaseDto {
            version: r.version.clone(),
            prerelease: r.prerelease,
            name: r.name.clone(),
            location: r.location.clone(),
            published_at_ms: r.published_at_ms,
            first_seen_ms: r.first_seen_ms,
            size: r.size,
            sha256: r.sha256.clone(),
            product: r.product.clone(),
            channel: r.channel.clone(),
            fields: r.fields.iter().cloned().collect(),
            artifacts: r.artifacts.iter().map(Into::into).collect(),
            updated_in_place: r.updated_in_place,
        }
    }
}

/// `NotificationPolicy` (the core's `quiet_hours` is `quietHours` on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPolicyDto {
    /// Which releases notify (`every-new`, `newer-than-installed`, `off`).
    pub mode: NotifyMode,
    /// Repeat until acknowledged.
    pub nag: bool,
    /// Hours when notifications wait.
    pub quiet_hours: Option<QuietHours>,
}

impl From<NotificationPolicy> for NotificationPolicyDto {
    fn from(p: NotificationPolicy) -> Self {
        NotificationPolicyDto {
            mode: p.mode,
            nag: p.nag,
            quiet_hours: p.quiet_hours,
        }
    }
}

impl From<NotificationPolicyDto> for NotificationPolicy {
    fn from(p: NotificationPolicyDto) -> Self {
        NotificationPolicy {
            mode: p.mode,
            nag: p.nag,
            quiet_hours: p.quiet_hours,
        }
    }
}

/// `VersionFilter` (`soak_hours` is `soakHours`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFilterDto {
    /// Only versions with this prefix.
    pub prefix: String,
    /// Hours a release must exist before it is announced.
    pub soak_hours: u32,
}

/// `WatchDraft`: a watch as edited and stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchDraftDto {
    /// `null` for a new watch.
    pub id: Option<String>,
    /// Display name.
    pub name: String,
    /// Where releases come from.
    pub source: SourceConfig,
    /// The version scheme id.
    pub versions: String,
    /// The pattern of a custom scheme.
    pub version_pattern: Option<String>,
    /// Keep pre-releases.
    pub include_prereleases: bool,
    /// Glob on release names.
    pub name_filter: String,
    /// When to check (`{"mode":"every","secs":3600}`, `times` or `cron`).
    pub schedule: Schedule,
    /// When to notify.
    pub notify: NotificationPolicyDto,
    /// Actions on a new release.
    pub actions: Vec<Action>,
    /// Extra channels.
    pub channels: Vec<NotificationChannel>,
    /// Version prefix and soak time.
    pub filter: VersionFilterDto,
    /// Not checked while paused.
    pub paused: bool,
    /// Which release is the latest, and which products are tracked.
    #[serde(default)]
    pub tracking: TrackingDto,
}

/// `ProductSetting`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductSettingDto {
    /// Display name; empty for the id.
    #[serde(default)]
    pub name: String,
    /// No notifications.
    #[serde(default)]
    pub muted: bool,
    /// Own soak time.
    #[serde(default)]
    pub soak_hours: Option<u32>,
}

/// `ProductsSpec`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductsDto {
    /// Track only these ids.
    #[serde(default)]
    pub only: Vec<String>,
    /// Announce products that appear later.
    #[serde(default)]
    pub announce_new: bool,
    /// Overrides by id.
    #[serde(default)]
    pub settings: BTreeMap<String, ProductSettingDto>,
}

/// `Tracking`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingDto {
    /// `version`, `modified`, `created` or `name`.
    #[serde(default)]
    pub latest_by: LatestBy,
    /// A missing number group is older than any number.
    #[serde(default)]
    pub missing_older: bool,
    /// Products.
    #[serde(default)]
    pub products: ProductsDto,
}

impl From<&Tracking> for TrackingDto {
    fn from(t: &Tracking) -> Self {
        TrackingDto {
            latest_by: t.latest_by,
            missing_older: t.missing_older,
            products: ProductsDto {
                only: t.products.only.clone(),
                announce_new: t.products.announce_new,
                settings: t
                    .products
                    .settings
                    .iter()
                    .map(|(k, s)| {
                        (
                            k.clone(),
                            ProductSettingDto {
                                name: s.name.clone(),
                                muted: s.muted,
                                soak_hours: s.soak_hours,
                            },
                        )
                    })
                    .collect(),
            },
        }
    }
}

impl From<TrackingDto> for Tracking {
    fn from(t: TrackingDto) -> Self {
        Tracking {
            latest_by: t.latest_by,
            missing_older: t.missing_older,
            products: ProductsSpec {
                only: t.products.only,
                announce_new: t.products.announce_new,
                settings: t
                    .products
                    .settings
                    .into_iter()
                    .map(|(k, s)| {
                        (
                            k,
                            ProductSetting {
                                name: s.name,
                                muted: s.muted,
                                soak_hours: s.soak_hours,
                            },
                        )
                    })
                    .collect(),
            },
        }
    }
}

impl WatchDraftDto {
    /// The core's spec. `allow_local` is not editable in the UI, so the caller passes the stored value.
    pub fn into_spec(self, allow_local: bool) -> WatchSpec {
        WatchSpec {
            id: self.id.unwrap_or_default(),
            name: self.name,
            source: self.source,
            versions: self.versions,
            version_pattern: self.version_pattern,
            include_prereleases: self.include_prereleases,
            name_filter: self.name_filter,
            schedule: self.schedule,
            notify: self.notify.into(),
            actions: self.actions,
            channels: self.channels,
            version_filter: VersionFilter {
                prefix: self.filter.prefix,
                soak_hours: self.filter.soak_hours,
            },
            allow_local,
            paused: self.paused,
            tracking: self.tracking.into(),
        }
    }
}

impl From<&WatchSpec> for WatchDraftDto {
    fn from(s: &WatchSpec) -> Self {
        WatchDraftDto {
            id: (!s.id.is_empty()).then(|| s.id.clone()),
            name: s.name.clone(),
            source: s.source.clone(),
            versions: s.versions.clone(),
            version_pattern: s.version_pattern.clone(),
            include_prereleases: s.include_prereleases,
            name_filter: s.name_filter.clone(),
            schedule: s.schedule.clone(),
            notify: s.notify.into(),
            actions: s.actions.clone(),
            channels: s.channels.clone(),
            filter: VersionFilterDto {
                prefix: s.version_filter.prefix.clone(),
                soak_hours: s.version_filter.soak_hours,
            },
            paused: s.paused,
            tracking: (&s.tracking).into(),
        }
    }
}

/// `WatchSummary`: one row of the watch list, the tiles and the tray.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchSummaryDto {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Source kind id.
    pub source_kind: String,
    /// Source kind label.
    pub source_label: String,
    /// Where it looks.
    pub source_description: String,
    /// Status after the last check.
    pub status: WatchStatusDto,
    /// Paused.
    pub paused: bool,
    /// A check is running now.
    pub checking: bool,
    /// The newest release.
    pub latest: Option<ReleaseDto>,
    /// The acknowledged version.
    pub acknowledged: Option<String>,
    /// The installed version.
    pub installed: Option<String>,
    /// The latest is newer than the acknowledged version.
    pub unseen: bool,
    /// The latest is newer than the installed version.
    pub newer_than_installed: Option<bool>,
    /// Last check.
    pub last_check_ms: Option<i64>,
    /// Next check.
    pub next_check_ms: Option<i64>,
    /// Failures since the last success.
    pub failures: u32,
    /// When the watch is checked, in words.
    pub schedule_text: String,
    /// How many products the watch tracks.
    pub product_count: u32,
}

impl From<&view::WatchSummary> for WatchSummaryDto {
    fn from(w: &view::WatchSummary) -> Self {
        WatchSummaryDto {
            id: w.id.clone(),
            name: w.name.clone(),
            source_kind: w.source_kind.clone(),
            source_label: w.source_label.clone(),
            source_description: w.source_description.clone(),
            status: (&w.status).into(),
            paused: w.paused,
            checking: w.checking,
            latest: w.latest.as_ref().map(Into::into),
            acknowledged: w.acknowledged.clone(),
            installed: w.installed.clone(),
            unseen: w.unseen,
            newer_than_installed: w.newer_than_installed,
            last_check_ms: w.last_check_ms,
            next_check_ms: w.next_check_ms,
            failures: w.failures,
            schedule_text: w.schedule_text.clone(),
            product_count: w.product_count,
        }
    }
}

/// `CheckRecord`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRecordDto {
    /// When.
    pub at_ms: i64,
    /// How long it took.
    pub took_ms: u64,
    /// The failure, if any.
    pub error: Option<String>,
    /// Releases found.
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
    /// The row.
    pub summary: WatchSummaryDto,
    /// The stored watch.
    pub spec: WatchDraftDto,
    /// Newest first.
    pub history: Vec<ReleaseDto>,
    /// Newest last.
    pub checks: Vec<CheckRecordDto>,
    /// Keys of secret fields that have a value (never the values).
    pub secrets_set: Vec<String>,
    /// The products of a watch whose pattern has an `id` group.
    pub products: Vec<ProductDto>,
}

/// `Product`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDto {
    /// The id.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// Notifications are off.
    pub muted: bool,
    /// Its newest release.
    pub latest: Option<ReleaseDto>,
    /// Newer than what was seen.
    pub unseen: bool,
    /// Newest first.
    pub history: Vec<ReleaseDto>,
}

impl From<&view::ProductView> for ProductDto {
    fn from(p: &view::ProductView) -> Self {
        ProductDto {
            id: p.id.clone(),
            name: p.name.clone(),
            muted: p.muted,
            latest: p.latest.as_ref().map(Into::into),
            unseen: p.unseen,
            history: p.history.iter().map(Into::into).collect(),
        }
    }
}

impl From<&view::WatchDetail> for WatchDetailDto {
    fn from(d: &view::WatchDetail) -> Self {
        WatchDetailDto {
            summary: (&d.summary).into(),
            spec: (&d.spec).into(),
            history: d.history.iter().map(Into::into).collect(),
            checks: d.checks.iter().map(Into::into).collect(),
            secrets_set: d.secrets_set.clone(),
            products: d.products.iter().map(Into::into).collect(),
        }
    }
}

/// One choice of a `choice` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChoiceDto {
    /// Stored value.
    pub value: String,
    /// Shown text.
    pub label: String,
}

/// `FieldSpec`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldSpecDto {
    /// Key in the source's values.
    pub key: String,
    /// Label.
    pub label: String,
    /// Help text.
    pub help: String,
    /// `text`, `url`, `folder`, `secret`, `integer`, `toggle` or `choice`.
    pub kind: &'static str,
    /// Required.
    pub required: bool,
    /// Default value.
    pub default: String,
    /// Placeholder.
    pub placeholder: String,
    /// Smallest integer.
    pub min: Option<i64>,
    /// Largest integer.
    pub max: Option<i64>,
    /// Choices.
    pub options: Vec<ChoiceDto>,
    /// Shown under "More options".
    pub advanced: bool,
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
                    .map(|c| ChoiceDto {
                        value: c.value.clone(),
                        label: c.label.clone(),
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
            advanced: f.advanced,
        }
    }
}

/// The group a source kind is shown under in the add-watch form.
pub fn group_of(kind_id: &str) -> &'static str {
    match kind_id {
        "folder" => "files",
        "github" | "gitea" | "gitlab" => "forge",
        "oci" | "npm" | "crates" => "registry",
        _ => "web",
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
    /// One sentence.
    pub description: String,
    /// `files`, `web`, `forge` or `registry`.
    pub group: &'static str,
    /// The form.
    pub fields: Vec<FieldSpecDto>,
}

impl From<&SourceKindSpec> for SourceKindDto {
    fn from(k: &SourceKindSpec) -> Self {
        SourceKindDto {
            id: k.id.clone(),
            label: k.label.clone(),
            description: k.description.clone(),
            group: group_of(&k.id),
            fields: k.fields.iter().map(Into::into).collect(),
        }
    }
}

/// `SourcePreview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePreviewDto {
    /// Problems with the draft.
    pub problems: Vec<String>,
    /// The check's failure.
    pub error: Option<WatchStatusDto>,
    /// Newest first.
    pub releases: Vec<ReleaseDto>,
    /// Names that held no version.
    pub without_version: Vec<String>,
    /// Pre-releases left out.
    pub prereleases_skipped: usize,
    /// Names left out by the filter.
    pub names_skipped: usize,
    /// How long it took.
    pub took_ms: u64,
    /// What the pattern read from the names.
    pub extraction: Option<ExtractionPreviewDto>,
}

/// `GroupSpan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupSpanDto {
    /// Group name.
    pub group: String,
    /// First UTF-16 code unit.
    pub start: usize,
    /// One past the last.
    pub end: usize,
}

impl From<&GroupSpan> for GroupSpanDto {
    fn from(s: &GroupSpan) -> Self {
        GroupSpanDto {
            group: s.group.clone(),
            start: s.start,
            end: s.end,
        }
    }
}

/// `ReadResult`: what was read from one name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResultDto {
    /// Product.
    pub id: Option<String>,
    /// Version as written.
    pub version: String,
    /// Channel label.
    pub channel: String,
    /// Variant.
    pub variant: String,
    /// A pre-release.
    pub prerelease: bool,
    /// Other named groups.
    pub fields: BTreeMap<String, String>,
    /// Where each group matched.
    pub spans: Vec<GroupSpanDto>,
}

impl From<&view::ReadView> for ReadResultDto {
    fn from(r: &view::ReadView) -> Self {
        ReadResultDto {
            id: r.id.clone(),
            version: r.version.clone(),
            channel: r.channel.clone(),
            variant: r.variant.clone(),
            prerelease: r.prerelease,
            fields: r.fields.iter().cloned().collect(),
            spans: r.spans.iter().map(Into::into).collect(),
        }
    }
}

/// `NameRow`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameRowDto {
    /// The text the pattern ran on.
    pub text: String,
    /// The file name.
    pub file_name: String,
    /// Size.
    pub size: Option<u64>,
    /// Last change.
    pub modified_ms: Option<i64>,
    /// What was read.
    pub read: Option<ReadResultDto>,
    /// Why it was ignored.
    pub ignored: Option<String>,
}

/// `VersionPreview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionPreviewDto {
    /// Version as written.
    pub version: String,
    /// Would be the latest.
    pub latest: bool,
    /// A pre-release.
    pub prerelease: bool,
    /// Files.
    pub artifacts: Vec<ArtifactDto>,
}

/// `ProductPreview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductPreviewDto {
    /// The id; null for a watch of one thing.
    pub id: Option<String>,
    /// The watch tracks it.
    pub tracked: bool,
    /// Newest first.
    pub versions: Vec<VersionPreviewDto>,
}

/// `ScanStats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatsDto {
    /// Entries looked at.
    pub scanned: u64,
    /// Entries kept.
    pub matched: u64,
    /// Folders walked.
    pub folders: u64,
    /// Milliseconds.
    pub took_ms: u64,
    /// The previous listing was reused.
    pub reused: bool,
    /// Files still changing.
    pub settling: u64,
}

/// `ExtractionPreview`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionPreviewDto {
    /// Matches first, then a sample of the ignored names.
    pub rows: Vec<NameRowDto>,
    /// Products with versions and files.
    pub products: Vec<ProductPreviewDto>,
    /// How the folder scan went.
    pub scan: Option<ScanStatsDto>,
    /// Names left out.
    pub ignored_count: u64,
    /// The pattern has an `id` group.
    pub has_id: bool,
    /// Creation times are reported.
    pub created_available: bool,
    /// How the latest is decided, in words.
    pub order_note: String,
    /// Things to know.
    pub warnings: Vec<String>,
}

impl From<&view::ExtractionPreview> for ExtractionPreviewDto {
    fn from(x: &view::ExtractionPreview) -> Self {
        ExtractionPreviewDto {
            rows: x
                .rows
                .iter()
                .map(|r| NameRowDto {
                    text: r.text.clone(),
                    file_name: r.file_name.clone(),
                    size: r.size,
                    modified_ms: r.modified_ms,
                    read: r.read.as_ref().map(Into::into),
                    ignored: r.ignored.clone(),
                })
                .collect(),
            products: x
                .products
                .iter()
                .map(|p| ProductPreviewDto {
                    id: p.id.clone(),
                    tracked: p.tracked,
                    versions: p
                        .versions
                        .iter()
                        .map(|v| VersionPreviewDto {
                            version: v.version.clone(),
                            latest: v.latest,
                            prerelease: v.prerelease,
                            artifacts: v.artifacts.iter().map(Into::into).collect(),
                        })
                        .collect(),
                })
                .collect(),
            scan: x.scan.map(|s| ScanStatsDto {
                scanned: s.scanned,
                matched: s.matched,
                folders: s.folders,
                took_ms: s.took_ms,
                reused: s.reused,
                settling: s.settling,
            }),
            ignored_count: x.ignored_count,
            has_id: x.has_id,
            created_available: x.created_available,
            order_note: x.order_note.clone(),
            warnings: x.warnings.clone(),
        }
    }
}

/// `PatternTest`: the result of trying a pattern on one name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatternTestDto {
    /// The pattern is not valid.
    pub error: Option<String>,
    /// What was read.
    pub read: Option<ReadResultDto>,
    /// Why nothing was read.
    pub ignored: Option<String>,
}

impl From<&PatternTest> for PatternTestDto {
    fn from(t: &PatternTest) -> Self {
        PatternTestDto {
            error: t.error.clone(),
            read: t.read.as_ref().map(Into::into),
            ignored: t.ignored.clone(),
        }
    }
}

/// `SchedulePreview`: a schedule in words with its next runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePreviewDto {
    /// The schedule in words.
    pub description: String,
    /// The next runs (milliseconds since the epoch).
    pub next_runs_ms: Vec<i64>,
    /// Things worth knowing (a clamped schedule).
    pub warnings: Vec<String>,
    /// The time zone the times are in.
    pub zone: String,
}

impl From<&SchedulePreview> for SchedulePreviewDto {
    fn from(p: &SchedulePreview) -> Self {
        SchedulePreviewDto {
            description: p.description.clone(),
            next_runs_ms: p.next_runs.iter().map(|t| t.0).collect(),
            warnings: p.warnings.clone(),
            zone: p.zone.clone(),
        }
    }
}

/// `PatternSelection`: a part of an example name the user selected.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PatternSelectionDto {
    /// `id`, `major`, `minor`, `patch`, `build`, `channel` or `variant`.
    pub role: String,
    /// First UTF-16 code unit.
    pub start: usize,
    /// One past the last.
    pub end: usize,
}

impl From<PatternSelectionDto> for Selection {
    fn from(s: PatternSelectionDto) -> Self {
        Selection {
            role: s.role,
            start: s.start,
            end: s.end,
        }
    }
}

/// `SchemePreset`: a pattern to start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemePresetDto {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// The pattern.
    pub pattern: String,
    /// A name it reads.
    pub example: String,
}

impl From<&SchemeDef> for SchemePresetDto {
    fn from(s: &SchemeDef) -> Self {
        SchemePresetDto {
            id: s.id.clone(),
            name: s.name.clone(),
            pattern: s.pattern.clone(),
            example: s.example.clone(),
        }
    }
}

impl From<&view::SourcePreview> for SourcePreviewDto {
    fn from(p: &view::SourcePreview) -> Self {
        SourcePreviewDto {
            problems: p.problems.clone(),
            error: p.error.as_ref().map(Into::into),
            releases: p.releases.iter().map(Into::into).collect(),
            without_version: p.without_version.clone(),
            prereleases_skipped: p.prereleases_skipped,
            names_skipped: p.names_skipped,
            took_ms: p.took_ms,
            extraction: p.extraction.as_ref().map(Into::into),
        }
    }
}

/// `Preset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetDto {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Category.
    pub category: String,
    /// The draft to start from.
    pub draft: WatchDraftDto,
}

impl From<&Preset> for PresetDto {
    fn from(p: &Preset) -> Self {
        PresetDto {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            category: p.category.clone(),
            draft: (&p.spec).into(),
        }
    }
}

/// `DiscoveredFeed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredFeedDto {
    /// Address.
    pub url: String,
    /// Title.
    pub title: String,
}

impl From<&DiscoveredFeed> for DiscoveredFeedDto {
    fn from(f: &DiscoveredFeed) -> Self {
        DiscoveredFeedDto {
            url: f.url.clone(),
            title: f.title.clone(),
        }
    }
}

/// `ImportResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportResultDto {
    /// Drafts, not yet saved.
    pub drafts: Vec<WatchDraftDto>,
    /// Entries left out, with the reason.
    pub skipped: Vec<String>,
}

/// `WhatsNewItem`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNewItemDto {
    /// The watch.
    pub watch_id: String,
    /// Its name.
    pub watch_name: String,
    /// Its source kind.
    pub source_kind: String,
    /// The release.
    pub release: ReleaseDto,
    /// When it came out.
    pub released_at_ms: i64,
    /// Newer than installed.
    pub newer_than_installed: Option<bool>,
    /// Not seen yet.
    pub unseen: bool,
}

/// `WatchNumbers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchNumbersDto {
    /// The watch.
    pub watch_id: String,
    /// Average time between releases.
    pub average_interval_ms: Option<i64>,
    /// Time since the last release.
    pub since_last_ms: Option<i64>,
    /// Releases per bucket.
    pub releases: Vec<u32>,
    /// Failed checks per bucket.
    pub failed_checks: Vec<u32>,
    /// Checks per bucket.
    pub checks: Vec<u32>,
    /// Releases per week over 12 weeks.
    pub sparkline: Vec<u32>,
}

/// `DownloadBucket`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DownloadBucketDto {
    /// Downloads.
    pub count: u32,
    /// Failed downloads.
    pub failed: u32,
    /// Bytes.
    pub bytes: u64,
}

/// `Dashboard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardDto {
    /// `30d`, `90d` or `1y`.
    pub range: String,
    /// Bucket starts, oldest first.
    pub buckets_ms: Vec<i64>,
    /// The newest releases.
    pub whats_new: Vec<WhatsNewItemDto>,
    /// Per watch.
    pub watches: Vec<WatchNumbersDto>,
    /// Downloads per bucket.
    pub downloads: Vec<DownloadBucketDto>,
    /// Engine time in microseconds.
    pub compute_micros: u64,
}

impl From<&view::DashboardView> for DashboardDto {
    fn from(d: &view::DashboardView) -> Self {
        DashboardDto {
            range: d.range.clone(),
            buckets_ms: d.buckets_ms.clone(),
            whats_new: d
                .whats_new
                .iter()
                .map(|w| WhatsNewItemDto {
                    watch_id: w.watch_id.clone(),
                    watch_name: w.watch_name.clone(),
                    source_kind: w.source_kind.clone(),
                    release: (&w.release).into(),
                    released_at_ms: w.released_at_ms,
                    newer_than_installed: w.newer_than_installed,
                    unseen: w.unseen,
                })
                .collect(),
            watches: d
                .watches
                .iter()
                .map(|n| WatchNumbersDto {
                    watch_id: n.watch.to_string(),
                    average_interval_ms: n.average_interval_ms,
                    since_last_ms: n.since_last_ms,
                    releases: n.releases.clone(),
                    failed_checks: n.failed_checks.clone(),
                    checks: n.checks.clone(),
                    sparkline: n.sparkline.clone(),
                })
                .collect(),
            downloads: d
                .downloads
                .iter()
                .map(|b| DownloadBucketDto {
                    count: b.count,
                    failed: b.failed,
                    bytes: b.bytes,
                })
                .collect(),
            compute_micros: d.compute_micros,
        }
    }
}

/// `AppEvent`: fields a kind does not use are null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEventDto {
    /// When.
    pub at_ms: i64,
    /// `new-release`, `check-failed`, `recovered`, `download-finished`, `action-failed` or `needs-you`.
    pub kind: String,
    /// The watch.
    pub watch_id: Option<String>,
    /// Its name.
    pub watch_name: Option<String>,
    /// Version.
    pub version: Option<String>,
    /// Release name.
    pub name: Option<String>,
    /// Message.
    pub message: Option<String>,
    /// File path.
    pub path: Option<String>,
    /// Size.
    pub size: Option<u64>,
    /// Checksum verified.
    pub verified: Option<bool>,
    /// Action.
    pub action: Option<String>,
}

impl From<&EventView> for AppEventDto {
    fn from(e: &EventView) -> Self {
        AppEventDto {
            at_ms: e.at_ms,
            kind: e.kind.clone(),
            watch_id: e.watch_id.clone(),
            watch_name: e.watch_name.clone(),
            version: e.version.clone(),
            name: e.name.clone(),
            message: e.message.clone(),
            path: e.path.clone(),
            size: e.size,
            verified: e.verified,
            action: e.action.clone(),
        }
    }
}

/// `Settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// Colour theme.
    pub theme: Theme,
    /// Notifications at all.
    pub notifications: bool,
    /// Paused until.
    pub notifications_paused_until_ms: Option<i64>,
    /// Check for Spotter's updates automatically.
    pub check_app_updates: bool,
    /// Where downloads go.
    pub downloads_folder: Option<String>,
    /// Interval for new watches.
    pub default_interval_secs: u32,
    /// `off`, `daily` or `weekly`.
    pub digest: DigestMode,
    /// Global quiet hours.
    pub quiet_hours: Option<QuietHours>,
    /// Use an OS webview when no Chromium is found.
    pub allow_os_webview: bool,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        SettingsDto {
            theme: s.theme,
            notifications: s.notifications,
            notifications_paused_until_ms: s.notifications_paused_until,
            check_app_updates: s.check_app_updates,
            downloads_folder: s.downloads_folder.clone(),
            default_interval_secs: s.default_interval_secs,
            digest: s.digest,
            quiet_hours: s.quiet_hours,
            allow_os_webview: s.allow_os_webview,
        }
    }
}

impl SettingsDto {
    /// The core's settings.
    pub fn into_settings(self) -> Settings {
        Settings {
            theme: self.theme,
            notifications: self.notifications,
            notifications_paused_until: self.notifications_paused_until_ms,
            check_app_updates: self.check_app_updates,
            downloads_folder: self.downloads_folder,
            default_interval_secs: self.default_interval_secs,
            digest: self.digest,
            quiet_hours: self.quiet_hours,
            allow_os_webview: self.allow_os_webview,
        }
    }
}

/// `AppInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    /// Spotter's version.
    pub version: String,
    /// The configuration file.
    pub config_path: String,
    /// The logs folder.
    pub logs_dir: String,
    /// The downloads folder.
    pub downloads_dir: String,
    /// Problems worked around at start-up.
    pub warnings: Vec<String>,
    /// The old Release Monitor's configuration was migrated.
    pub migrated_from_legacy: bool,
    /// `windows`, `macos` or `linux`.
    pub platform: &'static str,
}

/// The platform name the UI expects.
pub const fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// `AppUpdateStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatusDto {
    /// False in builds without the update key.
    pub available: bool,
    /// Why not.
    pub reason: Option<String>,
    /// This build's version.
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

/// `StatePush`: the event payload on `spotter://state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatePush {
    /// Every watch.
    pub watches: Vec<WatchSummaryDto>,
}

/// `CommandError.code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandErrorCode {
    /// The request is wrong.
    BadRequest,
    /// No such thing.
    NotFound,
    /// The input is invalid.
    Invalid,
    /// Something needed is unavailable.
    Unavailable,
    /// A bug or an I/O failure.
    Internal,
}

/// `CommandError`: what every rejected command carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// Machine-readable code.
    pub code: CommandErrorCode,
    /// What to do about it.
    pub message: String,
    /// For `invalid`: every problem.
    pub problems: Vec<String>,
}

impl CommandError {
    /// A wrong request.
    pub fn bad(message: impl Into<String>) -> Self {
        CommandError {
            code: CommandErrorCode::BadRequest,
            message: message.into(),
            problems: Vec::new(),
        }
    }

    /// Something unavailable.
    pub fn unavailable(message: impl Into<String>) -> Self {
        CommandError {
            code: CommandErrorCode::Unavailable,
            message: message.into(),
            problems: Vec::new(),
        }
    }

    /// A bug or failure on Spotter's own files.
    pub fn internal(message: impl Into<String>) -> Self {
        CommandError {
            code: CommandErrorCode::Internal,
            message: message.into(),
            problems: Vec::new(),
        }
    }
}

impl From<EngineError> for CommandError {
    fn from(e: EngineError) -> Self {
        let code = match &e {
            EngineError::BadRequest(_) => CommandErrorCode::BadRequest,
            EngineError::NotFound(_) => CommandErrorCode::NotFound,
            EngineError::Invalid(_) => CommandErrorCode::Invalid,
            EngineError::Unavailable(_) => CommandErrorCode::Unavailable,
            EngineError::Internal(_) => CommandErrorCode::Internal,
        };
        CommandError {
            code,
            message: e.to_string(),
            problems: e.problems().to_vec(),
        }
    }
}

/// Secrets the UI sends: key to value.
pub type Secrets = BTreeMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn draft_json() -> serde_json::Value {
        json!({
            "id": null, "name": "Example", "source": {"kind": "github", "values": {"repo": "o/r"}},
            "versions": "semver", "versionPattern": null, "includePrereleases": false, "nameFilter": "",
            "schedule": {"mode": "times", "days": ["mon", "fri"], "times": ["09:00", "14:30"], "zone": "Europe/Berlin"},
            "notify": {"mode": "newer-than-installed", "nag": true, "quietHours": {"start": "22:00", "end": "07:00"}},
            "actions": [{"kind": "open-location", "variant": "", "extension": ""},
                        {"kind": "download", "folder": "/tmp/x", "verify": true, "variant": "x64", "extension": "iso"},
                        {"kind": "run-command", "program": "/bin/echo", "args": ["{path}"]}],
            "channels": [{"kind": "ntfy", "target": "topic_1"}],
            "filter": {"prefix": "1.", "soakHours": 24}, "paused": false,
            "tracking": {"latestBy": "modified", "missingOlder": true,
                         "products": {"only": ["PRO"], "announceNew": true,
                                      "settings": {"PRO": {"name": "Acme Installer Pro", "muted": true, "soakHours": 6}}}}
        })
    }

    #[test]
    fn a_draft_round_trips_through_the_core_spec() {
        let dto: WatchDraftDto = serde_json::from_value(draft_json()).unwrap();
        let spec = dto.clone().into_spec(true);
        assert!(spec.allow_local && spec.id.is_empty());
        assert_eq!(spec.version_filter.soak_hours, 24);
        assert_eq!(spec.notify.mode, NotifyMode::NewerThanInstalled);
        let back = WatchDraftDto::from(&spec);
        assert_eq!(back, dto);
        assert_eq!(serde_json::to_value(&back).unwrap(), draft_json());
    }

    #[test]
    fn a_draft_without_tracking_or_artifact_choices_still_loads() {
        let mut j = draft_json();
        j.as_object_mut().unwrap().remove("tracking");
        let dto: WatchDraftDto = serde_json::from_value(j).unwrap();
        assert_eq!(dto.tracking, TrackingDto::default());
        let spec = dto.into_spec(false);
        assert_eq!(spec.tracking, Tracking::default());
        let mut bad = draft_json();
        bad["tracking"]["latestBy"] = json!("whenever");
        assert!(serde_json::from_value::<WatchDraftDto>(bad).is_err());
    }

    #[test]
    fn a_draft_with_unknown_enum_values_is_refused_at_the_edge() {
        let mut j = draft_json();
        j["notify"]["mode"] = json!("sometimes");
        assert!(serde_json::from_value::<WatchDraftDto>(j).is_err());
        let mut j = draft_json();
        j["notify"]["quietHours"]["start"] = json!("25:00");
        assert!(serde_json::from_value::<WatchDraftDto>(j).is_err());
    }

    #[test]
    fn statuses_use_the_ui_names() {
        let kinds: Vec<&str> = [
            WatchStatus::Pending,
            WatchStatus::Ok,
            WatchStatus::NoMatch,
            WatchStatus::Unreachable("a".into()),
            WatchStatus::AuthNeeded("a".into()),
            WatchStatus::NeedsYou("a".into()),
            WatchStatus::Invalid("a".into()),
            WatchStatus::Failed("a".into()),
        ]
        .iter()
        .map(|s| WatchStatusDto::from(s).kind)
        .collect();
        assert_eq!(
            kinds,
            [
                "pending",
                "ok",
                "noMatch",
                "unreachable",
                "authNeeded",
                "needsYou",
                "invalid",
                "failed"
            ]
        );
        assert_eq!(WatchStatusDto::from(&WatchStatus::Ok).message, None);
    }

    #[test]
    fn settings_round_trip_and_use_camel_case() {
        let s = Settings::default();
        let dto = SettingsDto::from(&s);
        let j = serde_json::to_value(&dto).unwrap();
        assert_eq!(j["theme"], "system");
        assert_eq!(j["digest"], "off");
        assert!(j.get("notificationsPausedUntilMs").is_some() && j.get("defaultIntervalSecs").is_some());
        assert_eq!(serde_json::from_value::<SettingsDto>(j).unwrap().into_settings(), s);
    }

    #[test]
    fn every_builtin_kind_has_a_group_and_fields_keep_their_shape() {
        for k in spotter_core::source::catalog::builtin_kinds() {
            let d = SourceKindDto::from(&k);
            assert!(["files", "web", "forge", "registry"].contains(&d.group), "{}", k.id);
            assert_eq!(d.fields.len(), k.fields.len());
        }
        let f = FieldSpec::new("n", "Count", FieldKind::Integer { min: 1, max: 10 });
        let d = FieldSpecDto::from(&f);
        assert_eq!((d.kind, d.min, d.max), ("integer", Some(1), Some(10)));
        let c = FieldSpec::new(
            "c",
            "Choice",
            FieldKind::Choice(vec![spotter_core::schema::ChoiceOption::new("a", "A")]),
        );
        assert_eq!(FieldSpecDto::from(&c).options.len(), 1);
        assert_eq!(group_of("folder"), "files");
        assert_eq!(group_of("gitlab"), "forge");
        assert_eq!(group_of("npm"), "registry");
        assert_eq!(group_of("web-page"), "web");
    }

    #[test]
    fn errors_keep_code_message_and_problems() {
        let e = CommandError::from(EngineError::Invalid(vec!["a".into(), "b".into()]));
        let j = serde_json::to_value(&e).unwrap();
        assert_eq!(j["code"], "invalid");
        assert_eq!(j["problems"].as_array().unwrap().len(), 2);
        assert_eq!(
            CommandError::from(EngineError::NotFound("x".into())).code,
            CommandErrorCode::NotFound
        );
        assert_eq!(
            CommandError::from(EngineError::Internal("x".into())).code,
            CommandErrorCode::Internal
        );
        assert_eq!(
            CommandError::from(EngineError::BadRequest("x".into())).code,
            CommandErrorCode::BadRequest
        );
        assert_eq!(
            CommandError::from(EngineError::Unavailable("x".into())).code,
            CommandErrorCode::Unavailable
        );
        assert_eq!(CommandError::bad("b").code, CommandErrorCode::BadRequest);
        assert_eq!(CommandError::unavailable("u").code, CommandErrorCode::Unavailable);
        assert_eq!(CommandError::internal("i").code, CommandErrorCode::Internal);
    }

    #[test]
    fn engine_views_convert_to_the_wire_shapes() {
        use spotter_core::source::SourceConfig;
        use spotter_engine::secrets::MemorySecrets;
        use spotter_engine::{Engine, EngineConfig, Services};
        use std::sync::Arc;

        let data = tempfile::tempdir().unwrap();
        let share = tempfile::tempdir().unwrap();
        std::fs::write(share.path().join("app-1.2.0.zip"), b"x").unwrap();
        let engine = Engine::open(
            EngineConfig::new(data.path(), "test"),
            Services::new(Arc::new(MemorySecrets::new())),
        )
        .unwrap();
        let mut spec = WatchDraftDto::from(&engine.presets()[0].spec).into_spec(false);
        spec.name = "Folder app".into();
        spec.source = SourceConfig::new("folder", &[("path", share.path().to_str().unwrap()), ("pattern", "app-*.zip")]);
        let saved = engine.save_watch(spec.clone(), &BTreeMap::new()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(engine.check_one(&saved.id)).unwrap();

        let summary = WatchSummaryDto::from(&engine.list_watches()[0]);
        assert_eq!(summary.name, "Folder app");
        assert_eq!(summary.latest.as_ref().map(|r| r.version.as_str()), Some("1.2.0"));
        let detail = WatchDetailDto::from(&engine.get_watch(&saved.id).unwrap());
        assert_eq!(detail.history.len(), 1);
        assert_eq!(detail.checks.len(), 1);
        assert!(detail.secrets_set.is_empty());
        let j = serde_json::to_value(&detail).unwrap();
        assert!(j["summary"]["sourceKind"] == "folder" && j["checks"][0]["tookMs"].is_u64());

        let preview = SourcePreviewDto::from(&rt.block_on(engine.test_source(&spec, &BTreeMap::new())));
        assert_eq!(preview.releases.len(), 1);
        assert!(preview.problems.is_empty() && preview.error.is_none());
        let preset = PresetDto::from(&engine.presets()[0]);
        assert!(!preset.id.is_empty() && !preset.category.is_empty());
        let feed = DiscoveredFeedDto::from(&spotter_core::source::web_page::DiscoveredFeed {
            url: "https://a.b/f".into(),
            title: "T".into(),
        });
        assert_eq!((feed.url.as_str(), feed.title.as_str()), ("https://a.b/f", "T"));

        let dash = DashboardDto::from(&engine.dashboard("30d").unwrap());
        assert_eq!(dash.range, "30d");
        assert_eq!(dash.whats_new.len(), 1);
        assert_eq!(dash.watches.len(), 1);
        assert_eq!(dash.downloads.len(), dash.buckets_ms.len());
        engine
            .record_download(&saved.id, "1.2.0", Ok(("/x/app.zip".into(), 1, true)))
            .unwrap();
        let events: Vec<AppEventDto> = engine.events(10, None).unwrap().iter().map(Into::into).collect();
        assert!(events.iter().any(|e| e.kind == "download-finished" && e.verified == Some(true)));
        let release = ReleaseDto::from(&engine.get_watch(&saved.id).unwrap().history[0]);
        assert_eq!(release.version, "1.2.0");
    }

    #[test]
    fn platform_is_one_of_the_three() {
        assert!(["windows", "macos", "linux"].contains(&platform_name()));
    }
}
