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
  CounterHistory,
  CounterHit,
  CounterObject,
  CounterObjectList,
  CounterReading,
  NetworkQuery,
  NetworkSnapshot,
  BenchOverview,
  StartBenchmark,
  Timestamp,
  HelperStatus,
  TuningSuggestion,
  WeeklyReport,
  ReportFormat,
  SavedReport,
  IssuesOverview,
  IssuesQuery,
  SavedIssues,
  UnusualAction,
  UnusualOverview,
  AddWatchedFolder,
  FileHashInfo,
  HashSource,
  SearchSeed,
  StartupFilter,
  StartupOverview,
  WatchChange,
  WatchOverview,
  WatchPreview,
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

  // The privileged helper (ADR 0016). Installing happens only from the confirmation dialog.
  helperStatus(): Promise<HelperStatus>;
  /** Starts the install or update (the system's permission prompt opens); resolves at once with state `installing`. */
  installHelper(): Promise<HelperStatus>;
  uninstallHelper(): Promise<HelperStatus>;
  startupEntries(): Promise<StartupEntry[]>;
  setStartupState(id: string, state: StartupState): Promise<ActivityEntry>;

  // What is installed (memory modules, CPU, disks) and hints.
  inventory(): Promise<SystemInventory>;

  // Performance counters (observe-only): browse, search, read now, and the readings kept with the app's events.
  counterObjects(): Promise<CounterObjectList>;
  counterObject(name: string): Promise<CounterObject>;
  counterSearch(query: string, limit: number): Promise<CounterHit[]>;
  /** Reads up to 32 counters now; the engine also adds the readings to their history. */
  counterValues(paths: string[]): Promise<CounterReading[]>;
  counterHistory(paths: string[], minutes: number): Promise<CounterHistory>;
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

  // Issues and unusual programs (observe-only).
  /** Crashes, hangs and other problems the OS recorded, with the stability score and how each log was read. */
  issues(query: IssuesQuery): Promise<IssuesOverview>;
  /** Reads the OS logs now and answers the same as `issues`. */
  rescanIssues(query: IssuesQuery): Promise<IssuesOverview>;
  /** Asks where to save (the OS save dialog) and writes the last `days` days as JSON lines for Debrief; `null` when they cancel. */
  saveIssues(days: number): Promise<SavedIssues | null>;
  /** Programs that are new or unusual on this computer. */
  unusual(includeIgnored: boolean): Promise<UnusualOverview>;
  /** Ignores, protects (adds to the protected list) or restores a program on the list. */
  unusualAct(key: string, action: UnusualAction): Promise<UnusualOverview>;
  /** Forgets the baseline of programs; learning starts again. */
  forgetBaseline(): Promise<UnusualOverview>;

  // Right-click search (WP-09), hash search (WP-33) and the startup filter (WP-34). Nothing is fetched or uploaded by Dyno: the
  // shell opens the default browser on the click.
  /** The redacted starting text for a web search about `text`. */
  searchSeed(text: string): Promise<SearchSeed>;
  /** Opens a web search for exactly this text in the default browser. */
  webSearch(query: string): Promise<void>;
  /** The SHA-256 of a program's file, computed here. */
  fileHash(source: HashSource): Promise<FileHashInfo>;
  /** Opens the VirusTotal page of a hash in the default browser. */
  openVirusTotal(sha256: string): Promise<void>;
  /** The startup entries after hiding the OS's and signed ones, with the counts. */
  startupOverview(filter: StartupFilter): Promise<StartupOverview>;

  // Watched folders (WP-10): files older than the keep-for time move to the 7-day quarantine, only in folders the person
  // chose, configured, previewed and turned on.
  watchedFolders(): Promise<WatchOverview>;
  /** Opens the OS folder dialog and adds the chosen folder (switched off); `null` when they cancel. */
  addWatchedFolder(request: AddWatchedFolder): Promise<WatchOverview | null>;
  setWatchedFolder(id: string, change: WatchChange): Promise<WatchOverview>;
  removeWatchedFolder(id: string): Promise<WatchOverview>;
  /** A dry run: what a sweep would move now. Needed before a folder can be turned on. */
  previewWatchedFolder(id: string): Promise<WatchPreview>;
  /** Sweeps now (after the preview); answers the list. */
  sweepWatchedFolder(id: string): Promise<WatchOverview>;

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
