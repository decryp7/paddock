import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
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
  SystemInventory,
  BenchComparison,
  BenchOverview,
  StartBenchmark,
  Timestamp,
  TuningSuggestion,
  WeeklyReport,
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

/** The push events of the shell. */
export const EVENTS = {
  live: 'dyno://live',
  alert: 'dyno://alert',
  activity: 'dyno://activity',
  agent: 'dyno://agent',
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

  act(target: ProcessRef, action: ProcessAction): Promise<ActivityEntry> {
    return this.request({ act: { target, action } });
  }

  actOnGroup(groupId: string, action: ProcessAction): Promise<ActivityEntry> {
    return this.request({ actOnGroup: { groupId, action } });
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

  undo(activityId: string): Promise<ActivityEntry> {
    return this.request({ undo: { activityId } });
  }

  releaseAction(target: ProcessRef): Promise<ActivityEntry> {
    return this.request({ releaseAction: { target } });
  }

  // ---------------------------------------------------------------------------------------------------- rules

  rules(): Promise<RuleSet> {
    return this.request('rules');
  }

  saveRules(rules: RuleSet): Promise<RuleSet> {
    return this.request({ saveRules: { rules } });
  }

  setProfile(profile: ProfileId): Promise<RuleSet> {
    return this.request({ setProfile: { profile } });
  }

  pauseRules(paused: boolean): Promise<RuleSet> {
    return this.request({ pauseRules: { paused } });
  }

  // -------------------------------------------------------------------------------------------------- cleanup

  cleanupCategories(): Promise<CleanupCategory[]> {
    return this.request('cleanupCategories');
  }

  scanCleanup(categories: string[]): Promise<CleanupScan> {
    return this.request({ scanCleanup: { categories } });
  }

  runCleanup(scanId: string, itemIds: string[], mode: DeleteMode): Promise<CleanupOutcome> {
    return this.request({ runCleanup: { scanId, itemIds, mode } });
  }

  quarantine(): Promise<QuarantineEntry[]> {
    return this.request('quarantine');
  }

  restoreQuarantine(id: string): Promise<void> {
    return this.request({ restoreQuarantine: { id } });
  }

  diskUsage(path: string | null): Promise<SizeNode> {
    return this.request({ diskUsage: { path } });
  }

  // ---------------------------------------------------------------------------------------- health and tuning

  health(): Promise<HealthReport> {
    return this.request('health');
  }

  tuning(): Promise<TuningSuggestion[]> {
    return this.request('tuning');
  }

  applyTuning(id: string): Promise<ActivityEntry> {
    return this.request({ applyTuning: { id } });
  }

  revertTuning(id: string): Promise<ActivityEntry> {
    return this.request({ revertTuning: { id } });
  }

  startupEntries(): Promise<StartupEntry[]> {
    return this.request('startupEntries');
  }

  setStartupState(id: string, state: StartupState): Promise<ActivityEntry> {
    return this.request({ setStartupState: { id, state } });
  }

  // ---------------------------------------------------------------------------------- reports, settings, plugins

  inventory(): Promise<SystemInventory> {
    return this.request('inventory');
  }

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

  weeklyReport(): Promise<WeeklyReport> {
    return this.request('weeklyReport');
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

  // --------------------------------------------------------------------------------------------------- plumbing

  private request<T>(request: DynoRequest): Promise<T> {
    return this.command<T>('dyno', { request });
  }

  private async command<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject({ code: 'timeout', message: 'Dyno did not answer in time.' }), this.timeoutMs);
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
