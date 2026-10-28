import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
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
  Settings,
  DiskChildren,
  DiskChildrenQuery,
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
  HelperStatus,
  WeeklyReport,
  ReportFormat,
  SavedReport,
  BackupReport,
  BootReport,
  ChangeTimeline,
  ChangesQuery,
  FinderQuery,
  FinderResult,
  GpuReport,
  InventoryFormat,
  MaintenanceStatus,
  SavedInventory,
  SleepReport,
  SoftwareInventory,
  UsageReport,
  IssuesOverview,
  IssuesQuery,
  SavedIssues,
  UnusualOverview,
  FileHashInfo,
  HashSource,
  SearchSeed,
  StartupFilter,
  StartupOverview,
} from './api.types';
import type { DynoBackend, Unsubscribe } from './backend';
import { toCommandError } from './command-error';

/** Calls a Tauri command (`invoke`). */
export type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
/** Listens to a Tauri event (`listen`); resolves with the function that stops listening. */
export type ListenFn = <T>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;

/** The externally tagged Rust `Request` enum: a unit variant is a string, a data variant `{ variant: fields }`. */
export type DynoRequest = string | Record<string, Record<string, unknown>>;

/** Every command fails with a `timeout` error after this long, so a panel shows an inline error with Retry, not a skeleton forever. */
export const REQUEST_TIMEOUT_MS = 15_000;
/** A command that waits for the person at an OS dialog may take as long as they do (within reason). */
export const DIALOG_TIMEOUT_MS = 10 * 60_000;
/** Hashing a large program may take longer than an ordinary request. */
export const HASH_TIMEOUT_MS = 60_000;
/** Reading the system logs can take a minute or two on a busy PC (the shell waits 150 s for the agent). */
export const LOG_READ_TIMEOUT_MS = 160_000;

/** The push events of the shell. */
export const EVENTS = {
  live: 'dyno://live',
  alert: 'dyno://alert',
  activity: 'dyno://activity',
  agent: 'dyno://agent',
  alarm: 'dyno://alarm',
  diskScan: 'dyno://disk-scan',
} as const;

/** One Tauri event shared by every UI listener: a single `listen`, fanned out, stopped with the last listener. */
class EventChannel<T> {
  private readonly listeners = new Set<(payload: T) => void>();
  private stop: Promise<() => void> | null = null;

  constructor(
    private readonly listenFn: ListenFn,
    private readonly name: string,
  ) {}

  add(listener: (payload: T) => void): Unsubscribe {
    this.listeners.add(listener);
    if (!this.stop) {
      this.stop = this.listenFn<T>(this.name, (e) => {
        for (const l of [...this.listeners]) l(e.payload);
      }).catch((e: unknown) => {
        console.error(`Could not listen to ${this.name}`, toCommandError(e).message);
        return () => undefined;
      });
    }
    return () => {
      if (!this.listeners.delete(listener) || this.listeners.size > 0 || !this.stop) return;
      const stop = this.stop;
      this.stop = null;
      void stop.then((fn) => fn());
    };
  }
}

/**
 * The real backend. Every engine request goes through the single `dyno` command with a typed `Request`; the window
 * helpers (`app_info`, tray) are their own commands. Push events arrive on the `dyno://…` channels.
 */
export class TauriBackend implements DynoBackend {
  private readonly live: EventChannel<LiveUpdate>;
  private readonly alertChannel: EventChannel<Alert>;
  private readonly activityChannel: EventChannel<ActivityEntry>;
  private readonly agentChannel: EventChannel<AgentStatus>;
  private readonly alarmChannel: EventChannel<AlarmChange>;
  private readonly diskScanChannel: EventChannel<DiskScanStatus>;
  /** How many live listeners want the full process list; the agent sends it only while one does. */
  private processListeners = 0;
  private liveListeners = 0;
  private sentDetail: LiveDetail | null = null;

  constructor(
    private readonly call: InvokeFn = invoke,
    listenFn: ListenFn = listen as ListenFn,
    /** How long a command may take before it fails with `timeout` (the first call in a fresh window can go unanswered). */
    private readonly timeoutMs: number = REQUEST_TIMEOUT_MS,
  ) {
    this.live = new EventChannel(listenFn, EVENTS.live);
    this.alertChannel = new EventChannel(listenFn, EVENTS.alert);
    this.activityChannel = new EventChannel(listenFn, EVENTS.activity);
    this.agentChannel = new EventChannel(listenFn, EVENTS.agent);
    this.alarmChannel = new EventChannel(listenFn, EVENTS.alarm);
    this.diskScanChannel = new EventChannel(listenFn, EVENTS.diskScan);
  }

  // ------------------------------------------------------------------------------------------------ live data

  onLive(detail: LiveDetail, listener: (u: LiveUpdate) => void): Unsubscribe {
    this.liveListeners++;
    if (detail === 'processes') this.processListeners++;
    this.syncDetail();
    const stop = this.live.add(listener);
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      stop();
      this.liveListeners--;
      if (detail === 'processes') this.processListeners--;
      this.syncDetail();
    };
  }

  onAlert(listener: (a: Alert) => void): Unsubscribe {
    return this.alertChannel.add(listener);
  }

  onActivity(listener: (a: ActivityEntry) => void): Unsubscribe {
    return this.activityChannel.add(listener);
  }

  onAgentStatus(listener: (s: AgentStatus) => void): Unsubscribe {
    return this.agentChannel.add(listener);
  }

  onAlarm(listener: (c: AlarmChange) => void): Unsubscribe {
    return this.alarmChannel.add(listener);
  }

  onDiskScan(listener: (s: DiskScanStatus) => void): Unsubscribe {
    return this.diskScanChannel.add(listener);
  }

  /** Tells the agent the detail level the UI needs now (only when it changed; nothing once nobody listens). */
  private syncDetail(): void {
    if (this.liveListeners === 0) {
      this.sentDetail = null;
      return;
    }
    const detail: LiveDetail = this.processListeners > 0 ? 'processes' : 'summary';
    if (detail === this.sentDetail) return;
    this.sentDetail = detail;
    this.request<void>({ subscribe: { detail } }).catch((e: unknown) => {
      // The next change retries; the agent status event tells the user when the agent is down.
      this.sentDetail = null;
      console.error('Could not subscribe to live updates', toCommandError(e).message);
    });
  }

  appInfo(): Promise<AppInfo> {
    return this.command<AppInfo>('app_info');
  }

  // ------------------------------------------------------------------------------------------------ processes

  processDetails(target: ProcessRef): Promise<ProcessDetails> {
    return this.request({ processDetails: { target } });
  }

  lockers(path: string): Promise<Locker[]> {
    return this.request({ lockers: { path } });
  }

  openLocation(target: ProcessRef): Promise<void> {
    return this.request({ openLocation: { target } });
  }

  takeSnapshot(label: string): Promise<SnapshotInfo> {
    return this.request({ takeSnapshot: { label } });
  }

  snapshots(): Promise<SnapshotInfo[]> {
    return this.request('snapshots');
  }

  diffSnapshots(before: string, after: string): Promise<SnapshotDiff> {
    return this.request({ diffSnapshots: { before, after } });
  }

  // ---------------------------------------------------------------------------------- diagnosis and history

  whySlow(): Promise<Diagnosis> {
    return this.request('whySlow');
  }

  anomalies(): Promise<Anomaly[]> {
    return this.request('anomalies');
  }

  history(range: HistoryRange, end: Timestamp | null): Promise<History> {
    return this.request({ history: { range, end } });
  }

  activity(limit: number): Promise<ActivityEntry[]> {
    return this.request({ activity: { limit } });
  }

  diskVolumes(): Promise<Volume[]> {
    return this.request('diskVolumes');
  }

  startDiskScan(target: DiskTarget): Promise<DiskScanStatus> {
    return this.request({ startDiskScan: { target } });
  }

  cancelDiskScan(scan: number): Promise<void> {
    return this.request({ cancelDiskScan: { scan } });
  }

  diskScans(): Promise<DiskScanStatus[]> {
    return this.request('diskScans');
  }

  diskChildren(q: DiskChildrenQuery): Promise<DiskChildren> {
    return this.request({ diskChildren: { scan: q.scan, node: q.node, offset: q.offset, limit: q.limit, minFraction: q.minFraction } });
  }

  diskPath(scan: number, node: number): Promise<string> {
    return this.request({ diskPath: { scan, node } });
  }

  // ---------------------------------------------------------------------------------------- health

  health(): Promise<HealthReport> {
    return this.request('health');
  }

  helperStatus(): Promise<HelperStatus> {
    return this.request('helperStatus');
  }

  installHelper(): Promise<HelperStatus> {
    return this.request('installHelper');
  }

  uninstallHelper(): Promise<HelperStatus> {
    return this.request('uninstallHelper');
  }

  startupEntries(): Promise<StartupEntry[]> {
    return this.request('startupEntries');
  }

  // ---------------------------------------------------------------------------------- reports, settings, plugins

  inventory(): Promise<SystemInventory> {
    return this.request('inventory');
  }

  counterObjects(): Promise<CounterObjectList> {
    return this.request('counterObjects');
  }

  counterObject(name: string): Promise<CounterObject> {
    return this.request({ counterObject: { name } });
  }

  counterSearch(query: string, limit: number): Promise<CounterHit[]> {
    return this.request({ counterSearch: { query, limit } });
  }

  counterValues(paths: string[]): Promise<CounterReading[]> {
    return this.request({ counterValues: { paths } });
  }

  counterHistory(paths: string[], minutes: number): Promise<CounterHistory> {
    return this.request({ counterHistory: { paths, minutes } });
  }

  throttling(): Promise<ThrottleOverview> {
    return this.request('throttling');
  }

  lifespan(): Promise<LifespanOverview> {
    return this.request('lifespan');
  }

  baselines(): Promise<BaselineOverview> {
    return this.request('baselines');
  }

  network(query: NetworkQuery): Promise<NetworkSnapshot> {
    return this.request({ network: { query } });
  }

  lookupIp(ip: string, template?: string): Promise<void> {
    return this.request({ lookupIp: { ip, template: template ?? null } });
  }

  resolveHosts(addresses: string[]): Promise<HostName[]> {
    return this.request({ resolveHosts: { addresses } });
  }

  // ---------------------------------------------------------------------------------------------------- alarms

  alarms(filter: AlarmFilter): Promise<Alarm[]> {
    return this.request({ alarms: { filter } });
  }

  alarmStats(): Promise<AlarmStats> {
    return this.request('alarmStats');
  }

  acknowledgeAlarms(ids: number[]): Promise<Alarm[]> {
    return this.request({ acknowledgeAlarms: { ids } });
  }

  shelveAlarm(id: number, until: ShelveFor): Promise<Alarm[]> {
    return this.request({ shelveAlarm: { id, until } });
  }

  unshelveAlarm(id: number): Promise<Alarm[]> {
    return this.request({ unshelveAlarm: { id } });
  }

  muteRule(rule: string, muted: boolean): Promise<Alarm[]> {
    return this.request({ muteRule: { rule, muted } });
  }

  noteAlarm(id: number, text: string): Promise<Alarm[]> {
    return this.request({ noteAlarm: { id, text } });
  }

  alarmHistory(alarmId: number | null, limit: number): Promise<AlarmChange[]> {
    return this.request({ alarmHistory: { alarmId, limit } });
  }

  // -------------------------------------------------------------------------------------- recorder, diagnostics

  capture(): Promise<RecorderSnapshotInfo> {
    return this.request('capture');
  }

  recorderSnapshots(): Promise<RecorderSnapshotInfo[]> {
    return this.request('recorderSnapshots');
  }

  recorderSnapshot(id: number): Promise<RecorderSnapshot> {
    return this.request({ recorderSnapshot: { id } });
  }

  diagnosticsPreview(): Promise<BundlePreview> {
    return this.request('diagnosticsPreview');
  }

  /** The shell shows the OS save dialog (the webview never sees a path it chose itself) and asks the agent to write there. */
  saveDiagnostics(): Promise<SavedDiagnostics | null> {
    return this.command<SavedDiagnostics | null>('save_diagnostics', undefined, DIALOG_TIMEOUT_MS);
  }

  // ------------------------------------------------------------------------- search, hash search, startup filter

  searchSeed(text: string): Promise<SearchSeed> {
    return this.request({ searchSeed: { text } });
  }

  /** The agent builds the address from the text; the shell opens it in the default browser. */
  webSearch(query: string): Promise<void> {
    return this.request({ webSearch: { query } });
  }

  fileHash(source: HashSource): Promise<FileHashInfo> {
    return this.request({ fileHash: { source } }, HASH_TIMEOUT_MS);
  }

  openVirusTotal(sha256: string): Promise<void> {
    return this.request({ virusTotalUrl: { sha256 } });
  }

  startupOverview(filter: StartupFilter): Promise<StartupOverview> {
    return this.request({ startupOverview: { filter } });
  }

  // ------------------------------------------------------------------------------------------------ benchmarks

  benchmarks(): Promise<BenchOverview> {
    return this.request('benchmarks');
  }

  startBenchmark(r: StartBenchmark): Promise<BenchOverview> {
    return this.request({ startBenchmark: { kinds: r.kinds, label: r.label, group: r.group ?? null, phase: r.phase ?? null, diskMib: r.diskMib ?? null } });
  }

  cancelBenchmark(): Promise<void> {
    return this.request('cancelBenchmark');
  }

  deleteBenchmark(id: string): Promise<void> {
    return this.request({ deleteBenchmark: { id } });
  }

  compareBenchmarks(before: string, after: string): Promise<BenchComparison> {
    return this.request({ compareBenchmarks: { before, after } });
  }

  weeklyReport(weeksAgo: number): Promise<WeeklyReport> {
    return this.request({ weeklyReport: { weeksAgo } });
  }

  saveReport(format: ReportFormat, weeksAgo: number): Promise<SavedReport | null> {
    return this.command<SavedReport | null>('save_report', { format, weeksAgo }, DIALOG_TIMEOUT_MS);
  }

  issues(query: IssuesQuery): Promise<IssuesOverview> {
    return this.request({ issues: { query } });
  }

  rescanIssues(query: IssuesQuery): Promise<IssuesOverview> {
    // Reading a day of system log can take a minute on a busy PC.
    return this.request({ rescanIssues: { query } }, LOG_READ_TIMEOUT_MS);
  }

  saveIssues(days: number): Promise<SavedIssues | null> {
    return this.command<SavedIssues | null>('save_issues', { days }, DIALOG_TIMEOUT_MS);
  }

  unusual(): Promise<UnusualOverview> {
    return this.request('unusual');
  }

  forgetBaseline(): Promise<UnusualOverview> {
    return this.request('forgetBaseline');
  }

  alerts(limit: number): Promise<Alert[]> {
    return this.request({ alerts: { limit } });
  }

  settings(): Promise<Settings> {
    return this.request('settings');
  }

  saveSettings(settings: Settings): Promise<Settings> {
    return this.request({ saveSettings: { settings } });
  }

  plugins(): Promise<PluginInfo[]> {
    return this.request('plugins');
  }

  // ----------------------------------------------------------------------------------------------------- tray

  trayOpenMain(): Promise<void> {
    return this.command<void>('tray_open_main');
  }

  popoverResize(height: number): Promise<void> {
    return this.command<void>('popover_resize', { height });
  }

  trayQuit(): Promise<void> {
    return this.command<void>('tray_quit');
  }

  // ----------------------------------------------------------------------------------------------- extras, backup

  changes(query: ChangesQuery): Promise<ChangeTimeline> {
    return this.request({ changes: { query } });
  }

  sleepBlockers(): Promise<SleepReport> {
    return this.request('sleepBlockers');
  }

  bootAnalysis(): Promise<BootReport> {
    return this.request('bootAnalysis');
  }

  appUsage(): Promise<UsageReport> {
    return this.request('appUsage');
  }

  gpuProcesses(): Promise<GpuReport> {
    return this.request('gpuProcesses');
  }

  softwareInventory(): Promise<SoftwareInventory> {
    return this.request('softwareInventory');
  }

  /** The shell shows the OS save dialog and asks the agent to write the report there. */
  saveInventoryReport(format: InventoryFormat): Promise<SavedInventory | null> {
    return this.command<SavedInventory | null>('save_inventory_report', { format }, DIALOG_TIMEOUT_MS);
  }

  /** The shell shows the OS folder dialog and answers the chosen path (or null). */
  pickFolder(): Promise<string | null> {
    return this.command<string | null>('pick_folder', undefined, DIALOG_TIMEOUT_MS);
  }

  findFiles(query: FinderQuery): Promise<FinderResult> {
    // Walking and hashing a big folder takes a while.
    return this.request({ findFiles: { query } }, LOG_READ_TIMEOUT_MS);
  }

  maintenance(): Promise<MaintenanceStatus> {
    return this.request('maintenance');
  }

  backup(): Promise<BackupReport> {
    return this.request('backup');
  }

  refreshBackup(): Promise<BackupReport> {
    return this.request('refreshBackup', LOG_READ_TIMEOUT_MS);
  }

  // --------------------------------------------------------------------------------------------------- plumbing

  private request<T>(request: DynoRequest, timeoutMs?: number): Promise<T> {
    return this.command<T>('dyno', { request }, timeoutMs);
  }

  private async command<T>(cmd: string, args?: Record<string, unknown>, timeoutMs: number = this.timeoutMs): Promise<T> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject({ code: 'timeout', message: 'Dyno did not answer in time.' }), timeoutMs);
    });
    try {
      return await Promise.race([args === undefined ? this.call<T>(cmd) : this.call<T>(cmd, args), timeout]);
    } catch (e) {
      throw toCommandError(e);
    } finally {
      clearTimeout(timer);
    }
  }
}
