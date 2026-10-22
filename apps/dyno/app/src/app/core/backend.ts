import { InjectionToken } from '@angular/core';
import type {
  ActivityEntry,
  AgentStatus,
  Alert,
  Anomaly,
  AppInfo,
  CleanupCategory,
  CleanupOutcome,
  CleanupScan,
  DeleteMode,
  Diagnosis,
  HealthReport,
  History,
  HistoryRange,
  LiveDetail,
  LiveUpdate,
  Locker,
  PluginInfo,
  ProcessAction,
  ProcessDetails,
  ProcessRef,
  ProfileId,
  QuarantineEntry,
  RuleSet,
  Settings,
  SizeNode,
  SnapshotDiff,
  SnapshotInfo,
  StartupEntry,
  StartupState,
  Timestamp,
  TuningSuggestion,
  WeeklyReport,
} from './api.types';

/** Stops a subscription. */
export type Unsubscribe = () => void;

/**
 * Everything the UI asks the engine. Components never call Tauri directly: they get this through DI, the real Tauri
 * backend in the app and the mock in the browser build and tests. Every method rejects with a `CommandError`.
 */
export interface DynoBackend {
  // Live data (pushed).
  /** Subscribes to live updates; `detail: 'processes'` adds the full process list (the task manager). */
  onLive(detail: LiveDetail, listener: (u: LiveUpdate) => void): Unsubscribe;
  onAlert(listener: (a: Alert) => void): Unsubscribe;
  onActivity(listener: (a: ActivityEntry) => void): Unsubscribe;
  onAgentStatus(listener: (s: AgentStatus) => void): Unsubscribe;

  appInfo(): Promise<AppInfo>;

  // Processes.
  processDetails(target: ProcessRef): Promise<ProcessDetails>;
  /** Acts on a process; resolves with the activity entry it logged. */
  act(target: ProcessRef, action: ProcessAction): Promise<ActivityEntry>;
  /** Acts on every process of an app group. */
  actOnGroup(groupId: string, action: ProcessAction): Promise<ActivityEntry>;
  lockers(path: string): Promise<Locker[]>;
  openLocation(target: ProcessRef): Promise<void>;
  takeSnapshot(label: string): Promise<SnapshotInfo>;
  snapshots(): Promise<SnapshotInfo[]>;
  diffSnapshots(before: string, after: string): Promise<SnapshotDiff>;

  // Diagnosis and history.
  whySlow(): Promise<Diagnosis>;
  anomalies(): Promise<Anomaly[]>;
  history(range: HistoryRange, end: Timestamp | null): Promise<History>;
  activity(limit: number): Promise<ActivityEntry[]>;
  undo(activityId: string): Promise<ActivityEntry>;
  /** Releases what a rule is doing to a process now (priority, efficiency mode, cap), without waiting for the rule. */
  releaseAction(target: ProcessRef): Promise<ActivityEntry>;

  // Rules.
  rules(): Promise<RuleSet>;
  saveRules(rules: RuleSet): Promise<RuleSet>;
  setProfile(profile: ProfileId): Promise<RuleSet>;
  pauseRules(paused: boolean): Promise<RuleSet>;

  // Cleanup.
  cleanupCategories(): Promise<CleanupCategory[]>;
  scanCleanup(categories: string[]): Promise<CleanupScan>;
  runCleanup(scanId: string, itemIds: string[], mode: DeleteMode): Promise<CleanupOutcome>;
  quarantine(): Promise<QuarantineEntry[]>;
  restoreQuarantine(id: string): Promise<void>;
  diskUsage(path: string | null): Promise<SizeNode>;

  // Health and tuning.
  health(): Promise<HealthReport>;
  tuning(): Promise<TuningSuggestion[]>;
  applyTuning(id: string): Promise<ActivityEntry>;
  revertTuning(id: string): Promise<ActivityEntry>;
  startupEntries(): Promise<StartupEntry[]>;
  setStartupState(id: string, state: StartupState): Promise<ActivityEntry>;

  // Reports, settings, plugins.
  weeklyReport(): Promise<WeeklyReport>;
  /** The most recent alerts, newest first. */
  alerts(limit: number): Promise<Alert[]>;
  settings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;
  plugins(): Promise<PluginInfo[]>;

  // The tray dropdown window.
  /** Shows and focuses the main window (and hides the tray dropdown). */
  trayOpenMain(): Promise<void>;
  /** Quits the Dyno app (the background agent keeps its own lifecycle). */
  trayQuit(): Promise<void>;
}

export const BACKEND = new InjectionToken<DynoBackend>('DynoBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
