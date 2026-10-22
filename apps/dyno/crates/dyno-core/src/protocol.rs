//! The app ↔ agent protocol: requests, replies and pushed events. Carried as postcard bodies in dyno-ipc frames
//! between the processes, and as camelCase JSON between the Tauri shell and the webview (`api.types.ts` mirrors it).
//! Append new variants at the end; anything else is a new [`PROTOCOL_VERSION`].

use crate::activity::ActivityEntry;
use crate::alerts::Alert;
use crate::anomaly::Anomaly;
use crate::cleanup::{CleanupCategory, CleanupOutcome, CleanupScan, DeleteMode, QuarantineEntry, SizeNode};
use crate::diagnosis::Diagnosis;
use crate::forecast::DiskTrend;
use crate::grouping::AppGroup;
use crate::health::HealthReport;
use crate::history::{Consumer, HistoryPoint, HistoryRange};
use crate::metrics::{MemoryPressure, SystemSample};
use crate::process::{Locker, ProcessAction, ProcessDetails, ProcessRef, ProcessSample};
use crate::report::WeeklyReport;
use crate::rules::{ActiveAction, ProfileId, RuleSet};
use crate::settings::Settings;
use crate::snapshot::{SnapshotDiff, SnapshotInfo};
use crate::startup::{StartupEntry, StartupState};
use crate::time::Timestamp;
use crate::tuning::TuningSuggestion;
use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change.
pub const PROTOCOL_VERSION: u32 = 1;

/// How much a live subscriber wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LiveDetail {
    /// System sample, top apps, overhead.
    #[default]
    Summary,
    /// Also every process and group (the task manager is open).
    Processes,
}

/// Something the app asks the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Request {
    /// Live updates at this detail.
    Subscribe {
        /// Detail.
        detail: LiveDetail,
    },
    /// One process's details.
    ProcessDetails {
        /// The process.
        target: ProcessRef,
    },
    /// Act on a process.
    Act {
        /// The process.
        target: ProcessRef,
        /// What to do.
        action: ProcessAction,
    },
    /// Act on every process of an app.
    ActOnGroup {
        /// The group id.
        group_id: String,
        /// What to do.
        action: ProcessAction,
    },
    /// Who holds a file open.
    Lockers {
        /// The file.
        path: String,
    },
    /// Show the executable in the file manager.
    OpenLocation {
        /// The process.
        target: ProcessRef,
    },
    /// Take a snapshot.
    TakeSnapshot {
        /// Its label.
        label: String,
    },
    /// List snapshots.
    Snapshots,
    /// Compare two snapshots.
    DiffSnapshots {
        /// Earlier id.
        before: String,
        /// Later id.
        after: String,
    },
    /// Why is it slow?
    WhySlow,
    /// Current anomalies.
    Anomalies,
    /// History for a range.
    History {
        /// The range.
        range: HistoryRange,
        /// Its end (now when absent).
        end: Option<Timestamp>,
    },
    /// The activity log, newest first.
    Activity {
        /// At most this many.
        limit: u32,
    },
    /// Undo an activity entry.
    Undo {
        /// The entry.
        activity_id: String,
    },
    /// Release what rules are doing to a process now.
    ReleaseAction {
        /// The process.
        target: ProcessRef,
    },
    /// The rule set.
    Rules,
    /// Save the rule set.
    SaveRules {
        /// The new set.
        rules: RuleSet,
    },
    /// Switch profile.
    SetProfile {
        /// The profile.
        profile: ProfileId,
    },
    /// Pause or resume all rules.
    PauseRules {
        /// Paused.
        paused: bool,
    },
    /// Cleanup categories.
    CleanupCategories,
    /// Scan categories.
    ScanCleanup {
        /// Category ids.
        categories: Vec<String>,
    },
    /// Delete items of a scan.
    RunCleanup {
        /// The scan.
        scan_id: String,
        /// The items.
        item_ids: Vec<String>,
        /// Recycle Bin or quarantine.
        mode: DeleteMode,
    },
    /// Quarantines.
    Quarantine,
    /// Restore a quarantine.
    RestoreQuarantine {
        /// Its id.
        id: String,
    },
    /// The size tree of a folder (the system disk when absent).
    DiskUsage {
        /// The folder.
        path: Option<String>,
    },
    /// The health report.
    Health,
    /// Tuning suggestions.
    Tuning,
    /// Apply a tuning suggestion.
    ApplyTuning {
        /// Its id.
        id: String,
    },
    /// Revert a tuning suggestion.
    RevertTuning {
        /// Its id.
        id: String,
    },
    /// Startup entries.
    StartupEntries,
    /// Change a startup entry.
    SetStartupState {
        /// The entry.
        id: String,
        /// The new state.
        state: StartupState,
    },
    /// The weekly report.
    WeeklyReport,
    /// Recent alerts, newest first.
    Alerts {
        /// At most this many.
        limit: u32,
    },
    /// Settings.
    Settings,
    /// Save settings.
    SaveSettings {
        /// The new settings.
        settings: Settings,
    },
    /// Plugins.
    Plugins,
    /// Asks the agent to exit (the app does this to restart an agent from an older build). Appended; answers `Ok`.
    Shutdown,
}

/// Dyno's own cost.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Overhead {
    /// The agent's CPU, percent of the machine.
    pub agent_cpu: f32,
    /// The agent's resident memory, bytes.
    pub agent_memory: u64,
    /// The current system sampling interval.
    pub sampling_interval_ms: u32,
    /// The current process sampling interval.
    pub process_interval_ms: u32,
}

/// Pushed every tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveUpdate {
    /// The sample.
    pub system: SystemSample,
    /// Memory pressure (the OS's or derived).
    pub pressure: MemoryPressure,
    /// Busiest apps by CPU.
    pub top_cpu: Vec<AppGroup>,
    /// Biggest apps by memory.
    pub top_memory: Vec<AppGroup>,
    /// Dyno's cost.
    pub overhead: Overhead,
    /// Active profile.
    pub profile: ProfileId,
    /// Rules paused.
    pub rules_paused: bool,
    /// What rules are doing now.
    pub active_actions: Vec<ActiveAction>,
    /// Every process, when a subscriber asked for them.
    pub processes: Option<Vec<ProcessSample>>,
    /// Every group, likewise.
    pub groups: Option<Vec<AppGroup>>,
}

/// A marker on the history timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    /// When.
    pub at: Timestamp,
    /// Kind.
    pub kind: TimelineEventKind,
    /// Label.
    pub label: String,
}

/// Timeline marker kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimelineEventKind {
    /// A rule lowered priority or turned on efficiency mode.
    Throttle,
    /// A rule capped CPU.
    Cap,
    /// A rule released an app.
    Release,
    /// A cleanup ran.
    Cleanup,
    /// A tuning change.
    Tuning,
    /// An alert.
    Alert,
    /// The machine started.
    Boot,
}

/// One boot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootRecord {
    /// When the machine started.
    pub at: Timestamp,
    /// Boot to the login session starting, seconds.
    pub boot_secs: Option<f32>,
    /// Login to the desktop settling, seconds.
    pub ready_secs: Option<f32>,
}

/// Crashes and restarts in one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCount {
    /// Day start.
    pub day: Timestamp,
    /// App crashes.
    pub crashes: u32,
    /// App restarts after an exit.
    pub restarts: u32,
}

/// The history of a range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    /// The range.
    pub range: HistoryRange,
    /// Start.
    pub from: Timestamp,
    /// End.
    pub to: Timestamp,
    /// Point spacing.
    pub resolution_ms: i64,
    /// Points.
    pub points: Vec<HistoryPoint>,
    /// Timeline markers.
    pub events: Vec<TimelineEvent>,
    /// Biggest consumers.
    pub top_consumers: Vec<Consumer>,
    /// Free space per disk with forecasts.
    pub disk_trends: Vec<DiskTrend>,
    /// Boots.
    pub boots: Vec<BootRecord>,
    /// Crashes per day.
    pub crashes: Vec<DayCount>,
    /// Per-core load per bucket (same tier and resolution as `points`); empty when the range has none.
    #[serde(default)]
    pub core_points: Vec<crate::history::CoreBucket>,
}

/// A plugin's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginState {
    /// Running.
    Running,
    /// Stopped.
    Stopped,
    /// Failed (will be retried).
    Failed,
    /// Failed too often; off until the user clears it.
    Quarantined,
}

/// A plugin as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
    /// State.
    pub state: PluginState,
    /// The last error.
    pub error: Option<String>,
    /// What it contributes ("sensors", "cleanup", "actions").
    pub contributes: Vec<String>,
}

/// The agent's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reply {
    /// Done, nothing to return.
    Ok,
    /// Process details.
    Details(Box<ProcessDetails>),
    /// One activity entry.
    Activity(ActivityEntry),
    /// Activity entries.
    Activities(Vec<ActivityEntry>),
    /// Lockers.
    Lockers(Vec<Locker>),
    /// A snapshot header.
    Snapshot(SnapshotInfo),
    /// Snapshot headers.
    Snapshots(Vec<SnapshotInfo>),
    /// A diff.
    Diff(Box<SnapshotDiff>),
    /// A diagnosis.
    Diagnosis(Diagnosis),
    /// Anomalies.
    Anomalies(Vec<Anomaly>),
    /// History.
    History(Box<History>),
    /// The rule set.
    Rules(Box<RuleSet>),
    /// Cleanup categories.
    Categories(Vec<CleanupCategory>),
    /// A scan.
    Scan(Box<CleanupScan>),
    /// A cleanup's outcome.
    Outcome(CleanupOutcome),
    /// Quarantines.
    Quarantines(Vec<QuarantineEntry>),
    /// A size tree.
    SizeTree(Box<SizeNode>),
    /// The health report.
    Health(Box<HealthReport>),
    /// Tuning suggestions.
    Tuning(Vec<TuningSuggestion>),
    /// Startup entries.
    Startup(Vec<StartupEntry>),
    /// The weekly report.
    Report(Box<WeeklyReport>),
    /// Alerts.
    Alerts(Vec<Alert>),
    /// Settings.
    Settings(Box<Settings>),
    /// Plugins.
    Plugins(Vec<PluginInfo>),
}

impl Reply {
    /// The payload as the webview expects it (the variant's content, or `null` for [`Reply::Ok`]).
    pub fn into_json(self) -> Result<serde_json::Value, serde_json::Error> {
        use serde_json::to_value as v;
        match self {
            Reply::Ok => Ok(serde_json::Value::Null),
            Reply::Details(x) => v(x),
            Reply::Activity(x) => v(x),
            Reply::Activities(x) => v(x),
            Reply::Lockers(x) => v(x),
            Reply::Snapshot(x) => v(x),
            Reply::Snapshots(x) => v(x),
            Reply::Diff(x) => v(x),
            Reply::Diagnosis(x) => v(x),
            Reply::Anomalies(x) => v(x),
            Reply::History(x) => v(x),
            Reply::Rules(x) => v(x),
            Reply::Categories(x) => v(x),
            Reply::Scan(x) => v(x),
            Reply::Outcome(x) => v(x),
            Reply::Quarantines(x) => v(x),
            Reply::SizeTree(x) => v(x),
            Reply::Health(x) => v(x),
            Reply::Tuning(x) => v(x),
            Reply::Startup(x) => v(x),
            Reply::Report(x) => v(x),
            Reply::Alerts(x) => v(x),
            Reply::Settings(x) => v(x),
            Reply::Plugins(x) => v(x),
        }
    }
}

/// Why a request failed (the UI's `CommandError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The agent isn't running.
    AgentUnavailable,
    /// The process, file or item is gone.
    NotFound,
    /// The OS refused.
    AccessDenied,
    /// Not possible here.
    Unsupported,
    /// The process is protected.
    Protected,
    /// The request is invalid.
    BadRequest,
    /// It took too long.
    Timeout,
    /// Another operation of the kind is running.
    Busy,
    /// A bug.
    Internal,
}

/// A failed request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct AgentError {
    /// Code.
    pub code: ErrorCode,
    /// Message for the user.
    pub message: String,
}

impl AgentError {
    /// An error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> AgentError {
        AgentError {
            code,
            message: message.into(),
        }
    }
}

impl From<crate::ports::PlatformError> for AgentError {
    fn from(e: crate::ports::PlatformError) -> AgentError {
        use crate::ports::PlatformError as P;
        let code = match &e {
            P::NotFound(_) => ErrorCode::NotFound,
            P::AccessDenied(_) => ErrorCode::AccessDenied,
            P::Unsupported(_) => ErrorCode::Unsupported,
            P::Timeout(_) => ErrorCode::Timeout,
            P::Failed(_) => ErrorCode::Internal,
        };
        AgentError::new(code, e.to_string())
    }
}

/// A body on the app ↔ agent link: a request, its outcome, or a pushed event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Message {
    /// App → agent.
    Request(Box<Request>),
    /// Agent → app: the answer.
    Reply(Box<Result<Reply, AgentError>>),
    /// Agent → app: a live update.
    Live(Box<LiveUpdate>),
    /// Agent → app: an alert.
    Alert(Alert),
    /// Agent → app: a new activity entry.
    Activity(ActivityEntry),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixtures::sample;
    use crate::ports::PlatformError;
    use crate::process::Priority;

    fn round(m: &Message) -> Message {
        postcard::from_bytes(&postcard::to_stdvec(m).unwrap()).unwrap()
    }

    #[test]
    fn requests_have_the_json_shape_of_api_types() {
        let j = serde_json::to_value(Request::Act {
            target: ProcessRef {
                pid: 4,
                started: Timestamp(9),
            },
            action: ProcessAction::SetPriority(Priority::Idle),
        })
        .unwrap();
        assert_eq!(j, serde_json::json!({"act": {"target": {"pid": 4, "started": 9}, "action": {"setPriority": "idle"}}}));
        assert_eq!(serde_json::to_value(Request::WhySlow).unwrap(), "whySlow");
        let r: Request = serde_json::from_value(serde_json::json!({"runCleanup": {"scanId": "s", "itemIds": ["a"], "mode": "quarantine"}})).unwrap();
        assert!(matches!(r, Request::RunCleanup { mode: DeleteMode::Quarantine, .. }));
        let r: Request = serde_json::from_value(serde_json::json!({"history": {"range": "week", "end": null}})).unwrap();
        assert_eq!(r, Request::History { range: HistoryRange::Week, end: None });
        let r: Request = serde_json::from_value(serde_json::json!({"setStartupState": {"id": "x", "state": {"delayed": 30}}})).unwrap();
        assert!(matches!(r, Request::SetStartupState { state: StartupState::Delayed(30), .. }));
    }

    #[test]
    fn messages_survive_postcard() {
        let live = LiveUpdate {
            system: sample(1),
            pressure: MemoryPressure::Normal,
            top_cpu: vec![],
            top_memory: vec![],
            overhead: Overhead::default(),
            profile: ProfileId::Gaming,
            rules_paused: false,
            active_actions: vec![],
            processes: None,
            groups: Some(vec![]),
        };
        for m in [
            Message::Request(Box::new(Request::SaveSettings { settings: Settings::default() })),
            Message::Request(Box::new(Request::SaveRules { rules: RuleSet::default() })),
            Message::Live(Box::new(live)),
            Message::Reply(Box::new(Ok(Reply::Settings(Box::default())))),
            Message::Reply(Box::new(Err(AgentError::new(ErrorCode::Protected, "no")))),
        ] {
            assert_eq!(round(&m), m);
        }
    }

    #[test]
    fn replies_unwrap_to_their_payload() {
        assert_eq!(Reply::Ok.into_json().unwrap(), serde_json::Value::Null);
        assert_eq!(Reply::Activities(vec![]).into_json().unwrap(), serde_json::json!([]));
        let s = Reply::Settings(Box::default()).into_json().unwrap();
        assert_eq!(s["version"], 1);
        let all = vec![
            Reply::Lockers(vec![]),
            Reply::Snapshots(vec![]),
            Reply::Anomalies(vec![]),
            Reply::Categories(vec![]),
            Reply::Quarantines(vec![]),
            Reply::Tuning(vec![]),
            Reply::Startup(vec![]),
            Reply::Alerts(vec![]),
            Reply::Plugins(vec![]),
            Reply::Rules(Box::default()),
        ];
        for r in all {
            assert!(r.into_json().is_ok());
        }
    }

    #[test]
    fn platform_errors_map_to_codes() {
        let cases = [
            (PlatformError::NotFound("a".into()), ErrorCode::NotFound),
            (PlatformError::AccessDenied("a".into()), ErrorCode::AccessDenied),
            (PlatformError::Unsupported("a".into()), ErrorCode::Unsupported),
            (PlatformError::Timeout("a".into()), ErrorCode::Timeout),
            (PlatformError::Failed("a".into()), ErrorCode::Internal),
        ];
        for (p, code) in cases {
            let e: AgentError = p.into();
            assert_eq!(e.code, code);
            assert_eq!(e.to_string(), "a");
        }
        assert_eq!(serde_json::to_value(AgentError::new(ErrorCode::BadRequest, "x")).unwrap(), serde_json::json!({"code": "badRequest", "message": "x"}));
    }
}
