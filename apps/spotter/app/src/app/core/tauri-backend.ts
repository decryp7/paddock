import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AppEvent,
  AppInfo,
  AppUpdateCheck,
  AppUpdateStatus,
  Dashboard,
  DashboardRange,
  DiscoveredFeed,
  ImportFormat,
  ImportResult,
  OpenTarget,
  PatternSelection,
  PatternTest,
  Preset,
  RendererStatus,
  SchemePreset,
  Settings,
  SourceKind,
  SourcePreview,
  StatePush,
  VersionSchemeId,
  WatchDetail,
  WatchDraft,
  WatchSummary,
} from './api.types';
import type { ReleaseTarget, SpotterBackend } from './backend';
import { timeoutFor, withTimeout } from './call-timeout';
import { toCommandError } from './command-error';
import type { Platform } from './window-chrome';

/** Calls one Tauri command (`invoke`, or a fake in tests). */
export type CommandCall = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
/** Subscribes to one Tauri event (`listen`, or a fake in tests). */
export type EventListen = <T>(event: string, handler: (event: { payload: T }) => void) => Promise<UnlistenFn>;

/** The event the shell pushes `StatePush` on. */
export const STATE_EVENT = 'spotter://state';
/** The event the shell asks the main window to show a route on (payload: the route string). */
export const NAVIGATE_EVENT = 'spotter://navigate';

/** The platform the webview's user agent names (only for the window chrome; no IPC before the first paint). */
export function platformFromUserAgent(ua: string): Platform {
  if (/Windows/i.test(ua)) return 'windows';
  if (/Mac OS X|Macintosh/i.test(ua)) return 'macos';
  if (/Linux|X11/i.test(ua)) return 'linux';
  return 'browser';
}

/**
 * The real backend: the Tauri commands of `app/src-tauri/src/commands.rs`. Command names are the snake_case of the
 * method names; argument keys are camelCase like the TypeScript parameters. Every rejection becomes a `CommandError`.
 */
export class TauriBackend implements SpotterBackend {
  constructor(
    private readonly call: CommandCall = invoke,
    private readonly listenTo: EventListen = listen,
  ) {}

  appInfo(): Promise<AppInfo> {
    return this.run('app_info');
  }
  listWatches(): Promise<WatchSummary[]> {
    return this.run('list_watches');
  }
  getWatch(id: string): Promise<WatchDetail> {
    return this.run('get_watch', { id });
  }
  sourceKinds(): Promise<SourceKind[]> {
    return this.run('source_kinds');
  }
  testSource(draft: WatchDraft, secrets: Record<string, string>): Promise<SourcePreview> {
    return this.run('test_source', { draft, secrets });
  }
  saveWatch(draft: WatchDraft, secrets: Record<string, string>): Promise<WatchSummary> {
    return this.run('save_watch', { draft, secrets });
  }
  deleteWatch(id: string): Promise<void> {
    return this.run('delete_watch', { id });
  }
  setPaused(id: string, paused: boolean): Promise<void> {
    return this.run('set_paused', { id, paused });
  }
  checkNow(id: string | null): Promise<void> {
    return this.run('check_now', { id });
  }
  acknowledge(id: string, version: string | null): Promise<void> {
    return this.run('acknowledge', { id, version });
  }
  resetAcknowledgement(id: string): Promise<void> {
    return this.run('reset_acknowledgement', { id });
  }
  setInstalled(id: string, version: string | null): Promise<void> {
    return this.run('set_installed', { id, version });
  }
  acknowledgeProduct(id: string, product: string): Promise<void> {
    return this.run('acknowledge_product', { id, product });
  }
  openRelease(id: string, version: string, target?: ReleaseTarget): Promise<void> {
    return this.run('open_release', { id, version, product: target?.product ?? null, artifact: target?.artifact ?? null });
  }
  downloadRelease(id: string, version: string, target?: ReleaseTarget): Promise<string> {
    return this.run('download_release', { id, version, product: target?.product ?? null, artifact: target?.artifact ?? null });
  }
  suggestPattern(example: string, selections: PatternSelection[]): Promise<string> {
    return this.run('suggest_pattern', { example, selections });
  }
  testPattern(versions: VersionSchemeId, versionPattern: string | null, missingOlder: boolean, text: string): Promise<PatternTest> {
    return this.run('test_pattern', { versions, versionPattern, missingOlder, text });
  }
  versionSchemes(): Promise<SchemePreset[]> {
    return this.run('version_schemes');
  }
  dashboard(range: DashboardRange): Promise<Dashboard> {
    return this.run('dashboard', { range });
  }
  events(limit: number, beforeMs: number | null): Promise<AppEvent[]> {
    return this.run('events', { limit, beforeMs });
  }
  getSettings(): Promise<Settings> {
    return this.run('get_settings');
  }
  saveSettings(settings: Settings): Promise<Settings> {
    return this.run('save_settings', { settings });
  }
  pauseNotifications(minutes: number | null): Promise<Settings> {
    return this.run('pause_notifications', { minutes });
  }
  getAutostart(): Promise<boolean> {
    return this.run('get_autostart');
  }
  setAutostart(enabled: boolean): Promise<boolean> {
    return this.run('set_autostart', { enabled });
  }
  open(target: OpenTarget): Promise<void> {
    return this.run('open', { target });
  }
  chooseFolder(): Promise<string | null> {
    return this.run('choose_folder');
  }
  chooseProgram(): Promise<string | null> {
    return this.run('choose_program');
  }
  presets(): Promise<Preset[]> {
    return this.run('presets');
  }
  discoverFeeds(url: string): Promise<DiscoveredFeed[]> {
    return this.run('discover_feeds', { url });
  }
  rendererStatus(): Promise<RendererStatus> {
    return this.run('renderer_status');
  }
  installRenderer(): Promise<RendererStatus> {
    return this.run('install_renderer');
  }
  importList(format: ImportFormat, text: string): Promise<ImportResult> {
    return this.run('import_list', { format, text });
  }
  appUpdateStatus(): Promise<AppUpdateStatus> {
    return this.run('app_update_status');
  }
  checkAppUpdate(): Promise<AppUpdateCheck> {
    return this.run('check_app_update');
  }
  installAppUpdate(): Promise<void> {
    return this.run('install_app_update');
  }
  showMainWindow(route: string | null): Promise<void> {
    return this.run('show_main_window', { route });
  }
  hideTray(): Promise<void> {
    return this.run('hide_tray');
  }

  onState(handler: (push: StatePush) => void): Promise<() => void> {
    return this.listenTo<StatePush>(STATE_EVENT, (e) => handler(e.payload));
  }
  onNavigate(handler: (route: string) => void): Promise<() => void> {
    return this.listenTo<string>(NAVIGATE_EVENT, (e) => handler(e.payload));
  }

  private async run<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await withTimeout(args === undefined ? this.call<T>(cmd) : this.call<T>(cmd, args), timeoutFor(cmd));
    } catch (e) {
      throw toCommandError(e);
    }
  }
}
