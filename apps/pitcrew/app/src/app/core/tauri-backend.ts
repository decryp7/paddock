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
  AppUpdateInfo,
  AppUpdateStatus,
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
import { timeoutFor, withTimeout } from './call-timeout';
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
/** How long start-up waits for the shell to name the platform. */
export const PLATFORM_WAIT_MS = 1000;

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
   * Asks the shell for the platform at start-up (the interface's `platform()` is synchronous). Start-up waits for it
   * at most {@link PLATFORM_WAIT_MS}: in a window that was just created the first IPC call can stay unanswered, and
   * an app initializer that never resolves leaves the window blank for good. Until the shell answers, the platform
   * comes from the user agent (it only picks the window chrome); a late answer still replaces it.
   */
  async init(waitMs = PLATFORM_WAIT_MS): Promise<void> {
    this.platformName = platformFromUserAgent(globalThis.navigator?.userAgent ?? '');
    const asked = this.call<Platform>('platform_name').then(
      (p) => {
        this.platformName = p;
      },
      () => undefined, // Window chrome falls back to the user agent's guess; nothing else depends on it.
    );
    await Promise.race([asked, new Promise<void>((resolve) => setTimeout(resolve, waitMs))]);
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
    return this.run('clean_disk', { runner });
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
    return this.run('engine_prune', { selection });
  }
  cacheReport(runner: string): Promise<CacheInfo> {
    return this.run('cache_report', { runner });
  }
  previewCacheClean(runner: string, limits: CacheLimits): Promise<CachePlan> {
    return this.run('cache_preview', { runner, limits });
  }
  cleanCache(runner: string, limits: CacheLimits): Promise<CacheCleaned> {
    return this.run('cache_clean', { runner, limits });
  }
  debriefStatus(): Promise<DebriefStatus> {
    return this.run('debrief_status');
  }
  openInDebrief(runner: string, label: string, startedAt: number | null, finishedAt: number | null): Promise<Handoff> {
    return this.run('open_in_debrief', { runner, label, startedAt, finishedAt });
  }
  channels(): Promise<ChannelSettings> {
    return this.run('channels_get');
  }
  saveChannels(settings: ChannelSettings): Promise<void> {
    return this.run('channels_save', { settings });
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
    return this.run('register_runner', { request });
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
  openLoginItems(): Promise<void> {
    return this.run('open_login_items');
  }

  appUpdateStatus(): Promise<AppUpdateStatus> {
    return this.run('app_update_status');
  }
  checkAppUpdate(): Promise<AppUpdateInfo | null> {
    return this.run('check_app_update');
  }
  installAppUpdate(): Promise<void> {
    return this.run('install_app_update');
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
      return await withTimeout(args === undefined ? this.call<T>(cmd) : this.call<T>(cmd, args), timeoutFor(cmd));
    } catch (e) {
      throw toCommandError(e);
    }
  }
}
