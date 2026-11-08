//! The management contract: what the server's `/pw/v1/...` routes return and the app forwards to the UI. It mirrors
//! `app/src/app/core/api.types.ts` (camelCase JSON); change both in the same commit. Types that belong to a domain
//! module are defined there and re-exported here.

pub use crate::config::UiSettings as Settings;
pub use crate::events::{Event as PitwallEvent, EventKind, Severity};
pub use crate::fit::{Fit, Offload, Verdict};
pub use crate::hf::HfModel;
pub use crate::limits::{AppLimits, AppPermissions};
pub use crate::limits::{LimitKind, LimitState, LimitStatus};
pub use crate::memory::{Preset, ReleaseReason};
pub use crate::metrics::{HistoryPoint, KeyedSeries, Range as HistoryRange};
pub use crate::presets::ModelPreset;
pub use crate::scheduler::{Depth, JobKind, Priority};
pub use crate::select::Capability;
pub use crate::update_state::{UpdateCheckSettings, UpdateSchedule};
pub use crate::updates::{UpdateKind, UpdateNotice};
pub use crate::usage::{AppUsageSeries, Counters as UsageCounters, UsageRange, UsageSeries};

use crate::time::Millis;
use serde::{Deserialize, Serialize};

/// A typed error for the UI (`CommandError`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct CommandError {
    /// The kind.
    pub code: ErrorCode,
    /// What to do.
    pub message: String,
}

impl CommandError {
    /// An error.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> CommandError {
        CommandError {
            code,
            message: message.into(),
        }
    }
}

/// Error kinds (`ErrorCode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The server is not running.
    ServerUnavailable,
    /// The app's admin key is missing or wrong.
    Unauthorized,
    /// Not allowed.
    Forbidden,
    /// No such thing.
    NotFound,
    /// Invalid input.
    BadRequest,
    /// The state doesn't allow it now.
    Conflict,
    /// Not enough memory.
    InsufficientMemory,
    /// Busy.
    Busy,
    /// Not available on this machine.
    NotAvailable,
    /// An outside host (Hugging Face, GitHub) could not be reached, refused, or sent bad data; try again later.
    Upstream,
    /// Not built yet.
    NotImplemented,
    /// A bug.
    Internal,
}

impl ErrorCode {
    /// The HTTP status the server answers with.
    pub fn status(self) -> u16 {
        match self {
            ErrorCode::ServerUnavailable => 503,
            ErrorCode::Unauthorized => 401,
            ErrorCode::Forbidden => 403,
            ErrorCode::NotFound => 404,
            ErrorCode::BadRequest => 400,
            ErrorCode::Conflict => 409,
            ErrorCode::InsufficientMemory | ErrorCode::Busy => 503,
            ErrorCode::NotAvailable | ErrorCode::NotImplemented => 501,
            ErrorCode::Upstream => 502,
            ErrorCode::Internal => 500,
        }
    }
}

/// Serving, paused or stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServingState {
    /// Requests are served.
    Serving,
    /// New requests are refused.
    Paused,
    /// Pitwall was quit with "stop serving": no model is loaded and new requests are refused until Pitwall is opened
    /// again or serving is resumed.
    Stopped,
}

/// LAN exposure (`LanStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanStatus {
    /// On.
    pub enabled: bool,
    /// `https://<address>:<port>`.
    pub url: Option<String>,
    /// The certificate's SHA-256 for pinning.
    pub cert_fingerprint: Option<String>,
}

/// The server (`ServerInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// Pitwall's version.
    pub version: String,
    /// The client contract version.
    pub api_version: u32,
    /// The process id.
    pub pid: u32,
    /// The loopback port.
    pub port: u16,
    /// When it started.
    pub started_at: Millis,
    /// Serving or paused.
    pub serving: ServingState,
    /// LAN exposure.
    pub lan: LanStatus,
}

/// Memory pressure (`MemoryPressure`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryPressure {
    /// Fine.
    Normal,
    /// Below the threshold or the OS warns.
    Elevated,
    /// The OS reports critical pressure.
    Critical,
}

/// A GPU reading (`GpuSample`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuSample {
    /// Its name.
    pub name: String,
    /// Total video memory.
    pub vram_total: Option<u64>,
    /// Used video memory.
    pub vram_used: Option<u64>,
    /// Utilisation 0–100.
    pub utilisation: Option<f32>,
    /// Shares system memory.
    pub unified: bool,
}

/// A memory reading (`MemorySample`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    /// When.
    pub at: Millis,
    /// Physical memory.
    pub ram_total: u64,
    /// Available physical memory.
    pub ram_available: u64,
    /// Windows commit charge.
    pub commit_used: Option<u64>,
    /// Windows commit limit.
    pub commit_limit: Option<u64>,
    /// Swap in use.
    pub swap_used: u64,
    /// Pressure.
    pub pressure: MemoryPressure,
    /// CPU 0–100.
    pub cpu_percent: f32,
    /// GPUs.
    pub gpus: Vec<GpuSample>,
    /// Pitwall plus its engines.
    pub pitwall_rss: u64,
    /// A foreground full-screen app is running.
    pub foreground_busy: bool,
}

/// A hot model's state (`HotState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HotState {
    /// Loading.
    Loading,
    /// Ready and idle.
    Ready,
    /// Serving.
    Busy,
    /// Releasing its context.
    ReleasingContext,
    /// Unloading.
    Unloading,
    /// Failed.
    Failed,
}

/// Engine kinds (`EngineKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineKind {
    /// llama.cpp's llama-server.
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
}

/// A loaded model (`LoadedModel`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModel {
    /// The model.
    pub model_id: String,
    /// Its display name.
    pub name: String,
    /// The engine.
    pub engine: EngineKind,
    /// The build (`vulkan`, …).
    pub build: String,
    /// State.
    pub state: HotState,
    /// Resident memory.
    pub ram_bytes: u64,
    /// Video memory.
    pub vram_bytes: u64,
    /// Its context per slot.
    pub context_tokens: u32,
    /// Parallel slots it was loaded with.
    #[serde(default = "one")]
    pub slots: u32,
    /// Keep loaded.
    pub pinned: bool,
    /// Running requests.
    pub active_requests: u32,
    /// When it loaded.
    pub loaded_at: Millis,
    /// Last used.
    pub last_used_at: Millis,
    /// When the idle rule releases it.
    pub idle_release_at: Option<Millis>,
    /// How long the load took.
    pub cold_start_ms: u64,
    /// What it can do.
    pub capabilities: Vec<Capability>,
}

/// Engine availability (`Availability`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Availability {
    /// Usable.
    Available,
    /// Needs a download.
    NotInstalled {
        /// What to do.
        reason: String,
    },
    /// Not on this platform or not built yet.
    NotAvailable {
        /// Why.
        reason: String,
    },
    /// Installed but broken.
    Failed {
        /// Why.
        reason: String,
    },
}

/// Process health (`ProcessHealth`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessHealth {
    /// Starting.
    Starting,
    /// Answering health checks.
    Healthy,
    /// Not answering.
    Unhealthy,
    /// Waiting to restart.
    Restarting,
    /// Stopped.
    Stopped,
}

/// An engine process (`EngineProcess`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProcess {
    /// Process id.
    pub pid: u32,
    /// The model it serves.
    pub model_id: String,
    /// Health.
    pub health: ProcessHealth,
    /// Restarts so far.
    pub restarts: u32,
    /// Resident memory.
    pub rss_bytes: u64,
    /// When it started.
    pub started_at: Millis,
}

/// Self-test result (`SelfTest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelfTest {
    /// Passed.
    Passed,
    /// Failed.
    Failed,
    /// Not run.
    NotRun,
}

/// An installed build (`InstalledBuild`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledBuild {
    /// Build id.
    pub id: String,
    /// Release tag.
    pub tag: String,
    /// Label.
    pub label: String,
    /// Bytes on disk.
    pub size_bytes: u64,
    /// Self-test.
    pub self_test: SelfTest,
    /// Self-test message.
    pub self_test_message: Option<String>,
    /// Devices it reported.
    pub devices: Vec<String>,
    /// In use.
    pub active: bool,
    /// Of an older pin.
    pub outdated: bool,
}

/// A downloadable build (`CatalogBuild`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogBuild {
    /// Build id.
    pub id: String,
    /// Label.
    pub label: String,
    /// Release tag.
    pub tag: String,
    /// Bytes to download.
    pub download_bytes: u64,
    /// Suggested for this machine.
    pub suggested: bool,
    /// Installed already.
    pub installed: bool,
}

/// An engine (`Engine`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    /// Kind.
    pub kind: EngineKind,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Availability.
    pub availability: Availability,
    /// `auto` or a build id.
    pub build_choice: String,
    /// Installed builds.
    pub builds: Vec<InstalledBuild>,
    /// The pinned catalog.
    pub catalog: Vec<CatalogBuild>,
    /// Processes.
    pub processes: Vec<EngineProcess>,
    /// The build a roll back returns to.
    pub rollback_to: Option<String>,
    /// Files of features Pitwall no longer has (an older install's llamafile), offered for removal, never deleted by
    /// themselves.
    pub leftovers: Vec<Leftover>,
}

/// Something an older version left on disk that nothing uses any more (`Leftover`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leftover {
    /// What to pass to `remove_leftover` (`pack-llamafile`).
    pub id: String,
    /// Its name (`llamafile`).
    pub label: String,
    /// Size on disk.
    pub size_bytes: u64,
}

/// One model's engine pin (`ModelEnginePin`, ADR 0018).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEnginePin {
    /// The model.
    pub model_id: String,
    /// The pinned build id.
    pub build_id: String,
    /// Its release tag, `None` when it is not installed any more.
    pub tag: Option<String>,
}

/// The build a one-click roll back goes to (`RollbackTarget`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackTarget {
    /// The build id.
    pub build_id: String,
    /// Its release tag.
    pub tag: String,
}

/// The engine pin state (`EnginePinState`, ADR 0018).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginePinState {
    /// The build pinned for all models.
    pub global: Option<String>,
    /// Per-model pins.
    pub models: Vec<ModelEnginePin>,
    /// The build that serves models in general now, and why in words (`pinned for all models`, `chosen automatically`).
    pub active: Option<String>,
    /// Why `active` was chosen.
    pub active_source: String,
    /// Where roll back goes, when it can.
    pub rollback_to: Option<RollbackTarget>,
    /// Why roll back is not possible (shown instead of the button), when it is not.
    pub rollback_blocked: Option<String>,
    /// Pins that cannot be used now (build uninstalled or failed its self-test), each with what to do.
    pub problems: Vec<String>,
}

/// The result of changing a pin or rolling back (`PinChange`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinChange {
    /// The state after the change.
    pub state: EnginePinState,
    /// Loaded models that keep running on their current build: the change applies when they next load.
    pub applies_on_next_load: Vec<String>,
    /// One sentence for the toast.
    pub message: String,
}

/// Whether a build supports a capability (`CapabilityStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityStatus {
    /// Supported.
    Supported,
    /// Needs a newer llama.cpp build.
    NeedsBuild,
    /// This kind of build cannot.
    NotSupported,
}

/// One capability of a build (`BuildCapability`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildCapability {
    /// The capability.
    pub capability: Capability,
    /// Plain words.
    pub label: String,
    /// The verdict.
    pub status: CapabilityStatus,
    /// The release number that would serve it (`NeedsBuild`).
    pub min_build: Option<u64>,
    /// What to do, or why not.
    pub hint: Option<String>,
}

/// What an installed build can do (`BuildCapabilities`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildCapabilities {
    /// The build id.
    pub build_id: String,
    /// Its release tag.
    pub tag: String,
    /// One entry per capability.
    pub capabilities: Vec<BuildCapability>,
}

/// Job states (`JobState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    /// Waiting.
    Queued,
    /// Running.
    Running,
    /// Done.
    Done,
    /// Failed.
    Failed,
    /// Cancelled.
    Cancelled,
}

/// A job (`Job`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// Id.
    pub id: String,
    /// App.
    pub app_id: String,
    /// Model.
    pub model_id: Option<String>,
    /// Priority.
    pub priority: Priority,
    /// Kind.
    pub kind: JobKind,
    /// State.
    pub state: JobState,
    /// Submitted.
    pub submitted_at: Millis,
    /// Started.
    pub started_at: Option<Millis>,
    /// 0–1.
    pub progress: Option<f32>,
    /// Tokens so far.
    pub tokens: u64,
}

/// The queue (`QueueSnapshot`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    /// Running.
    pub running: Vec<Job>,
    /// Waiting.
    pub waiting: Vec<Job>,
    /// Waiting per priority.
    pub depth: Depth,
}

/// Throughput now (`Throughput`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Throughput {
    /// Prompt tokens/s.
    pub prompt_tps: f64,
    /// Generated tokens/s.
    pub gen_tps: f64,
    /// Median time to first token.
    pub ttft_p50_ms: f64,
    /// Requests in the last minute.
    pub requests_per_min: f64,
    /// Running requests.
    pub active_requests: u32,
}

/// A key as listed (`ApiKeyInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfo {
    /// Id.
    pub id: String,
    /// First characters.
    pub prefix: String,
    /// Created.
    pub created_at: Millis,
    /// Last used.
    pub last_used_at: Option<Millis>,
}

/// An app (`AppInfo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// One of the suite's apps.
    pub suite: bool,
    /// Seen in the last five minutes.
    pub connected: bool,
    /// Last request.
    pub last_seen_at: Option<Millis>,
    /// Keys.
    pub keys: Vec<ApiKeyInfo>,
    /// Permissions.
    pub permissions: AppPermissions,
    /// Today's usage.
    pub today: UsageCounters,
}

/// One app in detail (`AppDetail`): the listing, this month, where each configured limit stands, and its latest
/// requests (metadata only, never content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDetail {
    /// The listing, with today's usage.
    pub app: AppInfo,
    /// This UTC month so far.
    pub month: UsageCounters,
    /// Every configured limit and how much of it is used.
    pub limits: Vec<LimitStatus>,
    /// The app's latest requests, newest first.
    pub recent: Vec<RequestRecord>,
}

/// A new key (`CreatedKey`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedKey {
    /// The app.
    pub app_id: String,
    /// The key's listing.
    pub key: ApiKeyInfo,
    /// The secret, shown once.
    pub secret: String,
}

/// A pairing request (`PairingRequest`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    /// Id.
    pub id: String,
    /// The app's id.
    pub app_id: String,
    /// Its name.
    pub app_name: String,
    /// Six digits.
    pub code: String,
    /// When.
    pub requested_at: Millis,
    /// What it asks for.
    pub permissions: AppPermissions,
    /// A re-pair (ADR 0057 §4): approving it revokes the app's older keys. Only an app that lost its key on purpose sends
    /// it (Debrief's "re-pair after the update"); a second machine pairing under the same name does not.
    #[serde(default)]
    pub re_pair: bool,
    /// For a re-pair of an app Pitwall has: that app as stored (not the requester's name) and how many keys approving
    /// revokes. `None` for a plain pairing, or a re-pair of an app Pitwall does not know.
    #[serde(default)]
    pub replaces: Option<ReplacedApp>,
}

/// The app a re-pair replaces the keys of (`ReplacedApp`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedApp {
    /// Its id.
    pub app_id: String,
    /// Its name as Pitwall stored it.
    pub app_name: String,
    /// How many keys an approval revokes.
    pub keys: u32,
}

/// An engine tile (`EngineStatusTile`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatusTile {
    /// Kind.
    pub kind: EngineKind,
    /// Name.
    pub name: String,
    /// Availability.
    pub availability: Availability,
    /// Active build.
    pub active_build: Option<String>,
    /// Processes.
    pub processes: u32,
    /// Unhealthy processes.
    pub unhealthy: u32,
}

/// Whether a model's engine is being restarted or has been given up on (`IssueState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueState {
    /// The engine died and Pitwall restarts it.
    Restarting,
    /// It kept crashing: no more automatic restarts until the user retries.
    Failed,
}

/// A model whose engine crashed (`EngineIssue`), shown on the dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineIssue {
    /// The model.
    pub model_id: String,
    /// Restarting or failed.
    pub state: IssueState,
    /// Crashes seen since the last retry.
    pub crashes: u32,
    /// When the next restart happens (Unix ms), while one is planned.
    pub restart_at: Option<u64>,
    /// What happened and what to do.
    pub message: String,
}

/// Where the automatic llama.cpp install stands (`EngineSetupState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineSetupState {
    /// Nothing is being installed (a build is installed, or none was needed yet).
    #[default]
    Idle,
    /// The suggested build is downloading, being checked or self-tested.
    Installing,
    /// The automatic install failed; the error says why and Retry tries again.
    Failed,
}

/// The automatic install of llama.cpp (`EngineSetup`): Pitwall installs the build suggested for this machine whenever no
/// build is installed, and models loaded or downloaded meanwhile wait for it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSetup {
    /// Where it stands.
    pub state: EngineSetupState,
    /// Why it failed (with the next step).
    pub error: Option<String>,
    /// The build's download while installing (progress).
    pub download: Option<Download>,
}

/// The dashboard (`Overview`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// Server.
    pub server: ServerInfo,
    /// Memory now.
    pub memory: MemorySample,
    /// Hot models.
    pub models: Vec<LoadedModel>,
    /// Engines.
    pub engines: Vec<EngineStatusTile>,
    /// Queue.
    pub queue: QueueSnapshot,
    /// Throughput.
    pub throughput: Throughput,
    /// Apps.
    pub apps: Vec<AppInfo>,
    /// Memory preset.
    pub preset: Preset,
    /// Pending pairings.
    pub pairings: Vec<PairingRequest>,
    /// Models whose engine crashed and is being restarted or was given up on.
    pub issues: Vec<EngineIssue>,
    /// The automatic llama.cpp install.
    #[serde(default)]
    pub engine_setup: EngineSetup,
}

/// History (`History`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    /// Range.
    pub range: HistoryRange,
    /// Step.
    pub step_ms: Millis,
    /// Points.
    pub points: Vec<HistoryPoint>,
    /// Requests per app.
    pub requests_by_app: Vec<KeyedSeries>,
    /// Requests per model.
    pub requests_by_model: Vec<KeyedSeries>,
    /// Events in the range.
    pub events: Vec<PitwallEvent>,
}

/// Download states (`DownloadState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    /// Waiting.
    Queued,
    /// Downloading.
    Downloading,
    /// Checking the SHA-256.
    Verifying,
    /// Unpacking.
    Unpacking,
    /// Testing a build.
    SelfTest,
    /// Done.
    Done,
    /// Failed.
    Failed,
    /// Paused (resumable).
    Paused,
    /// Cancelled.
    Cancelled,
}

/// What is downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadKind {
    /// A model.
    Model,
    /// An engine build.
    Engine,
}

/// A download (`Download`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    /// Id.
    pub id: String,
    /// Kind.
    pub kind: DownloadKind,
    /// What.
    pub name: String,
    /// From where.
    pub host: String,
    /// Bytes done.
    pub bytes_done: u64,
    /// Bytes total.
    pub bytes_total: u64,
    /// Speed.
    pub speed_bps: u64,
    /// State.
    pub state: DownloadState,
    /// Why it failed.
    pub error: Option<String>,
    /// When it started.
    pub started_at: Millis,
}

/// Disk usage (`DiskUsage`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    /// Models.
    pub models_bytes: u64,
    /// Engines.
    pub engines_bytes: u64,
    /// Partial downloads.
    pub partial_bytes: u64,
    /// Free on the data drive.
    pub free_bytes: u64,
    /// Models unused for `unused_after_days`.
    pub unused: Vec<String>,
    /// The unused threshold.
    pub unused_after_days: u32,
}

/// A local model (`LocalModel`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    /// Id.
    pub id: String,
    /// Source repository.
    pub repo: String,
    /// File.
    pub file: String,
    /// Name.
    pub name: String,
    /// Quantisation.
    pub quant: Option<String>,
    /// Bytes.
    pub size_bytes: u64,
    /// Parameters in billions.
    pub params_b: Option<f64>,
    /// Trained context.
    pub context_max: Option<u64>,
    /// Architecture.
    pub architecture: Option<String>,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Vision projector file.
    pub projector: Option<String>,
    /// Added.
    pub added_at: Millis,
    /// Last used.
    pub last_used_at: Option<Millis>,
    /// Loaded now.
    pub loaded: bool,
    /// Keep loaded.
    pub pinned: bool,
    /// Own idle time.
    pub idle_secs: Option<u64>,
    /// Parallel slots it loads with (its own preference or the default).
    #[serde(default = "one")]
    pub slots: u32,
    /// The slots are the model's own preference (false: the default from the settings).
    #[serde(default)]
    pub slots_custom: bool,
    /// The slots it is loaded with now (`None`: not loaded); a change applies on the next load.
    #[serde(default)]
    pub loaded_slots: Option<u32>,
    /// The context of one slot, in tokens: what one request may hold (the one it is loaded with, else the one it will be
    /// loaded with). `None` from an older server.
    #[serde(default)]
    pub context_per_slot: Option<u32>,
    /// Fit now, at its context per slot and slots.
    pub fit: Fit,
}

fn one() -> u32 {
    1
}

/// The result of changing a model's slots (`SlotsChange`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotsChange {
    /// The slots it will load with from now on.
    pub slots: u32,
    /// The slots it is loaded with now (`None`: not loaded).
    pub loaded_slots: Option<u32>,
    /// It is loaded with a different count: the change applies the next time it loads (a loaded model is never
    /// unloaded or restarted by this call, least of all while it is busy).
    pub applies_on_next_load: bool,
}

/// A quantisation variant (`QuantVariant`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantVariant {
    /// File (first part).
    pub file: String,
    /// Quantisation.
    pub quant: Option<String>,
    /// Bytes of all parts.
    pub size_bytes: u64,
    /// Parts.
    pub parts: u32,
    /// SHA-256 of the first part.
    pub sha256: Option<String>,
    /// Fit.
    pub fit: Fit,
    /// The most parallel slots that fit at the context the fit was checked at (0: not even one; the answer to
    /// "how many requests at once?").
    #[serde(default)]
    pub max_slots: u32,
    /// On disk already.
    pub downloaded: bool,
}

/// A repository (`HfRepo`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HfRepo {
    /// Id.
    pub repo: String,
    /// Model files.
    pub variants: Vec<QuantVariant>,
    /// Projectors.
    pub projectors: Vec<QuantVariant>,
    /// Licence.
    pub license: Option<String>,
    /// Gated (needs a Hugging Face login, which Pitwall doesn't do).
    pub gated: bool,
}

/// API families (`ApiFamily`).
pub use crate::api::ApiFamily;

/// How the model was chosen (`ChosenBy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChosenBy {
    /// Named by the client.
    Explicit,
    /// By capabilities.
    Capabilities,
    /// The default model.
    Default,
}

/// A request record (`RequestRecord`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    /// Id.
    pub id: String,
    /// When.
    pub at: Millis,
    /// App.
    pub app_id: String,
    /// API family.
    pub api: ApiFamily,
    /// Path.
    pub endpoint: String,
    /// Model.
    pub model_id: Option<String>,
    /// How chosen.
    pub chosen_by: ChosenBy,
    /// The preset (alias) the app asked for, when the `model` field named one; `model_id` is what it resolved to.
    pub alias: Option<String>,
    /// Priority.
    pub priority: Priority,
    /// HTTP status.
    pub status: u16,
    /// Streamed.
    pub stream: bool,
    /// Time queued.
    pub queued_ms: u64,
    /// Time to first token.
    pub ttft_ms: Option<u64>,
    /// Total time.
    pub total_ms: u64,
    /// Cold start.
    pub cold_start_ms: Option<u64>,
    /// Prompt tokens.
    pub prompt_tokens: u32,
    /// Generated tokens.
    pub completion_tokens: u32,
    /// Error message.
    pub error: Option<String>,
    /// Content kept.
    pub has_content: bool,
    /// What the response cache did (`None`: the request was not eligible or the cache is not used for it).
    #[serde(default)]
    pub cache: Option<crate::cache::Outcome>,
    /// Prompt tokens llama-server reused from its prompt cache (the prefix of an earlier request).
    #[serde(default)]
    pub cached_tokens: u32,
}

/// Request content (`RequestContent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContent {
    /// Id.
    pub id: String,
    /// The request body.
    pub request: String,
    /// The answer.
    pub response: String,
}

/// A tool call the model made, as a playground message carries it (`PlaygroundToolCall`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaygroundToolCall {
    /// The call's id; the `tool` message that answers it carries the same id.
    pub id: String,
    /// The tool's name.
    pub name: String,
    /// The arguments as JSON text.
    pub arguments: String,
}

/// A playground message (`PlaygroundMessage`). Text only; images and audio come with their own work packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaygroundMessage {
    /// `system`, `user`, `assistant` or `tool`.
    pub role: crate::chat::Role,
    /// Text.
    pub content: String,
    /// The tool calls of an assistant message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<PlaygroundToolCall>,
    /// The call a `tool` message answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl PlaygroundMessage {
    /// A text message without tool calls.
    pub fn text(role: crate::chat::Role, content: impl Into<String>) -> PlaygroundMessage {
        PlaygroundMessage {
            role,
            content: content.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
}

/// A tool the model may call (`PlaygroundTool`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaygroundTool {
    /// Its name.
    pub name: String,
    /// What it does.
    pub description: String,
    /// A JSON schema object for its arguments.
    pub parameters: serde_json::Value,
}

/// Whether and which tools the model must call (`PlaygroundToolChoice`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlaygroundToolChoice {
    /// The model decides.
    #[default]
    Auto,
    /// No tools.
    None,
    /// At least one tool.
    Required,
    /// This tool.
    Named {
        /// The tool's name.
        name: String,
    },
}

/// A playground request (`PlaygroundRequest`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaygroundRequest {
    /// Model.
    pub model_id: String,
    /// Messages.
    pub messages: Vec<PlaygroundMessage>,
    /// Temperature.
    pub temperature: f32,
    /// Max tokens.
    pub max_tokens: u32,
    /// The tools the model may call (none: a plain chat).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<PlaygroundTool>,
    /// Tool use.
    #[serde(default)]
    pub tool_choice: PlaygroundToolChoice,
}

/// What the engine is doing before the first token (`PlaygroundPhase`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlaygroundPhase {
    /// Waiting for a slot and loading the model; repeated about once a second with the time so far.
    #[serde(rename_all = "camelCase")]
    Loading {
        /// Milliseconds since the request arrived.
        elapsed_ms: u64,
    },
    /// The model is ready and generating.
    Generating,
}

/// Why an answer ended (`PlaygroundFinish`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaygroundFinish {
    /// The model finished.
    Stop,
    /// The token limit was reached.
    Length,
    /// The model called tools; the caller runs them and continues.
    ToolCalls,
    /// The request was cancelled.
    Cancelled,
}

/// A playground stream event (`PlaygroundEvent`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlaygroundEvent {
    /// What happens before the first token.
    Phase {
        /// The phase.
        phase: PlaygroundPhase,
    },
    /// Text.
    Delta {
        /// The text.
        text: String,
    },
    /// Reasoning text the engine separated from the answer.
    Reasoning {
        /// The text.
        text: String,
    },
    /// A piece of a tool call; the pieces with one `index` make one call.
    #[serde(rename_all = "camelCase")]
    ToolCall {
        /// Which call.
        index: u32,
        /// The id, in the first piece.
        id: Option<String>,
        /// The name, in the first piece.
        name: Option<String>,
        /// More of the arguments' JSON text.
        arguments: String,
    },
    /// The end.
    #[serde(rename_all = "camelCase")]
    Done {
        /// Why it ended.
        finish: PlaygroundFinish,
        /// Prompt tokens.
        prompt_tokens: u32,
        /// Generated tokens.
        completion_tokens: u32,
        /// Time from the request to the first token (text, reasoning or tool call), cold load included.
        ttft_ms: u64,
        /// Generated tokens/s.
        gen_tps: f64,
        /// Cold start.
        cold_start_ms: Option<u64>,
    },
    /// A failure.
    Error {
        /// What.
        error: CommandError,
    },
}

/// A benchmark result (`BenchmarkResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkResult {
    /// Model.
    pub model_id: String,
    /// Build.
    pub build: String,
    /// When.
    pub at: Millis,
    /// Prompt tokens/s.
    pub prompt_tps: f64,
    /// Generated tokens/s.
    pub gen_tps: f64,
    /// Time to first token.
    pub ttft_ms: u64,
    /// Context.
    pub context_tokens: u32,
}

/// A preset as the UI edits it (`PresetDraft`): plain text that the engine parses and validates once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetDraft {
    /// The name apps send as `model`.
    pub alias: String,
    /// A model id, or `auto:<capabilities>`.
    pub target: String,
    /// Context the model loads with.
    #[serde(default)]
    pub context: Option<u32>,
    /// Default temperature.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Default top-p.
    #[serde(default)]
    pub top_p: Option<f32>,
    /// Default most tokens.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// System prompt used when the request has none.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Use the response cache.
    #[serde(default)]
    pub cache_responses: bool,
    /// Keep the prompt prefix warm.
    #[serde(default)]
    pub keep_warm: bool,
}

impl PresetDraft {
    /// Parses and range-checks the draft; the first problem is reported in words.
    pub fn parse(self) -> Result<ModelPreset, crate::presets::PresetError> {
        use crate::presets::{AliasName, Target};
        ModelPreset {
            alias: AliasName::parse(&self.alias)?,
            target: Target::parse(&self.target)?,
            context: self.context,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            system_prompt: self.system_prompt,
            cache_responses: self.cache_responses,
            keep_warm: self.keep_warm,
        }
        .checked()
    }
}

/// Where a preset stands now (`PresetStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum PresetStatus {
    /// It resolves to a model.
    #[serde(rename_all = "camelCase")]
    Ready {
        /// The model that would serve it now.
        model_id: String,
        /// Fixed by the preset, or chosen by capabilities.
        chosen_by: ChosenBy,
        /// One sentence for the inspector.
        reason: String,
        /// Whether that model is loaded now.
        loaded: bool,
    },
    /// Its model was removed.
    Stale {
        /// What to do.
        message: String,
    },
    /// Its capability selection matches no installed model.
    NoMatch {
        /// What to do.
        message: String,
    },
    /// A model with the same name is installed, and requests for that name go to the model.
    Shadowed {
        /// What to do.
        message: String,
    },
}

impl PresetStatus {
    /// The status for the outcome of resolving a preset.
    pub fn of(outcome: Result<crate::presets::Resolution, crate::presets::PresetError>, loaded: impl FnOnce(&str) -> bool) -> PresetStatus {
        use crate::presets::PresetError;
        match outcome {
            Ok(r) => PresetStatus::Ready {
                loaded: loaded(r.model.as_str()),
                model_id: r.model.to_string(),
                chosen_by: r.chosen_by,
                reason: r.reason,
            },
            Err(e @ PresetError::StaleTarget { .. }) => PresetStatus::Stale { message: e.to_string() },
            Err(e) => PresetStatus::NoMatch { message: e.to_string() },
        }
    }
}

/// One preset in the listing (`PresetEntry`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetEntry {
    /// The preset.
    pub preset: ModelPreset,
    /// Where it stands now.
    pub status: PresetStatus,
}

/// The live check of a draft in the editor (`PresetPreview`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetPreview {
    /// What is wrong with the draft, in words (`None`: it can be saved).
    pub problem: Option<String>,
    /// What the target resolves to now, when the target itself is valid.
    pub status: Option<PresetStatus>,
}

/// A pushed update (`LiveUpdate`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LiveUpdate {
    /// Memory.
    Memory {
        /// The reading.
        sample: MemorySample,
    },
    /// Hot models.
    Models {
        /// The models.
        models: Vec<LoadedModel>,
    },
    /// Queue.
    Queue {
        /// The queue.
        queue: QueueSnapshot,
    },
    /// Engine crashes being handled.
    Issues {
        /// The current issues.
        issues: Vec<EngineIssue>,
    },
    /// Throughput.
    Throughput {
        /// The numbers.
        throughput: Throughput,
    },
    /// An event.
    Event {
        /// The event.
        event: PitwallEvent,
    },
    /// A download.
    Download {
        /// The download.
        download: Download,
    },
    /// The automatic llama.cpp install changed.
    EngineSetup {
        /// Where it stands.
        setup: EngineSetup,
    },
    /// Pairings.
    Pairing {
        /// Pending pairings.
        pairings: Vec<PairingRequest>,
    },
    /// The server.
    Server {
        /// The server.
        server: ServerInfo,
    },
}

/// How a model update check ended (`UpdateCheckOutcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateCheckOutcome {
    /// Hugging Face was asked and the notices are up to date.
    Checked,
    /// Another check ran moments ago.
    RateLimited,
    /// A check is running already.
    Busy,
    /// No installed model came from a Hugging Face repository.
    NothingToCheck,
    /// Hugging Face could not be asked (see the message).
    Failed,
}

/// The result of "Check now" (`UpdateCheckResult`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    /// How it ended.
    pub outcome: UpdateCheckOutcome,
    /// One sentence for the user.
    pub message: String,
    /// Repositories that were asked.
    pub repos_checked: u32,
    /// Notices shown now (not dismissed).
    pub notices: u32,
    /// Seconds until a new check is allowed (rate limit).
    pub retry_in_secs: Option<u64>,
}

/// The state of the model update checks (`UpdateCheckStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckStatus {
    /// The schedule setting.
    pub settings: UpdateCheckSettings,
    /// When a check last completed (Unix ms).
    pub last_checked_at: Option<Millis>,
    /// When one was last started (Unix ms).
    pub last_attempt_at: Option<Millis>,
    /// How the last attempt went.
    pub last_message: Option<String>,
    /// When the next scheduled check is due (`None`: the schedule is off).
    pub next_check_at: Option<Millis>,
    /// A check is running.
    pub running: bool,
    /// Notices shown now.
    pub notices: u32,
    /// The only host a check talks to.
    pub host: String,
    /// Most repositories one check asks about.
    pub max_repos: u32,
}

#[cfg(test)]
mod tests {
    #[test]
    fn update_check_dtos_use_the_documented_wire_names() {
        use super::*;
        let r = UpdateCheckResult {
            outcome: UpdateCheckOutcome::RateLimited,
            message: "m".into(),
            repos_checked: 2,
            notices: 1,
            retry_in_secs: Some(40),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"outcome": "rateLimited", "message": "m", "reposChecked": 2, "notices": 1, "retryInSecs": 40})
        );
        assert_eq!(serde_json::from_value::<UpdateCheckResult>(v).unwrap(), r);
        for (o, w) in [
            (UpdateCheckOutcome::Checked, "checked"),
            (UpdateCheckOutcome::Busy, "busy"),
            (UpdateCheckOutcome::NothingToCheck, "nothingToCheck"),
            (UpdateCheckOutcome::Failed, "failed"),
        ] {
            assert_eq!(serde_json::to_value(o).unwrap(), w);
        }
        let s = UpdateCheckStatus {
            settings: UpdateCheckSettings::default(),
            last_checked_at: None,
            last_attempt_at: Some(5),
            last_message: None,
            next_check_at: None,
            running: false,
            notices: 0,
            host: "huggingface.co".into(),
            max_repos: 40,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["settings"], serde_json::json!({"enabled": false, "schedule": "weekly"}));
        assert_eq!((v["lastAttemptAt"].as_u64(), v["maxRepos"].as_u64()), (Some(5), Some(40)));
    }

    use super::*;

    use serde_json::json;

    #[test]
    fn shapes_match_api_types_ts() {
        assert_eq!(serde_json::to_value(EngineKind::LlamaCpp).unwrap(), "llama.cpp");
        assert_eq!(
            serde_json::to_value(Availability::Available).unwrap(),
            json!({"state": "available"})
        );
        assert_eq!(
            serde_json::to_value(Availability::NotAvailable { reason: "r".into() }).unwrap(),
            json!({"state": "notAvailable", "reason": "r"})
        );
        let e = PlaygroundEvent::Done {
            finish: PlaygroundFinish::Stop,
            prompt_tokens: 1,
            completion_tokens: 2,
            ttft_ms: 3,
            gen_tps: 4.0,
            cold_start_ms: None,
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"kind": "done", "finish": "stop", "promptTokens": 1, "completionTokens": 2, "ttftMs": 3, "genTps": 4.0, "coldStartMs": null})
        );
        let err = PlaygroundEvent::Error {
            error: CommandError::new(ErrorCode::InsufficientMemory, "m"),
        };
        assert_eq!(serde_json::to_value(&err).unwrap()["error"]["code"], "insufficientMemory");
        let t = LiveUpdate::Throughput {
            throughput: Throughput::default(),
        };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["kind"], "throughput");
        assert_eq!(v["throughput"]["ttftP50Ms"], 0.0);
        assert_eq!(
            serde_json::to_value(Depth {
                interactive: 1,
                background: 2,
                batch: 3
            })
            .unwrap(),
            json!({"interactive": 1, "background": 2, "batch": 3})
        );
        assert_eq!(serde_json::to_value(HistoryRange::Day).unwrap(), "24h");
        assert_eq!(serde_json::to_value(UsageRange::Week).unwrap(), "7d");
        assert_eq!(serde_json::to_value(LimitKind::TokensPerMonth).unwrap(), "tokensPerMonth");
        assert_eq!(serde_json::to_value(LimitState::Warning).unwrap(), "warning");
        assert_eq!(serde_json::to_value(EventKind::LimitWarning).unwrap(), "limitWarning");
        assert_eq!(serde_json::to_value(EventKind::ModelUpdate).unwrap(), "modelUpdate");
        let l: AppLimits = serde_json::from_str(r#"{"requestsPerDay":5,"tokensPerMonth":9,"maxMemoryPercent":40}"#).unwrap();
        assert_eq!(
            (l.requests_per_day, l.tokens_per_month, l.max_memory_percent),
            (Some(5), Some(9), Some(40))
        );
        assert_eq!(serde_json::to_value(ReleaseReason::DeepIdle).unwrap(), "deepIdle");
        assert_eq!(serde_json::to_value(Preset::LowMemory).unwrap(), "lowMemory");
        assert_eq!(serde_json::to_value(DownloadState::SelfTest).unwrap(), "selfTest");
        assert_eq!(serde_json::to_value(ApiFamily::OpenAi).unwrap(), "openai");
        assert_eq!(serde_json::to_value(ChosenBy::Capabilities).unwrap(), "capabilities");
        assert_eq!(serde_json::to_value(HotState::ReleasingContext).unwrap(), "releasingContext");
        assert_eq!(
            serde_json::from_value::<PlaygroundMessage>(json!({"role": "user", "content": "x"}))
                .unwrap()
                .content,
            "x"
        );
        // Tool fields are additive: an old request still parses, a new one keeps its tools and choice.
        let old: PlaygroundRequest =
            serde_json::from_value(json!({"modelId": "m", "messages": [], "temperature": 0.5, "maxTokens": 8})).unwrap();
        assert!(old.tools.is_empty() && old.tool_choice == PlaygroundToolChoice::Auto);
        let new: PlaygroundRequest = serde_json::from_value(json!({
            "modelId": "m", "messages": [{"role": "tool", "content": "r", "toolCallId": "c1"}], "temperature": 0.5, "maxTokens": 8,
            "tools": [{"name": "t", "description": "d", "parameters": {"type": "object"}}],
            "toolChoice": {"kind": "named", "name": "t"}
        }))
        .unwrap();
        assert_eq!(new.tool_choice, PlaygroundToolChoice::Named { name: "t".into() });
        assert_eq!(new.messages[0].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(
            serde_json::to_value(PlaygroundToolChoice::Required).unwrap(),
            json!({"kind": "required"})
        );
        assert_eq!(
            serde_json::to_value(PlaygroundEvent::Phase {
                phase: PlaygroundPhase::Loading { elapsed_ms: 7 }
            })
            .unwrap(),
            json!({"kind": "phase", "phase": {"kind": "loading", "elapsedMs": 7}})
        );
        assert_eq!(
            serde_json::to_value(PlaygroundEvent::ToolCall {
                index: 0,
                id: Some("c".into()),
                name: None,
                arguments: "{".into()
            })
            .unwrap(),
            json!({"kind": "toolCall", "index": 0, "id": "c", "name": null, "arguments": "{"})
        );
        assert_eq!(
            serde_json::to_value(PlaygroundEvent::Reasoning { text: "t".into() }).unwrap()["kind"],
            "reasoning"
        );
        assert_eq!(serde_json::to_value(PlaygroundFinish::ToolCalls).unwrap(), "toolCalls");
        assert_eq!(PlaygroundMessage::text(crate::chat::Role::User, "x").tool_calls.len(), 0);
        let codes = [
            (ErrorCode::ServerUnavailable, 503),
            (ErrorCode::Unauthorized, 401),
            (ErrorCode::Forbidden, 403),
            (ErrorCode::NotFound, 404),
            (ErrorCode::BadRequest, 400),
            (ErrorCode::Conflict, 409),
            (ErrorCode::InsufficientMemory, 503),
            (ErrorCode::Busy, 503),
            (ErrorCode::NotAvailable, 501),
            (ErrorCode::Upstream, 502),
            (ErrorCode::NotImplemented, 501),
            (ErrorCode::Internal, 500),
        ];
        for (c, s) in codes {
            assert_eq!(c.status(), s);
        }
        assert_eq!(CommandError::new(ErrorCode::Busy, "b").to_string(), "b");
    }

    #[test]
    fn preset_drafts_parse_and_statuses_serialize() {
        use crate::presets::{PresetError, Resolution};
        let d: PresetDraft =
            serde_json::from_value(json!({"alias": " Debrief-Explain ", "target": "auto:chat,fast", "temperature": 0.3})).unwrap();
        let p = d.parse().unwrap();
        assert_eq!(
            (p.alias.as_str(), p.target.to_text().as_str(), p.temperature),
            ("debrief-explain", "auto:chat,fast", Some(0.3))
        );
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            (v["alias"].as_str(), v["target"].as_str(), v["maxTokens"].is_null()),
            (Some("debrief-explain"), Some("auto:chat,fast"), true)
        );
        for bad in [
            json!({"alias": "bad name", "target": "m"}),
            json!({"alias": "ok", "target": "auto:nope"}),
            json!({"alias": "ok", "target": "m", "temperature": 3.0}),
        ] {
            assert!(serde_json::from_value::<PresetDraft>(bad).unwrap().parse().is_err());
        }
        let ready = PresetStatus::of(
            Ok(Resolution {
                model: crate::ids::ModelId::parse("tiny").unwrap(),
                chosen_by: ChosenBy::Explicit,
                reason: "r".into(),
            }),
            |id| id == "tiny",
        );
        let v = serde_json::to_value(&ready).unwrap();
        assert_eq!(
            (
                v["state"].as_str(),
                v["modelId"].as_str(),
                v["chosenBy"].as_str(),
                v["loaded"].as_bool()
            ),
            (Some("ready"), Some("tiny"), Some("explicit"), Some(true))
        );
        let stale = PresetStatus::of(
            Err(PresetError::StaleTarget {
                alias: "a".into(),
                model: "m".into(),
            }),
            |_| false,
        );
        assert!(matches!(&stale, PresetStatus::Stale { message } if message.contains("no longer installed")));
        let none = PresetStatus::of(
            Err(PresetError::NoMatch {
                alias: "a".into(),
                reason: "r".into(),
            }),
            |_| false,
        );
        assert_eq!(serde_json::to_value(&none).unwrap()["state"], "noMatch");
        let other = PresetStatus::of(Err(PresetError::TooMany), |_| false);
        assert!(matches!(other, PresetStatus::NoMatch { .. }));
        assert_eq!(
            serde_json::to_value(PresetStatus::Shadowed { message: "m".into() }).unwrap()["state"],
            "shadowed"
        );
    }

    #[test]
    fn engine_pin_dtos_use_camel_case() {
        let c = BuildCapability {
            capability: Capability::Vision,
            label: "Image input".into(),
            status: CapabilityStatus::NeedsBuild,
            min_build: Some(5600),
            hint: Some("Install b5600".into()),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["status"], "needsBuild");
        assert_eq!(v["minBuild"], 5600);
        assert_eq!(v["capability"], "vision");
        let state = EnginePinState {
            global: Some("vulkan".into()),
            models: vec![ModelEnginePin {
                model_id: "m".into(),
                build_id: "cpu".into(),
                tag: None,
            }],
            active: None,
            active_source: "pinned for all models".into(),
            rollback_to: Some(RollbackTarget {
                build_id: "cpu".into(),
                tag: "b1".into(),
            }),
            rollback_blocked: None,
            problems: vec![],
        };
        let change = PinChange {
            state,
            applies_on_next_load: vec!["m".into()],
            message: "ok".into(),
        };
        let v = serde_json::to_value(&change).unwrap();
        assert_eq!(v["appliesOnNextLoad"][0], "m");
        assert_eq!(v["state"]["rollbackTo"]["buildId"], "cpu");
        assert_eq!(v["state"]["models"][0]["modelId"], "m");
        let back: PinChange = serde_json::from_value(v).unwrap();
        assert_eq!(back, change);
        assert_eq!(serde_json::to_value(CapabilityStatus::NotSupported).unwrap(), "notSupported");
        let caps = BuildCapabilities {
            build_id: "cpu".into(),
            tag: "b1".into(),
            capabilities: vec![c],
        };
        assert_eq!(serde_json::to_value(caps).unwrap()["buildId"], "cpu");
    }

    #[test]
    fn a_model_from_an_older_reply_loads_with_one_slot() {
        let v = serde_json::json!({
            "id": "m", "repo": "r", "file": "f.gguf", "name": "m", "quant": null, "sizeBytes": 1, "paramsB": null,
            "contextMax": null, "architecture": null, "capabilities": [], "projector": null, "addedAt": 0,
            "lastUsedAt": null, "loaded": false, "pinned": false, "idleSecs": null,
            "fit": {"verdict": "fits", "needRam": 0, "needVram": 0, "haveRam": 0, "haveVram": 0, "offload": "all", "message": ""}
        });
        let m: LocalModel = serde_json::from_value(v).unwrap();
        assert_eq!(m.slots, 1);
        assert!(!m.slots_custom);
        assert_eq!(m.loaded_slots, None);
    }
}
