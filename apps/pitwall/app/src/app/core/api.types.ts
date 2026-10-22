/**
 * The UI ↔ app wire contract (camelCase JSON). Mirrored by `pitwall_core::mgmt` in Rust: the Tauri commands return
 * exactly these shapes (they forward the server's management API, `/pw/v1/...`). Change both sides in one commit.
 *
 * Times are Unix milliseconds; sizes are bytes; durations are milliseconds unless the name says otherwise.
 */

// ---------------------------------------------------------------------------------------------------------------
// Errors

export type ErrorCode =
  | 'serverUnavailable'
  | 'unauthorized'
  | 'forbidden'
  | 'notFound'
  | 'badRequest'
  | 'conflict'
  | 'offline'
  | 'insufficientMemory'
  | 'busy'
  | 'notAvailable'
  | 'notImplemented'
  | 'internal';

export interface CommandError {
  code: ErrorCode;
  message: string;
}

// ---------------------------------------------------------------------------------------------------------------
// Shared vocabulary

export type Priority = 'interactive' | 'background' | 'batch';
export type Capability = 'chat' | 'vision' | 'tools' | 'json' | 'embeddings' | 'rerank' | 'transcription';
export type Preset = 'lowMemory' | 'balanced' | 'performance' | 'custom';
/** `evicted`: unloaded to make room for another model. */
export type ReleaseReason = 'idle' | 'deepIdle' | 'pressure' | 'yield' | 'manual' | 'shutdown' | 'failed' | 'evicted';
export type HistoryRange = '1h' | '24h' | '7d' | '30d';
export type Severity = 'info' | 'warn' | 'error';

// ---------------------------------------------------------------------------------------------------------------
// Server and memory

export type ServingState = 'serving' | 'paused';

export interface ServerInfo {
  version: string;
  /** The client contract version apps code against (`/x/v1`, discovery file `apiVersion`). */
  apiVersion: number;
  pid: number;
  port: number;
  startedAt: number;
  serving: ServingState;
  offline: boolean;
  lan: LanStatus;
}

export interface LanStatus {
  enabled: boolean;
  /** `https://<address>:<port>` when enabled. */
  url: string | null;
  /** SHA-256 of the self-signed certificate (hex, colon separated) for pinning. */
  certFingerprint: string | null;
}

export type MemoryPressure = 'normal' | 'elevated' | 'critical';

export interface GpuSample {
  name: string;
  vramTotal: number | null;
  vramUsed: number | null;
  /** 0–100, when the driver reports it. */
  utilisation: number | null;
  unified: boolean;
}

export interface MemorySample {
  at: number;
  ramTotal: number;
  ramAvailable: number;
  /** Windows commit charge and limit; null elsewhere. */
  commitUsed: number | null;
  commitLimit: number | null;
  swapUsed: number;
  pressure: MemoryPressure;
  /** 0–100 over all cores. */
  cpuPercent: number;
  gpus: GpuSample[];
  /** Pitwall's server process plus its engine processes. */
  pitwallRss: number;
  /** A foreground full-screen app or game was detected (yield mode). */
  foregroundBusy: boolean;
}

// ---------------------------------------------------------------------------------------------------------------
// Loaded models and engines

export type HotState = 'loading' | 'ready' | 'busy' | 'releasingContext' | 'unloading' | 'failed';

export interface LoadedModel {
  modelId: string;
  name: string;
  engine: EngineKind;
  build: string;
  state: HotState;
  ramBytes: number;
  vramBytes: number;
  contextTokens: number;
  pinned: boolean;
  activeRequests: number;
  loadedAt: number;
  lastUsedAt: number;
  /** When the idle rule releases it (null: pinned or busy). */
  idleReleaseAt: number | null;
  coldStartMs: number;
  capabilities: Capability[];
}

export type EngineKind = 'llama.cpp' | 'mlx' | 'vllm' | 'whisper.cpp';

export type Availability =
  | { state: 'available' }
  | { state: 'notInstalled'; reason: string }
  | { state: 'notAvailable'; reason: string }
  | { state: 'failed'; reason: string };

export type ProcessHealth = 'starting' | 'healthy' | 'unhealthy' | 'restarting' | 'stopped';

export interface EngineProcess {
  pid: number;
  modelId: string;
  health: ProcessHealth;
  restarts: number;
  rssBytes: number;
  startedAt: number;
}

export type SelfTest = 'passed' | 'failed' | 'notRun';

export interface InstalledBuild {
  /** `cpu`, `vulkan`, `cuda-13`, `metal`, … */
  id: string;
  /** The llama.cpp release tag, e.g. `b11669`. */
  tag: string;
  label: string;
  sizeBytes: number;
  selfTest: SelfTest;
  selfTestMessage: string | null;
  /** Devices `--list-devices` reported. */
  devices: string[];
  active: boolean;
  /** A newer pinned release exists. */
  outdated: boolean;
}

export interface CatalogBuild {
  id: string;
  label: string;
  tag: string;
  downloadBytes: number;
  suggested: boolean;
  installed: boolean;
}

export interface Engine {
  kind: EngineKind;
  name: string;
  description: string;
  availability: Availability;
  /** `auto` or a build id. */
  buildChoice: string;
  builds: InstalledBuild[];
  catalog: CatalogBuild[];
  processes: EngineProcess[];
  /** A previous build to roll back to after an update. */
  rollbackTo: string | null;
}

// ---------------------------------------------------------------------------------------------------------------
// Queue and throughput

export type JobKind = 'chat' | 'completion' | 'embedding' | 'transcription' | 'task' | 'batch' | 'benchmark';
export type JobState = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';

export interface Job {
  id: string;
  appId: string;
  modelId: string | null;
  priority: Priority;
  kind: JobKind;
  state: JobState;
  submittedAt: number;
  startedAt: number | null;
  /** 0–1 when known (batch jobs). */
  progress: number | null;
  tokens: number;
}

export interface QueueSnapshot {
  running: Job[];
  waiting: Job[];
  depth: Record<Priority, number>;
}

export interface Throughput {
  promptTps: number;
  genTps: number;
  ttftP50Ms: number;
  requestsPerMin: number;
  activeRequests: number;
}

// ---------------------------------------------------------------------------------------------------------------
// Apps, keys and usage

export interface UsageCounters {
  requests: number;
  promptTokens: number;
  completionTokens: number;
  busyMs: number;
  errors: number;
}

export interface AppLimits {
  /** null: no limit. */
  requestsPerMin: number | null;
  tokensPerDay: number | null;
  maxConcurrent: number | null;
}

export interface AppPermissions {
  /** `null`: every model. */
  models: string[] | null;
  maxPriority: Priority;
  tools: boolean;
  extendedApis: boolean;
  limits: AppLimits;
}

export interface ApiKeyInfo {
  id: string;
  /** The first characters, e.g. `pw_3f9a…`, to tell keys apart. Never the key. */
  prefix: string;
  createdAt: number;
  lastUsedAt: number | null;
}

export interface AppInfo {
  id: string;
  name: string;
  /** IMT, Pitcrew, Spotter, Dyno are `suite: true`. */
  suite: boolean;
  connected: boolean;
  lastSeenAt: number | null;
  keys: ApiKeyInfo[];
  permissions: AppPermissions;
  today: UsageCounters;
}

export interface CreatedKey {
  appId: string;
  key: ApiKeyInfo;
  /** Shown once, never stored by Pitwall in clear. */
  secret: string;
}

export interface PairingRequest {
  id: string;
  appId: string;
  appName: string;
  /** Six digits the requesting app shows too, so the user can match them. */
  code: string;
  requestedAt: number;
  permissions: AppPermissions;
}

// ---------------------------------------------------------------------------------------------------------------
// Dashboard

export interface Overview {
  server: ServerInfo;
  memory: MemorySample;
  models: LoadedModel[];
  engines: EngineStatusTile[];
  queue: QueueSnapshot;
  throughput: Throughput;
  apps: AppInfo[];
  preset: Preset;
  pairings: PairingRequest[];
}

export interface EngineStatusTile {
  kind: EngineKind;
  name: string;
  availability: Availability;
  activeBuild: string | null;
  processes: number;
  unhealthy: number;
}

export interface HistoryPoint {
  at: number;
  ramUsed: number;
  ramTotal: number;
  vramUsed: number | null;
  vramTotal: number | null;
  pitwallRss: number;
  cpuPercent: number;
  promptTps: number;
  genTps: number;
  p50Ms: number | null;
  p95Ms: number | null;
  requests: number;
}

export interface KeyedSeries {
  key: string;
  values: number[];
}

export type EventKind =
  | 'modelLoaded'
  | 'modelUnloaded'
  | 'contextReleased'
  | 'engineStarted'
  | 'engineRestarted'
  | 'engineFailed'
  | 'download'
  | 'pairing'
  | 'servingChanged'
  | 'presetChanged'
  | 'requestFailed';

export interface PitwallEvent {
  id: number;
  at: number;
  kind: EventKind;
  severity: Severity;
  message: string;
  modelId: string | null;
  appId: string | null;
  reason: ReleaseReason | null;
}

export interface History {
  range: HistoryRange;
  stepMs: number;
  points: HistoryPoint[];
  /** Requests per step, per app (same length as points). */
  requestsByApp: KeyedSeries[];
  /** Requests per step, per model. */
  requestsByModel: KeyedSeries[];
  events: PitwallEvent[];
}

/** Pushed by the server (`/pw/v1/live`, SSE) and forwarded by the app as the `pitwall://live` event. */
export type LiveUpdate =
  | { kind: 'memory'; sample: MemorySample }
  | { kind: 'models'; models: LoadedModel[] }
  | { kind: 'queue'; queue: QueueSnapshot }
  | { kind: 'throughput'; throughput: Throughput }
  | { kind: 'event'; event: PitwallEvent }
  | { kind: 'download'; download: Download }
  | { kind: 'pairing'; pairings: PairingRequest[] }
  | { kind: 'server'; server: ServerInfo };

// ---------------------------------------------------------------------------------------------------------------
// Model library

export type FitVerdict = 'fits' | 'tight' | 'wontFit' | 'unknown';

export interface Fit {
  verdict: FitVerdict;
  needRam: number;
  needVram: number;
  haveRam: number;
  haveVram: number;
  /** `all`, `partial` or `none` of the layers on the GPU. */
  offload: 'all' | 'partial' | 'none';
  message: string;
}

export interface LocalModel {
  id: string;
  repo: string;
  file: string;
  name: string;
  quant: string | null;
  sizeBytes: number;
  paramsB: number | null;
  contextMax: number | null;
  architecture: string | null;
  capabilities: Capability[];
  /** A vision projector downloaded with it. */
  projector: string | null;
  addedAt: number;
  lastUsedAt: number | null;
  loaded: boolean;
  pinned: boolean;
  /** Per-model idle release in seconds; null: the global setting. */
  idleSecs: number | null;
  fit: Fit;
}

export interface HfModel {
  repo: string;
  author: string;
  downloads: number;
  likes: number;
  updatedAt: number | null;
  tags: string[];
  pipeline: string | null;
}

export interface QuantVariant {
  /** One file, or the first of a split set (`…-00001-of-00003.gguf`). */
  file: string;
  quant: string | null;
  sizeBytes: number;
  parts: number;
  sha256: string | null;
  fit: Fit;
  downloaded: boolean;
}

export interface HfRepo {
  repo: string;
  variants: QuantVariant[];
  projectors: QuantVariant[];
  license: string | null;
  gated: boolean;
}

export type DownloadState = 'queued' | 'downloading' | 'verifying' | 'unpacking' | 'selfTest' | 'done' | 'failed' | 'paused' | 'cancelled';

export interface Download {
  id: string;
  kind: 'model' | 'engine';
  name: string;
  host: string;
  bytesDone: number;
  bytesTotal: number;
  speedBps: number;
  state: DownloadState;
  error: string | null;
  startedAt: number;
}

export interface DiskUsage {
  modelsBytes: number;
  enginesBytes: number;
  partialBytes: number;
  freeBytes: number;
  /** Models not used for `unusedAfterDays`. */
  unused: string[];
  unusedAfterDays: number;
}

// ---------------------------------------------------------------------------------------------------------------
// Request inspector

export type ApiFamily = 'openai' | 'anthropic' | 'ollama' | 'extended';
export type ChosenBy = 'explicit' | 'capabilities' | 'default';

export interface RequestRecord {
  id: string;
  at: number;
  appId: string;
  api: ApiFamily;
  endpoint: string;
  modelId: string | null;
  chosenBy: ChosenBy;
  priority: Priority;
  status: number;
  stream: boolean;
  queuedMs: number;
  ttftMs: number | null;
  totalMs: number;
  coldStartMs: number | null;
  promptTokens: number;
  completionTokens: number;
  error: string | null;
  /** Content was kept (Settings ▸ Privacy ▸ keep request content). */
  hasContent: boolean;
}

export interface RequestContent {
  id: string;
  request: string;
  response: string;
}

// ---------------------------------------------------------------------------------------------------------------
// Playground and benchmarks

export interface PlaygroundMessage {
  role: 'system' | 'user' | 'assistant';
  content: string;
}

export interface PlaygroundRequest {
  modelId: string;
  messages: PlaygroundMessage[];
  temperature: number;
  maxTokens: number;
}

export type PlaygroundEvent =
  | { kind: 'delta'; text: string }
  | { kind: 'done'; promptTokens: number; completionTokens: number; ttftMs: number; genTps: number; coldStartMs: number | null }
  | { kind: 'error'; error: CommandError };

export interface BenchmarkResult {
  modelId: string;
  build: string;
  at: number;
  promptTps: number;
  genTps: number;
  ttftMs: number;
  contextTokens: number;
}

// ---------------------------------------------------------------------------------------------------------------
// Settings

export type ThemeChoice = 'system' | 'dark' | 'light';

export interface MemorySettings {
  preset: Preset;
  /** Unload a model idle this long (seconds). */
  idleUnloadSecs: number;
  /** Stop every idle engine process after this long (seconds). */
  deepIdleSecs: number;
  /** Release the KV cache / context first, the weights at the idle limit. */
  releaseContextFirst: boolean;
  /** Release idle models when available RAM drops below this percentage of total. */
  pressureAvailablePercent: number;
  /** Never let a load leave less than this free (bytes). */
  minFreeBytes: number;
  /** Most hot models at once. */
  maxHotModels: number;
  /** Most memory hot models may hold together (bytes; null: what fits). */
  hotCapBytes: number | null;
  yieldToForeground: boolean;
  notifyOnRelease: boolean;
}

export interface ServerSettings {
  port: number;
  maxRequestBytes: number;
  requestTimeoutSecs: number;
  maxConcurrent: number;
  /** Accept requests without a key from this machine (for Ollama-style tools). Off by default. */
  allowKeylessLocal: boolean;
}

export interface NetworkSettings {
  /** No downloads at all. */
  offline: boolean;
  lanEnabled: boolean;
  lanPort: number;
}

export interface PrivacySettings {
  /** Keep request and response text for the inspector (off by default; kept on this machine only). */
  keepRequestContent: boolean;
  metricsRetentionDays: number;
}

export interface GeneralSettings {
  startAtLogin: boolean;
  theme: ThemeChoice;
  keepServingWhenClosed: boolean;
}

export interface Settings {
  general: GeneralSettings;
  server: ServerSettings;
  memory: MemorySettings;
  network: NetworkSettings;
  privacy: PrivacySettings;
}

export interface AppUpdateStatus {
  available: boolean;
  reason: string | null;
  currentVersion: string;
}
