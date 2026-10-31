//! What the popup's widgets show, and the actions they offer. Plain data: the engine computes it (`spotter-engine`'s
//! `widgets`), the UI only draws it (AGENTS.md "Where logic lives").

use paddock_widgets::{PlacementState, SurfaceValues, WidgetState};
use serde::{Deserialize, Serialize};

/// How a watch is doing, as one row of Watched sites shows it (an icon and a word in the UI, never colour alone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum SiteState {
    /// Checked, nothing new.
    Ok,
    /// A release nobody has seen yet.
    New,
    /// A check is running now.
    Checking,
    /// The watch is paused.
    Paused,
    /// The last check failed or needs the person.
    Attention,
    /// Not checked yet.
    Pending,
}

/// One watch in Watched sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SiteRow {
    /// The watch id.
    pub id: String,
    /// The watch name.
    pub name: String,
    /// How it is doing.
    pub state: SiteState,
    /// The newest version, when there is one.
    pub version: Option<String>,
}

/// One release in Recent changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRow {
    /// The watch id.
    pub id: String,
    /// The watch name.
    pub name: String,
    /// The version found.
    pub version: String,
    /// When Spotter first saw it, in milliseconds since the epoch.
    #[ts(type = "number")]
    pub first_seen_ms: i64,
    /// Nobody has seen it yet.
    pub unseen: bool,
}

/// Why a check failed, in the words the person needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// The place could not be reached.
    Unreachable,
    /// Credentials are missing or were refused.
    AuthNeeded,
    /// A page asked for a login or a captcha.
    NeedsYou,
    /// The watch's configuration is not valid.
    Invalid,
    /// Any other failure.
    Failed,
}

/// One watch in Failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FailureRow {
    /// The watch id.
    pub id: String,
    /// The watch name.
    pub name: String,
    /// Why it failed.
    pub kind: FailureKind,
    /// The engine's message for the person.
    pub message: String,
    /// Failed checks in a row.
    pub failures: u32,
}

/// Whether notifications go out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum NotificationMode {
    /// Notifications are on.
    On,
    /// Paused until the given time.
    Paused {
        /// When they resume, in milliseconds since the epoch.
        #[ts(type = "number")]
        until_ms: i64,
    },
    /// Switched off in Settings.
    Off,
}

/// What one widget shows. Rows are capped; `more` counts the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum PopoverValue {
    /// The watches and their state.
    WatchedSites {
        /// All watches.
        total: u32,
        /// The first few, attention first.
        rows: Vec<SiteRow>,
        /// Watches not listed.
        more: u32,
    },
    /// The newest releases.
    RecentChanges {
        /// Newest first.
        rows: Vec<ChangeRow>,
        /// Releases not listed.
        more: u32,
    },
    /// The next scheduled check.
    NextCheck {
        /// The watch id.
        id: String,
        /// The watch name.
        name: String,
        /// When, in milliseconds since the epoch.
        #[ts(type = "number")]
        at_ms: i64,
        /// Other watches due at the same time or earlier than a minute after it.
        also_due: u32,
    },
    /// The quiet hours.
    QuietHours {
        /// Start, `HH:MM` (local).
        start: String,
        /// End, `HH:MM` (local).
        end: String,
        /// Whether they are in force right now.
        active: bool,
    },
    /// The failing watches.
    Failures {
        /// Most failed checks first.
        rows: Vec<FailureRow>,
        /// Failing watches not listed.
        more: u32,
    },
    /// Whether notifications go out.
    Notifications {
        /// On, paused or off.
        mode: NotificationMode,
    },
}

/// What a widget can ask the app to do. A closed set: the app runs each through an existing command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum WidgetActionId {
    /// Show the main window.
    OpenSpotter,
    /// Show one watch's page.
    OpenWatch {
        /// The watch id.
        id: String,
    },
    /// Show the Watches page.
    OpenWatches,
    /// Start the wizard for a new watch.
    AddWatch,
    /// Check one watch now (Retry).
    RetryCheck {
        /// The watch id.
        id: String,
    },
    /// Check every watch now.
    CheckAll,
    /// Resume paused notifications.
    ResumeNotifications,
    /// Open Settings (where quiet hours and the popup's widgets are).
    OpenSettings,
}

/// One widget's state in the popup.
pub type PopoverState = WidgetState<PopoverValue, WidgetActionId>;
/// All of the popup's widget values.
pub type PopoverValues = SurfaceValues<PopoverValue, WidgetActionId>;
/// A placement's state in the popup.
pub type PopoverPlacementState = PlacementState<PopoverValue, WidgetActionId>;
