import { InjectionToken } from '@angular/core';
import type {
  ChatModels,
  ConnectedApp,
  AppInfo,
  CacheClearScope,
  CacheInfo,
  AppPermissions,
  BenchmarkResult,
  BuildCapabilities,
  EnginePinState,
  PinChange,
  CreatedKey,
  DiskUsage,
  SlotFit,
  SlotsChange,
  Download,
  Engine,
  EngineSetup,
  AppDetail,
  ModelPreset,
  PresetDraft,
  PresetEntry,
  PresetPreview,
  History,
  HistoryRange,
  UsageRange,
  UsageSeries,
  HfModel,
  HfRepo,
  JobEvent,
  LiveUpdate,
  LocalModel,
  Overview,
  PacksInfo,
  PairingRequest,
  PitwallEvent,
  PlaygroundEvent,
  PlaygroundRequest,
  Preset,
  RequestContent,
  RequestRecord,
  ServiceInstall,
  Settings,
  UpdateCheckResult,
  UpdateCheckStatus,
  UpdateNotice,
  UpdateSchedule,
} from './api.types';
import type { LoadingRows, PopoverHeader } from './generated/widgets';
import type { PopoverEdit, PopoverPush, PopoverValues, PopoverView } from './widgets.types';

/** Stops a subscription or a running stream. */
export type Unsubscribe = () => void;

/**
 * Everything the UI asks the backend. Components never call Tauri directly: they get this through DI, the real Tauri
 * backend in the app and the mock in the browser build and tests. Each method maps to one Tauri command
 * (`app/src-tauri/src/commands.rs`), which forwards one management call of the server (`/pw/v1/...`).
 */
export interface PitwallBackend {
  // Dashboard
  overview(): Promise<Overview>;
  history(range: HistoryRange): Promise<History>;
  /** Requests, tokens, errors and peak memory per app over time; one app, or `null` for all. */
  usageSeries(range: UsageRange, appId: string | null): Promise<UsageSeries>;
  events(limit: number): Promise<PitwallEvent[]>;
  /** Pushed updates (no polling). The listener gets every update until the returned function is called. */
  subscribe(listener: (update: LiveUpdate) => void): Unsubscribe;
  setServing(paused: boolean): Promise<void>;
  releaseMemory(): Promise<number>;

  // The tray popup's widgets (family ADR 0035): the layout, the values and the header are the app's, computed in Rust.
  /** The popup's layout and catalogue as the chooser draws them. */
  popoverView(): Promise<PopoverView>;
  /** What the popup's widgets show now. */
  popoverValues(): Promise<PopoverValues>;
  /** The status pill and which footer items work now. */
  popoverHeader(): Promise<PopoverHeader>;
  /** How many rows each widget draws, so a loading twin has the height of a ready widget. */
  popoverShape(): Promise<LoadingRows[]>;
  /** One edit of the layout; the answer is the new layout. */
  editPopover(edit: PopoverEdit): Promise<PopoverView>;
  /** Pushed after any change of the layout, the values or the header (no polling). */
  onPopover(handler: (push: PopoverPush) => void): Unsubscribe;
  /** Entries, size, hits and clean-up time of the response and embeddings caches (ADR 0019). */
  cacheInfo(): Promise<CacheInfo>;
  /** Empties caches; resolves to the number of entries removed. */
  clearCache(scope: CacheClearScope): Promise<number>;
  applyPreset(preset: Preset): Promise<Settings>;
  /** Cancels a queued or running inference request of the server's queue (the dashboard's queue card; string id). Not for background jobs: see {@link jobCancel}. */
  cancelJob(id: string): Promise<void>;
  /**
   * Events of every background job (ADR 0024) until the returned function is called. The methods in {@link JOB_METHODS}
   * run as jobs: they never time out in the UI (ER-19), they report phase and progress here, and Cancel works in every phase.
   */
  jobEvents(listener: (event: JobEvent) => void): Unsubscribe;
  /** Cancels a background job of the app ({@link JOB_METHODS}; numeric id). Not for queue requests: see {@link cancelJob}. False when it is unknown, finished or in a step that cannot be interrupted. */
  jobCancel(jobId: number): Promise<boolean>;
  /** Stops waiting for a job whose Cancel did not take effect (the server may still be working); false when not offered. */
  jobStopWaiting(jobId: number): Promise<boolean>;

  // Models
  localModels(): Promise<LocalModel[]>;
  searchModels(query: string): Promise<HfModel[]>;
  repoFiles(repo: string): Promise<HfRepo>;
  downloadModel(repo: string, file: string, projector: string | null): Promise<Download>;
  downloads(): Promise<Download[]>;
  cancelDownload(id: string): Promise<void>;
  deleteModel(id: string): Promise<void>;
  loadModel(id: string): Promise<void>;
  unloadModel(id: string): Promise<void>;
  pinModel(id: string, pinned: boolean): Promise<void>;
  setModelIdle(id: string, idleSecs: number | null): Promise<void>;
  /** Parallel slots for one model (1 to 16; null: the default). Applies on the model's next load. */
  setModelSlots(id: string, slots: number | null): Promise<SlotsChange>;
  /** How much context and concurrency fit for a model; unset arguments use its current context and slots. */
  modelFit(id: string, context: number | null, slots: number | null): Promise<SlotFit>;
  diskUsage(): Promise<DiskUsage>;
  /** Catalog pack problems and the catalog's recommendations for this machine, optionally for one tag. */
  packs(tag?: string | null): Promise<PacksInfo>;

  // Engines
  engines(): Promise<Engine[]>;
  installBuild(buildId: string): Promise<Download>;
  /** Removes something an older version left on disk (the llamafile download); only on the user's request. */
  removeLeftover(id: string): Promise<void>;
  /** Tries the automatic llama.cpp install again after it failed. */
  retryEngineSetup(): Promise<EngineSetup>;
  removeBuild(buildId: string, tag: string): Promise<void>;
  selectBuild(choice: string): Promise<void>;
  /** Goes back to the previous good build and pins it for all models; says which loaded models switch on their next load. */
  rollbackBuild(): Promise<PinChange>;
  /** The engine pins, what roll back goes to, and why not when it cannot. */
  enginePins(): Promise<EnginePinState>;
  /** Pins a build for all models (null: unpins). */
  setGlobalPin(buildId: string | null): Promise<PinChange>;
  /** Pins a build for one model (null: unpins). */
  setModelPin(modelId: string, buildId: string | null): Promise<PinChange>;
  /** What each installed build supports (vision, tools, embeddings, rerank, JSON-schema output). */
  buildCapabilities(): Promise<BuildCapabilities[]>;

  // Apps and keys
  apps(): Promise<AppInfo[]>;
  appDetail(appId: string): Promise<AppDetail>;
  createApp(name: string): Promise<AppInfo>;
  deleteApp(id: string): Promise<void>;
  createKey(appId: string): Promise<CreatedKey>;
  revokeKey(appId: string, keyId: string): Promise<void>;
  setPermissions(appId: string, permissions: AppPermissions): Promise<AppInfo>;
  approvePairing(id: string, permissions: AppPermissions): Promise<void>;
  denyPairing(id: string): Promise<void>;
  pairings(): Promise<PairingRequest[]>;

  // Model presets and aliases
  presets(): Promise<PresetEntry[]>;
  createPreset(preset: PresetDraft): Promise<ModelPreset>;
  /** Replaces the preset `alias` (the draft's alias may rename it). */
  updatePreset(alias: string, preset: PresetDraft): Promise<ModelPreset>;
  deletePreset(alias: string): Promise<void>;
  /** The engine's live check of a draft; `editing` is the alias being edited (not a duplicate of itself). */
  resolvePreset(preset: PresetDraft, editing: string | null): Promise<PresetPreview>;

  // Model update notices (check only; downloading is the user's decision)
  updateCheckStatus(): Promise<UpdateCheckStatus>;
  updateNotices(): Promise<UpdateNotice[]>;
  /** Asks Hugging Face now about the repositories you downloaded from. */
  checkUpdatesNow(): Promise<UpdateCheckResult>;
  /** Turns the scheduled check on or off (off until you turn it on) and sets how often it runs. */
  setUpdateCheck(enabled: boolean, schedule: UpdateSchedule): Promise<UpdateCheckStatus>;
  /** Hides a notice until the offer changes, or for `snoozeDays` days. Returns the notices still shown. */
  dismissUpdate(id: string, snoozeDays: number | null): Promise<UpdateNotice[]>;

  // Request inspector
  requests(limit: number): Promise<RequestRecord[]>;
  /** Only on demand: prompts are hidden by default. */
  requestContent(id: string): Promise<RequestContent>;

  // Playground and benchmarks
  chat(request: PlaygroundRequest, onEvent: (event: PlaygroundEvent) => void): Unsubscribe;
  benchmarks(): Promise<BenchmarkResult[]>;
  runBenchmark(modelId: string): Promise<BenchmarkResult>;

  // Settings
  settings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;

  // The server as an OS service, and start at log-in (app commands, not management calls)
  serviceState(): Promise<ServiceInstall>;
  installService(): Promise<ServiceInstall>;
  uninstallService(): Promise<ServiceInstall>;
  /** macOS: replaces the registration launchd refuses (unregister, wait, register). Runs in the background; the state shows the progress. Does not need the server. */
  repairService(): Promise<ServiceInstall>;
  /** The user said no to the service; do not ask again. */
  declineService(): Promise<void>;
  /** macOS: opens System Settings > General > Login Items. Pitwall never switches the item itself. */
  openLoginItems(): Promise<void>;
  /** Quits the app now, stopping serving first when asked; `remember` saves the choice as the "When you quit" setting. */
  quitApp(stopServing: boolean, remember: boolean): Promise<void>;
  /** A quit asked for while no window was open is waiting for the dialog: true once. */
  takeQuitAsk(): Promise<boolean>;
  /** Whether this build has the Playground's chat (Rust decides: the `chat` feature, until ADR 0049's switches). */
  chatEnabled(): Promise<boolean>;
  /** Settings > Connected apps: the family apps the chat can use. */
  connectedApps(): Promise<ConnectedApp[]>;
  /** Keeps the key pasted for an app and connects with it. */
  setAppKey(appId: string, key: string): Promise<ConnectedApp>;
  /** Connects an app again and lists its tools (Refresh). */
  connectApp(appId: string): Promise<ConnectedApp>;
  /** Forgets an app: its connection, key and remembered approvals. */
  forgetApp(appId: string): Promise<ConnectedApp>;
  /** The Chat tab's model picker. */
  chatModels(): Promise<ChatModels>;
  /** Chooses the chat's model (one without tools is never chosen). */
  setChatModel(modelId: string): Promise<ChatModels>;
  /** The app asks the open window to show the "When you quit" dialog. */
  onQuitAsk(handler: () => void): Unsubscribe;
  autostart(): Promise<boolean>;
  setAutostart(enabled: boolean): Promise<boolean>;
}

/** The calls that run as background jobs: no timeout, live phase-labelled progress, Cancel (ER-17..19, ADR 0024). */
export const JOB_METHODS: ReadonlySet<keyof PitwallBackend> = new Set<keyof PitwallBackend>([
  'loadModel',
  'runBenchmark',
  'checkUpdatesNow',
  'installService',
  'uninstallService',
  'repairService',
]);

export const BACKEND = new InjectionToken<PitwallBackend>('PitwallBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
