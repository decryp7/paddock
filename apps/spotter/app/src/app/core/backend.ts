import { InjectionToken } from '@angular/core';
import type {
  Avatar,
  MailStatus,
  AppEvent,
  AppInfo,
  Dashboard,
  DashboardRange,
  OpenTarget,
  PatternSelection,
  PatternTest,
  DiscoveredFeed,
  ImportFormat,
  ImportResult,
  Preset,
  RendererStatus,
  SchemePreset,
  Schedule,
  SchedulePreview,
  Settings,
  SourceKind,
  SourcePreview,
  StatePush,
  VersionSchemeId,
  WatchDetail,
  WatchDraft,
  WatchSummary,
} from './api.types';

/**
 * Everything the UI asks the backend. Components never call Tauri directly: they get this through DI — the real
 * Tauri backend in the app (`tauri-backend.ts`, commands in `app/src-tauri/src/commands.rs`) and the mock in the
 * browser build and tests. Every method rejects with a `CommandError`.
 */
/** Which release of a watch with products, and which of its files, a command acts on. */
export interface ReleaseTarget {
  /** The product (`id` group); null for a watch of one thing. */
  product?: string | null;
  /** A file name from the release's artifacts; null for the primary file. */
  artifact?: string | null;
}

export interface SpotterBackend {
  appInfo(): Promise<AppInfo>;

  // Watches
  listWatches(): Promise<WatchSummary[]>;
  getWatch(id: string): Promise<WatchDetail>;
  sourceKinds(): Promise<SourceKind[]>;
  /**
   * Checks a draft without saving it. `secrets` are values typed into secret fields (not stored). A shell job: the
   * Jobs list shows its phases and Cancel stops it; there is no time limit.
   */
  testSource(draft: WatchDraft, secrets: Record<string, string>): Promise<SourcePreview>;
  /** Saves a draft. `secrets`: a value stores it in the credential store, an empty string deletes it, a missing key keeps it. */
  saveWatch(draft: WatchDraft, secrets: Record<string, string>): Promise<WatchSummary>;
  deleteWatch(id: string): Promise<void>;
  setPaused(id: string, paused: boolean): Promise<void>;
  /**
   * Checks one watch now (resolves when it is done, however long that takes), or every watch when `id` is null
   * (resolves at once; progress "n of m" is in the Jobs list). A shell job.
   */
  checkNow(id: string | null): Promise<void>;
  /** Marks a version (the latest when null) as seen. */
  acknowledge(id: string, version: string | null): Promise<void>;
  resetAcknowledgement(id: string): Promise<void>;
  setInstalled(id: string, version: string | null): Promise<void>;
  /** Marks the latest release of one product of a watch as seen. */
  acknowledgeProduct(id: string, product: string): Promise<void>;
  /** Opens a release's location with the system's handler (a chosen file of the release, or the primary one). */
  openRelease(id: string, version: string, target?: ReleaseTarget): Promise<void>;
  /**
   * Downloads (or copies) a release into the downloads folder, verifying its checksum; resolves with the file path.
   * A shell job (phases, bytes, Cancel in the Jobs list); rejects with code `cancelled` when stopped.
   */
  downloadRelease(id: string, version: string, target?: ReleaseTarget): Promise<string>;
  /** Writes a regular expression from an example name and the parts the user selected (computed in Rust). */
  suggestPattern(example: string, selections: PatternSelection[]): Promise<string>;
  /** Tries a version scheme on one name (computed in Rust). */
  testPattern(versions: VersionSchemeId, versionPattern: string | null, missingOlder: boolean, text: string): Promise<PatternTest>;
  /** Describes a schedule and lists its next runs and warnings; a query, nothing is saved. Rejects with what to fix. */
  previewSchedule(schedule: Schedule, sourceKind: string): Promise<SchedulePreview>;
  /** The patterns the helper offers to start from. */
  versionSchemes(): Promise<SchemePreset[]>;

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

  // Presets, discovery, rendering, imports
  presets(): Promise<Preset[]>;
  /** Finds feeds a page advertises (blog and release feeds). */
  discoverFeeds(url: string): Promise<DiscoveredFeed[]>;
  rendererStatus(): Promise<RendererStatus>;
  /** Downloads the pinned, checksum-verified Chromium (on request only). A shell job: Downloading, Unpacking, Testing. */
  installRenderer(): Promise<RendererStatus>;
  /** Builds drafts from a winget export, Brewfile or OPML text. */
  importList(format: ImportFormat, text: string): Promise<ImportResult>;

  // Avatars and e-mail
  /** The picture of a watch: from its site (cached, offline-safe), else the bundled mark for its kind, else null (monogram). */
  watchAvatar(id: string): Promise<Avatar | null>;
  /** The picture of a catalogue preset. */
  presetAvatar(id: string): Promise<Avatar | null>;
  /** The mark bundled for a source kind (no network). */
  kindAvatar(kind: string): Promise<Avatar | null>;
  mailStatus(): Promise<MailStatus>;
  /** Stores the mail account's password in the credential store (null removes it). */
  setMailPassword(password: string | null): Promise<MailStatus>;
  /** Sends a test message through the saved mail settings; rejects with what to check. */
  sendTestEmail(to: string): Promise<void>;

  /** Subscribes to pushed state; returns the unsubscribe function. */
  onState(handler: (push: StatePush) => void): Promise<() => void>;

  // Tray popup (added by the UI; the popup is the window that loads `#/tray`)
  /** Shows and focuses the main window, then asks it to show `route` (`/watches/<id>`; null keeps the current page). */
  showMainWindow(route: string | null): Promise<void>;
  /** Hides the tray popup. */
  hideTray(): Promise<void>;
  /** Fits the tray popup's window to its content: `height` in CSS pixels as the page measured it (the shell clamps it). */
  fitTray(height: number): Promise<void>;
  /** The page the tray or a toast asked for before this window could listen (taken once). */
  pendingRoute(): Promise<string | null>;
  /** The main window is asked to show a route (Tauri event `spotter://navigate`, payload: the route string). */
  onNavigate(handler: (route: string) => void): Promise<() => void>;
}

export const BACKEND = new InjectionToken<SpotterBackend>('SpotterBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
