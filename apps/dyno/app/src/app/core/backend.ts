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
  Diagnosis,
  HealthReport,
  History,
  HistoryRange,
  LiveDetail,
  LiveUpdate,
  Locker,
  PluginInfo,
  ProcessDetails,
  ProcessRef,
  CustomAlertRule,
  Settings,
  DiskChildren,
  DiskChildrenQuery,
  DiskRings,
  DiskRingsQuery,
  DiskScanStatus,
  DiskTarget,
  Volume,
  SnapshotDiff,
  SnapshotInfo,
  StartupEntry,
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
  WeeklyReport,
  ReportFormat,
  SavedReport,
  BackupReport,
  BootReport,
  ChangeTimeline,
  ChangesQuery,
  GpuReport,
  InventoryFormat,
  MaintenanceStatus,
  SavedInventory,
  SleepReport,
  SoftwareInventory,
  UsageReport,
  IssuesOverview,
  IssuesQuery,
  JobOutput,
  JobSpec,
  JobStatus,
  SavedIssues,
  UnusualOverview,
  SearchSeed,
  StartupFilter,
  StartupOverview,
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
  /** A disk scan's status changed (at most four a second per scan, one final); keep the highest `seq`. No polling. */
  onDiskScan(listener: (s: DiskScanStatus) => void): Unsubscribe;
  /** The volumes mounted now changed (one came, one went, one stopped answering); the new list. */
  onDiskVolumes(listener: (v: Volume[]) => void): Unsubscribe;

  // Processes.
  processDetails(target: ProcessRef): Promise<ProcessDetails>;
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

  // Health.
  health(): Promise<HealthReport>;
  // The disk map.
  /** The volumes mounted now: internal, external and network, with capacity. */
  diskVolumes(): Promise<Volume[]>;
  /** Starts a scan (a job) and resolves at once with its status; the same target again returns the running scan. */
  startDiskScan(target: DiskTarget): Promise<DiskScanStatus>;
  /** Stops a scan and resolves at once; what was counted stays. */
  cancelDiskScan(scan: number): Promise<void>;
  /** The status of every kept scan (a window that opens late asks once). */
  diskScans(): Promise<DiskScanStatus[]>;
  /** One page of one level of a scan. */
  diskChildren(query: DiskChildrenQuery): Promise<DiskChildren>;
  /** Several levels under a folder in one answer (the sunburst's rings). */
  diskRings(query: DiskRingsQuery): Promise<DiskRings>;
  /** The full path of a node (Reveal, Copy path). */
  diskPath(scan: number, node: number): Promise<string>;
  /** Reads the folders a finished scan could not open once more, and only those; resolves at once with the status (the scan runs again). */
  retryDiskScan(scan: number): Promise<DiskScanStatus>;
  /** Opens the system's Full Disk Access pane (macOS); rejects with `unsupported` and the reason elsewhere. */
  openFullDiskAccessSettings(): Promise<void>;
  /** Shows a node in Finder, Explorer or the file manager. */
  diskReveal(scan: number, node: number): Promise<void>;

  startupEntries(): Promise<StartupEntry[]>;

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
  /** Adds an alert rule, or replaces the one with the same id; the engine validates it and answers the settings. */
  upsertAlertRule(rule: CustomAlertRule): Promise<Settings>;
  /** Deletes the alert rule with `id`; answers the settings. */
  deleteAlertRule(id: string): Promise<Settings>;
  plugins(): Promise<PluginInfo[]>;

  // Issues and unusual programs (observe-only).
  /** Crashes, hangs and other problems the OS recorded, with the stability score and how each log was read. */
  issues(query: IssuesQuery): Promise<IssuesOverview>;
  /** Asks where to save (the OS save dialog) and writes the last `days` days as JSON lines for Debrief; `null` when they cancel. */
  saveIssues(days: number): Promise<SavedIssues | null>;
  /** Programs that are new or unusual on this computer. */
  unusual(): Promise<UnusualOverview>;
  /** Forgets the baseline of programs; learning starts again. */
  forgetBaseline(): Promise<UnusualOverview>;

  // Right-click search (WP-09), hash search (WP-33) and the startup filter (WP-34). Nothing is fetched or uploaded by Dyno: the
  // shell opens the default browser on the click.
  /** The redacted starting text for a web search about `text`. */
  searchSeed(text: string): Promise<SearchSeed>;
  /** Opens a web search for exactly this text in the default browser. */
  webSearch(query: string): Promise<void>;
  /** Opens the VirusTotal page of a hash in the default browser. */
  openVirusTotal(sha256: string): Promise<void>;
  /** The startup entries after hiding the OS's and signed ones, with the counts. */
  startupOverview(filter: StartupFilter): Promise<StartupOverview>;

  // Extras (read-only views) and backup health.
  /** Installs, updates, new start-up items and setting changes in a window. */
  changes(query: ChangesQuery): Promise<ChangeTimeline>;
  /** What keeps the computer awake now and what woke it. */
  sleepBlockers(): Promise<SleepReport>;
  /** Recent boots and what slowed the last one. */
  bootAnalysis(): Promise<BootReport>;
  /** Network traffic and energy impact per app. */
  appUsage(): Promise<UsageReport>;
  /** GPU use per process. */
  gpuProcesses(): Promise<GpuReport>;
  /** The installed software. */
  softwareInventory(): Promise<SoftwareInventory>;
  /** Asks where to save (the OS save dialog) and writes the hardware and software report there; `null` when they cancel. */
  saveInventoryReport(format: InventoryFormat): Promise<SavedInventory | null>;
  /** Asks for a folder (the OS folder dialog; the webview never invents a path); `null` when they cancel. */
  pickFolder(): Promise<string | null>;
  /** Where the maintenance windows stand now. */
  maintenance(): Promise<MaintenanceStatus>;
  /** The last backup check (the engine runs one when there is none yet). */
  backup(): Promise<BackupReport>;

  // Long operations are jobs (ER-17 to ER-19): find large and duplicate files in a folder the person chose (deletes nothing),
  // read the OS logs now, check the backup tools now, hash a program's file. `JobRunner` drives these four calls; nothing
  // here waits for the work, so none of them needs a long timeout.
  /** Starts a job and resolves at once with its status. A second start of the same work answers the running job; another search or log read while one runs is refused with `busy`. */
  startJob(spec: JobSpec): Promise<JobStatus>;
  /** The status of every running job and of the last few that ended. */
  jobs(): Promise<JobStatus[]>;
  /** Asks a job to stop and resolves at once. A job that is unknown or over is left as it is. */
  cancelJob(id: number): Promise<void>;
  /** What a done job produced; `notFound` when the job is not done or too old to be kept. */
  jobResult(id: number): Promise<JobOutput>;

  // The tray popover window (the shell places and fits it).
  /** Shows and focuses the main window (and hides the tray popover). */
  showMainWindow(): Promise<void>;
  /** Tells the shell how tall the popover's content is (CSS pixels) so it can size the window to fit; no-op in the browser. */
  fitTray(height: number): Promise<void>;
  /** Closes the tray popover. */
  hideTray(): Promise<void>;
  /** Quits the Dyno app (the background agent keeps its own lifecycle). */
  quitApp(): Promise<void>;
}

export const BACKEND = new InjectionToken<DynoBackend>('DynoBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
