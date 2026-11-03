import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { DynoSurface, MetricReading, Navigate, ReadingQuery, SurfaceEdit, SurfaceView, WidgetActionId, WidgetChoices } from './generated/widgets';
import type { PopoverSurfaceValues } from './widget-types';
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
  NetworkSnapshot,
  BenchOverview,
  StartBenchmark,
  Timestamp,
  WeeklyReport,
  ReportFormat,
  SavedReport,
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
  ToolOffer,
} from './api.types';
import type { DynoBackend, Unsubscribe } from './backend';
import { toCommandError } from './command-error';

/** Calls a Tauri command (`invoke`). */
export type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
/** Listens to a Tauri event (`listen`); resolves with the function that stops listening. */
export type ListenFn = <T>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;

/** The externally tagged Rust `Request` enum: a unit variant is a string, a data variant `{ variant: fields }`. */
export type DynoRequest = string | Record<string, Record<string, unknown>>;

// There is no timer here (ER-19, ADR 0008 §4): the UI never gives up on running work. A quick request is bounded where it is
// answered (the shell's link to the agent answers an error after 15 s), a command that waits for the person at an OS dialog
// takes as long as they do, and every long operation is a job: starting one answers at once and the shell's jobs store follows it.

/** The push events of the shell. */
export const EVENTS = {
  live: 'dyno://live',
  alert: 'dyno://alert',
  activity: 'dyno://activity',
  agent: 'dyno://agent',
  alarm: 'dyno://alarm',
  diskScan: 'dyno://disk-scan',
  diskVolumes: 'dyno://disk-volumes',
  popoverValues: 'dyno://popover-values',
  widgetLayout: 'dyno://widget-layout',
  navigate: 'dyno://navigate',
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
  private readonly diskVolumesChannel: EventChannel<Volume[]>;
  private readonly popoverChannel: EventChannel<PopoverSurfaceValues>;
  private readonly layoutChannel: EventChannel<DynoSurface>;
  private readonly navigateChannel: EventChannel<Navigate>;
  /** How many live listeners want the full process list; the agent sends it only while one does. */
  private processListeners = 0;
  private liveListeners = 0;
  private sentDetail: LiveDetail | null = null;

  constructor(
    private readonly call: InvokeFn = invoke,
    listenFn: ListenFn = listen as ListenFn,
  ) {
    this.live = new EventChannel(listenFn, EVENTS.live);
    this.alertChannel = new EventChannel(listenFn, EVENTS.alert);
    this.activityChannel = new EventChannel(listenFn, EVENTS.activity);
    this.agentChannel = new EventChannel(listenFn, EVENTS.agent);
    this.alarmChannel = new EventChannel(listenFn, EVENTS.alarm);
    this.diskScanChannel = new EventChannel(listenFn, EVENTS.diskScan);
    this.diskVolumesChannel = new EventChannel(listenFn, EVENTS.diskVolumes);
    this.popoverChannel = new EventChannel(listenFn, EVENTS.popoverValues);
    this.layoutChannel = new EventChannel(listenFn, EVENTS.widgetLayout);
    this.navigateChannel = new EventChannel(listenFn, EVENTS.navigate);
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

  onDiskVolumes(listener: (v: Volume[]) => void): Unsubscribe {
    return this.diskVolumesChannel.add(listener);
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

  diskRings(q: DiskRingsQuery): Promise<DiskRings> {
    return this.request({ diskRings: { scan: q.scan, node: q.node, rings: q.rings, minShare: q.minShare, maxNodes: q.maxNodes } });
  }

  diskPath(scan: number, node: number): Promise<string> {
    return this.request({ diskPath: { scan, node } });
  }

  retryDiskScan(scan: number): Promise<DiskScanStatus> {
    return this.request({ retryDiskScan: { scan } });
  }

  openFullDiskAccessSettings(): Promise<void> {
    return this.request('openFullDiskAccessSettings');
  }

  diskReveal(scan: number, node: number): Promise<void> {
    return this.request({ diskReveal: { scan, node } });
  }

  // ---------------------------------------------------------------------------------------- health

  health(): Promise<HealthReport> {
    return this.request('health');
  }

  startupEntries(): Promise<StartupEntry[]> {
    return this.request('startupEntries');
  }

  // ---------------------------------------------------------------------------------- reports, settings, plugins

  inventory(): Promise<SystemInventory> {
    return this.request('inventory');
  }

  helperStatus(): Promise<HelperStatus> {
    return this.request('helperStatus');
  }

  toolOffer(): Promise<ToolOffer> {
    return this.request('toolOffer');
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
    return this.command<SavedDiagnostics | null>('save_diagnostics', undefined);
  }

  // ------------------------------------------------------------------------- search, hash search, startup filter

  searchSeed(text: string): Promise<SearchSeed> {
    return this.request({ searchSeed: { text } });
  }

  /** The agent builds the address from the text; the shell opens it in the default browser. */
  webSearch(query: string): Promise<void> {
    return this.request({ webSearch: { query } });
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
    return this.command<SavedReport | null>('save_report', { format, weeksAgo });
  }

  issues(query: IssuesQuery): Promise<IssuesOverview> {
    return this.request({ issues: { query } });
  }

  saveIssues(days: number): Promise<SavedIssues | null> {
    return this.command<SavedIssues | null>('save_issues', { days });
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

  upsertAlertRule(rule: CustomAlertRule): Promise<Settings> {
    return this.request({ upsertAlertRule: { rule } });
  }

  deleteAlertRule(id: string): Promise<Settings> {
    return this.request({ deleteAlertRule: { id } });
  }

  plugins(): Promise<PluginInfo[]> {
    return this.request('plugins');
  }

  // -------------------------------------------------------------------------------------------------- widget layouts

  widgetLayout(surface: DynoSurface): Promise<SurfaceView> {
    return this.command<SurfaceView>('widget_layout', { surface });
  }

  editWidgetLayout(edit: SurfaceEdit): Promise<SurfaceView> {
    return this.command<SurfaceView>('edit_widget_layout', { edit });
  }

  popoverValues(): Promise<PopoverSurfaceValues> {
    return this.command<PopoverSurfaceValues>('popover_values');
  }

  popoverPreview(): Promise<PopoverSurfaceValues> {
    return this.command<PopoverSurfaceValues>('popover_preview');
  }

  popoverSnapshot(): Promise<PopoverSurfaceValues> {
    return this.command<PopoverSurfaceValues>('popover_snapshot');
  }

  runWidgetAction(action: WidgetActionId): Promise<void> {
    return this.command<void>('run_widget_action', { action });
  }

  onPopoverValues(listener: (v: PopoverSurfaceValues) => void): Unsubscribe {
    return this.popoverChannel.add(listener);
  }

  onWidgetLayout(listener: (surface: DynoSurface) => void): Unsubscribe {
    return this.layoutChannel.add(listener);
  }

  onNavigate(listener: (to: Navigate) => void): Unsubscribe {
    return this.navigateChannel.add(listener);
  }

  takeNavigation(): Promise<Navigate | null> {
    return this.command<Navigate | null>('take_navigation');
  }

  metricReadings(queries: ReadingQuery[]): Promise<MetricReading[]> {
    return this.command<MetricReading[]>('metric_readings', { queries });
  }

  widgetChoices(): Promise<WidgetChoices> {
    return this.command<WidgetChoices>('widget_choices');
  }

  // ----------------------------------------------------------------------------------------------------- tray

  /** The shell's `window_focus` raises the main window; the popover then closes. */
  async showMainWindow(): Promise<void> {
    await this.command<void>('plugin:paddock-shell|window_focus', { label: 'main' });
    await this.hideTray();
  }

  fitTray(height: number): Promise<void> {
    return this.command<void>('plugin:paddock-shell|tray_fit_height', { height });
  }

  hideTray(): Promise<void> {
    return this.command<void>('plugin:paddock-shell|tray_close');
  }

  quitApp(): Promise<void> {
    return this.command<void>('plugin:paddock-shell|app_quit');
  }

  // ----------------------------------------------------------------------------------------------- extras, backup

  changes(query: ChangesQuery): Promise<ChangeTimeline> {
    return this.request({ changes: { query } });
  }

  gpuProcesses(): Promise<GpuReport> {
    return this.request('gpuProcesses');
  }

  /** The shell shows the OS save dialog and asks the agent to write the report there. */
  saveInventoryReport(format: InventoryFormat): Promise<SavedInventory | null> {
    return this.command<SavedInventory | null>('save_inventory_report', { format });
  }

  /** The shell opens the OS taskbar settings. */
  openTaskbarSettings(): Promise<void> {
    return this.command<void>('open_taskbar_settings', undefined);
  }

  /** The shell shows the OS folder dialog and answers the chosen path (or null). */
  pickFolder(): Promise<string | null> {
    return this.command<string | null>('pick_folder', undefined);
  }

  maintenance(): Promise<MaintenanceStatus> {
    return this.request('maintenance');
  }

  backup(): Promise<BackupReport> {
    return this.request('backup');
  }

  // ------------------------------------------------------------------------------------------------------ jobs

  async startJob(spec: JobSpec): Promise<number> {
    return (await this.request<{ id: number }>({ startJob: { spec } })).id;
  }

  jobResult(id: number): Promise<JobOutput> {
    return this.request({ jobResult: { id } });
  }

  // --------------------------------------------------------------------------------------------------- plumbing

  private request<T>(request: DynoRequest): Promise<T> {
    return this.command<T>('dyno', { request });
  }

  private async command<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await (args === undefined ? this.call<T>(cmd) : this.call<T>(cmd, args));
    } catch (e) {
      throw toCommandError(e);
    }
  }
}
