import { InjectionToken } from '@angular/core';
import type {
  ActivityReply,
  CacheCleaned,
  CacheInfo,
  CacheLimits,
  CachePlan,
  ChannelSettings,
  DebriefStatus,
  EngineReport,
  Handoff,
  InsightsReply,
  PruneResult,
  PrunePreview,
  PruneSelection,
  BackendEvent,
  Config,
  CleanupReport,
  ConfigEdit,
  ConfigPreview,
  ConfigSaved,
  DiskReply,
  KeepFor,
  ProcessList,
  RunnerConfigReply,
  LogLine,
  MetricsReply,
  RegisterRequest,
  RunDetails,
  RunnerConfig,
  ServiceInstall,
  SettingsReply,
  StatusReply,
  Version,
} from './api.types';

/** The platform the app runs on (`browser` for the browser build with the mock). */
export type Platform = 'windows' | 'macos' | 'linux' | 'browser';

/**
 * Everything the UI asks the backend. Components never call Tauri directly: they get this through DI, the real Tauri
 * backend in the app and the mock in the browser build and tests. Every method rejects with a `CommandError`.
 */
export interface PitcrewBackend {
  /** The platform (window chrome, wording). */
  platform(): Platform;

  // Status and push events.
  /** Every runner's status and the service's. */
  status(): Promise<StatusReply>;
  /** Subscribes to pushed events (status changes, notifications, the connection); returns the unsubscribe. */
  watch(listener: (event: BackendEvent) => void): () => void;

  /** Subscribes to requests from the shell to show a page (tray actions, `pitcrew://navigate`); returns the unsubscribe. */
  navigations(listener: (route: string) => void): () => void;
  /** Asks the main window to show a screen (`/updates`): a window with no pages of its own, like Settings, has no router. */
  openPage(route: string): Promise<void>;
  /** The page the shell asked for before this window listened, once (`null` when there is none). */
  takePendingNavigation(): Promise<string | null>;

  // Runner actions (the service waits for the current job where needed).
  start(runner: string): Promise<void>;
  stop(runner: string): Promise<void>;
  restart(runner: string): Promise<void>;
  drain(runner: string): Promise<void>;
  resume(runner: string): Promise<void>;

  // Updates.
  checkUpdate(runner: string): Promise<void>;
  applyUpdate(runner: string): Promise<void>;
  cancelUpdate(runner: string): Promise<void>;
  skipVersion(runner: string, version: Version): Promise<void>;
  /** `null` follows the latest release again. */
  pinVersion(runner: string, version: Version | null): Promise<void>;

  // Logs (paged; search is plain text).
  logs(runner: string, after: number | null, limit: number): Promise<LogLine[]>;
  searchLogs(runner: string, query: string, limit: number): Promise<LogLine[]>;

  // Activity, metrics and disk.
  activity(runner: string, days: number): Promise<ActivityReply>;
  metrics(runner: string | null, rangeHours: number): Promise<MetricsReply>;
  disk(runner: string): Promise<DiskReply>;
  cleanDisk(runner: string): Promise<CleanupReport>;
  /** The current job's details from the Gitea API (`null` without a read token or when the server cannot tell). */
  runDetails(runner: string): Promise<RunDetails | null>;

  // The runner's own config.yaml (ADR 0011): read, preview (writes nothing) and save (backs up, restarts after the job).
  runnerConfig(runner: string): Promise<RunnerConfigReply>;
  previewRunnerConfig(runner: string, edit: ConfigEdit): Promise<ConfigPreview>;
  /** `base` is the fingerprint from `runnerConfig`; the save is refused when the file changed since. */
  saveRunnerConfig(runner: string, edit: ConfigEdit, base: string): Promise<ConfigSaved>;

  // The runner's process tree and the leftovers of finished jobs (ADR 0012).
  runnerProcesses(runner: string): Promise<ProcessList>;
  /** Asks the process to stop, then ends it. `allowRunner` is true only when the user chose the runner process itself. */
  stopProcess(runner: string, pid: number, startedAt: number | null, allowRunner: boolean): Promise<void>;
  /** Adds time before the automatic cleanup stops a leftover. */
  keepProcess(runner: string, pid: number, startedAt: number, extra: KeepFor): Promise<void>;

  // Workflow insights (ADR 0015): the Gitea API with the read token, cached and bounded; the engine aggregates.
  insights(days: number): Promise<InsightsReply>;

  // Container engine hygiene and the Actions cache (ADR 0016). Previews delete nothing; prunes are refused while a job runs.
  engineReport(): Promise<EngineReport>;
  previewEnginePrune(selection: PruneSelection): Promise<PrunePreview>;
  pruneEngine(selection: PruneSelection): Promise<PruneResult>;
  cacheReport(runner: string): Promise<CacheInfo>;
  previewCacheClean(runner: string, limits: CacheLimits): Promise<CachePlan>;
  cleanCache(runner: string, limits: CacheLimits): Promise<CacheCleaned>;

  // Debrief, the owner's log-analysis app (ADR 0018). `startedAt`/`finishedAt` cut one job out of the runner's log.
  debriefStatus(): Promise<DebriefStatus>;
  openInDebrief(runner: string, label: string, startedAt: number | null, finishedAt: number | null): Promise<Handoff>;

  // Notification channels (ADR 0017). Secrets live in the OS credential store and never come back to the UI.
  channels(): Promise<ChannelSettings>;
  saveChannels(settings: ChannelSettings): Promise<void>;
  setChannelSecret(key: string, value: string): Promise<void>;
  clearChannelSecret(key: string): Promise<void>;
  hasChannelSecret(key: string): Promise<boolean>;
  /** Sends a test message through a saved channel; resolves with a sentence for the user. */
  testChannel(id: string): Promise<string>;

  // Settings and runners.
  settings(): Promise<SettingsReply>;
  saveSettings(config: Config): Promise<void>;
  addRunner(runner: RunnerConfig): Promise<void>;
  removeRunner(runner: string): Promise<void>;
  register(request: RegisterRequest): Promise<void>;

  // Gitea read token (OS credential store), per server URL.
  hasApiToken(serverUrl: string): Promise<boolean>;
  setApiToken(serverUrl: string, token: string): Promise<void>;
  clearApiToken(serverUrl: string): Promise<void>;

  // The service's registration with the OS.
  serviceInstall(): Promise<ServiceInstall>;
  /** Asks the OS for permission (UAC / Login Items) and installs and starts the service. */
  installService(): Promise<void>;
  uninstallService(): Promise<void>;
  /**
   * macOS: replaces the launchd job an update left behind (unregister, bootout, wait, register) in the background; the
   * progress shows in `serviceInstall().repair`. The app's own business: it works while the service is down.
   */
  repairService(): Promise<void>;
  /** macOS: opens System Settings ▸ General ▸ Login Items. Pitcrew never switches the item itself. */
  openLoginItems(): Promise<void>;

  // The app itself (updates, About and What's New are the shell's).
  autostart(): Promise<boolean>;
  setAutostart(enabled: boolean): Promise<void>;
  /** Writes the redacted diagnostics bundle and returns its path. */
  exportDiagnostics(): Promise<string>;
  /** Opens an http(s) link in the default browser (validated by the backend). */
  openUrl(url: string): Promise<void>;
  /** Opens the runner's log folder in the file manager. */
  revealLogs(runner: string): Promise<void>;
}

export const BACKEND = new InjectionToken<PitcrewBackend>('PitcrewBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
