import { InjectionToken } from '@angular/core';
import type {
  ActivityEntry,
  AgentStatus,
  TrayAvailability,
  Alarm,
  AlarmChange,
  AlarmFilter,
  AlarmStats,
  Alert,
  HostName,
  RecorderSnapshot,
  RecorderSnapshotInfo,
  ShelveFor,
  Anomaly,
  Diagnosis,
  HealthReport,
  History,
  HistoryRange,
  LiveDetail,
  LiveUpdate,
  LockerList,
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
  HelperStatus,
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
  AwakeSpec,
  EnergyReport,
  PowerView,
  Neighbour,
  ThisPcWol,
  WolDeviceInput,
  WolView,
  WakeInput,
  WakesView,
  NetworkSnapshot,
  BenchOverview,
  StartBenchmark,
  Timestamp,
  WeeklyReport,
  ReportFormat,
  SavedReport,
  GpuAdapter,
  BenchExportFormat,
  BenchEstimateRequest,
  BenchRunEstimate,
  BackupReport,
  ChangeTimeline,
  ChangesQuery,
  GpuReport,
  InventoryFormat,
  MaintenanceStatus,
  SavedInventory,
  IssuesOverview,
  IssuesQuery,
  JobOutput,
  JobSpec,
  SavedIssues,
  UnusualOverview,
  SearchSeed,
  StartupFilter,
  StartupOverview,
  IntelStatus,
  IntelEdit,
  EditOutcome,
  GeoSummary,
  FirewallView,
  ToolOffer,
} from './api.types';
import type { DynoSurface, MetricReading, Navigate, PreviewReply, PreviewRequest, ReadingQuery, SurfaceEdit, SurfaceView, WidgetActionId, WidgetChoices } from './generated/widgets';
import type { DashboardView, PopoverSurfaceValues } from './widget-types';

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
  /** Whether the desktop shows the tray icons now (Settings > Menu bar shows the engine's note when it does not). */
  trayAvailability(): Promise<TrayAvailability>;
  /** The desktop started or stopped showing the tray icons (a tray host appeared after the start, or went). */
  onTrayAvailability(listener: (a: TrayAvailability) => void): Unsubscribe;
  /** Alarm changes pushed by the agent (new, repeated, returned, acknowledged, shelved, muted, noted). */
  onAlarm(listener: (c: AlarmChange) => void): Unsubscribe;
  /** A disk scan's status changed (at most four a second per scan, one final); keep the highest `seq`. No polling. */
  onDiskScan(listener: (s: DiskScanStatus) => void): Unsubscribe;
  /** The volumes mounted now changed (one came, one went, one stopped answering); the new list. */
  onDiskVolumes(listener: (v: Volume[]) => void): Unsubscribe;

  // Processes.
  processDetails(target: ProcessRef): Promise<ProcessDetails>;
  lockers(path: string): Promise<LockerList>;
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
  /**
   * Starts a scan (a job) and resolves at once with its status; the same target again returns the running scan. `replaces` is
   * the kept scan of the same target a Rescan will replace: that map stays readable until the new pass is done.
   */
  startDiskScan(target: DiskTarget, replaces?: number | null): Promise<DiskScanStatus>;
  /** Forgets one saved map: its row in the cache is deleted. A scan of this run stays in memory. */
  forgetSavedDiskMap(scan: number): Promise<void>;
  /** Forgets every saved map. */
  forgetSavedDiskMaps(): Promise<void>;
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

  /** Whether the privileged helper is off, running, outdated and so on (it never blocks: an absent helper is a state). */
  helperStatus(): Promise<HelperStatus>;
  /** What Settings offers for the disk-health tool (smartmontools): the pinned download, or why not. */
  toolOffer(): Promise<ToolOffer>;

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

  // Power (ADR 0032): keep awake. The agent holds the OS object; a change is pushed to every window.
  powerState(): Promise<PowerView>;
  powerKeepAwake(spec: AwakeSpec): Promise<PowerView>;
  powerStopKeepAwake(): Promise<PowerView>;
  powerIdleCancel(): Promise<PowerView>;
  powerIdleSnooze(minutes: number): Promise<PowerView>;
  onPower(listener: (v: PowerView) => void): Unsubscribe;
  /** The energy report for the last `days` local days (1 to 366). */
  energyReport(days: number): Promise<EnergyReport>;

  // Wake-on-LAN (ADR 0032): the saved computers, "Add from network", and this PC's readiness. A wake itself is a job (`startJob`).
  wolDevices(): Promise<WolView>;
  wolCandidates(): Promise<Neighbour[]>;
  wolSave(device: WolDeviceInput): Promise<WolView>;
  wolRemove(id: number): Promise<WolView>;
  wolReadiness(): Promise<ThisPcWol>;
  // Scheduled wake (ADR 0032 section 5): the schedules and who sets the OS wake. The agent validates and acts.
  powerWakes(): Promise<WakesView>;
  powerSetWake(entry: WakeInput): Promise<WakesView>;
  powerCancelWake(id: number): Promise<WakesView>;
  /** The schedules changed (a save or cancel from any window, or a schedule that ran). */
  onWakes(listener: (v: WakesView) => void): Unsubscribe;
  /** The list changed (a save or remove from any window, or a wake that ended). */
  onWol(listener: (v: WolView) => void): Unsubscribe;

  // Open ports and connections (observe-only), and "Look up IP" (the shell opens the browser; Dyno never fetches).
  network(query: NetworkQuery): Promise<NetworkSnapshot>;
  /** Opens the lookup page for a public address in the default browser; `template` tests one that is not saved yet. */
  lookupIp(ip: string, template?: string): Promise<void>;

  /** Reverse DNS through the OS resolver for up to 64 addresses; cached by the agent. Only on request, never in the background. */
  resolveHosts(addresses: string[]): Promise<HostName[]>;

  /** The user's IP lists, the allow list, the network switches and the location databases (ADR 0031 and its amendments). */
  intelStatus(): Promise<IntelStatus>;
  /** Changes the lists, the allow list or the switches; a "Remove duplicates" preview changes nothing. */
  intelEdit(edit: IntelEdit): Promise<EditOutcome>;
  /** The connections map's data, aggregated before any page cap. */
  geoSummary(): Promise<GeoSummary>;
  /** The firewall of this computer (read-only). */
  firewall(): Promise<FirewallView>;

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

  // Benchmarks: start only on a click; poll `benchmarks()` while a run is in progress.
  benchmarks(): Promise<BenchOverview>;
  startBenchmark(request: StartBenchmark): Promise<BenchOverview>;
  cancelBenchmark(): Promise<void>;
  deleteBenchmark(id: string): Promise<void>;
  /** The graphics adapters the GPU runner sees (it is started for a moment). */
  benchGpuAdapters(): Promise<GpuAdapter[]>;
  /** The engine's estimate of a chosen run; `null` while a drive is still to be chosen. */
  benchEstimate(request: BenchEstimateRequest): Promise<BenchRunEstimate | null>;
  /** The shell shows the OS save dialog and the agent writes the chosen runs as JSON or CSV there; `null` when they cancel. */
  saveBenchmarks(ids: string[], format: BenchExportFormat): Promise<SavedReport | null>;
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
  /** GPU use per process. */
  gpuProcesses(): Promise<GpuReport>;
  /** Asks where to save (the OS save dialog) and writes the hardware and software report there; `null` when they cancel. */
  saveInventoryReport(format: InventoryFormat): Promise<SavedInventory | null>;
  /** Asks for a folder (the OS folder dialog; the webview never invents a path); `null` when they cancel. */
  pickFolder(): Promise<string | null>;
  /** Where the maintenance windows stand now. */
  maintenance(): Promise<MaintenanceStatus>;
  /** The last backup check (the engine runs one when there is none yet). */
  backup(): Promise<BackupReport>;

  // Long operations are jobs (ER-17 to ER-19): find large and duplicate files in a folder the person chose (deletes nothing),
  // read the OS logs now, check the backup tools now, hash a program's file, and the sleep, boot, app-usage and software reads.
  // The shell's jobs store follows them (`paddock://jobs`) and cancels them (`job_cancel`); `JobStarter` ties these together.
  // Nothing here waits for the work, so none of it needs a long timeout.
  /** Starts a job and resolves at once with its id. A second start of the same work answers the running job; another search or log read while one runs is refused with `busy`. */
  startJob(spec: JobSpec): Promise<number>;
  /** What a done job produced; `notFound` when the job is not done or too old to be kept. */
  jobResult(id: number): Promise<JobOutput>;

  // Widget layouts (family ADR 0035): the engine owns the layouts, their limits, edits and undo; the UI draws and sends edits.
  /** One surface's layout with its catalogue, built-in presets, notes, `canUndo` and `editable`. */
  widgetLayout(surface: DynoSurface): Promise<SurfaceView>;
  /**
   * Applies one edit; answers the new view, or rejects with the reason it was refused (the message says what to change).
   * `baseRevision` is the `revision` of the view the edit was computed on: a layout edited since rejects with code
   * `stale`, and the caller reads the view again and tries once more.
   */
  editWidgetLayout(edit: SurfaceEdit, baseRevision?: number): Promise<SurfaceView>;
  /** What a dashboard arrangement gesture would give on `request.revision` (answered in the app's own process; stores nothing). */
  previewWidgetLayout(request: PreviewRequest): Promise<PreviewReply<DashboardView>>;
  /** The popup's values now (the first paint; later ones are pushed). */
  popoverValues(): Promise<PopoverSurfaceValues>;
  /** The popup's values as they are now, for the Settings preview: asking changes nothing about the popover's visibility or its pushes. */
  popoverSnapshot(): Promise<PopoverSurfaceValues>;
  /** The values for the Settings preview, with the slow widgets asked of the agent now. It never marks the popover visible. */
  popoverPreview(): Promise<PopoverSurfaceValues>;
  /** A widget's button: open a page, retry, start the backup check, restart the agent, open Settings. */
  runWidgetAction(action: WidgetActionId): Promise<void>;
  /** The popup's values, pushed while the popover is visible (`dyno://popover-values`). */
  onPopoverValues(listener: (v: PopoverSurfaceValues) => void): Unsubscribe;
  /** A surface's layout changed in the agent, from any window (`dyno://widget-layout`). */
  onWidgetLayout(listener: (surface: DynoSurface) => void): Unsubscribe;
  /** The main window is asked to open a page or a Settings section (`dyno://navigate`). */
  onNavigate(listener: (to: Navigate) => void): Unsubscribe;
  /** The page or Settings section a widget asked for, once: a main window that was still being created misses the event. */
  takeNavigation(): Promise<Navigate | null>;
  /** Opens the OS taskbar settings (Windows `ms-settings:taskbar`), where the tray icons are switched on; refused on other systems. */
  openTaskbarSettings(): Promise<void>;
  /** Readings (value, text, scale and tone against each query's thresholds) from the newest live update, in the queries' order. */
  metricReadings(queries: ReadingQuery[]): Promise<MetricReading[]>;
  /** For each kind a chooser can add: a ready default widget and the metrics the engine accepts for it. */
  widgetChoices(): Promise<WidgetChoices>;

  // The tray popover window (the shell places and fits it).
  /** Shows and focuses the main window (and hides the tray popover). */
  showMainWindow(): Promise<void>;
  /** Tells the shell how tall the popover's content is (CSS pixels) so it can size the window to fit; no-op in the browser. */
  fitTray(height: number): Promise<void>;
  /** Closes the tray popover. */
  hideTray(): Promise<void>;
  /** Quits the Dyno app (the background agent keeps its own lifecycle). */
  quitApp(): Promise<void>;
  /** Whether the agent keeps measuring after a plain Quit: a login service is installed and supervises it (the engine decides). */
  quitKeepsMeasuring(): Promise<boolean>;
  /** Quit and stop measuring: stops the agent (until Dyno is opened again or the next login), then closes the app. The engine confirms with the system that the agent is gone. */
  quitAndStopAgent(): Promise<void>;
}

export const BACKEND = new InjectionToken<DynoBackend>('DynoBackend');

/** Whether the page runs inside the Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
