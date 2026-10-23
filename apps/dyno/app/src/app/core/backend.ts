import { InjectionToken } from '@angular/core';
import type {
  ActivityEntry,
  AgentStatus,
  Alarm,
  AlarmChange,
  AlarmFilter,
  AlarmStats,
  Alert,
  BundlePreview,
  HostName,
  RecorderSnapshot,
  RecorderSnapshotInfo,
  SavedDiagnostics,
  ShelveFor,
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
  SystemInventory,
  ThrottleOverview,
  LifespanOverview,
  BaselineOverview,
  BenchComparison,
  NetworkQuery,
  NetworkSnapshot,
  BenchOverview,
  StartBenchmark,
  Timestamp,
  TuningSuggestion,
  WeeklyReport,
  ReportFormat,
  SavedReport,
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
  /** Alarm changes pushed by the agent (new, repeated, returned, acknowledged, shelved, muted, noted). */
  onAlarm(listener: (c: AlarmChange) => void): Unsubscribe;

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
  /** Puts the previous setting back; `force` overwrites a setting something else changed since Dyno applied it. */
  revertTuning(id: string, force?: boolean): Promise<ActivityEntry>;
  startupEntries(): Promise<StartupEntry[]>;
  setStartupState(id: string, state: StartupState): Promise<ActivityEntry>;

  // What is installed (memory modules, CPU, disks) and hints.
  inventory(): Promise<SystemInventory>;

  // Is the CPU throttled and why (WP-23), how long disks and batteries have left (WP-24), what is normal for now (WP-25).
  throttling(): Promise<ThrottleOverview>;
  lifespan(): Promise<LifespanOverview>;
  baselines(): Promise<BaselineOverview>;

  // Open ports and connections (observe-only), and "Look up IP" (the shell opens the browser; Dyno never fetches).
  network(query: NetworkQuery): Promise<NetworkSnapshot>;
  /** Opens the lookup page for a public address in the default browser; `template` tests one that is not saved yet. */
  lookupIp(ip: string, template?: string): Promise<void>;

  /** Reverse DNS through the OS resolver for up to 64 addresses; cached by the agent. Only on request, never in the background. */
  resolveHosts(addresses: string[]): Promise<HostName[]>;

  // Alarms (ISA-18.2 style): one open alarm per condition, acknowledge, shelve, mute, notes. Each change answers the changed alarms.
  alarms(filter: AlarmFilter): Promise<Alarm[]>;
  alarmStats(): Promise<AlarmStats>;
  acknowledgeAlarms(ids: number[]): Promise<Alarm[]>;
  shelveAlarm(id: number, until: ShelveFor): Promise<Alarm[]>;
  unshelveAlarm(id: number): Promise<Alarm[]>;
  muteRule(rule: string, muted: boolean): Promise<Alarm[]>;
  noteAlarm(id: number, text: string): Promise<Alarm[]>;
  /** The change log of one alarm, or of all alarms when `alarmId` is null, newest first (1..500). */
  alarmHistory(alarmId: number | null, limit: number): Promise<AlarmChange[]>;

  // Flight recorder: the last ten minutes at one-second resolution, saved on demand or when an alarm fires.
  capture(): Promise<RecorderSnapshotInfo>;
  recorderSnapshots(): Promise<RecorderSnapshotInfo[]>;
  recorderSnapshot(id: number): Promise<RecorderSnapshot>;

  // Diagnostics bundle: a redacted zip for a bug report. Nothing is written until the user picks a file.
  diagnosticsPreview(): Promise<BundlePreview>;
  /** Asks the user where to save (the OS save dialog) and writes the bundle there; resolves `null` when they cancel. */
  saveDiagnostics(): Promise<SavedDiagnostics | null>;

  // Benchmarks: start only on a click; poll `benchmarks()` while a run is in progress.
  benchmarks(): Promise<BenchOverview>;
  startBenchmark(request: StartBenchmark): Promise<BenchOverview>;
  cancelBenchmark(): Promise<void>;
  deleteBenchmark(id: string): Promise<void>;
  compareBenchmarks(before: string, after: string): Promise<BenchComparison>;

  // Reports, settings, plugins.
  /** The report for the seven days ending `weeksAgo` weeks before now (0 = the last seven days, at most 52). */
  weeklyReport(weeksAgo: number): Promise<WeeklyReport>;
  /** Asks where to save (the OS save dialog) and writes the week's report there; resolves `null` when they cancel. */
  saveReport(format: ReportFormat, weeksAgo: number): Promise<SavedReport | null>;
  /** The most recent alerts, newest first. */
  alerts(limit: number): Promise<Alert[]>;
  settings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;
  plugins(): Promise<PluginInfo[]>;

  // The tray dropdown window.
  /** Shows and focuses the main window (and hides the tray dropdown). */
  trayOpenMain(): Promise<void>;
  /** Tells the shell how tall the tray popover's content is (CSS pixels) so it can size the window to fit; no-op in the browser. */
  popoverResize(height: number): Promise<void>;
  /** Quits the Dyno app (the background agent keeps its own lifecycle). */
  trayQuit(): Promise<void>;
}

export const BACKEND = new InjectionToken<DynoBackend>('DynoBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
