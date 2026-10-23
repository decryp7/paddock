import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  AppInfo,
  CacheClearScope,
  CacheInfo,
  AppPermissions,
  AppUpdateStatus,
  BenchmarkResult,
  BuildCapabilities,
  EnginePinState,
  PinChange,
  CreatedKey,
  DiskUsage,
  SlotFit,
  SlotsChange,
  Download,
  Engine,
  AppDetail,
  ModelPreset,
  PresetDraft,
  PresetEntry,
  PresetPreview,
  History,
  HistoryRange,
  UsageRange,
  UsageSeries,
  HfModel,
  HfRepo,
  LiveUpdate,
  LocalModel,
  Overview,
  PacksInfo,
  PairingRequest,
  PitwallEvent,
  PlaygroundEvent,
  PlaygroundRequest,
  Preset,
  RequestContent,
  RequestRecord,
  ServiceInstall,
  Settings,
  UpdateCheckResult,
  UpdateCheckStatus,
  UpdateInfo,
  UpdateNotice,
  UpdateSchedule,
} from './api.types';
import type { PitwallBackend, Unsubscribe } from './backend';
import { toCommandError } from './command-error';

/** The Tauri event the app forwards every live update on. */
export const LIVE_EVENT = 'pitwall://live';

/** The Tauri primitives the backend uses, injectable so a spec can pin every command name and argument shape. */
export interface TauriBridge {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
  channel<T>(onMessage: (message: T) => void): unknown;
}

/* v8 ignore start -- the real Tauri primitives exist only inside the app's webview. */
const realBridge: TauriBridge = {
  invoke: (cmd, args) => invoke(cmd, args),
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
  channel: <T>(onMessage: (message: T) => void) => {
    const channel = new Channel<T>();
    channel.onmessage = onMessage;
    return channel;
  },
};
/* v8 ignore stop */

/**
 * The real backend: one Tauri command per method (`app/src-tauri/src/commands.rs`), named after the method in
 * snake_case, with the method's parameters as camelCase arguments. Rejections become typed {@link CommandError}s.
 */
export class TauriBackend implements PitwallBackend {
  constructor(private readonly bridge: TauriBridge = realBridge) {}

  // Dashboard
  overview(): Promise<Overview> {
    return this.run('overview');
  }
  history(range: HistoryRange): Promise<History> {
    return this.run('history', { range });
  }
  usageSeries(range: UsageRange, appId: string | null): Promise<UsageSeries> {
    return this.run('usage_series', { range, appId });
  }
  events(limit: number): Promise<PitwallEvent[]> {
    return this.run('events', { limit });
  }
  subscribe(listener: (update: LiveUpdate) => void): Unsubscribe {
    let stopped = false;
    let unlisten: (() => void) | null = null;
    this.bridge
      .listen<LiveUpdate>(LIVE_EVENT, (update) => {
        if (!stopped) listener(update);
      })
      .then(
        (u) => {
          if (stopped) u();
          else unlisten = u;
        },
        () => undefined,
      );
    return () => {
      stopped = true;
      unlisten?.();
    };
  }
  setServing(paused: boolean): Promise<void> {
    return this.run('set_serving', { paused });
  }
  releaseMemory(): Promise<number> {
    return this.run('release_memory');
  }
  cacheInfo(): Promise<CacheInfo> {
    return this.run('cache_info');
  }
  clearCache(scope: CacheClearScope): Promise<number> {
    return this.run('clear_cache', { scope });
  }
  applyPreset(preset: Preset): Promise<Settings> {
    return this.run('apply_preset', { preset });
  }
  cancelJob(id: string): Promise<void> {
    return this.run('cancel_job', { id });
  }

  // Models
  localModels(): Promise<LocalModel[]> {
    return this.run('local_models');
  }
  searchModels(query: string): Promise<HfModel[]> {
    return this.run('search_models', { query });
  }
  repoFiles(repo: string): Promise<HfRepo> {
    return this.run('repo_files', { repo });
  }
  downloadModel(repo: string, file: string, projector: string | null): Promise<Download> {
    return this.run('download_model', { repo, file, projector });
  }
  downloads(): Promise<Download[]> {
    return this.run('downloads');
  }
  cancelDownload(id: string): Promise<void> {
    return this.run('cancel_download', { id });
  }
  deleteModel(id: string): Promise<void> {
    return this.run('delete_model', { id });
  }
  loadModel(id: string): Promise<void> {
    return this.run('load_model', { id });
  }
  unloadModel(id: string): Promise<void> {
    return this.run('unload_model', { id });
  }
  pinModel(id: string, pinned: boolean): Promise<void> {
    return this.run('pin_model', { id, pinned });
  }
  setModelIdle(id: string, idleSecs: number | null): Promise<void> {
    return this.run('set_model_idle', { id, idleSecs });
  }
  setModelSlots(id: string, slots: number | null): Promise<SlotsChange> {
    return this.run('set_model_slots', { id, slots });
  }
  modelFit(id: string, context: number | null, slots: number | null): Promise<SlotFit> {
    return this.run('model_fit', { id, context, slots });
  }
  diskUsage(): Promise<DiskUsage> {
    return this.run('disk_usage');
  }
  packs(tag: string | null = null): Promise<PacksInfo> {
    return this.run('packs', { tag });
  }
  setModelEngine(id: string, engine: string | null): Promise<void> {
    return this.run('set_model_engine', { id, engine });
  }

  // Engines
  engines(): Promise<Engine[]> {
    return this.run('engines');
  }
  installBuild(buildId: string): Promise<Download> {
    return this.run('install_build', { buildId });
  }
  removeBuild(buildId: string, tag: string): Promise<void> {
    return this.run('remove_build', { buildId, tag });
  }
  selectBuild(choice: string): Promise<void> {
    return this.run('select_build', { choice });
  }
  rollbackBuild(): Promise<PinChange> {
    return this.run('rollback_build');
  }
  enginePins(): Promise<EnginePinState> {
    return this.run('engine_pins');
  }
  setGlobalPin(buildId: string | null): Promise<PinChange> {
    return this.run('set_global_pin', { buildId });
  }
  setModelPin(modelId: string, buildId: string | null): Promise<PinChange> {
    return this.run('set_model_pin', { modelId, buildId });
  }
  buildCapabilities(): Promise<BuildCapabilities[]> {
    return this.run('build_capabilities');
  }

  // Model presets and aliases
  presets(): Promise<PresetEntry[]> {
    return this.run('presets');
  }
  createPreset(preset: PresetDraft): Promise<ModelPreset> {
    return this.run('create_preset', { preset });
  }
  updatePreset(alias: string, preset: PresetDraft): Promise<ModelPreset> {
    return this.run('update_preset', { alias, preset });
  }
  async deletePreset(alias: string): Promise<void> {
    await this.run('delete_preset', { alias });
  }
  resolvePreset(preset: PresetDraft, editing: string | null): Promise<PresetPreview> {
    return this.run('resolve_preset', { preset, editing });
  }

  // Model update notices
  updateCheckStatus(): Promise<UpdateCheckStatus> {
    return this.run('update_check_status');
  }
  updateNotices(): Promise<UpdateNotice[]> {
    return this.run('update_notices');
  }
  checkUpdatesNow(): Promise<UpdateCheckResult> {
    return this.run('check_updates_now');
  }
  setUpdateCheck(enabled: boolean, schedule: UpdateSchedule): Promise<UpdateCheckStatus> {
    return this.run('set_update_check', { enabled, schedule });
  }
  dismissUpdate(id: string, snoozeDays: number | null): Promise<UpdateNotice[]> {
    return this.run('dismiss_update', { id, snoozeDays });
  }

  // Apps and keys
  apps(): Promise<AppInfo[]> {
    return this.run('apps');
  }
  appDetail(appId: string): Promise<AppDetail> {
    return this.run('app_detail', { appId });
  }
  createApp(name: string): Promise<AppInfo> {
    return this.run('create_app', { name });
  }
  deleteApp(id: string): Promise<void> {
    return this.run('delete_app', { id });
  }
  createKey(appId: string): Promise<CreatedKey> {
    return this.run('create_key', { appId });
  }
  revokeKey(appId: string, keyId: string): Promise<void> {
    return this.run('revoke_key', { appId, keyId });
  }
  setPermissions(appId: string, permissions: AppPermissions): Promise<AppInfo> {
    return this.run('set_permissions', { appId, permissions });
  }
  approvePairing(id: string, permissions: AppPermissions): Promise<void> {
    return this.run('approve_pairing', { id, permissions });
  }
  denyPairing(id: string): Promise<void> {
    return this.run('deny_pairing', { id });
  }
  pairings(): Promise<PairingRequest[]> {
    return this.run('pairings');
  }

  // Request inspector
  requests(limit: number): Promise<RequestRecord[]> {
    return this.run('requests', { limit });
  }
  requestContent(id: string): Promise<RequestContent> {
    return this.run('request_content', { id });
  }

  // Playground and benchmarks
  chat(request: PlaygroundRequest, onEvent: (event: PlaygroundEvent) => void): Unsubscribe {
    let stopped = false;
    let id: string | null = null;
    const cancel = (streamId: string) => this.bridge.invoke('chat_cancel', { id: streamId }).catch(() => undefined);
    const channel = this.bridge.channel<PlaygroundEvent>((event) => {
      if (!stopped) onEvent(event);
    });
    this.run<string>('chat', { request, onEvent: channel }).then(
      (streamId) => {
        id = streamId;
        if (stopped) void cancel(streamId);
      },
      (error) => {
        if (!stopped) onEvent({ kind: 'error', error });
      },
    );
    return () => {
      if (stopped) return;
      stopped = true;
      if (id !== null) void cancel(id);
    };
  }
  benchmarks(): Promise<BenchmarkResult[]> {
    return this.run('benchmarks');
  }
  runBenchmark(modelId: string): Promise<BenchmarkResult> {
    return this.run('run_benchmark', { modelId });
  }

  // Settings
  settings(): Promise<Settings> {
    return this.run('settings');
  }
  saveSettings(settings: Settings): Promise<Settings> {
    return this.run('save_settings', { settings });
  }
  appUpdateStatus(): Promise<AppUpdateStatus> {
    return this.run('app_update_status');
  }
  checkAppUpdate(): Promise<UpdateInfo | null> {
    return this.run('check_app_update');
  }
  installAppUpdate(): Promise<void> {
    return this.run('install_app_update');
  }

  // The server as an OS service, and start at log-in
  serviceState(): Promise<ServiceInstall> {
    return this.run('service_state');
  }
  installService(): Promise<ServiceInstall> {
    return this.run('install_service');
  }
  uninstallService(): Promise<ServiceInstall> {
    return this.run('uninstall_service');
  }
  declineService(): Promise<void> {
    return this.run('decline_service');
  }
  openLoginItems(): Promise<void> {
    return this.run('open_login_items');
  }
  autostart(): Promise<boolean> {
    return this.run('get_autostart');
  }
  setAutostart(enabled: boolean): Promise<boolean> {
    return this.run('set_autostart', { enabled });
  }

  private async run<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await (args === undefined ? this.bridge.invoke<T>(cmd) : this.bridge.invoke<T>(cmd, args));
    } catch (e) {
      throw toCommandError(e);
    }
  }
}
