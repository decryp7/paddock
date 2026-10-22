import { InjectionToken } from '@angular/core';
import type {
  AppEvent,
  AppInfo,
  AppUpdateCheck,
  AppUpdateStatus,
  Dashboard,
  DashboardRange,
  OpenTarget,
  PluginInfo,
  Settings,
  SourceKind,
  SourcePreview,
  StatePush,
  WatchDetail,
  WatchDraft,
  WatchSummary,
} from './api.types';

/**
 * Everything the UI asks the backend. Components never call Tauri directly: they get this through DI — the real
 * Tauri backend in the app (`tauri-backend.ts`, commands in `app/src-tauri/src/commands.rs`) and the mock in the
 * browser build and tests. Every method rejects with a `CommandError`.
 */
export interface SpotterBackend {
  appInfo(): Promise<AppInfo>;

  // Watches
  listWatches(): Promise<WatchSummary[]>;
  getWatch(id: string): Promise<WatchDetail>;
  sourceKinds(): Promise<SourceKind[]>;
  /** Checks a draft without saving it. `secrets` are values typed into secret fields (not stored). */
  testSource(draft: WatchDraft, secrets: Record<string, string>): Promise<SourcePreview>;
  /** Saves a draft. `secrets`: a value stores it in the credential store, an empty string deletes it, a missing key keeps it. */
  saveWatch(draft: WatchDraft, secrets: Record<string, string>): Promise<WatchSummary>;
  deleteWatch(id: string): Promise<void>;
  setPaused(id: string, paused: boolean): Promise<void>;
  /** Checks one watch now, or every watch when `id` is null. */
  checkNow(id: string | null): Promise<void>;
  /** Marks a version (the latest when null) as seen. */
  acknowledge(id: string, version: string | null): Promise<void>;
  resetAcknowledgement(id: string): Promise<void>;
  setInstalled(id: string, version: string | null): Promise<void>;
  /** Opens a release's location with the system's handler. */
  openRelease(id: string, version: string): Promise<void>;
  /** Downloads (or copies) a release into the downloads folder, verifying its checksum; resolves with the file path. */
  downloadRelease(id: string, version: string): Promise<string>;

  // Dashboard
  dashboard(range: DashboardRange): Promise<Dashboard>;
  /** Newest first; `beforeMs` pages back. */
  events(limit: number, beforeMs: number | null): Promise<AppEvent[]>;

  // Settings
  getSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;
  /** Pauses notifications for `minutes` (null resumes). */
  pauseNotifications(minutes: number | null): Promise<Settings>;
  getAutostart(): Promise<boolean>;
  setAutostart(enabled: boolean): Promise<boolean>;
  open(target: OpenTarget): Promise<void>;
  /** Native folder / program pickers (null when cancelled). */
  chooseFolder(): Promise<string | null>;
  chooseProgram(): Promise<string | null>;

  // Plugins
  plugins(): Promise<PluginInfo[]>;
  rescanPlugins(): Promise<PluginInfo[]>;
  setPluginEnabled(id: string, enabled: boolean): Promise<PluginInfo[]>;
  resetPlugin(id: string): Promise<PluginInfo[]>;

  // Spotter's own updates
  appUpdateStatus(): Promise<AppUpdateStatus>;
  checkAppUpdate(): Promise<AppUpdateCheck>;
  installAppUpdate(): Promise<void>;

  /** Subscribes to pushed state; returns the unsubscribe function. */
  onState(handler: (push: StatePush) => void): Promise<() => void>;
}

export const BACKEND = new InjectionToken<SpotterBackend>('SpotterBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
