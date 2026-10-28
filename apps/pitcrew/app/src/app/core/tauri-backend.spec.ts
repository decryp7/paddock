import type { BackendEvent, Config, RegisterRequest, RunnerConfig } from './api.types';
import { EVENT_NAME, jobEnd, platformFromUserAgent, TauriBackend, type CommandCall, type EventListen } from './tauri-backend';

function setup(result: unknown = undefined) {
  const call = vi.fn(async () => result);
  const backend = new TauriBackend(call as unknown as CommandCall, vi.fn() as unknown as EventListen);
  return { call, backend };
}

describe('TauriBackend commands', () => {
  const cases: [string, (b: TauriBackend) => Promise<unknown>, string, Record<string, unknown> | undefined][] = [
    ['status', (b) => b.status(), 'runner_status', undefined],
    ['connectionState', (b) => b.connectionState(), 'connection_state', undefined],
    ['start', (b) => b.start('r'), 'runner_action', { runner: 'r', action: 'start' }],
    ['stop', (b) => b.stop('r'), 'runner_action', { runner: 'r', action: 'stop' }],
    ['restart', (b) => b.restart('r'), 'runner_action', { runner: 'r', action: 'restart' }],
    ['drain', (b) => b.drain('r'), 'runner_action', { runner: 'r', action: 'drain' }],
    ['resume', (b) => b.resume('r'), 'runner_action', { runner: 'r', action: 'resume' }],
    ['checkUpdate', (b) => b.checkUpdate('r'), 'runner_action', { runner: 'r', action: 'checkUpdate' }],
    ['applyUpdate', (b) => b.applyUpdate('r'), 'runner_action', { runner: 'r', action: 'applyUpdate' }],
    ['cancelUpdate', (b) => b.cancelUpdate('r'), 'runner_action', { runner: 'r', action: 'cancelUpdate' }],
    ['skipVersion', (b) => b.skipVersion('r', '1.2.3'), 'skip_version', { runner: 'r', version: '1.2.3' }],
    ['pinVersion', (b) => b.pinVersion('r', null), 'pin_version', { runner: 'r', version: null }],
    ['logs', (b) => b.logs('r', 5, 100), 'runner_logs', { runner: 'r', after: 5, limit: 100 }],
    ['searchLogs', (b) => b.searchLogs('r', 'q', 10), 'search_logs', { runner: 'r', query: 'q', limit: 10 }],
    ['metrics', (b) => b.metrics(null, 24), 'machine_metrics', { runner: null, rangeHours: 24 }],
    ['disk', (b) => b.disk('r'), 'runner_disk', { runner: 'r' }],
    ['runDetails', (b) => b.runDetails('r'), 'run_details', { runner: 'r' }],
    ['runnerConfig', (b) => b.runnerConfig('r'), 'runner_config_get', { runner: 'r' }],
    ['previewRunnerConfig', (b) => b.previewRunnerConfig('r', { mode: 'raw', text: 'a' }), 'runner_config_preview', { runner: 'r', edit: { mode: 'raw', text: 'a' } }],
    ['saveRunnerConfig', (b) => b.saveRunnerConfig('r', { mode: 'raw', text: 'a' }, 'fp'), 'runner_config_save', { runner: 'r', edit: { mode: 'raw', text: 'a' }, base: 'fp' }],
    ['runnerProcesses', (b) => b.runnerProcesses('r'), 'runner_processes', { runner: 'r' }],
    ['stopProcess', (b) => b.stopProcess('r', 7, null, false), 'runner_process_stop', { runner: 'r', pid: 7, startedAt: null, allowRunner: false }],
    ['keepProcess', (b) => b.keepProcess('r', 7, 5, { amount: 1, unit: 'hours' }), 'runner_process_keep', { runner: 'r', pid: 7, startedAt: 5, extra: { amount: 1, unit: 'hours' } }],
    ['engineReport', (b) => b.engineReport(), 'engine_report', undefined],
    ['previewEnginePrune', (b) => b.previewEnginePrune({ kinds: ['volumes'], olderThanDays: 0 }), 'engine_prune_preview', { selection: { kinds: ['volumes'], olderThanDays: 0 } }],
    ['cacheReport', (b) => b.cacheReport('r'), 'cache_report', { runner: 'r' }],
    ['previewCacheClean', (b) => b.previewCacheClean('r', { maxMb: 1, maxAgeDays: 0 }), 'cache_preview', { runner: 'r', limits: { maxMb: 1, maxAgeDays: 0 } }],
    ['debriefStatus', (b) => b.debriefStatus(), 'debrief_status', undefined],
    ['openInDebrief', (b) => b.openInDebrief('r', 'run 1', 1, 2), 'open_in_debrief', { runner: 'r', label: 'run 1', startedAt: 1, finishedAt: 2 }],
    ['channels', (b) => b.channels(), 'notification_channels_get', undefined],
    ['saveChannels', (b) => b.saveChannels({ channels: [] }), 'notification_channels_save', { settings: { channels: [] } }],
    ['setChannelSecret', (b) => b.setChannelSecret('k', 'v'), 'channel_secret_set', { key: 'k', value: 'v' }],
    ['clearChannelSecret', (b) => b.clearChannelSecret('k'), 'channel_secret_clear', { key: 'k' }],
    ['hasChannelSecret', (b) => b.hasChannelSecret('k'), 'channel_secret_present', { key: 'k' }],
    ['testChannel', (b) => b.testChannel('c'), 'channel_test', { id: 'c' }],
    ['settings', (b) => b.settings(), 'get_settings', undefined],
    ['saveSettings', (b) => b.saveSettings({ schema_version: 1 } as Config), 'save_settings', { config: { schema_version: 1 } }],
    ['addRunner', (b) => b.addRunner({ name: 'x' } as RunnerConfig), 'add_runner', { runner: { name: 'x' } }],
    ['removeRunner', (b) => b.removeRunner('r'), 'remove_runner', { runner: 'r' }],
    ['hasApiToken', (b) => b.hasApiToken('https://g'), 'has_api_token', { serverUrl: 'https://g' }],
    ['setApiToken', (b) => b.setApiToken('https://g', 't'), 'set_api_token', { serverUrl: 'https://g', token: 't' }],
    ['clearApiToken', (b) => b.clearApiToken('https://g'), 'clear_api_token', { serverUrl: 'https://g' }],
    ['serviceInstall', (b) => b.serviceInstall(), 'service_install_state', undefined],
    ['installService', (b) => b.installService(), 'install_service', undefined],
    ['uninstallService', (b) => b.uninstallService(), 'uninstall_service', undefined],
    ['repairService', (b) => b.repairService(), 'repair_service', undefined],
    ['openLoginItems', (b) => b.openLoginItems(), 'pitcrew_open_login_items', undefined],
    ['autostart', (b) => b.autostart(), 'get_autostart', undefined],
    ['setAutostart', (b) => b.setAutostart(true), 'set_autostart', { enabled: true }],
    ['exportDiagnostics', (b) => b.exportDiagnostics(), 'export_diagnostics', undefined],
    ['openUrl', (b) => b.openUrl('https://x'), 'open_url', { url: 'https://x' }],
    ['revealLogs', (b) => b.revealLogs('r'), 'reveal_logs', { runner: 'r' }],
  ];

  it.each(cases)('%s sends the right command and arguments', async (_name, fn, cmd, args) => {
    const { call, backend } = setup('ok');
    expect(await fn(backend)).toBe('ok');
    expect(call).toHaveBeenCalledTimes(1);
    expect(call.mock.calls[0]).toEqual(args === undefined ? [cmd] : [cmd, args]);
  });

  it('sends the UTC offset with activity', async () => {
    const { call, backend } = setup();
    await backend.activity('r', 7);
    expect(call).toHaveBeenCalledWith('runner_activity', {
      runner: 'r',
      days: 7,
      utcOffsetMinutes: -new Date().getTimezoneOffset(),
    });
  });

  it('turns rejections into CommandErrors', async () => {
    const call = vi.fn(async () => {
      throw { code: 'busy', message: 'later' };
    });
    const backend = new TauriBackend(call as unknown as CommandCall, vi.fn() as unknown as EventListen);
    await expect(backend.start('r')).rejects.toEqual({ code: 'busy', message: 'later' });
    call.mockRejectedValueOnce('text' as never);
    await expect(backend.status()).rejects.toEqual({ code: 'internal', message: 'text' });
  });
});

describe('TauriBackend platform', () => {
  it('takes the platform the shell names', async () => {
    const { backend } = setup('macos');
    expect(backend.platform()).toBe('browser');
    await backend.init();
    expect(backend.platform()).toBe('macos');
  });

  it('falls back to the user agent when the shell cannot answer', async () => {
    const call = vi.fn(async () => {
      throw new Error('no');
    });
    const backend = new TauriBackend(call as unknown as CommandCall, vi.fn() as unknown as EventListen);
    await backend.init();
    expect(backend.platform()).toBe(platformFromUserAgent(navigator.userAgent));
  });

  it('never waits for the shell at start-up (a blank window otherwise), and a late answer still counts', async () => {
    let answer: (p: string) => void = () => undefined;
    const call = vi.fn(() => new Promise((resolve) => (answer = resolve)));
    const backend = new TauriBackend(call as unknown as CommandCall, vi.fn() as unknown as EventListen);
    await backend.init();
    expect(backend.platform()).toBe(platformFromUserAgent(navigator.userAgent));
    answer('linux');
    await Promise.resolve();
    await Promise.resolve();
    expect(backend.platform()).toBe('linux');
  });

  it('reads the platform from user agents', () => {
    expect(platformFromUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15')).toBe('macos');
    expect(platformFromUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/130.0')).toBe('windows');
    expect(platformFromUserAgent('Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15')).toBe('linux');
    expect(platformFromUserAgent('')).toBe('browser');
  });
});

describe('TauriBackend.watch', () => {
  function watchSetup(connection: unknown = { state: 'connected', serviceVersion: '1' }) {
    let handler: ((e: { payload: BackendEvent }) => void) | null = null;
    const unlisten = vi.fn();
    let resolveListen!: (fn: () => void) => void;
    const listen = vi.fn((_event: string, h: (e: { payload: BackendEvent }) => void) => {
      handler = h;
      return new Promise<() => void>((r) => (resolveListen = r));
    });
    const call = vi.fn(async () => connection);
    const backend = new TauriBackend(call as unknown as CommandCall, listen as unknown as EventListen);
    return {
      backend,
      listen,
      unlisten,
      call,
      emit: (e: BackendEvent) => handler?.({ payload: e }),
      resolve: () => resolveListen(unlisten),
    };
  }
  const flush = () => new Promise((r) => setTimeout(r));

  it('subscribes, delivers the connection first and then pushed events', async () => {
    const t = watchSetup();
    const seen: BackendEvent[] = [];
    const stop = t.backend.watch((e) => seen.push(e));
    expect(t.listen).toHaveBeenCalledWith(EVENT_NAME, expect.any(Function));
    t.resolve();
    await flush();
    expect(seen).toEqual([{ kind: 'connection', connection: { state: 'connected', serviceVersion: '1' } }]);
    t.emit({ kind: 'connection', connection: { state: 'connecting' } });
    expect(seen).toHaveLength(2);
    stop();
    expect(t.unlisten).toHaveBeenCalledTimes(1);
  });

  it('unsubscribes immediately when stopped before listen resolved', async () => {
    const t = watchSetup();
    const seen: BackendEvent[] = [];
    const stop = t.backend.watch((e) => seen.push(e));
    stop();
    t.resolve();
    await flush();
    expect(t.unlisten).toHaveBeenCalledTimes(1);
    expect(seen).toEqual([]);
    expect(t.call).not.toHaveBeenCalled();
  });

  it('does not deliver the connection when stopped while it was loading', async () => {
    const t = watchSetup();
    const seen: BackendEvent[] = [];
    const stop = t.backend.watch((e) => seen.push(e));
    t.resolve();
    await Promise.resolve();
    stop();
    await flush();
    expect(seen).toEqual([]);
  });

  it('survives a failing listen or connection query', async () => {
    const listen = vi.fn(async () => {
      throw new Error('no');
    });
    const call = vi.fn(async () => undefined);
    const b1 = new TauriBackend(call as unknown as CommandCall, listen as unknown as EventListen);
    expect(() => b1.watch(() => undefined)()).not.toThrow();
    await flush();

    const t = watchSetup();
    t.call.mockRejectedValueOnce(new Error('down'));
    const seen: BackendEvent[] = [];
    t.backend.watch((e) => seen.push(e));
    t.resolve();
    await flush();
    expect(seen).toEqual([]);
  });
});

describe('TauriBackend.navigations', () => {
  const flush = () => new Promise((r) => setTimeout(r));

  it('delivers the routes the shell asks for and unsubscribes', async () => {
    let handler: ((e: { payload: string }) => void) | null = null;
    const unlisten = vi.fn();
    const listen = vi.fn(async (_e: string, h: (e: { payload: string }) => void) => {
      handler = h;
      return unlisten;
    });
    const backend = new TauriBackend(vi.fn() as unknown as CommandCall, listen as unknown as EventListen);
    const seen: string[] = [];
    const stop = backend.navigations((r) => seen.push(r));
    await flush();
    expect(listen).toHaveBeenCalledWith('pitcrew://navigate', expect.any(Function));
    handler!({ payload: '/logs/a' });
    expect(seen).toEqual(['/logs/a']);
    stop();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('unsubscribes at once when stopped before listen resolved, and survives a failing listen', async () => {
    const unlisten = vi.fn();
    const slow = vi.fn(() => Promise.resolve(unlisten));
    const stop = new TauriBackend(vi.fn() as unknown as CommandCall, slow as unknown as EventListen).navigations(() => undefined);
    stop();
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);
    const failing = vi.fn(async () => {
      throw new Error('no');
    });
    expect(() => new TauriBackend(vi.fn() as unknown as CommandCall, failing as unknown as EventListen).navigations(() => undefined)()).not.toThrow();
    await flush();
  });
});

describe('TauriBackend jobs (long work never times out, rule 12)', () => {
  /** A backend whose job command answers with `id`, and a way to send job snapshots as the shell does. */
  function jobSetup(id: number, result: unknown) {
    const handlers: ((e: { payload: unknown }) => void)[] = [];
    const unlisten = vi.fn();
    const listen = vi.fn(async (_event: string, h: (e: { payload: unknown }) => void) => {
      handlers.push(h);
      return unlisten;
    });
    const call = vi.fn(async (cmd: string) => (cmd === 'job_result' ? result : id));
    const backend = new TauriBackend(call as unknown as CommandCall, listen as unknown as EventListen);
    const send = (payload: unknown) => handlers.forEach((h) => h({ payload }));
    return { backend, call, listen, send, unlisten };
  }
  const flush = () => new Promise((r) => setTimeout(r));

  const jobs: [string, (b: TauriBackend) => Promise<unknown>, string, Record<string, unknown>][] = [
    ['cleanDisk', (b) => b.cleanDisk('r'), 'clean_disk', { runner: 'r' }],
    ['pruneEngine', (b) => b.pruneEngine({ kinds: ['volumes'], olderThanDays: 0 }), 'engine_prune', { selection: { kinds: ['volumes'], olderThanDays: 0 } }],
    ['cleanCache', (b) => b.cleanCache('r', { maxMb: 1, maxAgeDays: 0 }), 'cache_clean', { runner: 'r', limits: { maxMb: 1, maxAgeDays: 0 } }],
    ['register', (b) => b.register({ runner: 'x' } as RegisterRequest), 'register_runner', { request: { runner: 'x' } }],
  ];

  it.each(jobs)('%s starts a job, waits for its end and reads its result', async (_name, fn, cmd, args) => {
    const { backend, call, send, unlisten } = jobSetup(7, 'the result');
    const done = fn(backend);
    await flush();
    expect(call).toHaveBeenCalledWith(cmd, args);
    // Other jobs and progress do not end it.
    send({ id: 8, state: 'done' });
    send({ id: 7, state: 'running', phase: null });
    await flush();
    let settled = false;
    void done.then(() => (settled = true));
    await flush();
    expect(settled).toBe(false);
    send({ id: 7, state: 'done' });
    expect(await done).toBe('the result');
    expect(call).toHaveBeenCalledWith('job_result', { id: 7 });
    expect(unlisten).toHaveBeenCalled();
  });

  it('keeps a job that ended before its id arrived', async () => {
    const { backend, send } = jobSetup(3, 'r');
    const handlersReady = flush();
    const done = backend.cleanDisk('r');
    await handlersReady;
    send({ id: 3, state: 'done' });
    expect(await done).toBe('r');
  });

  it('rejects with the reason when the job failed or was cancelled', async () => {
    const failed = jobSetup(1, null);
    const a = failed.backend.cleanDisk('r');
    await flush();
    failed.send({ id: 1, state: 'failed', message: 'the disk is gone' });
    await expect(a).rejects.toEqual({ code: 'internal', message: 'the disk is gone' });
    const cancelled = jobSetup(2, null);
    const b = cancelled.backend.cleanDisk('r');
    await flush();
    cancelled.send({ id: 2, state: 'cancelled', reason: 'user' });
    await expect(b).rejects.toMatchObject({ code: 'cancelled' });
  });

  it('waits through the shell\'s job follow, so the shell does not announce the end a second time', async () => {
    const stop = vi.fn();
    let finish!: (job: unknown) => void;
    const follow = vi.fn(() => ({ finished: new Promise((r) => (finish = r)), stop }));
    const listen = vi.fn(async () => vi.fn());
    const call = vi.fn(async (cmd: string) => (cmd === 'job_result' ? 'the result' : 9));
    const backend = new TauriBackend(call as unknown as CommandCall, listen as unknown as EventListen, follow as never);
    const done = backend.cleanDisk('r');
    await flush();
    expect(follow).toHaveBeenCalledWith(9);
    expect(listen).not.toHaveBeenCalledWith('paddock://jobs', expect.anything());
    finish({ id: 9, state: 'done' });
    expect(await done).toBe('the result');
    const failed = new TauriBackend(call as unknown as CommandCall, listen as unknown as EventListen, (() => ({ finished: Promise.resolve({ id: 9, state: 'failed', message: 'no disk' }), stop })) as never);
    await expect(failed.cleanDisk('r')).rejects.toEqual({ code: 'internal', message: 'no disk' });
    const cancelled = new TauriBackend(call as unknown as CommandCall, listen as unknown as EventListen, (() => ({ finished: Promise.resolve({ id: 9, state: 'cancelled', reason: 'user' }), stop })) as never);
    await expect(cancelled.cleanDisk('r')).rejects.toMatchObject({ code: 'cancelled' });
  });

  it('parses job payloads, never trusting them', () => {
    expect(jobEnd(null)).toBeNull();
    expect(jobEnd({ id: 'x', state: 'done' })).toBeNull();
    expect(jobEnd({ id: 1, state: 'running' })).toBeNull();
    expect(jobEnd({ id: 1, state: 'failed' })).toEqual({ id: 1, state: 'failed', message: 'The work failed.' });
  });
});
