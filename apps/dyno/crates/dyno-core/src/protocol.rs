//! The app ↔ agent protocol: requests, replies and pushed events. Carried as postcard bodies in dyno-ipc frames
//! between the processes, and as camelCase JSON between the Tauri shell and the webview (`api.types.ts` mirrors it).
//! Append new variants at the end; anything else is a new [`PROTOCOL_VERSION`].

use crate::activity::ActivityEntry;
use crate::alerts::Alert;
use crate::anomaly::Anomaly;
use crate::diagnosis::Diagnosis;
use crate::forecast::DiskTrend;
use crate::grouping::AppGroup;
use crate::health::HealthReport;
use crate::history::{Consumer, HistoryPoint, HistoryRange};
use crate::metrics::{MemoryPressure, SystemSample};
use crate::others::OtherUsers;
use crate::process::{Locker, ProcessDetails, ProcessRef, ProcessSample};
use crate::report::WeeklyReport;
use crate::settings::Settings;
use crate::snapshot::{SnapshotDiff, SnapshotInfo};
use crate::startup::StartupEntry;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change.
///
/// 3: `Settings` lost `tray` and `tray_items` and holds the dashboard, menu bar and popup layouts as `paddock-widgets`
/// layouts (family ADR 0035); `WidgetLayout`, `EditWidgetLayout`, `PopoverValues` and `WidgetLayoutChanged` are new.
///
/// 4: `baseline::Learning::Learning` holds `day`, `days_needed`, `since` and `ready_at` (was `days_done`, `days_needed`).
pub const PROTOCOL_VERSION: u32 = 4;

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
    /// The health report.
    Health,
    /// Startup entries.
    StartupEntries,
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

    // ---- performance counters browser (WP-12) ----
    /// The counter objects the OS offers, with sizes only (answers `CounterObjects`). Appended.
    CounterObjects,
    /// One object with its counters and instances (answers `CounterObject`). Appended.
    CounterObject {
        /// The object's name.
        name: String,
    },
    /// Counters matching the words typed, best first (answers `CounterHits`). Appended.
    CounterSearch {
        /// The words; every one must match.
        query: String,
        /// At most this many (1-200).
        limit: u32,
    },
    /// Reads up to 32 counters now (answers `CounterValues`) and adds them to their charts. Appended.
    CounterValues {
        /// The paths, as `\Object(Instance)\Counter`.
        paths: Vec<String>,
    },
    /// The readings kept for counters over the last minutes, with the app's events in the same window (answers
    /// `CounterHistory`). Appended.
    CounterHistory {
        /// The paths (up to 32).
        paths: Vec<String>,
        /// How far back, 1-60 minutes.
        minutes: u32,
    },
    // ---- throttling, lifespan and baselines (WP-23, WP-24, WP-25) ----
    /// Is the CPU throttled, why, and the recent episodes. Appended.
    Throttling,
    /// Life forecasts for SSDs and the battery. Appended.
    Lifespan,
    /// What is usual for this time of the week, and how the current values compare. Appended.
    Baselines,

    // ---- issues and unusual programs (WP-06, WP-07) ----
    /// The Issues page: crashes, hangs and other problems the OS recorded, with the stability score, leaderboards and how
    /// each log source was read. A query: it reads what the agent stored. Appended.
    Issues {
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
    Unusual,
    /// Forgets the baseline of programs; learning starts again. Appended.
    ForgetBaseline,

    // ---- right-click search, hash search, startup filter, watched folders (WP-09, WP-33, WP-34, WP-10) ----
    /// The redacted starting text of a web search for `text` (home folders, user and computer names, addresses, GUIDs,
    /// serial numbers removed) and what was removed. A query; nothing is sent anywhere. Appended.
    SearchSeed {
        /// What the person right-clicked (a process name, an issue title, ...).
        text: String,
    },
    /// The address of a web search for exactly this text (answers `Url`; the shell opens it in the default browser on the
    /// click, Dyno never fetches it). Appended.
    WebSearch {
        /// The text the person confirmed.
        query: String,
    },
    /// The VirusTotal page of a hash (answers `Url`; the shell opens it in the default browser). Appended.
    VirusTotalUrl {
        /// 64 hexadecimal digits.
        sha256: String,
    },
    /// The startup entries after hiding the operating system's and signed ones, with the counts. A query. Appended.
    StartupOverview {
        /// What to hide.
        filter: crate::startup::StartupFilter,
    },
    // ---- extras, backup health and taming proof (WP-13, WP-14, WP-36) ----
    /// "What changed": installs, updates, new startup items and setting changes in a window. A query. Appended.
    Changes {
        /// The window.
        query: crate::extras::changes::ChangesQuery,
    },
    /// GPU use per process. A query. Appended.
    GpuProcesses,
    /// Writes the inventory report (hardware and software) to a path the user picked in the OS save dialog; answers
    /// `InventorySaved`. Appended.
    SaveInventoryReport {
        /// Where (absolute, ending in the format's extension, in an existing folder).
        path: String,
        /// The file format.
        format: crate::extras::software::InventoryFormat,
    },
    /// The maintenance windows' state now. A query. Appended.
    Maintenance,
    /// Backup health (Time Machine, Windows Backup, File History, restic, Borg, Duplicati): the last check, without
    /// running one unless there is none. Appended.
    Backup,

    // ---- long operations as jobs (ER-17, ER-18, ER-19): every request below answers at once ----
    /// Starts a long read-only operation (find files, read the OS logs, check the backups, hash a file) in the
    /// background and answers its status at once. A second start of a kind that is running answers the running job.
    StartJob {
        /// What to run.
        spec: crate::jobs::JobSpec,
    },
    /// The status of every running job and of the last few that ended (what a window that opens late needs). A query.
    Jobs,
    /// Asks a job to stop and returns at once (answers `JobCancelled`); what it was building is dropped and nothing else changes.
    CancelJob {
        /// The job.
        id: u64,
    },
    /// What a done job produced; `NotFound` when the job is unknown, not done, or too old to be kept.
    JobResult {
        /// The job.
        id: u64,
    },

    // ---- the disk map (ADR 0024 job: pushed `Message::DiskScan` events, no polling) ----
    /// The volumes mounted now: internal, external and network, with capacity. Appended.
    DiskVolumes,
    /// Starts a scan of a volume or a folder and answers its status at once; a second start of the same target answers
    /// the running scan. Appended.
    StartDiskScan {
        /// What to scan.
        target: crate::disk::scan::DiskTarget,
    },
    /// Stops a scan and returns at once; what was counted stays. Appended.
    CancelDiskScan {
        /// The scan.
        scan: u64,
    },
    /// The status of every kept scan (what a window that opens late needs). Appended.
    DiskScans,
    /// One page of one level of a scan: the children of a folder, biggest first, the small ones folded into "other". Appended.
    DiskChildren {
        /// The scan.
        scan: u64,
        /// The folder (the scan's root when absent).
        node: Option<u32>,
        /// Where the page starts.
        offset: u32,
        /// How many children at most (1 to 256).
        limit: u32,
        /// Children below this share of the folder (0 to 1) are folded into "other"; 0 shows all.
        min_fraction: f64,
    },
    /// The full path of a node, for Reveal and Copy path. Appended.
    DiskPath {
        /// The scan.
        scan: u64,
        /// The node.
        node: u32,
    },
    /// Several levels of a scan under a folder in one answer: the nested children down to `rings` deep (1 to 6), those with at
    /// least `min_share` of the folder (0 to 1) shown and the rest folded into each level's "other", at most `max_nodes` nodes
    /// (1 to 2000) in all. Appended.
    DiskRings {
        /// The scan.
        scan: u64,
        /// The centre (the scan's root when absent).
        node: Option<u32>,
        /// How many levels.
        rings: u8,
        /// The smallest share of the centre a child may have to be shown.
        min_share: f64,
        /// The most nodes in the answer.
        max_nodes: u32,
    },
    /// Reads the folders a finished scan could not open once more, and only those; answers the scan's status at once. The
    /// scan runs again as a job (same id, new events). Appended.
    RetryDiskScan {
        /// The scan.
        scan: u64,
    },
    /// Opens the system's privacy pane where Full Disk Access is given (macOS; other systems answer `Unsupported` with the
    /// reason). Appended.
    OpenFullDiskAccessSettings,
    /// Shows a node in Finder, Explorer or the file manager. The path comes from the scan, never from the caller. Appended.
    DiskReveal {
        /// The scan.
        scan: u64,
        /// The node.
        node: u32,
    },
    /// Adds a custom alert rule, or replaces the one with the same id; answers the settings. The rule is validated by
    /// the agent. Appended.
    UpsertAlertRule {
        /// The rule.
        rule: crate::alarms::custom::CustomAlertRule,
    },
    /// Deletes the custom alert rule with this id; answers the settings. Appended.
    DeleteAlertRule {
        /// The rule's id.
        id: String,
    },
    /// One surface's layout, with its catalogue, built-in presets, notes and whether it can be edited (ADR 0035).
    /// Answers [`Reply::WidgetLayout`].
    WidgetLayout {
        /// Which surface.
        surface: crate::widgets::DynoSurface,
    },
    /// Edits one surface's layout; the agent validates the whole result, saves it and answers the new view
    /// ([`Reply::WidgetLayout`]) or the reason it was refused (`BadRequest`, the message says what to change). Pushes
    /// [`Message::WidgetLayoutChanged`] to every app.
    EditWidgetLayout {
        /// The edit; its variant names the surface.
        edit: crate::widgets::SurfaceEdit,
    },
    /// The popup's values for the widgets that are not live (alarms, disk space, backup, last issue, pinned counters),
    /// from state the agent already keeps. Placements that are not in the popup layout, or are live, are left out.
    /// Answers [`Reply::PopoverValues`].
    PopoverValues {
        /// Which placements.
        placements: Vec<paddock_widgets::PlacementId>,
    },
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
    /// Every process, when a subscriber asked for them.
    pub processes: Option<Vec<ProcessSample>>,
    /// Every group, likewise.
    pub groups: Option<Vec<AppGroup>>,
    /// Other users' processes folded into one row; `processes` and `groups` leave them out. `None` when there are none
    /// or the grouping is off.
    pub other_users: Option<OtherUsers>,
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
    /// An alert.
    Alert,
    /// The machine started.
    Boot,
    /// A Wi-Fi join or leave, a VPN coming up or going down, or an IP address change. Appended.
    Network,
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
    /// What it contributes ("sensors").
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
    /// The health report.
    Health(Box<HealthReport>),
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

    // ---- performance counters browser (WP-12) ----
    /// The counter objects. Appended.
    CounterObjects(Box<crate::counters::CounterObjectList>),
    /// One counter object. Appended.
    CounterObject(Box<crate::counters::CounterObject>),
    /// Counters found by search. Appended.
    CounterHits(Vec<crate::counters::CounterHit>),
    /// Counter readings. Appended.
    CounterValues(Vec<crate::counters::CounterReading>),
    /// Counter history with events. Appended.
    CounterHistory(Box<crate::counters::CounterHistory>),
    // ---- throttling, lifespan and baselines (WP-23, WP-24, WP-25) ----
    /// The throttling overview. Appended.
    Throttling(Box<crate::throttle::ThrottleOverview>),
    /// The life forecasts. Appended.
    Lifespan(Box<crate::lifespan::LifespanOverview>),
    /// The baseline comparison. Appended.
    Baselines(Box<crate::baseline::BaselineOverview>),

    // ---- issues and unusual programs (WP-06, WP-07) ----
    /// The Issues page. Appended.
    Issues(Box<crate::issues::IssuesOverview>),
    /// The issues file was written. Appended.
    IssuesSaved(crate::issues::SavedIssues),
    /// New and unusual programs. Appended.
    Unusual(Box<crate::unusual::UnusualOverview>),

    // ---- right-click search, hash search, startup filter, watched folders ----
    /// The starting text of a search. Appended.
    SearchSeed(crate::weblinks::SearchSeed),
    /// The filtered startup list. Appended.
    StartupOverview(Box<crate::startup::StartupOverview>),
    // ---- extras, backup health and taming proof (WP-13, WP-14, WP-36) ----
    /// What changed. Appended.
    Changes(Box<crate::extras::changes::ChangeTimeline>),
    /// GPU per process. Appended.
    Gpu(Box<crate::extras::gpu::GpuReport>),
    /// The inventory report was written. Appended.
    InventorySaved(SavedInventory),
    /// The maintenance windows' state. Appended.
    Maintenance(crate::extras::maintenance::MaintenanceStatus),
    /// Backup health. Appended.
    Backup(Box<crate::backup::BackupReport>),
    /// The volumes. Appended.
    DiskVolumes(Vec<crate::disk::volume::Volume>),
    /// One scan's status. Appended.
    DiskScan(Box<crate::disk::scan::DiskScanStatus>),
    /// Every kept scan's status. Appended.
    DiskScans(Vec<crate::disk::scan::DiskScanStatus>),
    /// One page of one level. Appended.
    DiskChildren(Box<crate::disk::index::DiskChildren>),
    /// A node's full path. Appended.
    DiskPath(String),
    /// Several levels of a scan.
    DiskRings(Box<crate::disk::index::DiskRings>),

    // ---- jobs ----
    /// A job's status (the answer to starting one).
    Job(Box<paddock_jobs::wire::WireSnapshot>),
    /// Every running job and the last few that ended.
    Jobs(Vec<paddock_jobs::wire::WireSnapshot>),
    /// Whether `CancelJob` found a running job to stop (false: unknown or already over).
    JobCancelled(bool),
    /// What a done job produced.
    JobOutput(Box<crate::jobs::JobOutput>),

    // ---- widget layouts ----
    /// One surface's layout view.
    WidgetLayout(Box<crate::widgets::SurfaceView>),
    /// The popup's values for the asked placements.
    PopoverValues(crate::widgets::JsonOnLink<Vec<crate::widgets::popover_values::PopoverState>>),
}

/// The inventory report that was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedInventory {
    /// Where.
    pub path: String,
    /// Its size in bytes.
    pub bytes: u64,
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
            Reply::Health(x) => v(x),
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
            Reply::CounterObjects(x) => v(x),
            Reply::CounterObject(x) => v(x),
            Reply::CounterHits(x) => v(x),
            Reply::CounterValues(x) => v(x),
            Reply::CounterHistory(x) => v(x),
            Reply::Throttling(x) => v(x),
            Reply::Lifespan(x) => v(x),
            Reply::Baselines(x) => v(x),
            Reply::Issues(x) => v(x),
            Reply::IssuesSaved(x) => v(x),
            Reply::Unusual(x) => v(x),
            Reply::SearchSeed(x) => v(x),
            Reply::StartupOverview(x) => v(x),
            Reply::Changes(x) => v(x),
            Reply::Gpu(x) => v(x),
            Reply::InventorySaved(x) => v(x),
            Reply::Maintenance(x) => v(x),
            Reply::Backup(x) => v(x),
            Reply::DiskVolumes(x) => v(x),
            Reply::DiskScan(x) => v(x),
            Reply::DiskScans(x) => v(x),
            Reply::DiskChildren(x) => v(x),
            Reply::DiskPath(x) => v(x),
            Reply::DiskRings(x) => v(x),
            // The UI's shape of a job (`paddock_jobs::JobSnapshot`), not the postcard-safe one.
            Reply::Job(x) => v(paddock_jobs::JobSnapshot::from(*x)),
            Reply::Jobs(x) => v(x.into_iter().map(paddock_jobs::JobSnapshot::from).collect::<Vec<_>>()),
            Reply::JobCancelled(x) => v(x),
            Reply::JobOutput(x) => v(x),
            Reply::WidgetLayout(x) => v(x),
            Reply::PopoverValues(x) => v(x),
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
    /// Agent → app: a disk scan's status changed (at most four per second per scan, a final one at the end). Appended.
    DiskScan(Box<crate::disk::scan::DiskScanStatus>),
    /// Agent → app: the volumes mounted now changed (one came, one went, one stopped answering). Appended.
    DiskVolumes(Vec<crate::disk::volume::Volume>),
    /// Agent → app: a job's status changed (the newest of each job wins; a final state is never dropped). Appended.
    Job(Box<paddock_jobs::wire::WireSnapshot>),
    /// Agent -> app: a surface's layout changed (an edit from any window). Appended.
    WidgetLayoutChanged(crate::widgets::DynoSurface),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixtures::sample;
    use crate::ports::PlatformError;

    fn round(m: &Message) -> Message {
        postcard::from_bytes(&postcard::to_stdvec(m).unwrap()).unwrap()
    }

    #[test]
    fn requests_have_the_json_shape_of_api_types() {
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
        let r: Request = serde_json::from_value(serde_json::json!({"history": {"range": "week", "end": null}})).unwrap();
        assert_eq!(
            r,
            Request::History {
                range: HistoryRange::Week,
                end: None
            }
        );
    }

    #[test]
    fn counter_requests_have_the_json_shape_of_api_types() {
        let r: Request = serde_json::from_value(serde_json::json!({"counterHistory": {"paths": ["\\a\\b"], "minutes": 5}})).unwrap();
        assert_eq!(
            r,
            Request::CounterHistory {
                paths: vec!["\\a\\b".into()],
                minutes: 5
            }
        );
        assert_eq!(serde_json::to_value(Request::CounterObjects).unwrap(), "counterObjects");
        let r: Request = serde_json::from_value(serde_json::json!({"counterSearch": {"query": "pages", "limit": 20}})).unwrap();
        assert!(matches!(r, Request::CounterSearch { limit: 20, .. }));
    }

    #[test]
    fn messages_survive_postcard() {
        let live = LiveUpdate {
            system: sample(1),
            pressure: MemoryPressure::Normal,
            top_cpu: vec![],
            top_memory: vec![],
            overhead: Overhead::default(),
            processes: None,
            groups: Some(vec![]),
            other_users: None,
        };
        for m in [
            Message::Request(Box::new(Request::SaveSettings { settings: Box::default() })),
            Message::Live(Box::new(live)),
            Message::Reply(Box::new(Ok(Reply::Settings(Box::default())))),
            Message::Reply(Box::new(Err(AgentError::new(ErrorCode::Busy, "no")))),
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
            Reply::Alarms(vec![]),
            Reply::AlarmStats(Box::default()),
            Reply::AlarmChanges(vec![]),
            Reply::Benchmarks(Box::default()),
            Reply::Network(Box::default()),
            Reply::HostNames(vec![]),
            Reply::Changes(Box::default()),
            Reply::Gpu(Box::default()),
            Reply::InventorySaved(SavedInventory {
                path: "/x.md".into(),
                bytes: 1,
            }),
            Reply::Maintenance(
                crate::extras::maintenance::MaintenanceSettings::default().status(Timestamp(0), &crate::time::FixedZone::UTC),
            ),
            Reply::Backup(Box::default()),
            Reply::CounterObjects(Box::default()),
            Reply::CounterHits(vec![]),
            Reply::CounterValues(vec![]),
            Reply::CounterHistory(Box::default()),
            Reply::CounterObject(Box::new(crate::counters::CounterObject {
                name: "Memory".into(),
                counters: vec![],
                instances: vec![],
                instances_total: 0,
            })),
            Reply::Url("https://example.com/".into()),
            Reply::RecorderSnapshots(vec![]),
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

    fn disk_status() -> crate::disk::scan::DiskScanStatus {
        use crate::disk::scan::*;
        DiskScanStatus {
            scan: 1,
            seq: 1,
            name: "Macintosh HD".into(),
            target: DiskTarget::Folder { path: "/x".into() },
            volume: None,
            state: ScanState::Running,
            phase: ScanPhase::ReadingFolders,
            progress: ScanProgress::default(),
            truncated: false,
            unreadable_first: vec![],
            needs_full_disk_access: false,
            items_per_second: Some(10),
            eta_seconds: None,
            hidden_bytes: None,
            scanned_at: None,
        }
    }

    #[test]
    fn disk_map_requests_replies_and_pushes_survive_postcard_and_json() {
        let index = crate::disk::index::DiskIndex::new("r", crate::disk::index::ScanLimits::default());
        let page = index.children(index.root(), 0, 10, 0.0).unwrap();
        let replies = [
            Reply::DiskVolumes(vec![]),
            Reply::DiskScan(Box::new(disk_status())),
            Reply::DiskScans(vec![disk_status()]),
            Reply::DiskChildren(Box::new(page)),
            Reply::DiskPath("/x".into()),
            Reply::DiskRings(Box::new(index.rings(index.root(), 2, 0.0, 10).unwrap())),
            Reply::Jobs(vec![]),
            Reply::JobCancelled(true),
            Reply::JobOutput(Box::new(crate::jobs::JobOutput::Backup(Box::default()))),
        ];
        for r in replies {
            let m = Message::Reply(Box::new(Ok(r.clone())));
            assert_eq!(round(&m), m);
            assert!(r.into_json().is_ok());
        }
        let requests = [
            Request::DiskVolumes,
            Request::StartDiskScan {
                target: crate::disk::scan::DiskTarget::Volume { id: "/".into() },
            },
            Request::CancelDiskScan { scan: 1 },
            Request::DiskScans,
            Request::DiskChildren {
                scan: 1,
                node: Some(2),
                offset: 0,
                limit: 48,
                min_fraction: 0.004,
            },
            Request::DiskPath { scan: 1, node: 2 },
            Request::RetryDiskScan { scan: 1 },
            Request::DiskReveal { scan: 1, node: 2 },
            Request::OpenFullDiskAccessSettings,
            Request::WidgetLayout {
                surface: crate::widgets::DynoSurface::MenuBar,
            },
            Request::EditWidgetLayout {
                edit: crate::widgets::SurfaceEdit::Popover(paddock_widgets::Edit::ResetToDefaults),
            },
            Request::PopoverValues {
                placements: vec![paddock_widgets::PlacementId::new("disk-space").unwrap()],
            },
            Request::DiskRings {
                scan: 1,
                node: Some(2),
                rings: 3,
                min_share: 0.01,
                max_nodes: 500,
            },
        ];
        for r in requests {
            let m = Message::Request(Box::new(r));
            assert_eq!(round(&m), m);
        }
        let push = Message::DiskScan(Box::new(disk_status()));
        assert_eq!(round(&push), push);
        let volumes = Message::DiskVolumes(vec![]);
        assert_eq!(round(&volumes), volumes);
        let changed = Message::WidgetLayoutChanged(crate::widgets::DynoSurface::Popover);
        assert_eq!(round(&changed), changed);
        for reply in [
            Reply::WidgetLayout(Box::new(crate::widgets::layout_view(
                &Settings::default(),
                crate::widgets::DynoSurface::Popover,
            ))),
            Reply::PopoverValues(crate::widgets::JsonOnLink(vec![])),
        ] {
            let m = Message::Reply(Box::new(Ok(reply)));
            assert_eq!(round(&m), m);
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
        assert_eq!(
            serde_json::to_value(AgentError::new(ErrorCode::BadRequest, "x")).unwrap(),
            serde_json::json!({"code": "badRequest", "message": "x"})
        );
    }

    #[test]
    fn extras_requests_have_the_json_shape_of_api_types() {
        for (name, r) in [
            ("gpuProcesses", Request::GpuProcesses),
            ("maintenance", Request::Maintenance),
            ("backup", Request::Backup),
            ("jobs", Request::Jobs),
        ] {
            assert_eq!(serde_json::to_value(&r).unwrap(), name);
            assert_eq!(serde_json::from_value::<Request>(serde_json::json!(name)).unwrap(), r);
        }
        let r: Request = serde_json::from_value(serde_json::json!({"changes": {"query": {"around": 5, "hours": 6}}})).unwrap();
        assert!(matches!(r, Request::Changes { query } if query.hours == 6));
        let r: Request = serde_json::from_value(serde_json::json!({"changes": {"query": {}}})).unwrap();
        assert!(matches!(r, Request::Changes { query } if query.around.is_none()));
        let r: Request =
            serde_json::from_value(serde_json::json!({"startJob": {"spec": {"findFiles": {"query": {"folder": "/x", "minBytes": 5}}}}}))
                .unwrap();
        assert!(
            matches!(r, Request::StartJob { spec: crate::jobs::JobSpec::FindFiles { query } } if query.mode == crate::extras::files::FinderMode::Both)
        );
        let r: Request =
            serde_json::from_value(serde_json::json!({"saveInventoryReport": {"path": "/r.md", "format": "markdown"}})).unwrap();
        assert!(matches!(r, Request::SaveInventoryReport { .. }));
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
                day: 3,
                days_needed: 7,
                since: Some(Timestamp(1)),
                ready_at: Some(Timestamp(2)),
            },
            items: vec![],
        }));
        let j = base.into_json().unwrap();
        assert_eq!(j["learning"]["learning"]["daysNeeded"], 7);
        assert_eq!(j["learning"]["learning"]["day"], 3);
        assert_eq!(j["learning"]["learning"]["since"], 1);
        assert_eq!(j["learning"]["learning"]["readyAt"], 2);
    }
}

#[cfg(test)]
mod search_watch_tests {
    use super::*;

    fn round<T: Serialize + serde::de::DeserializeOwned>(v: &T) -> T {
        postcard::from_bytes(&postcard::to_stdvec(v).unwrap()).unwrap()
    }

    #[test]
    fn the_new_requests_have_the_json_shape_of_api_types() {
        let j = |r: Request| serde_json::to_value(r).unwrap();
        assert_eq!(
            j(Request::SearchSeed { text: "x".into() }),
            serde_json::json!({"searchSeed": {"text": "x"}})
        );
        assert_eq!(
            j(Request::WebSearch { query: "q".into() }),
            serde_json::json!({"webSearch": {"query": "q"}})
        );
        assert_eq!(
            j(Request::StartJob {
                spec: crate::jobs::JobSpec::FileHash {
                    source: crate::weblinks::HashSource::Path("/a".into())
                }
            }),
            serde_json::json!({"startJob": {"spec": {"fileHash": {"source": {"path": "/a"}}}}})
        );
        assert_eq!(
            j(Request::StartupOverview {
                filter: crate::startup::StartupFilter {
                    hide_os: true,
                    hide_signed: false
                }
            }),
            serde_json::json!({"startupOverview": {"filter": {"hideOs": true, "hideSigned": false}}})
        );
        assert_eq!(serde_json::to_value(TimelineEventKind::Network).unwrap(), "network");
    }

    #[test]
    fn the_new_requests_and_replies_survive_the_wire() {
        let reqs = vec![
            Request::SearchSeed { text: "a".into() },
            Request::WebSearch { query: "b".into() },
            Request::StartJob {
                spec: crate::jobs::JobSpec::FileHash {
                    source: crate::weblinks::HashSource::Process(ProcessRef {
                        pid: 3,
                        started: Some(Timestamp(4)),
                    }),
                },
            },
            Request::StartJob {
                spec: crate::jobs::JobSpec::RefreshBackup,
            },
            Request::Jobs,
            Request::CancelJob { id: 9 },
            Request::JobResult { id: 9 },
            Request::VirusTotalUrl { sha256: "0".repeat(64) },
            Request::StartupOverview {
                filter: Default::default(),
            },
        ];
        for r in reqs {
            assert_eq!(round(&r), r);
        }
        let seed = Reply::SearchSeed(crate::weblinks::SearchSeed {
            text: "t".into(),
            removed: vec!["1 IP address".into()],
        });
        assert_eq!(round(&seed), seed);
        assert_eq!(seed.into_json().unwrap()["removed"][0], "1 IP address");
    }
}
