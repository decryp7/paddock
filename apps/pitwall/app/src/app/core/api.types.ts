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
  | 'insufficientMemory'
  | 'busy'
  | 'notAvailable'
  | 'upstream'
  | 'notImplemented'
  | 'cancelled'
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

/** `stopped`: Pitwall was quit with "stop serving"; nothing is loaded until Pitwall is opened again or serving is resumed. */
export type ServingState = 'serving' | 'paused' | 'stopped';

export interface ServerInfo {
  version: string;
  /** The client contract version apps code against (`/x/v1`, discovery file `apiVersion`). */
  apiVersion: number;
  pid: number;
  port: number;
  startedAt: number;
  serving: ServingState;
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
  /** Context per slot. */
  contextTokens: number;
  /** Parallel slots it was loaded with; a change in its preferences applies on the next load. */
  slots: number;
  pinned: boolean;
  activeRequests: number;
  loadedAt: number;
  lastUsedAt: number;
  /** When the idle rule releases it (null: pinned or busy). */
  idleReleaseAt: number | null;
  coldStartMs: number;
  capabilities: Capability[];
}

/** llama.cpp is the only engine (ADR 0020). */
export type EngineKind = 'llama.cpp';

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
  /** Files of removed features an older version left on disk (llamafile): offered for removal, never deleted silently. */
  leftovers: Leftover[];
}

/** Something an older version left on disk that nothing uses any more. */
export interface Leftover {
  /** What `removeLeftover` takes (`pack-llamafile`). */
  id: string;
  /** `llamafile`. */
  label: string;
  sizeBytes: number;
}

/** One model's engine pin (ADR 0018). */
export interface ModelEnginePin {
  modelId: string;
  buildId: string;
  /** The pinned build's release tag; null when it is not installed any more. */
  tag: string | null;
}

/** Where a one-click roll back goes. */
export interface RollbackTarget {
  buildId: string;
  tag: string;
}

/** The engine pins, what roll back goes to, and why not when it cannot. */
export interface EnginePinState {
  /** The build pinned for all models. */
  global: string | null;
  models: ModelEnginePin[];
  /** The build that serves models in general now. */
  active: string | null;
  /** Why it was chosen, in words: `pinned for all models`, `chosen automatically`. */
  activeSource: string;
  rollbackTo: RollbackTarget | null;
  /** Why roll back is not possible, when it is not. */
  rollbackBlocked: string | null;
  /** Pins that cannot be used now (build uninstalled or failed its self-test), each with what to do. */
  problems: string[];
}

/** The result of changing a pin or rolling back. */
export interface PinChange {
  state: EnginePinState;
  /** Loaded models that keep their current engine: the change applies when they next load. */
  appliesOnNextLoad: string[];
  message: string;
}

export type CapabilityStatus = 'supported' | 'needsBuild' | 'notSupported';

/** Whether one build supports one capability. */
export interface BuildCapability {
  capability: Capability;
  label: string;
  status: CapabilityStatus;
  /** The llama.cpp release number that would serve it (`needsBuild`). */
  minBuild: number | null;
  /** What to do, or why not. */
  hint: string | null;
}

/** What an installed build can do. */
export interface BuildCapabilities {
  buildId: string;
  tag: string;
  capabilities: BuildCapability[];
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
  /** The largest footprint of the model that served the app's requests (RAM plus video memory, bytes). */
  peakMemoryBytes: number;
  /** Requests answered from the response or embeddings cache. */
  cacheHits: number;
  /** Cache-eligible requests that were not found in the cache. */
  cacheMisses: number;
  /** Prompt tokens the engine reused from its prompt cache. */
  prefixTokens: number;
  /** Tokens cache hits saved the model from processing and generating. */
  savedTokens: number;
}

export interface AppLimits {
  /** null: no limit. */
  requestsPerMin: number | null;
  tokensPerDay: number | null;
  maxConcurrent: number | null;
  /** Requests per UTC day. */
  requestsPerDay: number | null;
  /** Tokens per UTC calendar month. */
  tokensPerMonth: number | null;
  /** The most of the machine's RAM (1 to 100 percent) the model serving one request may occupy. */
  maxMemoryPercent: number | null;
}

export type UsageRange = '24h' | '7d' | '30d';

export interface AppUsageSeries {
  /** The app, or `other` for the apps beyond the busiest eight. */
  appId: string;
  /** One entry per bucket of the series. */
  points: UsageCounters[];
  totals: UsageCounters;
}

/** Usage over a range: hourly buckets for 24h and 7d, daily for 30d; every bucket is present (zero when idle). */
export interface UsageSeries {
  range: UsageRange;
  stepMs: number;
  /** The start of every bucket, oldest first. */
  starts: number[];
  total: UsageCounters[];
  totals: UsageCounters;
  /** Busiest first. */
  apps: AppUsageSeries[];
}

export type LimitKind = 'requestsPerMin' | 'requestsPerDay' | 'tokensPerDay' | 'tokensPerMonth' | 'memoryShare';
export type LimitState = 'ok' | 'warning' | 'exceeded';

/** One configured limit and how much of it is used. `used` and `limit` are requests, tokens or bytes by kind. */
export interface LimitStatus {
  kind: LimitKind;
  used: number;
  limit: number;
  percent: number;
  state: LimitState;
  /** An explanation for a warning or an exceeded limit. */
  message: string | null;
}

export interface AppPermissions {
  /** `null`: every model. */
  models: string[] | null;
  maxPriority: Priority;
  tools: boolean;
  extendedApis: boolean;
  /** Identical deterministic requests may be answered from the response cache (off until turned on; ADR 0019). */
  cacheResponses: boolean;
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
  /** Debrief, Pitcrew, Spotter, Dyno are `suite: true`. */
  suite: boolean;
  connected: boolean;
  lastSeenAt: number | null;
  keys: ApiKeyInfo[];
  permissions: AppPermissions;
  today: UsageCounters;
}

export interface AppDetail {
  app: AppInfo;
  /** This UTC month so far. */
  month: UsageCounters;
  limits: LimitStatus[];
  /** Latest requests, newest first; metadata only. */
  recent: RequestRecord[];
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
  /** A re-pair (ADR 0057 §4): approving it revokes the keys the app had before. */
  rePair: boolean;
  /** For a re-pair of an app Pitwall has: that app as stored and how many keys approving revokes. */
  replaces: ReplacedApp | null;
}

/** The app a re-pair replaces the keys of. */
export interface ReplacedApp {
  appId: string;
  /** Its name as Pitwall stored it, not the requester's. */
  appName: string;
  keys: number;
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
  /** Models whose engine crashed and is being restarted or was given up on. */
  issues: EngineIssue[];
  /** Pitwall installs llama.cpp by itself when no build is installed. */
  engineSetup: EngineSetup;
}

export type EngineSetupState = 'idle' | 'installing' | 'failed';

/** `EngineSetup` in `pitwall_core::mgmt`: the automatic llama.cpp install. */
export interface EngineSetup {
  state: EngineSetupState;
  /** Why it failed, with the next step (failed). */
  error: string | null;
  /** The build's download, for progress (installing). */
  download: Download | null;
}

export type IssueState = 'restarting' | 'failed';

/** `EngineIssue` in `pitwall_core::mgmt`. */
export interface EngineIssue {
  modelId: string;
  state: IssueState;
  crashes: number;
  /** Unix ms of the next restart while one is planned. */
  restartAt: number | null;
  message: string;
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
  | 'requestFailed'
  | 'limitWarning'
  | 'modelUpdate';

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
  | { kind: 'issues'; issues: EngineIssue[] }
  | { kind: 'throughput'; throughput: Throughput }
  | { kind: 'event'; event: PitwallEvent }
  | { kind: 'download'; download: Download }
  | { kind: 'engineSetup'; setup: EngineSetup }
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
  /** Parallel slots it loads with: its own choice or the default. */
  slots: number;
  /** The slots are this model's own choice (false: the default from the settings). */
  slotsCustom: boolean;
  /** The slots it is loaded with now (null: not loaded). A different `slots` applies on the next load. */
  loadedSlots: number | null;
}

/** One row of the slots fit table: what `slots` parallel slots mean at the asked context per slot. */
export interface SlotChoice {
  slots: number;
  verdict: FitVerdict;
  /** Memory the model needs with this many slots. */
  needBytes: number;
  /** What this adds over a single slot. */
  extraBytes: number;
  /** Largest context per slot that fits ("tight" counts). */
  maxContext: number;
  /** Largest context per slot that fits with room to spare. */
  comfortableContext: number;
}

/** The engine's answer to "how much context or concurrency fits" for a model. */
export interface SlotFit {
  /** Context per slot asked about. */
  context: number;
  /** The slots chosen. */
  slots: number;
  verdict: FitVerdict;
  /** Memory independent of the slots (weights, projector, compute buffers). */
  baseBytes: number;
  /** What each slot adds at this context. */
  perSlotBytes: number;
  /** The KV size comes from the model's header (false: a typical-model estimate). */
  kvExact: boolean;
  /** The most slots that fit at this context (0: not even one). */
  maxSlots: number;
  /** The most slots that fit with room to spare. */
  comfortableSlots: number;
  choices: SlotChoice[];
  message: string;
}

/** The result of changing a model's slots. */
export interface SlotsChange {
  slots: number;
  loadedSlots: number | null;
  /** It is loaded with a different count: the change applies the next time it loads. */
  appliesOnNextLoad: boolean;
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
  /** The most parallel slots that fit at the context the fit was checked at (0: not even one). */
  maxSlots: number;
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
  /** The preset (alias) the app asked for, when its `model` field named one; `modelId` is what it resolved to. */
  alias: string | null;
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
  /** What the response cache did; `null` when the request was not eligible for it. */
  cache: CacheOutcome | null;
  /** Prompt tokens the engine reused from its prompt cache (the prefix of an earlier request). */
  cachedTokens: number;
}

// ---------------------------------------------------------------------------------------------------------------
// Model presets and aliases (ADR 0014)

/** A saved preset: `alias` is what apps send as `model`. */
export interface ModelPreset {
  alias: string;
  /** An installed model id, or a capability selection such as `auto:chat,fast`. */
  target: string;
  /** Context the model loads with for this preset. */
  context: number | null;
  temperature: number | null;
  topP: number | null;
  maxTokens: number | null;
  /** Added in front only when the request has no system message of its own. */
  systemPrompt: string | null;
  /** Answer identical deterministic requests from the response cache (ADR 0019). */
  cacheResponses: boolean;
  /** Keep the system prompt's prefix warm: saved when the model unloads, restored on load. */
  keepWarm: boolean;
}

/** What the editor sends: the same fields, parsed and validated by the engine. */
export type PresetDraft = ModelPreset;

/** Where a preset stands now (the engine's answer). */
export type PresetStatus =
  | { state: 'ready'; modelId: string; chosenBy: ChosenBy; reason: string; loaded: boolean }
  | { state: 'stale'; message: string }
  | { state: 'noMatch'; message: string }
  | { state: 'shadowed'; message: string };

export interface PresetEntry {
  preset: ModelPreset;
  status: PresetStatus;
}

/** The editor's live check: what is wrong with the draft, and what its target resolves to now. */
export interface PresetPreview {
  problem: string | null;
  status: PresetStatus | null;
}

export interface RequestContent {
  id: string;
  request: string;
  response: string;
}

// ---------------------------------------------------------------------------------------------------------------
// Playground and benchmarks

export interface PlaygroundToolCall {
  id: string;
  name: string;
  /** The arguments as JSON text. */
  arguments: string;
}

export interface PlaygroundMessage {
  role: 'system' | 'user' | 'assistant' | 'tool';
  content: string;
  /** The tool calls of an assistant message. */
  toolCalls?: PlaygroundToolCall[];
  /** The call a `tool` message answers. */
  toolCallId?: string;
}

export interface PlaygroundTool {
  name: string;
  description: string;
  /** A JSON schema object for the arguments. */
  parameters: Record<string, unknown>;
}

export type PlaygroundToolChoice = { kind: 'auto' } | { kind: 'none' } | { kind: 'required' } | { kind: 'named'; name: string };

export interface PlaygroundRequest {
  modelId: string;
  messages: PlaygroundMessage[];
  temperature: number;
  maxTokens: number;
  /** The tools the model may call; none is a plain chat. */
  tools?: PlaygroundTool[];
  /** Defaults to `{ kind: 'auto' }`. */
  toolChoice?: PlaygroundToolChoice;
}

/** What happens before the first token: waiting for a slot and loading (repeated about once a second), then generating. */
export type PlaygroundPhase = { kind: 'loading'; elapsedMs: number } | { kind: 'generating' };

export type PlaygroundFinish = 'stop' | 'length' | 'toolCalls' | 'cancelled';

export type PlaygroundEvent =
  | { kind: 'phase'; phase: PlaygroundPhase }
  | { kind: 'delta'; text: string }
  | { kind: 'reasoning'; text: string }
  /** A piece of a tool call; the pieces with one `index` make one call. */
  | { kind: 'toolCall'; index: number; id: string | null; name: string | null; arguments: string }
  | { kind: 'done'; finish: PlaygroundFinish; promptTokens: number; completionTokens: number; ttftMs: number; genTps: number; coldStartMs: number | null }
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
  lanEnabled: boolean;
  lanPort: number;
}

export interface PrivacySettings {
  /** Keep request and response text for the inspector (off by default; kept on this machine only). */
  keepRequestContent: boolean;
  metricsRetentionDays: number;
}

/** What quitting Pitwall does about serving. */
export type OnQuit = 'keepServing' | 'stopServing' | 'ask';

export interface GeneralSettings {
  theme: ThemeChoice;
  onQuit: OnQuit;
}

export interface Settings {
  general: GeneralSettings;
  server: ServerSettings;
  memory: MemorySettings;
  network: NetworkSettings;
  privacy: PrivacySettings;
  cache: CacheSettings;
}

// ---------------------------------------------------------------------------------------------------------------
// Caching (ADR 0019)

export type CacheStorage = 'memory' | 'disk';
export type CacheOutcome = 'hit' | 'miss' | 'bypass';

export interface CacheSettings {
  /** Send a conversation's requests to the same llama-server slot so its prompt cache hits. */
  prefixAffinity: boolean;
  /** Host-memory prompt cache per model in MiB; `null` sizes it from this machine's memory. */
  promptCacheMib: number | null;
  /** Cache embeddings (on by default; they are deterministic). */
  embeddings: boolean;
  embeddingsStorage: CacheStorage;
  embeddingsMaxMib: number;
  /** Seconds since last use (sliding); 0: never. Default 90 days. */
  embeddingsTtlSecs: number;
  responsesStorage: CacheStorage;
  responsesMaxMib: number;
  /** Seconds since stored; 0: never. Default 7 days. */
  responsesTtlSecs: number;
  /** Days a saved prompt prefix (keep warm) is kept unused. Default 30. */
  keepWarmDays: number;
}

export interface CacheKindInfo {
  storage: CacheStorage;
  entries: number;
  bytes: number;
  maxBytes: number;
  hits: number;
  oldestAt: number | null;
  /** `null`: never. */
  expiresAfterSecs: number | null;
  /** Counted from the last use instead of from when stored. */
  sliding: boolean;
  /** Why the disk cache is not in use (it then behaves as memory only). */
  diskError: string | null;
}

export interface CacheInfo {
  responses: CacheKindInfo;
  embeddings: CacheKindInfo;
  keepWarmFiles: number;
  keepWarmBytes: number;
  nextSweepAt: number;
}

export type CacheClearScope =
  | { kind: 'all' }
  | { kind: 'responses' }
  | { kind: 'embeddings' }
  | { kind: 'app'; appId: string }
  | { kind: 'model'; modelId: string };

export type Platform = 'windows' | 'macos' | 'linux';

/** The server's registration with the OS (`ServiceInstall` in `app/src-tauri/src/service_install.rs`). */
export interface ServiceInstall {
  platform: Platform;
  /** Registered with the OS. */
  installed: boolean;
  /** The server answers. */
  running: boolean;
  canInstall: boolean;
  /** The user has not been asked yet: show the "run Pitwall's service?" question. */
  askConsent: boolean;
  /** What to do next, when something is off. */
  message: string | null;
  /** macOS: the automatic re-registration of a service launchd refused, while it works and after. */
  repair: RepairNotice | null;
  /** macOS: the user switched Pitwall's background item off in System Settings > General > Login Items. */
  disabledInLoginItems: boolean;
  /** macOS: launchd has the service but cannot start it (typically after an update) and the server is down: offer Repair. */
  startFailed: boolean;
  /** macOS: launchd still has the job but Login Items does not list Pitwall: offer Repair. */
  notInLoginItems: boolean;
}

/** Progress of the background repair of the service's registration (macOS). */
export interface RepairNotice {
  state: 'working' | 'succeeded' | 'failed';
  /** Ready to show. */
  message: string;
}

// ---------------------------------------------------------------------------------------------------------------
// Model catalog packs (ADR 0011, ADR 0020)

/** A pack file that was skipped (problem) or loaded with a note (warning). */
export interface PackNote {
  file: string;
  message: string;
}

/** A catalog model and the variant that fits this machine best, with its verdict. */
export interface Recommendation {
  id: string;
  name: string;
  repo: string;
  file: string;
  quant: string | null;
  sizeBytes: number;
  tags: string[];
  fit: Fit;
  contextTokens: number;
}

export interface PacksInfo {
  problems: PackNote[];
  warnings: PackNote[];
  recommendations: Recommendation[];
}

// Model update notices (ADR 0017): newer revisions and better quantisations of the models you downloaded.

export type UpdateSchedule = 'weekly' | 'daily';

export interface UpdateCheckSettings {
  /** The scheduled check. Off until you turn it on. */
  enabled: boolean;
  schedule: UpdateSchedule;
}

export type UpdateKind = 'newerRevision' | 'betterQuant' | 'imatrix';

/** One offered update. Pitwall only offers; downloading it is the user's decision. */
export interface UpdateNotice {
  /** `<model id>#<kind>`. */
  id: string;
  modelId: string;
  repo: string;
  kind: UpdateKind;
  /** The file to download. */
  file: string;
  quant: string | null;
  sizeBytes: number;
  installedFile: string;
  installedQuant: string | null;
  /** One sentence for the user. */
  summary: string;
  foundAt: number;
  targetSha256: string | null;
}

export type UpdateCheckOutcome = 'checked' | 'rateLimited' | 'busy' | 'nothingToCheck' | 'failed';

export interface UpdateCheckResult {
  outcome: UpdateCheckOutcome;
  message: string;
  reposChecked: number;
  /** Notices shown now (not dismissed). */
  notices: number;
  /** Seconds until a new check is allowed. */
  retryInSecs: number | null;
}

export interface UpdateCheckStatus {
  settings: UpdateCheckSettings;
  lastCheckedAt: number | null;
  lastAttemptAt: number | null;
  lastMessage: string | null;
  /** Null when the schedule is off. */
  nextCheckAt: number | null;
  running: boolean;
  notices: number;
  /** The only host a check talks to. */
  host: string;
  maxRepos: number;
}

// ---------------------------------------------------------------------------------------------------------------
// Jobs (ADR 0024): long operations the app runs in the background; mirrors `app/src-tauri/src/jobs.rs` (`JobEvent`).

/** What a job's `done` and `total` count. */
export type JobUnit = 'items' | 'bytes';

/** One event of a job, as the app pushes it on the channel of the command that started the job. */
export type JobEvent =
  | { kind: 'started'; jobId: number; title: string }
  | {
      kind: 'progress';
      jobId: number;
      phase: string;
      done: number;
      total: number | null;
      unit: JobUnit;
      /** Cancel did not take effect: the UI may also offer "Stop waiting". */
      canStopWaiting: boolean;
      elapsedSecs: number;
      cancellable: boolean;
      notCancellableReason: string | null;
    }
  | { kind: 'done'; jobId: number; result: unknown }
  | { kind: 'failed'; jobId: number; error: CommandError }
  | { kind: 'cancelled'; jobId: number; stoppedWaiting: boolean };

/** One tool of a connected app, as Settings lists it (`chat_host::apps::AppTool`). */
export interface AppTool {
  name: string;
  /** Its name for people ("Top processes"). */
  title: string;
  /** It only looks: asked once, then remembered when allowed. Otherwise an action, asked every call. */
  readOnly: boolean;
  /** The user said Always allow for it. */
  remembered: boolean;
}

/** What only the user can do before an app connects (`chat_host::apps::AppNeed`). */
export type AppNeed = 'install' | 'update';

/** A family app the chat can use, in Settings > Connected apps (`chat_host::apps::ConnectedApp`, ADR 0023 Decision 7). */
export interface ConnectedApp {
  id: string;
  title: string;
  keySaved: boolean;
  connected: boolean;
  /** Why the last connect failed, with the next step. */
  problem: string | null;
  /** The app refused the saved key: only a new key helps (Refresh is hidden). */
  keyRefused: boolean;
  /** The connection was lost; Pitwall is trying again (the chat answers without the app's tools meanwhile). */
  reconnecting: boolean;
  /** What only the user can do before it connects (install it, update it); null otherwise. Pitwall does not retry meanwhile. */
  needs: AppNeed | null;
  tools: AppTool[];
  /** Tools always allowed. */
  remembered: number;
}

/** Settings > Chats (`chat_host::chats::ChatStorage`, ADR 0023 Decision 3). */
export interface ChatStorage {
  /** New chats are saved unless the user says "Don't save this chat". */
  saveNew: boolean;
  /** Saved chats. */
  chats: number;
  /** Their files, in bytes. */
  bytes: number;
  /** Their attachments, in bytes. */
  attachmentBytes: number;
  /** Chats kept until Pitwall closes ("Don't save"). */
  unsaved: number;
}

/** One model of the Chat tab's picker (`chat_host::models::ChatModel`). */
export interface ChatModel {
  id: string;
  name: string;
  loaded: boolean;
  /** Why it can't be chosen (no tool calls); null when it can. */
  disabled: string | null;
}

/** The Chat tab's picker: tool-capable models first, and the one the chat uses (`chat_host::models::ChatModels`). */
export interface ChatModels {
  models: ChatModel[];
  selected: string | null;
  /** For each reason a model can't be chosen ("Can't use tools", "Not downloaded"), its full note. */
  notes: Record<string, string>;
}

/** What "Open <app>" came to (`chat_host::apps::Opened`): starting, or the words saying why not. */
export type OpenedApp = { kind: 'started' } | { kind: 'problem'; message: string };
