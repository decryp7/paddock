import { InjectionToken } from '@angular/core';
import type {
  AppInfo,
  AppPermissions,
  AppUpdateStatus,
  BenchmarkResult,
  CreatedKey,
  DiskUsage,
  Download,
  Engine,
  History,
  HistoryRange,
  HfModel,
  HfRepo,
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
  Settings,
} from './api.types';

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
  events(limit: number): Promise<PitwallEvent[]>;
  /** Pushed updates (no polling). The listener gets every update until the returned function is called. */
  subscribe(listener: (update: LiveUpdate) => void): Unsubscribe;
  setServing(paused: boolean): Promise<void>;
  releaseMemory(): Promise<number>;
  applyPreset(preset: Preset): Promise<Settings>;
  cancelJob(id: string): Promise<void>;

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
  diskUsage(): Promise<DiskUsage>;
  /** Engine packs, pack problems and the catalog's recommendations for this machine, optionally for one tag. */
  packs(tag?: string | null): Promise<PacksInfo>;
  /** The engine for one model: an installed pack engine id, or null for llama.cpp. */
  setModelEngine(id: string, engine: string | null): Promise<void>;

  // Engines
  engines(): Promise<Engine[]>;
  installBuild(buildId: string): Promise<Download>;
  removeBuild(buildId: string, tag: string): Promise<void>;
  selectBuild(choice: string): Promise<void>;
  rollbackBuild(): Promise<void>;

  // Apps and keys
  apps(): Promise<AppInfo[]>;
  createApp(name: string): Promise<AppInfo>;
  deleteApp(id: string): Promise<void>;
  createKey(appId: string): Promise<CreatedKey>;
  revokeKey(appId: string, keyId: string): Promise<void>;
  setPermissions(appId: string, permissions: AppPermissions): Promise<AppInfo>;
  approvePairing(id: string, permissions: AppPermissions): Promise<void>;
  denyPairing(id: string): Promise<void>;
  pairings(): Promise<PairingRequest[]>;

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
  appUpdateStatus(): Promise<AppUpdateStatus>;
}

export const BACKEND = new InjectionToken<PitwallBackend>('PitwallBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
