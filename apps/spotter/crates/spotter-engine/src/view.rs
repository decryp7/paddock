//! The engine's answers as plain structs (the shell maps them to its camelCase DTOs).
use spotter_core::journal::{Event, EventKind};
use spotter_core::state::{CheckRecord, KnownRelease, WatchStatus};
use spotter_core::watch::WatchSpec;

/// One release.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseView {
    /// The version as the scheme shows it.
    pub version: String,
    /// Whether it is a pre-release.
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
}

impl From<&KnownRelease> for ReleaseView {
    fn from(k: &KnownRelease) -> ReleaseView {
        ReleaseView {
            version: k.release.version.text.clone(),
            prerelease: k.release.version.is_prerelease(),
            name: k.release.name.clone(),
            location: k.release.location.clone(),
            published_at_ms: k.release.published_at.map(|t| t.0),
            first_seen_ms: k.first_seen.0,
            size: k.release.size,
            sha256: k.release.sha256.map(|d| d.to_hex()),
        }
    }
}

/// One row of the watch list, the tiles and the tray.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchSummary {
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
    pub status: WatchStatus,
    /// Paused.
    pub paused: bool,
    /// A check is running now.
    pub checking: bool,
    /// The newest release.
    pub latest: Option<ReleaseView>,
    /// The acknowledged version.
    pub acknowledged: Option<String>,
    /// The installed version.
    pub installed: Option<String>,
    /// The latest is newer than the acknowledged version.
    pub unseen: bool,
    /// The latest is newer than the installed version (`None` when unknown).
    pub newer_than_installed: Option<bool>,
    /// Last check.
    pub last_check_ms: Option<i64>,
    /// Next check.
    pub next_check_ms: Option<i64>,
    /// Failures since the last success.
    pub failures: u32,
    /// Seconds between checks.
    pub interval_secs: u32,
}

/// The watch detail page.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchDetail {
    /// The row.
    pub summary: WatchSummary,
    /// The stored watch.
    pub spec: WatchSpec,
    /// Newest first.
    pub history: Vec<ReleaseView>,
    /// Newest last.
    pub checks: Vec<CheckRecord>,
    /// Keys of secret fields that have a value in the credential store.
    pub secrets_set: Vec<String>,
}

/// The live "test this source" result.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourcePreview {
    /// Problems with the draft (nothing was checked when there are any).
    pub problems: Vec<String>,
    /// The check's failure.
    pub error: Option<WatchStatus>,
    /// Newest first, at most 20.
    pub releases: Vec<ReleaseView>,
    /// Names that matched but held no version (up to 20).
    pub without_version: Vec<String>,
    /// Pre-releases left out.
    pub prereleases_skipped: usize,
    /// Names left out by the name filter.
    pub names_skipped: usize,
    /// How long the check took.
    pub took_ms: u64,
}

/// A recent release for the "What's new" hero.
#[derive(Debug, Clone, PartialEq)]
pub struct WhatsNewView {
    /// The watch.
    pub watch_id: String,
    /// Its name.
    pub watch_name: String,
    /// Its source kind.
    pub source_kind: String,
    /// The release.
    pub release: ReleaseView,
    /// When it came out.
    pub released_at_ms: i64,
    /// Newer than the installed version.
    pub newer_than_installed: Option<bool>,
    /// Not seen yet.
    pub unseen: bool,
}

/// The dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct DashboardView {
    /// The range asked for (`30d`, `90d`, `1y`).
    pub range: String,
    /// Bucket starts (ms), oldest first.
    pub buckets_ms: Vec<i64>,
    /// The newest releases.
    pub whats_new: Vec<WhatsNewView>,
    /// Per watch.
    pub watches: Vec<spotter_core::dashboard::WatchNumbers>,
    /// Downloads per bucket.
    pub downloads: Vec<spotter_core::dashboard::DownloadBucket>,
    /// How long computing took, in microseconds.
    pub compute_micros: u64,
}

/// One entry of the events feed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventView {
    /// When.
    pub at_ms: i64,
    /// `new-release`, `check-failed`, `recovered`, `download-finished`, `action-failed` or `needs-you`.
    pub kind: String,
    /// The watch.
    pub watch_id: Option<String>,
    /// Its name (when it still exists).
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
    /// Action name.
    pub action: Option<String>,
}

impl EventView {
    /// The view of an event; `name_of` finds a watch's current name.
    pub fn of(e: &Event, name_of: impl Fn(&str) -> Option<String>) -> EventView {
        let watch_id = e.watch.as_ref().map(|w| w.as_str().to_string());
        let mut v = EventView {
            at_ms: e.at.0,
            watch_name: watch_id.as_deref().and_then(name_of),
            watch_id,
            ..EventView::default()
        };
        match &e.kind {
            EventKind::NewRelease { version, name } => {
                v.kind = "new-release".into();
                v.version = Some(version.clone());
                v.name = Some(name.clone());
            }
            EventKind::CheckFailed { message } => {
                v.kind = "check-failed".into();
                v.message = Some(message.clone());
            }
            EventKind::NeedsYou { message } => {
                v.kind = "needs-you".into();
                v.message = Some(message.clone());
            }
            EventKind::Recovered => v.kind = "recovered".into(),
            EventKind::DownloadFinished {
                version,
                path,
                size,
                verified,
            } => {
                v.kind = "download-finished".into();
                v.version = Some(version.clone());
                v.path = Some(path.clone());
                v.size = Some(*size);
                v.verified = Some(*verified);
            }
            EventKind::ActionFailed { action, message } => {
                v.kind = "action-failed".into();
                v.action = Some(action.clone());
                v.message = Some(message.clone());
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::time::Timestamp;
    use spotter_core::watch::WatchId;

    #[test]
    fn every_event_kind_has_a_view() {
        let kinds = [
            (
                EventKind::NewRelease {
                    version: "1".into(),
                    name: "n".into(),
                },
                "new-release",
            ),
            (EventKind::CheckFailed { message: "m".into() }, "check-failed"),
            (EventKind::NeedsYou { message: "m".into() }, "needs-you"),
            (EventKind::Recovered, "recovered"),
            (
                EventKind::DownloadFinished {
                    version: "1".into(),
                    path: "p".into(),
                    size: 3,
                    verified: true,
                },
                "download-finished",
            ),
            (
                EventKind::ActionFailed {
                    action: "download".into(),
                    message: "m".into(),
                },
                "action-failed",
            ),
        ];
        for (kind, name) in kinds {
            let e = Event {
                at: Timestamp(7),
                watch: WatchId::parse("a").ok(),
                kind,
            };
            let v = EventView::of(&e, |id| Some(format!("name of {id}")));
            assert_eq!((v.kind.as_str(), v.at_ms, v.watch_name.as_deref()), (name, 7, Some("name of a")));
        }
        let orphan = EventView::of(
            &Event {
                at: Timestamp(1),
                watch: None,
                kind: EventKind::Recovered,
            },
            |_| None,
        );
        assert_eq!((orphan.watch_id, orphan.watch_name), (None, None));
    }
}
