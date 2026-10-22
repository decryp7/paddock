import { InjectionToken } from '@angular/core';
import type {
  ActivityReply,
  AppUpdateInfo,
  AppUpdateStatus,
  BackendEvent,
  Config,
  CleanupReport,
  DiskReply,
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

  // The app itself.
  appUpdateStatus(): Promise<AppUpdateStatus>;
  checkAppUpdate(): Promise<AppUpdateInfo | null>;
  /** Downloads, verifies and installs the update, then restarts Pitcrew. */
  installAppUpdate(): Promise<void>;
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
