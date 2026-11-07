import type { AppPermissions, JobEvent, LiveUpdate, PlaygroundEvent, PlaygroundRequest, PresetDraft, Settings } from './api.types';
import type { PitwallBackend } from './backend';
import { JOB_METHODS } from './backend';
import { LIVE_EVENT, POPOVER_EVENT, TauriBackend, type TauriBridge } from './tauri-backend';

interface Call {
  cmd: string;
  args?: Record<string, unknown>;
}

function fakeBridge(reply: (cmd: string) => unknown = () => null) {
  const calls: Call[] = [];
  const listeners: ((p: unknown) => void)[] = [];
  const unlistened: string[] = [];
  const channels: ((m: unknown) => void)[] = [];
  const bridge: TauriBridge = {
    invoke: <T>(cmd: string, args?: Record<string, unknown>) => {
      calls.push(args === undefined ? { cmd } : { cmd, args });
      const r = reply(cmd);
      return (r instanceof Promise ? r : Promise.resolve(r)) as Promise<T>;
    },
    listen: <T>(event: string, handler: (p: T) => void) => {
      listeners.push(handler as (p: unknown) => void);
      return Promise.resolve(() => void unlistened.push(event));
    },
    channel: <T>(onMessage: (m: T) => void) => {
      channels.push(onMessage as (m: unknown) => void);
      return { channel: channels.length };
    },
  };
  return { bridge, calls, listeners, unlistened, channels };
}

const PERMS: AppPermissions = {
  models: null,
  maxPriority: 'interactive',
  tools: false,
  extendedApis: false,
  cacheResponses: false,
  limits: { requestsPerMin: null, tokensPerDay: null, maxConcurrent: null, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null },
};
const SETTINGS = { general: { theme: 'dark' } } as unknown as Settings;

const DRAFT: PresetDraft = { alias: 'debrief-explain', target: 'auto:chat', context: null, temperature: 0.3, topP: null, maxTokens: null, systemPrompt: null, cacheResponses: false, keepWarm: false };

/**
 * The command table the Rust side implements (`app/src-tauri/src/commands.rs`). Each row: the call, the command
 * name and the exact arguments. Changing a row is a contract change: mirror it in Rust in the same commit.
 */
const TABLE: [string, (b: PitwallBackend) => Promise<unknown>, string, Record<string, unknown> | undefined][] = [
  ['overview', (b) => b.overview(), 'overview', undefined],
  ['history', (b) => b.history('24h'), 'history', { range: '24h' }],
  ['usageSeries', (b) => b.usageSeries('7d', 'pitcrew'), 'usage_series', { range: '7d', appId: 'pitcrew' }],
  ['usageSeries (all apps)', (b) => b.usageSeries('24h', null), 'usage_series', { range: '24h', appId: null }],
  ['events', (b) => b.events(50), 'events', { limit: 50 }],
  ['setServing', (b) => b.setServing(true), 'set_serving', { paused: true }],
  ['releaseMemory', (b) => b.releaseMemory(), 'release_memory', undefined],
  ['popoverView', (b) => b.popoverView(), 'popover_view', undefined],
  ['popoverValues', (b) => b.popoverValues(), 'popover_values', undefined],
  ['popoverHeader', (b) => b.popoverHeader(), 'popover_header', undefined],
  ['popoverShape', (b) => b.popoverShape(), 'popover_shape', undefined],
  ['editPopover', (b) => b.editPopover('resetToDefaults'), 'edit_popover', { edit: 'resetToDefaults' }],
  ['cacheInfo', (b) => b.cacheInfo(), 'cache_info', undefined],
  ['clearCache', (b) => b.clearCache({ kind: 'app', appId: 'debrief' }), 'clear_cache', { scope: { kind: 'app', appId: 'debrief' } }],
  ['applyPreset', (b) => b.applyPreset('lowMemory'), 'apply_preset', { preset: 'lowMemory' }],
  ['cancelJob', (b) => b.cancelJob('j1'), 'cancel_job', { id: 'j1' }],
  ['jobCancel', (b) => b.jobCancel(7), 'app_job_cancel', { id: 7 }],
  ['jobStopWaiting', (b) => b.jobStopWaiting(7), 'app_job_stop_waiting', { id: 7 }],
  ['localModels', (b) => b.localModels(), 'local_models', undefined],
  ['searchModels', (b) => b.searchModels('qwen'), 'search_models', { query: 'qwen' }],
  ['repoFiles', (b) => b.repoFiles('a/b'), 'repo_files', { repo: 'a/b' }],
  ['downloadModel', (b) => b.downloadModel('a/b', 'f.gguf', null), 'download_model', { repo: 'a/b', file: 'f.gguf', projector: null }],
  ['downloads', (b) => b.downloads(), 'downloads', undefined],
  ['cancelDownload', (b) => b.cancelDownload('d1'), 'cancel_download', { id: 'd1' }],
  ['deleteModel', (b) => b.deleteModel('m1'), 'delete_model', { id: 'm1' }],
  ['unloadModel', (b) => b.unloadModel('m1'), 'unload_model', { id: 'm1' }],
  ['pinModel', (b) => b.pinModel('m1', true), 'pin_model', { id: 'm1', pinned: true }],
  ['setModelIdle', (b) => b.setModelIdle('m1', null), 'set_model_idle', { id: 'm1', idleSecs: null }],
  ['setModelSlots', (b) => b.setModelSlots('m1', 4), 'set_model_slots', { id: 'm1', slots: 4 }],
  ['setModelSlots default', (b) => b.setModelSlots('m1', null), 'set_model_slots', { id: 'm1', slots: null }],
  ['modelFit', (b) => b.modelFit('m1', 8192, 2), 'model_fit', { id: 'm1', context: 8192, slots: 2 }],
  ['diskUsage', (b) => b.diskUsage(), 'disk_usage', undefined],
  ['engines', (b) => b.engines(), 'engines', undefined],
  ['packs', (b) => b.packs('code'), 'packs', { tag: 'code' }],
  ['packs without a tag', (b) => b.packs(), 'packs', { tag: null }],
  ['quitApp', (b) => b.quitApp(true, false), 'quit_app', { stopServing: true, rememberChoice: false }],
  ['takeQuitAsk', (b) => b.takeQuitAsk(), 'take_quit_ask', undefined],
  ['chatEnabled', (b) => b.chatEnabled(), 'chat_enabled', undefined],
  ['connectedApps', (b) => b.connectedApps(), 'connected_apps', undefined],
  ['setAppKey', (b) => b.setAppKey('dyno', 'pmk_x'), 'set_app_key', { appId: 'dyno', key: 'pmk_x' }],
  ['connectApp', (b) => b.connectApp('dyno'), 'connect_app', { appId: 'dyno' }],
  ['forgetApp', (b) => b.forgetApp('dyno'), 'forget_app', { appId: 'dyno' }],
  ['openConnectedApp', (b) => b.openConnectedApp('dyno'), 'open_connected_app', { appId: 'dyno' }],
  ['revokeToolApproval', (b) => b.revokeToolApproval('dyno', 'disks'), 'revoke_tool_approval', { appId: 'dyno', tool: 'disks' }],
  ['chatModels', (b) => b.chatModels(), 'chat_models_list', undefined],
  ['setChatModel', (b) => b.setChatModel('m1'), 'set_chat_model', { modelId: 'm1' }],
  ['removeLeftover', (b) => b.removeLeftover('pack-llamafile'), 'remove_leftover', { id: 'pack-llamafile' }],
  ['retryEngineSetup', (b) => b.retryEngineSetup(), 'retry_engine_setup', undefined],
  ['installBuild', (b) => b.installBuild('cuda-13'), 'install_build', { buildId: 'cuda-13' }],
  ['removeBuild', (b) => b.removeBuild('cpu', 'b1'), 'remove_build', { buildId: 'cpu', tag: 'b1' }],
  ['selectBuild', (b) => b.selectBuild('auto'), 'select_build', { choice: 'auto' }],
  ['rollbackBuild', (b) => b.rollbackBuild(), 'rollback_build', undefined],
  ['enginePins', (b) => b.enginePins(), 'engine_pins', undefined],
  ['setGlobalPin', (b) => b.setGlobalPin('vulkan'), 'set_global_pin', { buildId: 'vulkan' }],
  ['setGlobalPin off', (b) => b.setGlobalPin(null), 'set_global_pin', { buildId: null }],
  ['setModelPin', (b) => b.setModelPin('m1', 'cpu'), 'set_model_pin', { modelId: 'm1', buildId: 'cpu' }],
  ['buildCapabilities', (b) => b.buildCapabilities(), 'build_capabilities', undefined],
  ['apps', (b) => b.apps(), 'apps', undefined],
  ['appDetail', (b) => b.appDetail('pitcrew'), 'app_detail', { appId: 'pitcrew' }],
  ['presets', (b) => b.presets(), 'presets', undefined],
  ['createPreset', (b) => b.createPreset(DRAFT), 'create_preset', { preset: DRAFT }],
  ['updatePreset', (b) => b.updatePreset('old', DRAFT), 'update_preset', { alias: 'old', preset: DRAFT }],
  ['deletePreset', (b) => b.deletePreset('old'), 'delete_preset', { alias: 'old' }],
  ['resolvePreset', (b) => b.resolvePreset(DRAFT, 'old'), 'resolve_preset', { preset: DRAFT, editing: 'old' }],
  ['updateCheckStatus', (b) => b.updateCheckStatus(), 'update_check_status', undefined],
  ['updateNotices', (b) => b.updateNotices(), 'update_notices', undefined],
  ['setUpdateCheck', (b) => b.setUpdateCheck(true, 'daily'), 'set_update_check', { enabled: true, schedule: 'daily' }],
  ['dismissUpdate', (b) => b.dismissUpdate('m#betterQuant', 7), 'dismiss_update', { id: 'm#betterQuant', snoozeDays: 7 }],
  ['dismissUpdate for good', (b) => b.dismissUpdate('m#betterQuant', null), 'dismiss_update', { id: 'm#betterQuant', snoozeDays: null }],
  ['createApp', (b) => b.createApp('Tool'), 'create_app', { name: 'Tool' }],
  ['deleteApp', (b) => b.deleteApp('a1'), 'delete_app', { id: 'a1' }],
  ['createKey', (b) => b.createKey('a1'), 'create_key', { appId: 'a1' }],
  ['revokeKey', (b) => b.revokeKey('a1', 'k1'), 'revoke_key', { appId: 'a1', keyId: 'k1' }],
  ['setPermissions', (b) => b.setPermissions('a1', PERMS), 'set_permissions', { appId: 'a1', permissions: PERMS }],
  ['approvePairing', (b) => b.approvePairing('p1', PERMS), 'approve_pairing', { id: 'p1', permissions: PERMS }],
  ['denyPairing', (b) => b.denyPairing('p1'), 'deny_pairing', { id: 'p1' }],
  ['pairings', (b) => b.pairings(), 'pairings', undefined],
  ['requests', (b) => b.requests(500), 'requests', { limit: 500 }],
  ['requestContent', (b) => b.requestContent('r1'), 'request_content', { id: 'r1' }],
  ['benchmarks', (b) => b.benchmarks(), 'benchmarks', undefined],
  ['settings', (b) => b.settings(), 'settings', undefined],
  ['saveSettings', (b) => b.saveSettings(SETTINGS), 'save_settings', { settings: SETTINGS }],
  ['serviceState', (b) => b.serviceState(), 'service_state', undefined],
  ['declineService', (b) => b.declineService(), 'decline_service', undefined],
  ['openLoginItems', (b) => b.openLoginItems(), 'open_service_login_items', undefined],
  ['autostart', (b) => b.autostart(), 'get_autostart', undefined],
  ['setAutostart', (b) => b.setAutostart(true), 'set_autostart', { enabled: true }],
];

/** The calls that run as jobs: method, the `start_*` command and its arguments (`onEvent` is the job's channel). */
const JOBS: [string, (b: PitwallBackend) => Promise<unknown>, string, Record<string, unknown>][] = [
  ['loadModel', (b) => b.loadModel('m1'), 'start_load_model', { id: 'm1' }],
  ['runBenchmark', (b) => b.runBenchmark('m1'), 'start_benchmark', { modelId: 'm1' }],
  ['checkUpdatesNow', (b) => b.checkUpdatesNow(), 'start_check_updates', {}],
  ['installService', (b) => b.installService(), 'start_install_service', {}],
  ['uninstallService', (b) => b.uninstallService(), 'start_uninstall_service', {}],
  ['repairService', (b) => b.repairService(), 'start_repair_service', {}],
];

describe('TauriBackend', () => {
  it.each(TABLE)('%s invokes its command with the documented arguments', async (_name, call, cmd, args) => {
    const { bridge, calls } = fakeBridge();
    await call(new TauriBackend(bridge));
    expect(calls).toEqual([args === undefined ? { cmd } : { cmd, args }]);
  });

  it('covers every request/response method of the backend interface', () => {
    const methods = Object.getOwnPropertyNames(TauriBackend.prototype).filter((m) => m !== 'constructor' && m !== 'run' && m !== 'job');
    const pinned = new Set([...TABLE.map((r) => r[0]), ...JOBS.map((r) => r[0]), 'subscribe', 'onPopover', 'onQuitAsk', 'onConnectedApps', 'chat', 'jobEvents']);
    expect(methods.filter((m) => !pinned.has(m))).toEqual([]);
  });

  it('turns rejections into typed errors', async () => {
    const { bridge } = fakeBridge(() => Promise.reject({ code: 'busy', message: 'Try again.' }));
    await expect(new TauriBackend(bridge).searchModels('x')).rejects.toEqual({ code: 'busy', message: 'Try again.' });
    const odd = fakeBridge(() => Promise.reject(new Error('boom')));
    await expect(new TauriBackend(odd.bridge).overview()).rejects.toEqual({ code: 'internal', message: 'boom' });
  });

  it('subscribes to the live event and stops on unsubscribe', async () => {
    const { bridge, listeners, unlistened } = fakeBridge();
    const got: LiveUpdate[] = [];
    const stop = new TauriBackend(bridge).subscribe((u) => got.push(u));
    await Promise.resolve();
    const update: LiveUpdate = { kind: 'pairing', pairings: [] };
    listeners[0]!(update);
    expect(got).toEqual([update]);
    stop();
    listeners[0]!(update);
    expect(got.length).toBe(1);
    expect(unlistened).toEqual([LIVE_EVENT]);
  });

  it('listens for the popup\'s pushes on their own event and stops on unsubscribe, also before listening resolved', async () => {
    const { bridge, listeners, unlistened } = fakeBridge();
    const got: unknown[] = [];
    const stop = new TauriBackend(bridge).onPopover((p) => got.push(p));
    await Promise.resolve();
    listeners[0]!({ header: 1 });
    expect(got).toEqual([{ header: 1 }]);
    stop();
    listeners[0]!({ header: 2 });
    expect(got.length).toBe(1);
    expect(unlistened).toEqual(['pitwall://popover']);
    expect(POPOVER_EVENT).toBe('pitwall://popover');
    const early = fakeBridge();
    new TauriBackend(early.bridge).onPopover(() => undefined)();
    await Promise.resolve();
    await Promise.resolve();
    expect(early.unlistened).toEqual([POPOVER_EVENT]);
    const failing = new TauriBackend({ ...fakeBridge().bridge, listen: () => Promise.reject(new Error('no ipc')) }).onPopover(() => undefined);
    await Promise.resolve();
    expect(() => failing()).not.toThrow();
  });

  it('listens for the quit question and stops listening on unsubscribe', async () => {
    const { bridge, listeners, unlistened } = fakeBridge();
    let asked = 0;
    const stop = new TauriBackend(bridge).onQuitAsk(() => asked++);
    await Promise.resolve();
    listeners[0]!(null);
    expect(asked).toBe(1);
    stop();
    expect(unlistened).toEqual(['pitwall://quit-ask']);
    const early = new TauriBackend(fakeBridge().bridge).onQuitAsk(() => undefined);
    early();
    const failing = new TauriBackend({ ...fakeBridge().bridge, listen: () => Promise.reject(new Error('no ipc')) }).onQuitAsk(() => undefined);
    await Promise.resolve();
    expect(() => failing()).not.toThrow();
  });

  it("hears every window's connected-apps change, with the apps, and stops on unsubscribe", async () => {
    const { bridge, listeners, unlistened } = fakeBridge();
    const seen: unknown[] = [];
    const stop = new TauriBackend(bridge).onConnectedApps((apps) => seen.push(apps));
    await Promise.resolve();
    listeners[0]!([{ id: 'dyno' }]);
    expect(seen).toEqual([[{ id: 'dyno' }]]);
    stop();
    expect(unlistened).toEqual(['pitwall://connected-apps']);
    const early = new TauriBackend(fakeBridge().bridge).onConnectedApps(() => undefined);
    early();
  });

  it('unlistens at once when unsubscribed before listening resolved', async () => {
    const { bridge, unlistened } = fakeBridge();
    const stop = new TauriBackend(bridge).subscribe(() => undefined);
    stop();
    await Promise.resolve();
    await Promise.resolve();
    expect(unlistened).toEqual([LIVE_EVENT]);
  });

  it('survives a failing listen', async () => {
    const bridge: TauriBridge = { ...fakeBridge().bridge, listen: () => Promise.reject(new Error('no ipc')) };
    const stop = new TauriBackend(bridge).subscribe(() => undefined);
    await Promise.resolve();
    expect(() => stop()).not.toThrow();
  });

  const REQ: PlaygroundRequest = { modelId: 'm1', messages: [{ role: 'user', content: 'hi' }], temperature: 0.7, maxTokens: 64 };

  it('streams chat over a channel and cancels by stream id', async () => {
    const { bridge, calls, channels } = fakeBridge((cmd) => (cmd === 'chat' ? 'stream-1' : null));
    const events: PlaygroundEvent[] = [];
    const stop = new TauriBackend(bridge).chat(REQ, (e) => events.push(e));
    await Promise.resolve();
    await Promise.resolve();
    expect(calls[0]).toEqual({ cmd: 'chat', args: { request: REQ, onEvent: { channel: 1 } } });
    channels[0]!({ kind: 'delta', text: 'Hel' });
    expect(events).toEqual([{ kind: 'delta', text: 'Hel' }]);
    stop();
    stop();
    channels[0]!({ kind: 'delta', text: 'lo' });
    expect(events.length).toBe(1);
    expect(calls.slice(1)).toEqual([{ cmd: 'chat_cancel', args: { id: 'stream-1' } }]);
  });

  it('cancels a chat stopped before its id arrived', async () => {
    const { bridge, calls } = fakeBridge((cmd) => (cmd === 'chat' ? 'stream-2' : null));
    const stop = new TauriBackend(bridge).chat(REQ, () => undefined);
    stop();
    for (let i = 0; i < 4; i++) await Promise.resolve();
    expect(calls.map((c) => c.cmd)).toEqual(['chat', 'chat_cancel']);
  });

  it('reports a chat that failed to start as an error event', async () => {
    const { bridge } = fakeBridge((cmd) => (cmd === 'chat' ? Promise.reject({ code: 'notFound', message: 'No such model' }) : null));
    const events: PlaygroundEvent[] = [];
    new TauriBackend(bridge).chat(REQ, (e) => events.push(e));
    for (let i = 0; i < 4; i++) await Promise.resolve();
    expect(events).toEqual([{ kind: 'error', error: { code: 'notFound', message: 'No such model' } }]);
  });

  describe('jobs', () => {
    it('every job method is declared in JOB_METHODS and the other way round', () => {
      expect(JOBS.map((r) => r[0]).sort()).toEqual([...JOB_METHODS].sort());
    });

    it.each(JOBS)('%s starts its job over a channel and resolves with the result of the done event', async (_n, call, cmd, args) => {
      const { bridge, calls, channels } = fakeBridge(() => 1);
      const pending = call(new TauriBackend(bridge));
      await Promise.resolve();
      expect(calls).toEqual([{ cmd, args: { ...args, onEvent: { channel: 1 } } }]);
      channels[0]!({ kind: 'done', jobId: 1, result: { ok: true } } satisfies JobEvent);
      await expect(pending).resolves.toEqual({ ok: true });
    });

    it('forwards every event to the job listeners until they stop, and never times out a quiet job', async () => {
      vi.useFakeTimers();
      try {
        const { bridge, channels } = fakeBridge(() => 1);
        const backend = new TauriBackend(bridge);
        const seen: JobEvent[] = [];
        const stop = backend.jobEvents((e) => seen.push(e));
        let settled = false;
        const pending = backend.loadModel('m1').finally(() => (settled = true));
        await vi.advanceTimersByTimeAsync(24 * 3600 * 1000);
        expect(settled).toBe(false);
        const progress: JobEvent = { kind: 'progress', jobId: 1, phase: 'Loading', done: 3, total: null, unit: 'items', canStopWaiting: false, elapsedSecs: 3, cancellable: true, notCancellableReason: null };
        channels[0]!(progress);
        stop();
        channels[0]!({ kind: 'done', jobId: 1, result: null });
        await pending;
        expect(seen).toEqual([progress]);
      } finally {
        vi.useRealTimers();
      }
    });

    it('rejects with the typed error of a failed job, and with the cancelled code for a cancelled one', async () => {
      const failed = fakeBridge(() => 1);
      const a = new TauriBackend(failed.bridge).loadModel('m1');
      failed.channels[0]!({ kind: 'failed', jobId: 1, error: { code: 'insufficientMemory', message: 'Too big.' } });
      await expect(a).rejects.toEqual({ code: 'insufficientMemory', message: 'Too big.' });

      const cancelled = fakeBridge(() => 1);
      const b = new TauriBackend(cancelled.bridge).runBenchmark('m1');
      cancelled.channels[0]!({ kind: 'cancelled', jobId: 1, stoppedWaiting: false });
      await expect(b).rejects.toEqual({ code: 'cancelled', message: 'Cancelled.' });

      const stopped = fakeBridge(() => 1);
      const c = new TauriBackend(stopped.bridge).runBenchmark('m1');
      stopped.channels[0]!({ kind: 'cancelled', jobId: 1, stoppedWaiting: true });
      await expect(c).rejects.toMatchObject({ code: 'cancelled', message: expect.stringContaining('may still be working') });
    });

    it('rejects when the job cannot start (for example another check is running)', async () => {
      const { bridge } = fakeBridge(() => Promise.reject({ code: 'busy', message: 'A check is running.' }));
      await expect(new TauriBackend(bridge).checkUpdatesNow()).rejects.toEqual({ code: 'busy', message: 'A check is running.' });
    });

    it('drops events of an unknown shape, survives a throwing listener and ignores events of another job', async () => {
      const { bridge, channels } = fakeBridge(() => 4);
      const backend = new TauriBackend(bridge);
      const seen: JobEvent[] = [];
      backend.jobEvents(() => {
        throw new Error('listener bug');
      });
      backend.jobEvents((e) => seen.push(e));
      const pending = backend.loadModel('m1');
      await Promise.resolve();
      await Promise.resolve();
      const send = channels[0]! as (m: unknown) => void;
      send({ kind: 'paused', jobId: 4 });
      send({ kind: 'done' });
      send('junk');
      send({ kind: 'done', jobId: 99, result: 'other job' });
      expect(seen).toEqual([]);
      send({ kind: 'done', jobId: 4, result: 'mine' });
      await expect(pending).resolves.toBe('mine');
      expect(seen).toHaveLength(1);
    });
  });
});
