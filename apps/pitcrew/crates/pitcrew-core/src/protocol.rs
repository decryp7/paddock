//! The protocol between the Pitcrew app and the Pitcrew service (ADR 0001, ADR 0004).
//!
//! Over a private local stream (a named pipe on Windows, a Unix socket on macOS). Each frame is a 4-byte
//! little-endian length followed by that many bytes of JSON. The first request on a connection is
//! [`Request::Hello`]; the service refuses other protocol versions with [`ErrorCode::ProtocolMismatch`]. Then either
//! one request, one response, repeated; or, after [`Request::Subscribe`], a stream of [`Response::Event`]s pushed by
//! the service. Frames are bounded by [`MAX_FRAME`], so neither side can be made to allocate without limit.

use crate::config::{Config, RunnerConfig, SessionMode};
use crate::controller::Pending;
use crate::distribution::RunnerKind;
use crate::health::RunnerStatus;
use crate::history::{HistoryEvent, HistorySummary};
use crate::hold::HoldReason;
use crate::housekeeping::{CleanupPlan, CleanupPolicy, CleanupReport, DiskScan};
use crate::hygiene::{CacheEntry, CacheLimits, EngineReport, PrunePreview, PruneResult, PruneSelection};
use crate::jobs::CurrentJob;
use crate::logs::LogLine;
use crate::metrics::{MachineSample, UsagePoint};
use crate::names::RunnerName;
use crate::notify::Notification;
use crate::process_policy::KeepFor;
use crate::process_track::ProcessEntry;
use crate::runner_config::{ConfigEdit, ConfigView, Preview};
use crate::session::SessionState;
use crate::time::Timestamp;
use crate::update::watch::{Deadline, UpdateLoop};
use crate::update::{UpdateOutcome, UpdatePhase, UpdatePolicy};
use crate::usage::Usage;
use crate::version::RunnerVersion;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// The protocol version this build speaks. 2: push events, maintenance, settings, registration, history, disk.
/// 3: the runner's configuration file (get, preview, save) and the processes a runner started.
/// 4: holds, update deadlines and loops in the snapshot; container engine hygiene and the Actions cache folder.
pub const PROTOCOL_VERSION: u32 = 4;
/// Largest frame either side sends or accepts.
pub const MAX_FRAME: usize = 1024 * 1024;
/// Longest secret (registration token) accepted.
pub const MAX_SECRET_LEN: usize = 256;

/// A secret passed through Pitcrew (a registration token): never printed, never logged, never stored by Pitcrew.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Secret(String);

impl Secret {
    /// A secret from text: 1-256 visible ASCII characters (surrounding white space is dropped).
    pub fn new(s: &str) -> Result<Self, String> {
        let t = s.trim();
        if t.is_empty() || t.len() > MAX_SECRET_LEN || !t.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(format!("the token must be 1-{MAX_SECRET_LEN} characters without spaces"));
        }
        Ok(Secret(t.to_string()))
    }

    /// The secret itself, for the one place that passes it on.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl TryFrom<String> for Secret {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Secret::new(&s)
    }
}

impl From<Secret> for String {
    fn from(s: Secret) -> Self {
        s.0
    }
}

/// A request from the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Request {
    /// Opens the conversation.
    Hello {
        /// The app's protocol version.
        protocol: u32,
    },
    /// Every runner's status.
    Status,
    /// Turns this connection into an event stream: the service answers [`Response::Ok`], then pushes
    /// [`Response::Event`] frames (a status at once and on every change, and notifications) until it closes.
    Subscribe,
    /// Start a runner (clears a crash loop and a pause).
    Start {
        /// The runner.
        runner: RunnerName,
    },
    /// Stop a runner after its current job.
    Stop {
        /// The runner.
        runner: RunnerName,
    },
    /// Restart a runner after its current job.
    Restart {
        /// The runner.
        runner: RunnerName,
    },
    /// Maintenance: finish the current job, take no more, then pause.
    Drain {
        /// The runner.
        runner: RunnerName,
    },
    /// Leave maintenance.
    Resume {
        /// The runner.
        runner: RunnerName,
    },
    /// Log lines after `after`.
    Logs {
        /// The runner.
        runner: RunnerName,
        /// The last sequence number the app has.
        after: Option<u64>,
        /// Most lines to return.
        limit: u32,
    },
    /// Log lines containing `query` (plain text).
    SearchLogs {
        /// The runner.
        runner: RunnerName,
        /// The text.
        query: String,
        /// Most lines to return.
        limit: u32,
    },
    /// Check for a runner update now.
    CheckUpdate {
        /// The runner.
        runner: RunnerName,
    },
    /// Install the available update now (between jobs).
    ApplyUpdate {
        /// The runner.
        runner: RunnerName,
    },
    /// Cancel an update that has not swapped the binary yet.
    CancelUpdate {
        /// The runner.
        runner: RunnerName,
    },
    /// Never offer this version.
    SkipVersion {
        /// The runner.
        runner: RunnerName,
        /// The version.
        version: RunnerVersion,
    },
    /// Stay on (or go to) this version; `None` follows the latest release again.
    PinVersion {
        /// The runner.
        runner: RunnerName,
        /// The version.
        version: Option<RunnerVersion>,
    },
    /// The runner's history over the last `days` local days.
    History {
        /// The runner.
        runner: RunnerName,
        /// How many days (capped by the core).
        days: u32,
        /// The app's time zone, minutes east of UTC (days are local days).
        utc_offset_minutes: i32,
    },
    /// The runner folder's disk use and what a cleanup would delete (a dry run).
    Disk {
        /// The runner.
        runner: RunnerName,
    },
    /// Clean the runner's folder now (between jobs).
    CleanDisk {
        /// The runner.
        runner: RunnerName,
    },
    /// The configuration.
    GetSettings,
    /// Replace the configuration (validated by the service like a file it loads).
    SaveSettings {
        /// The new configuration.
        config: Config,
    },
    /// Add a runner (adopt a registered runner folder, or one to register next).
    AddRunner {
        /// Its configuration.
        runner: RunnerConfig,
    },
    /// Stop a runner (after its job) and forget it; its folder stays.
    RemoveRunner {
        /// The runner.
        runner: RunnerName,
    },
    /// Register a runner with a server: installs the runner binary if needed, runs its `register` command with the
    /// token (passed on, never stored) and starts it.
    Register {
        /// The runner (it must have been added).
        runner: RunnerName,
        /// The server, e.g. `https://git.example.com`.
        server_url: String,
        /// The registration token from the server's runner settings.
        token: Secret,
        /// The name the server shows (default: the runner's name).
        display_name: Option<String>,
    },
    /// Redacted diagnostics for a support bundle.
    Diagnostics,
    /// Machine metrics (and a runner's folder size, CPU and memory) over the last `range_hours`.
    Metrics {
        /// The runner whose folder size to include.
        runner: Option<RunnerName>,
        /// How far back (capped at 90 days).
        range_hours: u32,
    },
    /// The runner's `config.yaml`: its text, the form's view of it and the findings (ADR 0011).
    GetRunnerConfig {
        /// The runner.
        runner: RunnerName,
    },
    /// What saving a change to the runner's `config.yaml` would do: the new text, a diff and the findings. Writes
    /// nothing.
    PreviewRunnerConfig {
        /// The runner.
        runner: RunnerName,
        /// The change.
        edit: ConfigEdit,
    },
    /// Saves a change to the runner's `config.yaml` (after a backup) and, when the runner must restart to use it,
    /// restarts it after the current job.
    SaveRunnerConfig {
        /// The runner.
        runner: RunnerName,
        /// The change.
        edit: ConfigEdit,
        /// The fingerprint of the file the change was made against (from [`Response::RunnerConfig`]); the save is
        /// refused when the file has changed since.
        base: String,
    },
    /// The processes the runner started (its process tree), with their jobs and the leftovers (ADR 0012).
    Processes {
        /// The runner.
        runner: RunnerName,
    },
    /// Asks one process of the runner's tree to stop (then ends it if it ignores the request).
    StopProcess {
        /// The runner.
        runner: RunnerName,
        /// The process.
        pid: u32,
        /// When the app saw it start (guards against a reused process id).
        started_at: Option<Timestamp>,
        /// The runner process itself may be stopped (the user chose it explicitly).
        allow_runner: bool,
    },
    /// Keeps a leftover longer: adds time before the automatic cleanup stops it.
    KeepProcess {
        /// The runner.
        runner: RunnerName,
        /// The process.
        pid: u32,
        /// When the app saw it start.
        started_at: Timestamp,
        /// How much to add.
        extra: KeepFor,
    },
    /// The container engine's disk use and the act cache folders (a query; ADR 0016).
    EngineReport,
    /// What pruning the container engine would remove (a dry run; a query).
    EnginePrunePreview {
        /// What to prune.
        selection: PruneSelection,
    },
    /// Prunes the container engine. Refused while a job runs.
    EnginePrune {
        /// What to prune.
        selection: PruneSelection,
    },
    /// The runner's Actions cache folder: its size, entries and limit (a query).
    CacheReport {
        /// The runner.
        runner: RunnerName,
    },
    /// What trimming the runner's cache to `limits` would delete (a dry run; a query).
    CachePreview {
        /// The runner.
        runner: RunnerName,
        /// The limits.
        limits: CacheLimits,
    },
    /// Trims the runner's cache to `limits`. Refused while a job runs.
    CacheClean {
        /// The runner.
        runner: RunnerName,
        /// The limits.
        limits: CacheLimits,
    },
}

impl Request {
    /// Whether the request changes anything (status, logs, history, disk and settings queries do not).
    pub fn mutates(&self) -> bool {
        !matches!(
            self,
            Request::Hello { .. }
                | Request::Status
                | Request::Subscribe
                | Request::Logs { .. }
                | Request::SearchLogs { .. }
                | Request::History { .. }
                | Request::Disk { .. }
                | Request::GetSettings
                | Request::Diagnostics
                | Request::Metrics { .. }
                | Request::GetRunnerConfig { .. }
                | Request::PreviewRunnerConfig { .. }
                | Request::Processes { .. }
                | Request::EngineReport
                | Request::EnginePrunePreview { .. }
                | Request::CacheReport { .. }
                | Request::CachePreview { .. }
        )
    }

    /// Whether the service accepts the request only from an elevated administrator (Windows, where the service runs as
    /// LocalSystem and its pipe is open to every interactive user; rule 77): a request that changes **what runs or where**,
    /// that is, the configuration the service later acts on (runners, their programs, folders, sessions and environment, the
    /// runner's own `config.yaml`, a registration).
    ///
    /// Operational requests on an existing runner (start, stop, restart, drain, resume, updates of the managed binary from
    /// its checked release source, disk and process housekeeping) and every read stay open to the pipe's users, like `sc stop`
    /// for a service a user may control. The match names every request, so a new one cannot be added without deciding.
    pub fn requires_admin(&self) -> bool {
        match self {
            Request::SaveSettings { .. }
            | Request::AddRunner { .. }
            | Request::RemoveRunner { .. }
            | Request::Register { .. }
            | Request::SaveRunnerConfig { .. } => true,
            Request::Hello { .. }
            | Request::Status
            | Request::Subscribe
            | Request::Start { .. }
            | Request::Stop { .. }
            | Request::Restart { .. }
            | Request::Drain { .. }
            | Request::Resume { .. }
            | Request::Logs { .. }
            | Request::SearchLogs { .. }
            | Request::CheckUpdate { .. }
            | Request::ApplyUpdate { .. }
            | Request::CancelUpdate { .. }
            | Request::SkipVersion { .. }
            | Request::PinVersion { .. }
            | Request::History { .. }
            | Request::Disk { .. }
            | Request::CleanDisk { .. }
            | Request::GetSettings
            | Request::Diagnostics
            | Request::Metrics { .. }
            | Request::GetRunnerConfig { .. }
            | Request::PreviewRunnerConfig { .. }
            | Request::Processes { .. }
            | Request::StopProcess { .. }
            | Request::KeepProcess { .. }
            | Request::EngineReport
            | Request::EnginePrunePreview { .. }
            | Request::EnginePrune { .. }
            | Request::CacheReport { .. }
            | Request::CachePreview { .. }
            | Request::CacheClean { .. } => false,
        }
    }
}

/// What the app shows about one runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerSnapshot {
    /// The runner.
    pub name: RunnerName,
    /// Which runner it is.
    pub kind: RunnerKind,
    /// Its status.
    pub status: RunnerStatus,
    /// The user wants it running (not stopped or paused).
    pub enabled: bool,
    /// What happens after the current job (stop, pause, restart).
    pub pending: Option<Pending>,
    /// Where it runs.
    pub session: SessionState,
    /// Since when the current process runs.
    pub running_since: Option<Timestamp>,
    /// When it starts again (backoff or crash-loop cool-down).
    pub next_start_at: Option<Timestamp>,
    /// Unplanned exits and stall restarts in the last 24 hours.
    pub restarts_24h: u32,
    /// The last problem worth showing (a failed start), for the user.
    pub problem: Option<String>,
    /// The job running now.
    pub current_job: Option<CurrentJob>,
    /// The installed version, when known.
    pub current_version: Option<RunnerVersion>,
    /// The latest version found by the last check.
    pub latest_version: Option<RunnerVersion>,
    /// When the last check ran.
    pub checked_at: Option<Timestamp>,
    /// The version the policy would install now or in its window.
    pub update_target: Option<RunnerVersion>,
    /// How the last update ended.
    pub last_update: Option<UpdateRecord>,
    /// The update in progress.
    pub update_phase: UpdatePhase,
    /// The update policy in effect.
    pub update_policy: UpdatePolicy,
    /// Pitcrew may update its binary (false for a user-provided binary).
    pub managed_binary: bool,
    /// Its folder.
    pub dir: String,
    /// Its labels.
    pub labels: Vec<String>,
    /// Its server, when known.
    pub server_url: Option<String>,
    /// Where it runs on Windows.
    pub session_mode: SessionMode,
    /// The size of its folder and the free space of its disk, from the last scan.
    pub disk: Option<DiskUse>,
    /// When the next automatic cleanup is due (`None`: automatic cleanup is off).
    pub next_cleanup_at: Option<Timestamp>,
    /// What the runner and the processes it started use now (`None`: it is not running).
    #[serde(default)]
    pub usage: Option<Usage>,
    /// How many processes a finished job left behind are still alive.
    #[serde(default)]
    pub leftovers: u32,
    /// Why Pitcrew itself holds the runner now (low disk, a pause window, a busy machine).
    #[serde(default)]
    pub hold: Option<HoldInfo>,
    /// The countdown to the owner's update deadline (only while behind the latest release).
    #[serde(default)]
    pub deadline: Option<Deadline>,
    /// Updates going round in circles, with the likely cause.
    #[serde(default)]
    pub update_loop: Option<UpdateLoop>,
}

/// A hold Pitcrew applied or is applying (see [`crate::hold`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldInfo {
    /// The reasons in force, the first is the headline.
    pub reasons: Vec<HoldReason>,
    /// The runner takes no new jobs now (false while the current job finishes).
    pub paused: bool,
    /// Minutes until the pause window closes (only for a window).
    pub minutes_left: Option<u32>,
}

/// Disk use from the last scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUse {
    /// The runner folder's size.
    pub folder_bytes: u64,
    /// Free space on its disk.
    pub free_bytes: Option<u64>,
    /// Below the free-space minimum.
    pub low: bool,
}

/// A finished update, as kept by the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecord {
    /// How it ended.
    pub outcome: UpdateOutcome,
    /// When it ended.
    pub finished_at: Timestamp,
}

/// About the service itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    /// Its version.
    pub version: String,
    /// When it started.
    pub started_at: Timestamp,
    /// Its data folder.
    pub data_dir: String,
    /// Problems with the configuration, for the user.
    pub config_problems: Vec<String>,
    /// The configuration is from a newer Pitcrew and cannot be saved.
    pub read_only: bool,
    /// The machine now.
    pub machine: Option<MachineSample>,
    /// The service's own CPU use, per mille of one machine.
    pub own_cpu_permille: u32,
    /// The service's own memory use.
    pub own_mem_bytes: u64,
}

/// Something the service pushes to subscribers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServiceEvent {
    /// Every runner's status (sent at once and whenever it changes).
    Status {
        /// The runners.
        runners: Vec<RunnerSnapshot>,
        /// The service.
        service: ServiceInfo,
    },
    /// Tell the user.
    Notification {
        /// What.
        notification: Notification,
    },
}

/// A named part of the diagnostics (already redacted by the service).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSection {
    /// A file name for it in the bundle, e.g. `config.toml`.
    pub name: String,
    /// Its text.
    pub text: String,
}

/// What happens to a runner after its configuration file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RestartPlan {
    /// The change does not affect the runner (comments or layout only).
    NotNeeded,
    /// The runner is not running; its next start uses the file.
    NotRunning,
    /// The runner restarts after its current job (at once when it is idle).
    AfterJob,
}

/// The processes of a runner's tree (a query; see [`Request::Processes`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessList {
    /// The runner process, when it runs.
    pub runner_pid: Option<u32>,
    /// No job runs now (leftovers may be stopped).
    pub idle: bool,
    /// Automatic cleanup of leftovers is on for this runner.
    pub cleanup: bool,
    /// The default time a leftover is kept after its job ended.
    pub keep: KeepFor,
    /// The rows: leftovers first, then the runner, then by memory.
    pub entries: Vec<ProcessEntry>,
    /// Rows were left out (the list is bounded).
    pub truncated: bool,
    /// When the process table was read.
    pub taken_at: Timestamp,
}

/// A runner's Actions cache folder (the runner's own cache server stores its data there).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    /// The runner's `cache.enabled` (`None`: not set in its configuration, so the runner's default, on).
    pub enabled: Option<bool>,
    /// The folder.
    pub dir: String,
    /// The folder is the runner's default (`$HOME/.cache/actcache`), not one set in its configuration.
    pub default_dir: bool,
    /// The folder exists.
    pub exists: bool,
    /// Its size (a lower bound when `truncated`).
    pub bytes: u64,
    /// Entries directly inside it.
    pub entries: u32,
    /// The biggest entries, biggest first (at most 50).
    pub top: Vec<CacheEntry>,
    /// The runner's own `cache.size_limit`, in MB (`None`: not set).
    pub size_limit_mb: Option<u64>,
    /// The cache is bigger than its limit.
    pub over_limit: bool,
    /// Free space on its disk.
    pub free_bytes: Option<u64>,
    /// Measuring stopped at a limit.
    pub truncated: bool,
    /// Why Pitcrew cannot clean it (a folder that is too broad), with a next step.
    pub problem: Option<String>,
}

/// The entries a cache trim deletes (or would).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePlan {
    /// The entries and why.
    pub entries: Vec<(CacheEntry, crate::hygiene::CacheReason)>,
    /// Their total size.
    pub bytes: u64,
}

/// A response from the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Response {
    /// The service's answer to [`Request::Hello`].
    Hello {
        /// The service's protocol version.
        protocol: u32,
        /// The service's version.
        service_version: String,
    },
    /// Every runner's status.
    Status {
        /// The runners.
        runners: Vec<RunnerSnapshot>,
        /// The service.
        service: ServiceInfo,
    },
    /// Log lines.
    Logs {
        /// The lines, oldest first.
        lines: Vec<LogLine>,
    },
    /// A history summary.
    History {
        /// The summary.
        summary: HistorySummary,
        /// The newest events, newest first (at most 200).
        events: Vec<HistoryEvent>,
    },
    /// Disk use and the cleanup preview.
    Disk {
        /// The scan.
        scan: DiskScan,
        /// What a cleanup would delete now.
        preview: CleanupPlan,
        /// The last cleanup.
        last_cleanup: Option<CleanupReport>,
        /// The policy in effect.
        policy: CleanupPolicy,
    },
    /// A cleanup ran.
    Cleaned {
        /// How it went.
        report: CleanupReport,
    },
    /// The configuration.
    Settings {
        /// It.
        config: Config,
        /// Problems with it, for the user.
        problems: Vec<String>,
        /// It cannot be saved (written by a newer Pitcrew).
        read_only: bool,
    },
    /// Redacted diagnostics.
    Diagnostics {
        /// The sections.
        sections: Vec<DiagnosticsSection>,
    },
    /// Machine metrics.
    Metrics {
        /// The machine's points, oldest first.
        machine: Vec<MachineSample>,
        /// The runner folder's size, `[time, bytes]`, oldest first.
        folder: Vec<(Timestamp, u64)>,
        /// The runner's own CPU and memory (with its processes), oldest first; empty without a runner.
        #[serde(default)]
        usage: Vec<UsagePoint>,
    },
    /// The runner's configuration file.
    RunnerConfig {
        /// Its path.
        path: String,
        /// Its text (empty when the file does not exist yet).
        text: String,
        /// Fingerprint of the text, to send back with the save.
        fingerprint: String,
        /// The form's view of it.
        view: ConfigView,
        /// How many backups exist.
        backups: u32,
        /// A change is waiting for the runner to restart after its job.
        restart_pending: bool,
    },
    /// What saving a change would do.
    RunnerConfigPreview {
        /// The preview.
        preview: Preview,
    },
    /// A change was saved.
    RunnerConfigSaved {
        /// The backup of the previous file; `None` when there was no file.
        backup: Option<String>,
        /// What happens to the runner.
        restart: RestartPlan,
        /// Fingerprint of the file now.
        fingerprint: String,
        /// The keys that changed, dotted.
        changed_keys: Vec<String>,
    },
    /// The runner's processes.
    Processes {
        /// The list.
        list: ProcessList,
    },
    /// The container engine's use and the act cache folders.
    EngineReport {
        /// The report.
        report: EngineReport,
    },
    /// What a container engine prune would remove.
    EnginePreview {
        /// The preview.
        preview: PrunePreview,
    },
    /// A container engine prune ran.
    EnginePruned {
        /// How it went.
        result: PruneResult,
    },
    /// The runner's Actions cache folder.
    CacheReport {
        /// The folder.
        cache: CacheInfo,
    },
    /// What trimming the cache would delete.
    CachePreview {
        /// The entries that would go, with the reason.
        plan: CachePlan,
    },
    /// The cache was trimmed.
    CacheCleaned {
        /// The result.
        plan: CachePlan,
        /// Entries that could not be deleted, with the reason.
        failures: Vec<(String, String)>,
    },
    /// A pushed event (only on a subscribed connection).
    Event {
        /// The event.
        event: ServiceEvent,
    },
    /// Done.
    Ok,
    /// Refused or failed.
    Error(ServiceError),
}

/// Why the service refused or failed a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The feature is not built (on this platform).
    NotImplemented,
    /// The app and the service speak different protocol versions.
    ProtocolMismatch,
    /// No runner has this name.
    UnknownRunner,
    /// The request is malformed or its values are not allowed.
    BadRequest,
    /// The caller may not do this.
    PermissionDenied,
    /// Something else is in progress (e.g. an update).
    Busy,
    /// An internal error; the service log has the details.
    Internal,
}

/// An error with a message for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct ServiceError {
    /// What kind of error.
    pub code: ErrorCode,
    /// What happened and what to do, for the user.
    pub message: String,
}

impl ServiceError {
    /// A "not implemented" error for `what`.
    pub fn not_implemented(what: &str) -> Self {
        ServiceError::new(ErrorCode::NotImplemented, format!("{what} is not available on this platform yet"))
    }

    /// An error with a code and a message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ServiceError {
            code,
            message: message.into(),
        }
    }
}

/// Problems reading or writing frames.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The stream failed or closed.
    #[error("the connection failed: {0}")]
    Io(#[from] std::io::Error),
    /// The frame is larger than [`MAX_FRAME`].
    #[error("a message of {0} bytes is larger than the limit of {MAX_FRAME} bytes")]
    TooLarge(usize),
    /// The frame is not a valid message.
    #[error("the message is not valid: {0}")]
    Malformed(String),
}

/// One message as a frame (length prefix and JSON body), for async transports.
pub fn encode_frame<T: Serialize>(message: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(message).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if body.len() > MAX_FRAME {
        return Err(FrameError::TooLarge(body.len()));
    }
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(body.len()))?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// The body length a frame header announces, refused beyond [`MAX_FRAME`].
pub fn frame_len(header: [u8; 4]) -> Result<usize, FrameError> {
    let len = usize::try_from(u32::from_le_bytes(header)).unwrap_or(usize::MAX);
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    Ok(len)
}

/// Parses a frame body.
pub fn decode_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(body).map_err(|e| FrameError::Malformed(e.to_string()))
}

/// Writes one message as a frame.
pub fn write_frame<T: Serialize>(w: &mut impl Write, message: &T) -> Result<(), FrameError> {
    w.write_all(&encode_frame(message)?)?;
    w.flush()?;
    Ok(())
}

/// Reads one frame and parses it as a message.
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> Result<T, FrameError> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header)?;
    let len = frame_len(header)?;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    decode_body(&body)
}

/// The service's answer to a hello.
pub fn answer_hello(app_protocol: u32, service_version: &str) -> Response {
    if app_protocol == PROTOCOL_VERSION {
        Response::Hello {
            protocol: PROTOCOL_VERSION,
            service_version: service_version.to_string(),
        }
    } else {
        Response::Error(ServiceError {
            code: ErrorCode::ProtocolMismatch,
            message: format!(
                "the Pitcrew app speaks protocol {app_protocol} but the service speaks {PROTOCOL_VERSION}; install the same Pitcrew version for both"
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn name(s: &str) -> RunnerName {
        RunnerName::parse(s).unwrap()
    }

    #[test]
    fn frames_round_trip() {
        let requests = vec![
            Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
            Request::Status,
            Request::Subscribe,
            Request::Logs {
                runner: name("a"),
                after: Some(3),
                limit: 100,
            },
            Request::SearchLogs {
                runner: name("a"),
                query: "timed out".into(),
                limit: 10,
            },
            Request::History {
                runner: name("a"),
                days: 7,
                utc_offset_minutes: 480,
            },
        ];
        let mut buf = vec![];
        for r in &requests {
            write_frame(&mut buf, r).unwrap();
        }
        let mut cur = Cursor::new(buf);
        for r in &requests {
            assert_eq!(&read_frame::<Request>(&mut cur).unwrap(), r);
        }
        assert!(matches!(read_frame::<Request>(&mut cur), Err(FrameError::Io(_))), "end of stream");
    }

    #[test]
    fn the_wire_format_is_camel_case_json_with_a_type_tag() {
        let json = serde_json::to_string(&Request::CheckUpdate { runner: name("build-pc") }).unwrap();
        assert_eq!(json, r#"{"type":"checkUpdate","runner":"build-pc"}"#);
        let json = serde_json::to_string(&Request::History {
            runner: name("a"),
            days: 7,
            utc_offset_minutes: -60,
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"history","runner":"a","days":7,"utcOffsetMinutes":-60}"#);
        let err = Response::Error(ServiceError::new(ErrorCode::Busy, "later"));
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"type":"error","code":"busy","message":"later"}"#
        );
        assert!(ServiceError::not_implemented("Registration").message.contains("Registration"));
    }

    #[test]
    fn oversized_and_malformed_frames_are_refused() {
        let mut huge = (u32::try_from(MAX_FRAME).unwrap() + 1).to_le_bytes().to_vec();
        huge.extend([0u8; 16]);
        assert!(matches!(
            read_frame::<Request>(&mut Cursor::new(huge)),
            Err(FrameError::TooLarge(_))
        ));
        let mut junk = 3u32.to_le_bytes().to_vec();
        junk.extend(b"{x}");
        assert!(matches!(
            read_frame::<Request>(&mut Cursor::new(junk)),
            Err(FrameError::Malformed(_))
        ));
        let bad_name = br#"{"type":"start","runner":"../etc"}"#;
        let mut frame = u32::try_from(bad_name.len()).unwrap().to_le_bytes().to_vec();
        frame.extend(bad_name);
        assert!(
            matches!(read_frame::<Request>(&mut Cursor::new(frame)), Err(FrameError::Malformed(_))),
            "names are validated on the way in"
        );
        let too_big = Request::SearchLogs {
            runner: name("a"),
            query: "x".repeat(MAX_FRAME),
            limit: 1,
        };
        assert!(matches!(write_frame(&mut vec![], &too_big), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn secrets_never_show_and_are_validated() {
        let s = Secret::new("  abc123  ").unwrap();
        assert_eq!(s.expose(), "abc123");
        assert_eq!(format!("{s:?}"), "Secret(..)");
        let r = Request::Register {
            runner: name("a"),
            server_url: "https://g".into(),
            token: s,
            display_name: None,
        };
        assert!(!format!("{r:?}").contains("abc123"));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""serverUrl":"https://g""#), "{json}");
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), r);
        for bad in ["", "a b", &"x".repeat(MAX_SECRET_LEN + 1)] {
            assert!(Secret::new(bad).is_err());
        }
        assert!(serde_json::from_str::<Secret>("\"\"").is_err());
        assert_eq!(String::from(Secret::new("k").unwrap()), "k");
    }

    #[test]
    fn queries_do_not_mutate() {
        for q in [
            Request::Status,
            Request::Subscribe,
            Request::Diagnostics,
            Request::GetSettings,
            Request::GetRunnerConfig { runner: name("a") },
            Request::PreviewRunnerConfig {
                runner: name("a"),
                edit: ConfigEdit::Raw { text: String::new() },
            },
            Request::Processes { runner: name("a") },
        ] {
            assert!(!q.mutates(), "{q:?}");
        }
        for m in [
            Request::SaveRunnerConfig {
                runner: name("a"),
                edit: ConfigEdit::Raw { text: String::new() },
                base: String::new(),
            },
            Request::StopProcess {
                runner: name("a"),
                pid: 5,
                started_at: None,
                allow_runner: false,
            },
            Request::KeepProcess {
                runner: name("a"),
                pid: 5,
                started_at: Timestamp(1),
                extra: KeepFor::hours(1),
            },
        ] {
            assert!(m.mutates(), "{m:?}");
        }
        assert!(Request::Drain { runner: name("a") }.mutates());
        assert!(Request::CleanDisk { runner: name("a") }.mutates());
        assert!(!Request::Disk { runner: name("a") }.mutates());
    }

    #[test]
    fn only_requests_that_change_what_runs_or_where_need_an_administrator() {
        let runner: crate::config::RunnerConfig =
            toml::from_str("name = \"a\"\nkind = \"gitea-runner\"\ndir = '/r'\nsession = \"service\"\n").unwrap();
        for r in [
            Request::SaveSettings {
                config: crate::config::Config::default(),
            },
            Request::AddRunner { runner },
            Request::RemoveRunner { runner: name("a") },
            Request::Register {
                runner: name("a"),
                server_url: "https://git.example.com".into(),
                token: Secret::new("t").unwrap(),
                display_name: None,
            },
            Request::SaveRunnerConfig {
                runner: name("a"),
                edit: ConfigEdit::Raw { text: String::new() },
                base: String::new(),
            },
        ] {
            assert!(r.requires_admin(), "{r:?}");
        }
        for r in [
            Request::Start { runner: name("a") },
            Request::Stop { runner: name("a") },
            Request::Restart { runner: name("a") },
            Request::Drain { runner: name("a") },
            Request::Resume { runner: name("a") },
            Request::CleanDisk { runner: name("a") },
            Request::Status,
            Request::GetSettings,
            Request::Diagnostics,
        ] {
            assert!(!r.requires_admin(), "{r:?}");
        }
        // A request that needs an administrator always changes something.
        assert!(Request::Drain { runner: name("a") }.mutates() && !Request::Drain { runner: name("a") }.requires_admin());
    }

    #[test]
    fn the_config_and_process_requests_use_the_documented_wire_format() {
        let r = Request::SaveRunnerConfig {
            runner: name("a"),
            edit: ConfigEdit::Raw {
                text: "log:\n  level: info\n".into(),
            },
            base: "abc".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"type":"saveRunnerConfig","runner":"a","edit":{"mode":"raw","text":"log:\n  level: info\n"},"base":"abc"}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), r);
        assert!(!format!("{r:?}").contains("level"), "the file's text is never in a debug print");
        let r = Request::StopProcess {
            runner: name("a"),
            pid: 7,
            started_at: Some(Timestamp(5)),
            allow_runner: false,
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"type":"stopProcess","runner":"a","pid":7,"startedAt":5,"allowRunner":false}"#
        );
        let r = Request::KeepProcess {
            runner: name("a"),
            pid: 7,
            started_at: Timestamp(5),
            extra: KeepFor::hours(2),
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"type":"keepProcess","runner":"a","pid":7,"startedAt":5,"extra":{"amount":2,"unit":"hours"}}"#
        );
        assert_eq!(serde_json::to_string(&RestartPlan::AfterJob).unwrap(), r#""afterJob""#);
    }

    #[test]
    fn async_framing_helpers_agree_with_the_stream_ones() {
        let f = encode_frame(&Request::Status).unwrap();
        let len = frame_len([f[0], f[1], f[2], f[3]]).unwrap();
        assert_eq!(len, f.len() - 4);
        assert_eq!(decode_body::<Request>(&f[4..]).unwrap(), Request::Status);
        assert!(frame_len(u32::MAX.to_le_bytes()).is_err());
        assert!(decode_body::<Request>(b"{").is_err());
    }

    #[test]
    fn hello_checks_the_protocol_version() {
        assert_eq!(
            answer_hello(PROTOCOL_VERSION, "0.1.0"),
            Response::Hello {
                protocol: PROTOCOL_VERSION,
                service_version: "0.1.0".into()
            }
        );
        assert!(matches!(
            answer_hello(99, "0.1.0"),
            Response::Error(ServiceError {
                code: ErrorCode::ProtocolMismatch,
                ..
            })
        ));
    }
}
