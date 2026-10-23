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
        /// Put the previous setting back even though something else changed it since Dyno applied it.
        #[serde(default)]
        force: bool,
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
    /// The weekly report for the seven days ending `weeks_ago` weeks before now (0 = the last seven days; at most 52).
    WeeklyReport {
        /// How many weeks back the week ends.
        weeks_ago: u32,
    },
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
        settings: Box<Settings>,
    },
    /// Plugins.
    Plugins,
    /// Asks the agent to exit (the app does this to restart an agent from an older build). Appended; answers `Ok`.
    Shutdown,
    /// The hardware inventory (memory slots, CPU, disks); read once and cached by the agent. Appended.
    Inventory,
    /// Alarms matching a filter, newest first. Appended.
    Alarms {
        /// Which alarms.
        filter: crate::alarms::AlarmFilter,
    },
    /// Alarm statistics (noisiest rules, time to acknowledge, suggestions). Appended.
    AlarmStats,
    /// Acknowledge alarms; answers the changed alarms. Appended.
    AcknowledgeAlarms {
        /// The alarm ids.
        ids: Vec<u64>,
    },
    /// Shelve an alarm; answers the changed alarm. Appended.
    ShelveAlarm {
        /// The alarm.
        id: u64,
        /// For how long.
        until: crate::alarms::ShelveFor,
    },
    /// Bring a shelved alarm back. Appended.
    UnshelveAlarm {
        /// The alarm.
        id: u64,
    },
    /// Mute or unmute every alarm of a rule. Appended.
    MuteRule {
        /// The rule.
        rule: String,
        /// Muted (true) or not.
        muted: bool,
    },
    /// Attach a note to an alarm. Appended.
    NoteAlarm {
        /// The alarm.
        id: u64,
        /// The text.
        text: String,
    },
    /// The change log of an alarm (all alarms when absent), newest first. Appended.
    AlarmHistory {
        /// The alarm.
        alarm_id: Option<u64>,
        /// At most this many (1-500).
        limit: u32,
    },
    /// Save the flight recorder's last ten minutes as a snapshot now; answers the snapshot's info. Appended.
    Capture,
    /// The saved flight recorder snapshots, newest first. Appended.
    RecorderSnapshots,
    /// One saved snapshot with its samples. Appended.
    RecorderSnapshot {
        /// Its id.
        id: u64,
    },
    /// What a diagnostics bundle would contain, with sizes and what was redacted; nothing is written. Appended.
    DiagnosticsPreview,
    /// Write the diagnostics bundle to this path (chosen by the user in a save dialog: absolute, ending in `.zip`,
    /// in an existing folder). Appended.
    SaveDiagnostics {
        /// The destination.
        path: String,
    },
    /// Open ports and connections per process, filtered (observe-only). Appended.
    Network {
        /// What to show.
        query: crate::network::NetworkQuery,
    },
    /// The URL "Look up IP" opens for an address, built from the saved template (or `template` when testing one that is
    /// not saved yet). The shell opens it in the default browser; Dyno never fetches it. Appended.
    LookupIp {
        /// The address as the page shows it.
        ip: String,
        /// A template to test instead of the saved one.
        template: Option<String>,
    },
    /// The benchmark runs and the one in progress. Appended.
    Benchmarks,
    /// Starts a benchmark run (answers the overview; refused with `Busy` while one runs). Never automatic. Appended.
    StartBenchmark {
        /// What to measure.
        kinds: Vec<crate::bench::BenchKind>,
        /// The user's name for the run.
        label: String,
        /// The change this run brackets (shared by its before and after runs).
        group: Option<String>,
        /// Before or after that change.
        phase: Option<crate::bench::BenchPhase>,
        /// Scratch file size for the disk test, MiB (default and cap in `bench`).
        disk_mib: Option<u32>,
    },
    /// Stops the run in progress after the current kind (that kind is stopped at once). Appended.
    CancelBenchmark,
    /// Deletes a saved run. Appended.
    DeleteBenchmark {
        /// The run id.
        id: String,
    },
    /// Compares two saved runs. Appended.
    CompareBenchmarks {
        /// The earlier run.
        before: String,
        /// The later run.
        after: String,
    },
    /// Reverse DNS names for up to 64 addresses, through the OS resolver (answers `HostNames`; cached by the agent).
    /// Sent only when the user asks for names, never in the background. Appended.
    ResolveHosts {
        /// Literal IP addresses.
        addresses: Vec<String>,
    },

    // ---- weekly report export (WP-08) ----
    /// Writes a weekly report as Markdown or HTML to a path the user picked in the OS save dialog (answers `ReportSaved`).
    /// Appended.
    SaveReport {
        /// Where (absolute, ending in the format's extension, in an existing folder).
        path: String,
        /// The file format.
        format: crate::report::ReportFormat,
        /// Which week, as for `WeeklyReport`.
        weeks_ago: u32,
    },

    // ---- throttling, lifespan and baselines (WP-23, WP-24, WP-25) ----
    /// Is the CPU throttled, why, and the recent episodes. Appended.
    Throttling,
    /// Life forecasts for SSDs and the battery. Appended.
    Lifespan,
    /// What is usual for this time of the week, and how the current values compare. Appended.
    Baselines,
    // ---- the privileged helper (WP-01, ADR 0016) ----
    /// Whether the privileged helper is installed and usable (answers `Helper`). Appended.
    HelperStatus,
    /// Installs or updates the helper after the user confirmed; the system's permission prompt opens. Answers `Helper`
    /// at once with the state `installing`; poll `HelperStatus`. Sent only from the confirmation dialog. Appended.
    InstallHelper,
    /// Removes the helper (answers `Helper`). Appended.
    UninstallHelper,

    // ---- issues and unusual programs (WP-06, WP-07) ----
    /// The Issues page: crashes, hangs and other problems the OS recorded, with the stability score, leaderboards and how
    /// each log source was read. A query: it reads what the agent stored. Appended.
    Issues {
        /// The window.
        query: crate::issues::IssuesQuery,
    },
    /// Reads the OS logs now (instead of waiting for the next scheduled read) and answers the Issues page. Appended.
    RescanIssues {
        /// The window.
        query: crate::issues::IssuesQuery,
    },
    /// Writes the issues of the last `days` as JSON lines (for Debrief) to a path the user picked in the OS save dialog;
    /// answers `IssuesSaved`. Appended.
    SaveIssues {
        /// Where (absolute, ending in `.jsonl`, in an existing folder).
        path: String,
        /// How many days back (1 to 90).
        days: u32,
    },
    /// New and unusual programs. A query. Appended.
    Unusual {
        /// Also list the programs the user ignored or protected.
        include_ignored: bool,
    },
    /// Ignores, protects or restores a program on the list; answers the list. Appended.
    UnusualAct {
        /// The program's key from the list.
        key: String,
        /// What to do.
        action: crate::unusual::baseline::UnusualAction,
    },
    /// Forgets the baseline of programs; learning starts again. Appended.
    ForgetBaseline,
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
    /// The hardware inventory. Appended.
    Inventory(Box<crate::inventory::SystemInventory>),
    /// Alarms. Appended.
    Alarms(Vec<crate::alarms::Alarm>),
    /// Alarm statistics. Appended.
    AlarmStats(Box<crate::alarms::AlarmStats>),
    /// Alarm changes. Appended.
    AlarmChanges(Vec<crate::alarms::AlarmChange>),
    /// A flight recorder snapshot's header (answer to `Capture`). Appended.
    Captured(crate::recorder::RecorderSnapshotInfo),
    /// Flight recorder snapshot headers. Appended.
    RecorderSnapshots(Vec<crate::recorder::RecorderSnapshotInfo>),
    /// A flight recorder snapshot with its samples. Appended.
    RecorderSnapshot(Box<crate::recorder::RecorderSnapshot>),
    /// The bundle's listing. Appended.
    DiagnosticsPreview(Box<crate::diagnostics::BundlePreview>),
    /// The bundle was written. Appended.
    DiagnosticsSaved(crate::diagnostics::SavedDiagnostics),
    /// A web address for the shell to open in the browser (https only). Appended.
    Url(String),
    /// Open ports and connections. Appended.
    Network(Box<crate::network::NetworkSnapshot>),
    /// The benchmark overview. Appended.
    Benchmarks(Box<crate::bench::BenchOverview>),
    /// Two runs compared. Appended.
    BenchComparison(Box<crate::bench::Comparison>),
    /// Host names for the addresses asked about. Appended.
    HostNames(Vec<crate::network_names::HostName>),

    // ---- weekly report export (WP-08) ----
    /// The report file was written. Appended.
    ReportSaved(crate::report::SavedReport),

    // ---- throttling, lifespan and baselines (WP-23, WP-24, WP-25) ----
    /// The throttling overview. Appended.
    Throttling(Box<crate::throttle::ThrottleOverview>),
    /// The life forecasts. Appended.
    Lifespan(Box<crate::lifespan::LifespanOverview>),
    /// The baseline comparison. Appended.
    Baselines(Box<crate::baseline::BaselineOverview>),
    // ---- the privileged helper (WP-01, ADR 0016) ----
    /// The helper's state. Appended.
    Helper(Box<crate::helper_wire::HelperStatus>),

    // ---- issues and unusual programs (WP-06, WP-07) ----
    /// The Issues page. Appended.
    Issues(Box<crate::issues::IssuesOverview>),
    /// The issues file was written. Appended.
    IssuesSaved(crate::issues::SavedIssues),
    /// New and unusual programs. Appended.
    Unusual(Box<crate::unusual::UnusualOverview>),
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
            Reply::Inventory(x) => v(x),
            Reply::Alarms(x) => v(x),
            Reply::AlarmStats(x) => v(x),
            Reply::AlarmChanges(x) => v(x),
            Reply::Captured(x) => v(x),
            Reply::RecorderSnapshots(x) => v(x),
            Reply::RecorderSnapshot(x) => v(x),
            Reply::DiagnosticsPreview(x) => v(x),
            Reply::DiagnosticsSaved(x) => v(x),
            Reply::Url(x) => v(x),
            Reply::Network(x) => v(x),
            Reply::Benchmarks(x) => v(x),
            Reply::BenchComparison(x) => v(x),
            Reply::HostNames(x) => v(x),
            Reply::ReportSaved(x) => v(x),
            Reply::Throttling(x) => v(x),
            Reply::Lifespan(x) => v(x),
            Reply::Baselines(x) => v(x),
            Reply::Helper(x) => v(x),
            Reply::Issues(x) => v(x),
            Reply::IssuesSaved(x) => v(x),
            Reply::Unusual(x) => v(x),
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
    /// Agent → app: an alarm changed (raised, acknowledged, cleared...). Appended.
    Alarm(crate::alarms::AlarmChange),
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
    fn a_revert_without_force_does_not_overwrite_a_changed_setting() {
        let r: Request = serde_json::from_str(r#"{"revertTuning":{"id":"powerPlan"}}"#).unwrap();
        assert_eq!(
            r,
            Request::RevertTuning {
                id: "powerPlan".into(),
                force: false
            }
        );
        let forced: Request = serde_json::from_str(r#"{"revertTuning":{"id":"x","force":true}}"#).unwrap();
        assert!(matches!(forced, Request::RevertTuning { force: true, .. }));
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
        assert_eq!(
            j,
            serde_json::json!({"act": {"target": {"pid": 4, "started": 9}, "action": {"setPriority": "idle"}}})
        );
        assert_eq!(serde_json::to_value(Request::WhySlow).unwrap(), "whySlow");
        let r: Request = serde_json::from_value(serde_json::json!({"weeklyReport": {"weeksAgo": 2}})).unwrap();
        assert_eq!(r, Request::WeeklyReport { weeks_ago: 2 });
        let r: Request =
            serde_json::from_value(serde_json::json!({"saveReport": {"path": "/r.md", "format": "markdown", "weeksAgo": 0}})).unwrap();
        assert!(matches!(
            r,
            Request::SaveReport {
                format: crate::report::ReportFormat::Markdown,
                ..
            }
        ));
        let r: Request =
            serde_json::from_value(serde_json::json!({"runCleanup": {"scanId": "s", "itemIds": ["a"], "mode": "quarantine"}})).unwrap();
        assert!(matches!(
            r,
            Request::RunCleanup {
                mode: DeleteMode::Quarantine,
                ..
            }
        ));
        let r: Request = serde_json::from_value(serde_json::json!({"history": {"range": "week", "end": null}})).unwrap();
        assert_eq!(
            r,
            Request::History {
                range: HistoryRange::Week,
                end: None
            }
        );
        let r: Request = serde_json::from_value(serde_json::json!({"setStartupState": {"id": "x", "state": {"delayed": 30}}})).unwrap();
        assert!(matches!(
            r,
            Request::SetStartupState {
                state: StartupState::Delayed(30),
                ..
            }
        ));
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
            Message::Request(Box::new(Request::SaveSettings { settings: Box::default() })),
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
            Reply::Inventory(Box::new(crate::inventory::SystemInventory::new(
                String::new(),
                None,
                Default::default(),
                crate::inventory::MemoryLayout::unified(1, Default::default(), None),
                vec![],
            ))),
            Reply::Rules(Box::default()),
            Reply::Alarms(vec![]),
            Reply::AlarmStats(Box::default()),
            Reply::AlarmChanges(vec![]),
            Reply::Benchmarks(Box::default()),
            Reply::Network(Box::default()),
            Reply::HostNames(vec![]),
            Reply::Url("https://example.com/".into()),
            Reply::RecorderSnapshots(vec![]),
            Reply::Helper(Box::new(crate::helper_wire::assess(
                crate::helper::TuningHost::Linux,
                Ok(()),
                crate::helper_wire::Probe::Absent,
                "1",
            ))),
            Reply::ReportSaved(crate::report::SavedReport {
                path: "/x.md".into(),
                bytes: 1,
            }),
            Reply::IssuesSaved(crate::issues::SavedIssues {
                path: "/x.jsonl".into(),
                bytes: 1,
                count: 1,
            }),
            Reply::DiagnosticsSaved(crate::diagnostics::SavedDiagnostics {
                path: "/x.zip".into(),
                bytes: 1,
                files: 1,
            }),
            Reply::RecorderSnapshot(Box::new(crate::recorder::RecorderSnapshot::new(
                Timestamp(0),
                crate::recorder::CaptureReason::Manual,
                vec![],
            ))),
        ];
        for r in all {
            assert!(r.into_json().is_ok());
        }
    }

    #[test]
    fn helper_requests_and_status_have_the_json_shape_of_api_types() {
        for (name, r) in [
            ("helperStatus", Request::HelperStatus),
            ("installHelper", Request::InstallHelper),
            ("uninstallHelper", Request::UninstallHelper),
        ] {
            assert_eq!(serde_json::to_value(&r).unwrap(), name);
            assert_eq!(serde_json::from_value::<Request>(serde_json::json!(name)).unwrap(), r);
        }
        let status = crate::helper_wire::assess(crate::helper::TuningHost::Macos, Ok(()), crate::helper_wire::Probe::Absent, "1");
        let j = Reply::Helper(Box::new(status)).into_json().unwrap();
        assert_eq!(j["state"], "notInstalled");
        assert_eq!(j["error"], serde_json::Value::Null);
        assert!(j["prompt"].as_str().unwrap().contains("password"));
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
        assert_eq!(
            serde_json::to_value(AgentError::new(ErrorCode::BadRequest, "x")).unwrap(),
            serde_json::json!({"code": "badRequest", "message": "x"})
        );
    }

    #[test]
    fn hardware_requests_and_replies_have_the_json_shape_of_api_types() {
        for (r, name) in [
            (Request::Throttling, "throttling"),
            (Request::Lifespan, "lifespan"),
            (Request::Baselines, "baselines"),
        ] {
            assert_eq!(serde_json::to_value(&r).unwrap(), name);
            assert_eq!(serde_json::from_value::<Request>(serde_json::json!(name)).unwrap(), r);
            let bytes = postcard::to_stdvec(&Message::Request(Box::new(r.clone()))).unwrap();
            assert_eq!(postcard::from_bytes::<Message>(&bytes).unwrap(), Message::Request(Box::new(r)));
        }
        let life = Reply::Lifespan(Box::new(crate::lifespan::LifespanOverview {
            at: Timestamp(5),
            forecasts: vec![crate::lifespan::unavailable(
                crate::lifespan::LifeSubject::Battery,
                "Battery",
                "No battery",
            )],
        }));
        let j = life.into_json().unwrap();
        assert_eq!(j["forecasts"][0]["outcome"]["unavailable"]["reason"], "No battery");
        assert_eq!(j["forecasts"][0]["subject"], "battery");
        let base = Reply::Baselines(Box::new(crate::baseline::BaselineOverview {
            at: Timestamp(5),
            learning: crate::baseline::Learning::Learning {
                days_done: 2,
                days_needed: 7,
            },
            items: vec![],
        }));
        assert_eq!(base.into_json().unwrap()["learning"]["learning"]["daysNeeded"], 7);
    }
}
