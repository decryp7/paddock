import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ActivityReply,
  CacheCleaned,
  CacheInfo,
  CacheLimits,
  CachePlan,
  ChannelSettings,
  DebriefStatus,
  EngineReport,
  Handoff,
  InsightsReply,
  PruneResult,
  PrunePreview,
  PruneSelection,
  BackendEvent,
  CleanupReport,
  Config,
  ConfigEdit,
  ConfigPreview,
  ConfigSaved,
  ConnectionState,
  DiskReply,
  KeepFor,
  ProcessList,
  RunnerConfigReply,
  LogLine,
  MetricsReply,
  RegisterRequest,
  RunDetails,
  RunnerConfig,
  ServiceInstall,
  SettingsReply,
  StatusReply,
  Version,
} from './api.types';
import type { Platform, PitcrewBackend } from './backend';
import { toCommandError } from './command-error';

/** Calls one Tauri command (`invoke`, or a fake in tests). */
export type CommandCall = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
/** Subscribes to one Tauri event (`listen`, or a fake in tests). */
export type EventListen = <T>(event: string, handler: (event: { payload: T }) => void) => Promise<UnlistenFn>;

/** The runner actions of the `runner_action` command. */
export type RunnerAction = 'start' | 'stop' | 'restart' | 'drain' | 'resume' | 'checkUpdate' | 'applyUpdate' | 'cancelUpdate';

/** The event the shell pushes `BackendEvent`s on. */
export const EVENT_NAME = 'pitcrew://event';
/** The event the shell asks the UI to show a page on (payload: a route such as `/logs/build-pc`). */
export const NAVIGATE_EVENT = 'pitcrew://navigate';

/**
 * The real backend: the Tauri commands of `app/src-tauri/src/commands.rs`. Argument keys are camelCase; Tauri maps
 * them to the commands' snake_case parameters. Every rejection becomes a typed `CommandError`.
 */
/** The platform the webview's user agent names (only for the window chrome until the shell answers). */
export function platformFromUserAgent(ua: string): Platform {
  if (/Windows/i.test(ua)) return 'windows';
  if (/Mac OS X|Macintosh/i.test(ua)) return 'macos';
  if (/Linux|X11/i.test(ua)) return 'linux';
  return 'browser';
}

export class TauriBackend implements PitcrewBackend {
  private platformName: Platform = 'browser';

  constructor(
    private readonly call: CommandCall = invoke,
    private readonly listenTo: EventListen = listen,
  ) {}

  /**
   * Asks the shell for the platform at start-up (the interface's `platform()` is synchronous). The window never waits
   * for the answer (the start-up contract reports ready on its own, rule 12): until it comes, the platform is the user
   * agent's guess, which only picks the window chrome.
   */
  async init(): Promise<void> {
    this.platformName = platformFromUserAgent(globalThis.navigator?.userAgent ?? '');
    void this.call<Platform>('platform_name').then(
      (p) => {
        this.platformName = p;
      },
      () => undefined, // Window chrome falls back to the user agent's guess; nothing else depends on it.
    );
  }

  platform(): Platform {
    return this.platformName;
  }

  status(): Promise<StatusReply> {
    return this.run('runner_status');
  }

  /** The connection to the service (also pushed as `connection` events). */
  connectionState(): Promise<ConnectionState> {
    return this.run('connection_state');
  }

  watch(listener: (event: BackendEvent) => void): () => void {
    let unlisten: UnlistenFn | null = null;
    let closed = false;
    this.listenTo<BackendEvent>(EVENT_NAME, (e) => listener(e.payload)).then(
      (fn) => {
        if (closed) {
          fn();
          return;
        }
        unlisten = fn;
        // The current connection, so a subscriber does not wait for the next change to learn it.
        this.connectionState().then(
          (connection) => {
            if (!closed) listener({ kind: 'connection', connection });
          },
          () => undefined,
        );
      },
      () => undefined,
    );
    return () => {
      closed = true;
      unlisten?.();
    };
  }

  takePendingNavigation(): Promise<string | null> {
    return this.run('take_pending_navigation');
  }

  navigations(listener: (route: string) => void): () => void {
    let unlisten: UnlistenFn | null = null;
    let closed = false;
    this.listenTo<string>(NAVIGATE_EVENT, (e) => listener(e.payload)).then(
      (fn) => {
        if (closed) fn();
        else unlisten = fn;
      },
      () => undefined,
    );
    return () => {
      closed = true;
      unlisten?.();
    };
  }

  start(runner: string): Promise<void> {
    return this.action(runner, 'start');
  }
  stop(runner: string): Promise<void> {
    return this.action(runner, 'stop');
  }
  restart(runner: string): Promise<void> {
    return this.action(runner, 'restart');
  }
  drain(runner: string): Promise<void> {
    return this.action(runner, 'drain');
  }
  resume(runner: string): Promise<void> {
    return this.action(runner, 'resume');
  }
  checkUpdate(runner: string): Promise<void> {
    return this.action(runner, 'checkUpdate');
  }
  applyUpdate(runner: string): Promise<void> {
    return this.action(runner, 'applyUpdate');
  }
  cancelUpdate(runner: string): Promise<void> {
    return this.action(runner, 'cancelUpdate');
  }
  skipVersion(runner: string, version: Version): Promise<void> {
    return this.run('skip_version', { runner, version });
  }
  pinVersion(runner: string, version: Version | null): Promise<void> {
    return this.run('pin_version', { runner, version });
  }

  logs(runner: string, after: number | null, limit: number): Promise<LogLine[]> {
    return this.run('runner_logs', { runner, after, limit });
  }
  searchLogs(runner: string, query: string, limit: number): Promise<LogLine[]> {
    return this.run('search_logs', { runner, query, limit });
  }

  activity(runner: string, days: number): Promise<ActivityReply> {
    return this.run('runner_activity', { runner, days, utcOffsetMinutes: -new Date().getTimezoneOffset() });
  }
  metrics(runner: string | null, rangeHours: number): Promise<MetricsReply> {
    return this.run('machine_metrics', { runner, rangeHours });
  }
  disk(runner: string): Promise<DiskReply> {
    return this.run('runner_disk', { runner });
  }
  cleanDisk(runner: string): Promise<CleanupReport> {
    return this.job('clean_disk', { runner });
  }
  runDetails(runner: string): Promise<RunDetails | null> {
    return this.run('run_details', { runner });
  }

  runnerConfig(runner: string): Promise<RunnerConfigReply> {
    return this.run('runner_config_get', { runner });
  }
  previewRunnerConfig(runner: string, edit: ConfigEdit): Promise<ConfigPreview> {
    return this.run('runner_config_preview', { runner, edit });
  }
  saveRunnerConfig(runner: string, edit: ConfigEdit, base: string): Promise<ConfigSaved> {
    return this.run('runner_config_save', { runner, edit, base });
  }
  runnerProcesses(runner: string): Promise<ProcessList> {
    return this.run('runner_processes', { runner });
  }
  stopProcess(runner: string, pid: number, startedAt: number | null, allowRunner: boolean): Promise<void> {
    return this.run('runner_process_stop', { runner, pid, startedAt, allowRunner });
  }
  keepProcess(runner: string, pid: number, startedAt: number, extra: KeepFor): Promise<void> {
    return this.run('runner_process_keep', { runner, pid, startedAt, extra });
  }

  insights(days: number): Promise<InsightsReply> {
    return this.run('fleet_insights', { days, utcOffsetMinutes: -new Date().getTimezoneOffset() });
  }
  engineReport(): Promise<EngineReport> {
    return this.run('engine_report');
  }
  previewEnginePrune(selection: PruneSelection): Promise<PrunePreview> {
    return this.run('engine_prune_preview', { selection });
  }
  pruneEngine(selection: PruneSelection): Promise<PruneResult> {
    return this.job('engine_prune', { selection });
  }
  cacheReport(runner: string): Promise<CacheInfo> {
    return this.run('cache_report', { runner });
  }
  previewCacheClean(runner: string, limits: CacheLimits): Promise<CachePlan> {
    return this.run('cache_preview', { runner, limits });
  }
  cleanCache(runner: string, limits: CacheLimits): Promise<CacheCleaned> {
    return this.job('cache_clean', { runner, limits });
  }
  debriefStatus(): Promise<DebriefStatus> {
    return this.run('debrief_status');
  }
  openInDebrief(runner: string, label: string, startedAt: number | null, finishedAt: number | null): Promise<Handoff> {
    return this.run('open_in_debrief', { runner, label, startedAt, finishedAt });
  }
  channels(): Promise<ChannelSettings> {
    return this.run('notification_channels_get');
  }
  saveChannels(settings: ChannelSettings): Promise<void> {
    return this.run('notification_channels_save', { settings });
  }
  setChannelSecret(key: string, value: string): Promise<void> {
    return this.run('channel_secret_set', { key, value });
  }
  clearChannelSecret(key: string): Promise<void> {
    return this.run('channel_secret_clear', { key });
  }
  hasChannelSecret(key: string): Promise<boolean> {
    return this.run('channel_secret_present', { key });
  }
  testChannel(id: string): Promise<string> {
    return this.run('channel_test', { id });
  }

  settings(): Promise<SettingsReply> {
    return this.run('get_settings');
  }
  saveSettings(config: Config): Promise<void> {
    return this.run('save_settings', { config });
  }
  addRunner(runner: RunnerConfig): Promise<void> {
    return this.run('add_runner', { runner });
  }
  removeRunner(runner: string): Promise<void> {
    return this.run('remove_runner', { runner });
  }
  register(request: RegisterRequest): Promise<void> {
    return this.job<void>('register_runner', { request });
  }

  hasApiToken(serverUrl: string): Promise<boolean> {
    return this.run('has_api_token', { serverUrl });
  }
  setApiToken(serverUrl: string, token: string): Promise<void> {
    return this.run('set_api_token', { serverUrl, token });
  }
  clearApiToken(serverUrl: string): Promise<void> {
    return this.run('clear_api_token', { serverUrl });
  }

  serviceInstall(): Promise<ServiceInstall> {
    return this.run('service_install_state');
  }
  installService(): Promise<void> {
    return this.run('install_service');
  }
  uninstallService(): Promise<void> {
    return this.run('uninstall_service');
  }
  repairService(): Promise<void> {
    return this.run('repair_service');
  }
  openLoginItems(): Promise<void> {
    return this.run('pitcrew_open_login_items');
  }

  autostart(): Promise<boolean> {
    return this.run('get_autostart');
  }
  setAutostart(enabled: boolean): Promise<void> {
    return this.run('set_autostart', { enabled });
  }
  exportDiagnostics(): Promise<string> {
    return this.run('export_diagnostics');
  }
  openUrl(url: string): Promise<void> {
    return this.run('open_url', { url });
  }
  revealLogs(runner: string): Promise<void> {
    return this.run('reveal_logs', { runner });
  }

  private action(runner: string, action: RunnerAction): Promise<void> {
    return this.run('runner_action', { runner, action });
  }

  private async run<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await (args === undefined ? this.call<T>(cmd) : this.call<T>(cmd, args));
    } catch (e) {
      throw toCommandError(e);
    }
  }

  /**
   * Long work is a job (rule 10-12): the command registers it and answers with its id at once; its progress and Cancel are
   * the shell's Jobs list. This waits for the job's end on `paddock://jobs` and reads its result. It never gives up on the
   * job (no timeout); a cancelled or failed job rejects with the reason.
   */
  private async job<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
    const ended = new Map<number, JobEnd>();
    let waiting: { id: number; resolve: (end: JobEnd) => void } | null = null;
    const unlisten = await this.listenTo<unknown>(JOBS_EVENT, (e) => {
      const end = jobEnd(e.payload);
      if (!end) return;
      ended.set(end.id, end);
      if (waiting?.id === end.id) waiting.resolve(end);
    });
    try {
      const id = await this.run<number>(cmd, args);
      const end = ended.get(id) ?? (await new Promise<JobEnd>((resolve) => (waiting = { id, resolve })));
      if (end.state === 'failed') throw toCommandError({ code: 'internal', message: end.message });
      if (end.state === 'cancelled') throw toCommandError({ code: 'cancelled', message: 'Cancelled.' });
      return (await this.run<T | null>('job_result', { id })) as T;
    } catch (e) {
      throw toCommandError(e);
    } finally {
      unlisten();
    }
  }
}

/** The event every job snapshot arrives on (the shell's `paddock://jobs`). */
export const JOBS_EVENT = 'paddock://jobs';

/** How a job ended, from its last snapshot. */
export type JobEnd = { id: number; state: 'done' } | { id: number; state: 'failed'; message: string } | { id: number; state: 'cancelled' };

/** A terminal job snapshot of the event, or `null` for anything else (the payload is parsed, never trusted). */
export function jobEnd(payload: unknown): JobEnd | null {
  if (typeof payload !== 'object' || payload === null) return null;
  const p = payload as { id?: unknown; state?: unknown; message?: unknown };
  if (typeof p.id !== 'number') return null;
  if (p.state === 'done' || p.state === 'cancelled') return { id: p.id, state: p.state };
  if (p.state === 'failed') return { id: p.id, state: 'failed', message: typeof p.message === 'string' ? p.message : 'The work failed.' };
  return null;
}
