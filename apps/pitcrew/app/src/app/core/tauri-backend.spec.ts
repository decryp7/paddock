import type { BackendEvent, Config, RegisterRequest, RunnerConfig } from './api.types';
import { EVENT_NAME, TauriBackend, type CommandCall, type EventListen } from './tauri-backend';

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
    ['cleanDisk', (b) => b.cleanDisk('r'), 'clean_disk', { runner: 'r' }],
    ['runDetails', (b) => b.runDetails('r'), 'run_details', { runner: 'r' }],
    ['settings', (b) => b.settings(), 'get_settings', undefined],
    ['saveSettings', (b) => b.saveSettings({ schema_version: 1 } as Config), 'save_settings', { config: { schema_version: 1 } }],
    ['addRunner', (b) => b.addRunner({ name: 'x' } as RunnerConfig), 'add_runner', { runner: { name: 'x' } }],
    ['removeRunner', (b) => b.removeRunner('r'), 'remove_runner', { runner: 'r' }],
    ['register', (b) => b.register({ runner: 'x' } as RegisterRequest), 'register_runner', { request: { runner: 'x' } }],
    ['hasApiToken', (b) => b.hasApiToken('https://g'), 'has_api_token', { serverUrl: 'https://g' }],
    ['setApiToken', (b) => b.setApiToken('https://g', 't'), 'set_api_token', { serverUrl: 'https://g', token: 't' }],
    ['clearApiToken', (b) => b.clearApiToken('https://g'), 'clear_api_token', { serverUrl: 'https://g' }],
    ['serviceInstall', (b) => b.serviceInstall(), 'service_install_state', undefined],
    ['installService', (b) => b.installService(), 'install_service', undefined],
    ['uninstallService', (b) => b.uninstallService(), 'uninstall_service', undefined],
    ['appUpdateStatus', (b) => b.appUpdateStatus(), 'app_update_status', undefined],
    ['checkAppUpdate', (b) => b.checkAppUpdate(), 'check_app_update', undefined],
    ['installAppUpdate', (b) => b.installAppUpdate(), 'install_app_update', undefined],
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
  it('is the browser until init asks the shell', async () => {
    const { backend } = setup('macos');
    expect(backend.platform()).toBe('browser');
    await backend.init();
    expect(backend.platform()).toBe('macos');
  });

  it('keeps the browser platform when the shell cannot answer', async () => {
    const call = vi.fn(async () => {
      throw new Error('no');
    });
    const backend = new TauriBackend(call as unknown as CommandCall, vi.fn() as unknown as EventListen);
    await backend.init();
    expect(backend.platform()).toBe('browser');
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
