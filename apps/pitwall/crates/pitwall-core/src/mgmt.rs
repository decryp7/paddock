//! The management contract: what the server's `/pw/v1/...` routes return and the app forwards to the UI. It mirrors
//! `app/src/app/core/api.types.ts` (camelCase JSON); change both in the same commit. Types that belong to a domain
//! module are defined there and re-exported here.

pub use crate::config::UiSettings as Settings;
pub use crate::events::{Event as PitwallEvent, EventKind, Severity};
pub use crate::fit::{Fit, Offload, Verdict};
pub use crate::hf::HfModel;
pub use crate::limits::{AppLimits, AppPermissions};
pub use crate::memory::{Preset, ReleaseReason};
pub use crate::metrics::{HistoryPoint, KeyedSeries, Range as HistoryRange};
pub use crate::scheduler::{Depth, JobKind, Priority};
pub use crate::select::Capability;
pub use crate::usage::Counters as UsageCounters;

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
    /// Offline mode.
    Offline,
    /// Not enough memory.
    InsufficientMemory,
    /// Busy.
    Busy,
    /// Not available on this machine.
    NotAvailable,
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
            ErrorCode::Forbidden | ErrorCode::Offline => 403,
            ErrorCode::NotFound => 404,
            ErrorCode::BadRequest => 400,
            ErrorCode::Conflict => 409,
            ErrorCode::InsufficientMemory | ErrorCode::Busy => 503,
            ErrorCode::NotAvailable | ErrorCode::NotImplemented => 501,
            ErrorCode::Internal => 500,
        }
    }
}

/// Serving or paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServingState {
    /// Requests are served.
    Serving,
    /// New requests are refused.
    Paused,
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
    /// Offline mode.
    pub offline: bool,
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
    /// Apple MLX.
    #[serde(rename = "mlx")]
    Mlx,
    /// vLLM.
    #[serde(rename = "vllm")]
    Vllm,
    /// whisper.cpp.
    #[serde(rename = "whisper.cpp")]
    WhisperCpp,
    /// A declarative engine pack (ADR 0011).
    #[serde(rename = "pack")]
    Pack,
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
    /// Its context.
    pub context_tokens: u32,
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
    /// The engine pack that serves it (`None`: llama.cpp).
    #[serde(default)]
    pub engine: Option<String>,
    /// Fit now.
    pub fit: Fit,
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

/// A playground message (`PlaygroundMessage`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaygroundMessage {
    /// `system`, `user` or `assistant`.
    pub role: crate::chat::Role,
    /// Text.
    pub content: String,
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
}

/// A playground stream event (`PlaygroundEvent`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlaygroundEvent {
    /// Text.
    Delta {
        /// The text.
        text: String,
    },
    /// The end.
    #[serde(rename_all = "camelCase")]
    Done {
        /// Prompt tokens.
        prompt_tokens: u32,
        /// Generated tokens.
        completion_tokens: u32,
        /// Time to first token.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shapes_match_api_types_ts() {
        assert_eq!(serde_json::to_value(EngineKind::LlamaCpp).unwrap(), "llama.cpp");
        assert_eq!(serde_json::to_value(EngineKind::WhisperCpp).unwrap(), "whisper.cpp");
        assert_eq!(
            serde_json::to_value(Availability::Available).unwrap(),
            json!({"state": "available"})
        );
        assert_eq!(
            serde_json::to_value(Availability::NotAvailable { reason: "r".into() }).unwrap(),
            json!({"state": "notAvailable", "reason": "r"})
        );
        let e = PlaygroundEvent::Done {
            prompt_tokens: 1,
            completion_tokens: 2,
            ttft_ms: 3,
            gen_tps: 4.0,
            cold_start_ms: None,
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"kind": "done", "promptTokens": 1, "completionTokens": 2, "ttftMs": 3, "genTps": 4.0, "coldStartMs": null})
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
        let codes = [
            (ErrorCode::ServerUnavailable, 503),
            (ErrorCode::Unauthorized, 401),
            (ErrorCode::Forbidden, 403),
            (ErrorCode::Offline, 403),
            (ErrorCode::NotFound, 404),
            (ErrorCode::BadRequest, 400),
            (ErrorCode::Conflict, 409),
            (ErrorCode::InsufficientMemory, 503),
            (ErrorCode::Busy, 503),
            (ErrorCode::NotAvailable, 501),
            (ErrorCode::NotImplemented, 501),
            (ErrorCode::Internal, 500),
        ];
        for (c, s) in codes {
            assert_eq!(c.status(), s);
        }
        assert_eq!(CommandError::new(ErrorCode::Busy, "b").to_string(), "b");
    }
}
