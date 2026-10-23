import { EVENTS, REQUEST_TIMEOUT_MS, TauriBackend, type InvokeFn, type ListenFn } from './tauri-backend';

const REF = { pid: 7, started: 1000 };
const SETTINGS = { theme: 'dark' } as never;
const RULES = { rules: [] } as never;

function fake() {
  const invoke = vi.fn().mockResolvedValue('ok');
  const handlers = new Map<string, Array<(e: { payload: unknown }) => void>>();
  const stops: Array<ReturnType<typeof vi.fn>> = [];
  const listen = vi.fn((name: string, h: (e: { payload: unknown }) => void) => {
    handlers.set(name, [...(handlers.get(name) ?? []), h]);
    const stop = vi.fn();
    stops.push(stop);
    return Promise.resolve(stop);
  });
  const backend = new TauriBackend(invoke as unknown as InvokeFn, listen as unknown as ListenFn);
  const emit = (name: string, payload: unknown) => handlers.get(name)?.forEach((h) => h({ payload }));
  return { backend, invoke, listen, emit, stops };
}

describe('TauriBackend: request shapes', () => {
  const cases: Array<[string, (b: TauriBackend) => Promise<unknown>, unknown]> = [
    ['processDetails', (b) => b.processDetails(REF), { processDetails: { target: REF } }],
    ['act', (b) => b.act(REF, { setPriority: 'idle' }), { act: { target: REF, action: { setPriority: 'idle' } } }],
    ['actOnGroup', (b) => b.actOnGroup('chrome', 'end'), { actOnGroup: { groupId: 'chrome', action: 'end' } }],
    ['lockers', (b) => b.lockers('/tmp/x'), { lockers: { path: '/tmp/x' } }],
    ['openLocation', (b) => b.openLocation(REF), { openLocation: { target: REF } }],
    ['takeSnapshot', (b) => b.takeSnapshot('before'), { takeSnapshot: { label: 'before' } }],
    ['snapshots', (b) => b.snapshots(), 'snapshots'],
    ['diffSnapshots', (b) => b.diffSnapshots('a', 'b'), { diffSnapshots: { before: 'a', after: 'b' } }],
    ['whySlow', (b) => b.whySlow(), 'whySlow'],
    ['anomalies', (b) => b.anomalies(), 'anomalies'],
    ['history', (b) => b.history('week', 5), { history: { range: 'week', end: 5 } }],
    ['history now', (b) => b.history('hour', null), { history: { range: 'hour', end: null } }],
    ['activity', (b) => b.activity(30), { activity: { limit: 30 } }],
    ['undo', (b) => b.undo('act-1'), { undo: { activityId: 'act-1' } }],
    ['releaseAction', (b) => b.releaseAction(REF), { releaseAction: { target: REF } }],
    ['rules', (b) => b.rules(), 'rules'],
    ['saveRules', (b) => b.saveRules(RULES), { saveRules: { rules: RULES } }],
    ['setProfile', (b) => b.setProfile('gaming'), { setProfile: { profile: 'gaming' } }],
    ['pauseRules', (b) => b.pauseRules(true), { pauseRules: { paused: true } }],
    ['cleanupCategories', (b) => b.cleanupCategories(), 'cleanupCategories'],
    ['scanCleanup', (b) => b.scanCleanup(['temp']), { scanCleanup: { categories: ['temp'] } }],
    ['runCleanup', (b) => b.runCleanup('s1', ['i1'], 'quarantine'), { runCleanup: { scanId: 's1', itemIds: ['i1'], mode: 'quarantine' } }],
    ['quarantine', (b) => b.quarantine(), 'quarantine'],
    ['restoreQuarantine', (b) => b.restoreQuarantine('q1'), { restoreQuarantine: { id: 'q1' } }],
    ['diskUsage', (b) => b.diskUsage(null), { diskUsage: { path: null } }],
    ['health', (b) => b.health(), 'health'],
    ['tuning', (b) => b.tuning(), 'tuning'],
    ['applyTuning', (b) => b.applyTuning('t'), { applyTuning: { id: 't' } }],
    ['revertTuning', (b) => b.revertTuning('t'), { revertTuning: { id: 't', force: false } }],
    ['revertTuning forced', (b) => b.revertTuning('t', true), { revertTuning: { id: 't', force: true } }],
    ['helperStatus', (b) => b.helperStatus(), 'helperStatus'],
    ['installHelper', (b) => b.installHelper(), 'installHelper'],
    ['uninstallHelper', (b) => b.uninstallHelper(), 'uninstallHelper'],
    ['startupEntries', (b) => b.startupEntries(), 'startupEntries'],
    ['setStartupState', (b) => b.setStartupState('s', { delayed: 30 }), { setStartupState: { id: 's', state: { delayed: 30 } } }],
    ['weeklyReport', (b) => b.weeklyReport(3), { weeklyReport: { weeksAgo: 3 } }],
    ['alerts', (b) => b.alerts(10), { alerts: { limit: 10 } }],
    ['settings', (b) => b.settings(), 'settings'],
    ['saveSettings', (b) => b.saveSettings(SETTINGS), { saveSettings: { settings: SETTINGS } }],
    ['plugins', (b) => b.plugins(), 'plugins'],
  ];

  it.each(cases)('%s sends the externally tagged request through the dyno command', async (_name, call, request) => {
    const { backend, invoke } = fake();
    await expect(call(backend)).resolves.toBe('ok');
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('dyno', { request });
  });

  it('saveReport asks the shell for the dialog with the format and the week, and waits as long as the person does', async () => {
    const { backend, invoke } = fake();
    await backend.saveReport('markdown', 1);
    expect(invoke).toHaveBeenCalledWith('save_report', { format: 'markdown', weeksAgo: 1 });
  });

  it('popoverResize sends the measured height to popover_resize', async () => {
    const { backend, invoke } = fake();
    await backend.popoverResize(342);
    expect(invoke.mock.calls).toEqual([['popover_resize', { height: 342 }]]);
  });

  it('appInfo and the tray use their own commands without arguments', async () => {
    const { backend, invoke } = fake();
    await backend.appInfo();
    await backend.trayOpenMain();
    await backend.trayQuit();
    expect(invoke.mock.calls).toEqual([['app_info'], ['tray_open_main'], ['tray_quit']]);
  });
});

describe('TauriBackend: errors and timeout', () => {
  it('turns a well-formed rejection into a CommandError', async () => {
    const { backend, invoke } = fake();
    invoke.mockRejectedValueOnce({ code: 'protected', message: 'no' });
    await expect(backend.whySlow()).rejects.toEqual({ code: 'protected', message: 'no' });
  });
  it('turns other rejections into internal errors', async () => {
    const { backend, invoke } = fake();
    invoke.mockRejectedValueOnce('plain text');
    await expect(backend.rules()).rejects.toEqual({ code: 'internal', message: 'plain text' });
    invoke.mockRejectedValueOnce(new Error('boom'));
    await expect(backend.rules()).rejects.toEqual({ code: 'internal', message: 'boom' });
  });

  describe('timeout', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it('rejects with code timeout when the shell does not answer', async () => {
      const { invoke, listen } = fake();
      invoke.mockReturnValue(new Promise(() => undefined));
      const backend = new TauriBackend(invoke as unknown as InvokeFn, listen as unknown as ListenFn, 50);
      const result = backend.health();
      const assertion = expect(result).rejects.toEqual({ code: 'timeout', message: 'Dyno did not answer in time.' });
      await vi.advanceTimersByTimeAsync(50);
      await assertion;
    });
    it('does not time out once answered, and clears its timer', async () => {
      const { backend } = fake();
      await backend.health();
      expect(vi.getTimerCount()).toBe(0);
    });
    it('uses 15 s by default', async () => {
      const { invoke } = fake();
      invoke.mockReturnValue(new Promise(() => undefined));
      const backend = new TauriBackend(invoke as unknown as InvokeFn, vi.fn() as unknown as ListenFn);
      expect(REQUEST_TIMEOUT_MS).toBe(15_000);
      let rejected: unknown;
      backend.health().catch((e: unknown) => (rejected = e));
      await vi.advanceTimersByTimeAsync(REQUEST_TIMEOUT_MS - 1);
      expect(rejected).toBeUndefined();
      await vi.advanceTimersByTimeAsync(1);
      expect(rejected).toMatchObject({ code: 'timeout' });
    });
  });
});

describe('TauriBackend: live subscription', () => {
  const requests = (invoke: ReturnType<typeof vi.fn>) => invoke.mock.calls.map((c) => (c[1] as { request: unknown }).request);

  it('sends subscribe once per change of detail', async () => {
    const { backend, invoke } = fake();
    const a = backend.onLive('summary', vi.fn());
    const b = backend.onLive('summary', vi.fn());
    expect(requests(invoke)).toEqual([{ subscribe: { detail: 'summary' } }]);
    const c = backend.onLive('processes', vi.fn());
    expect(requests(invoke).at(-1)).toEqual({ subscribe: { detail: 'processes' } });
    const d = backend.onLive('processes', vi.fn());
    expect(invoke).toHaveBeenCalledTimes(2);
    c();
    expect(invoke).toHaveBeenCalledTimes(2); // another process listener remains
    d();
    expect(requests(invoke).at(-1)).toEqual({ subscribe: { detail: 'summary' } });
    a();
    b();
    expect(invoke).toHaveBeenCalledTimes(3);
  });
  it('sends nothing once nobody listens, and re-sends on the next listener', () => {
    const { backend, invoke } = fake();
    const stop = backend.onLive('summary', vi.fn());
    stop();
    expect(invoke).toHaveBeenCalledTimes(1);
    backend.onLive('summary', vi.fn());
    expect(invoke).toHaveBeenCalledTimes(2);
  });
  it('unsubscribing twice is harmless', () => {
    const { backend, invoke } = fake();
    const keep = backend.onLive('processes', vi.fn());
    const stop = backend.onLive('processes', vi.fn());
    stop();
    stop();
    expect(invoke).toHaveBeenCalledTimes(1);
    keep();
  });
  it('fans one listen out to every listener and stops with the last', async () => {
    const { backend, listen, emit, stops } = fake();
    const one = vi.fn();
    const two = vi.fn();
    const stopOne = backend.onLive('summary', one);
    const stopTwo = backend.onLive('summary', two);
    expect(listen).toHaveBeenCalledTimes(1);
    expect(listen).toHaveBeenCalledWith(EVENTS.live, expect.any(Function));
    emit(EVENTS.live, { n: 1 });
    expect(one).toHaveBeenCalledWith({ n: 1 });
    expect(two).toHaveBeenCalledWith({ n: 1 });
    stopOne();
    emit(EVENTS.live, { n: 2 });
    expect(one).toHaveBeenCalledTimes(1);
    expect(two).toHaveBeenCalledTimes(2);
    expect(stops[0]).not.toHaveBeenCalled();
    stopTwo();
    await Promise.resolve();
    await Promise.resolve();
    expect(stops[0]).toHaveBeenCalledTimes(1);
  });
  it('listens again after the last listener left', async () => {
    const { backend, listen } = fake();
    backend.onLive('summary', vi.fn())();
    backend.onLive('summary', vi.fn());
    expect(listen).toHaveBeenCalledTimes(2);
  });
  it('retries the subscribe after a failure', async () => {
    const { backend, invoke } = fake();
    const log = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    invoke.mockRejectedValueOnce({ code: 'agentUnavailable', message: 'down' });
    const a = backend.onLive('summary', vi.fn());
    await new Promise((r) => setTimeout(r));
    expect(log).toHaveBeenCalledWith('Could not subscribe to live updates', 'down');
    const b = backend.onLive('summary', vi.fn());
    expect(invoke).toHaveBeenCalledTimes(2);
    a();
    b();
    log.mockRestore();
  });
  it('a failing listen is logged and does not throw', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const listen = vi.fn().mockRejectedValue(new Error('no events'));
    const log = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const backend = new TauriBackend(invoke as unknown as InvokeFn, listen as unknown as ListenFn);
    const stop = backend.onAlert(vi.fn());
    await new Promise((r) => setTimeout(r));
    expect(log).toHaveBeenCalledWith(`Could not listen to ${EVENTS.alert}`, 'no events');
    expect(() => stop()).not.toThrow();
    log.mockRestore();
  });
});

describe('TauriBackend: push channels', () => {
  it.each([
    ['onAlert', EVENTS.alert],
    ['onActivity', EVENTS.activity],
    ['onAgentStatus', EVENTS.agent],
  ] as const)('%s listens to its own event', async (method, event) => {
    const { backend, emit, listen, stops } = fake();
    const l = vi.fn();
    const stop = (backend[method] as (l: (x: unknown) => void) => () => void)(l);
    expect(listen).toHaveBeenCalledWith(event, expect.any(Function));
    emit(event, { id: 1 });
    expect(l).toHaveBeenCalledWith({ id: 1 });
    stop();
    await Promise.resolve();
    await Promise.resolve();
    expect(stops[0]).toHaveBeenCalled();
    emit(event, { id: 2 });
    expect(l).toHaveBeenCalledTimes(1);
  });
});
