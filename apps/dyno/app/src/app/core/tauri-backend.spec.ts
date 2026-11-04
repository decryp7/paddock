import type { JobSpec } from './api.types';
import { EVENTS, TauriBackend, type InvokeFn, type ListenFn } from './tauri-backend';

const REF = { pid: 7, started: 1000 };
const SETTINGS = { launchAtLogin: true } as never;

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

/** `startJob` answers the job's id; the shape test only looks at what was sent. */
const started = async (b: TauriBackend, spec: JobSpec): Promise<string> => {
  await b.startJob(spec);
  return 'ok';
};

describe('TauriBackend: request shapes', () => {
  const cases: Array<[string, (b: TauriBackend) => Promise<unknown>, unknown]> = [
    ['processDetails', (b) => b.processDetails(REF), { processDetails: { target: REF } }],
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
    ['diskVolumes', (b) => b.diskVolumes(), 'diskVolumes'],
    ['startDiskScan of a volume', (b) => b.startDiskScan({ volume: { id: '/' } }), { startDiskScan: { target: { volume: { id: '/' } }, replaces: null } }],
    ['startDiskScan replacing a kept scan', (b) => b.startDiskScan({ volume: { id: '/' } }, 5), { startDiskScan: { target: { volume: { id: '/' } }, replaces: 5 } }],
    ['forgetSavedDiskMap', (b) => b.forgetSavedDiskMap(7), { forgetSavedDiskMap: { scan: 7 } }],
    ['forgetSavedDiskMaps', (b) => b.forgetSavedDiskMaps(), 'forgetSavedDiskMaps'],
    ['startDiskScan of a folder', (b) => b.startDiskScan({ folder: { path: '/x' } }), { startDiskScan: { target: { folder: { path: '/x' } }, replaces: null } }],
    ['cancelDiskScan', (b) => b.cancelDiskScan(7), { cancelDiskScan: { scan: 7 } }],
    ['diskScans', (b) => b.diskScans(), 'diskScans'],
    ['diskChildren', (b) => b.diskChildren({ scan: 7, node: null, offset: 48, limit: 48, minFraction: 0.004 }), { diskChildren: { scan: 7, node: null, offset: 48, limit: 48, minFraction: 0.004 } }],
    ['diskRings', (b) => b.diskRings({ scan: 7, node: null, rings: 3, minShare: 0.01, maxNodes: 500 }), { diskRings: { scan: 7, node: null, rings: 3, minShare: 0.01, maxNodes: 500 } }],
    ['diskPath', (b) => b.diskPath(7, 4), { diskPath: { scan: 7, node: 4 } }],
    ['retryDiskScan', (b) => b.retryDiskScan(7), { retryDiskScan: { scan: 7 } }],
    ['openFullDiskAccessSettings', (b) => b.openFullDiskAccessSettings(), 'openFullDiskAccessSettings'],
    ['diskReveal', (b) => b.diskReveal(7, 4), { diskReveal: { scan: 7, node: 4 } }],
    ['health', (b) => b.health(), 'health'],
    ['startupEntries', (b) => b.startupEntries(), 'startupEntries'],
    ['weeklyReport', (b) => b.weeklyReport(3), { weeklyReport: { weeksAgo: 3 } }],
    ['alerts', (b) => b.alerts(10), { alerts: { limit: 10 } }],
    ['changes', (b) => b.changes({ hours: 24 }), { changes: { query: { hours: 24 } } }],
    ['gpuProcesses', (b) => b.gpuProcesses(), 'gpuProcesses'],
    ['startJob findFiles', (b) => started(b, { findFiles: { query: { folder: '/x', mode: 'both', minBytes: 5 } } }), { startJob: { spec: { findFiles: { query: { folder: '/x', mode: 'both', minBytes: 5 } } } } }],
    ['startJob rescanIssues', (b) => started(b, { rescanIssues: { query: { days: 7 } } }), { startJob: { spec: { rescanIssues: { query: { days: 7 } } } } }],
    ['startJob refreshBackup', (b) => started(b, 'refreshBackup'), { startJob: { spec: 'refreshBackup' } }],
    ['startJob fileHash of a process', (b) => started(b, { fileHash: { source: { process: REF } } }), { startJob: { spec: { fileHash: { source: { process: REF } } } } }],
    ['startJob fileHash of a path', (b) => started(b, { fileHash: { source: { path: '/a/b' } } }), { startJob: { spec: { fileHash: { source: { path: '/a/b' } } } } }],
    ['startJob sleepBlockers', (b) => started(b, 'sleepBlockers'), { startJob: { spec: 'sleepBlockers' } }],
    ['startJob bootAnalysis', (b) => started(b, 'bootAnalysis'), { startJob: { spec: 'bootAnalysis' } }],
    ['startJob appUsage', (b) => started(b, 'appUsage'), { startJob: { spec: 'appUsage' } }],
    ['startJob softwareInventory', (b) => started(b, 'softwareInventory'), { startJob: { spec: 'softwareInventory' } }],
    ['startJob helper turnOn', (b) => started(b, { helper: { action: 'turnOn' } }), { startJob: { spec: { helper: { action: 'turnOn' } } } }],
    ['helperStatus', (b) => b.helperStatus(), 'helperStatus'],
    ['jobResult', (b) => b.jobResult(4), { jobResult: { id: 4 } }],
    ['maintenance', (b) => b.maintenance(), 'maintenance'],
    ['backup', (b) => b.backup(), 'backup'],
    ['settings', (b) => b.settings(), 'settings'],
    ['saveSettings', (b) => b.saveSettings(SETTINGS), { saveSettings: { settings: SETTINGS } }],
    ['plugins', (b) => b.plugins(), 'plugins'],
    ['issues', (b) => b.issues({ days: 14 }), { issues: { query: { days: 14 } } }],
    ['unusual', (b) => b.unusual(), 'unusual'],
    ['forgetBaseline', (b) => b.forgetBaseline(), 'forgetBaseline'],
    ['searchSeed', (b) => b.searchSeed('Slack'), { searchSeed: { text: 'Slack' } }],
    ['webSearch', (b) => b.webSearch('slack high cpu'), { webSearch: { query: 'slack high cpu' } }],
    ['openVirusTotal', (b) => b.openVirusTotal('ab'.repeat(32)), { virusTotalUrl: { sha256: 'ab'.repeat(32) } }],
    ['startupOverview', (b) => b.startupOverview({ hideOs: true, hideSigned: false }), { startupOverview: { filter: { hideOs: true, hideSigned: false } } }],
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

  it('saveIssues asks the shell for the dialog with the window', async () => {
    const { backend, invoke } = fake();
    await backend.saveIssues(30);
    expect(invoke).toHaveBeenCalledWith('save_issues', { days: 30 });
  });

  it('saveInventoryReport and pickFolder ask the shell for its dialogs', async () => {
    const { backend, invoke } = fake();
    await backend.saveInventoryReport('json');
    expect(invoke).toHaveBeenCalledWith('save_inventory_report', { format: 'json' });
    await backend.pickFolder();
    expect(invoke).toHaveBeenLastCalledWith('pick_folder');
  });

  it('the tray popover is the shell’s window: fit, close, show the main window and quit are the shell’s commands', async () => {
    const { backend, invoke } = fake();
    await backend.fitTray(342);
    await backend.hideTray();
    await backend.quitApp();
    await backend.showMainWindow();
    expect(invoke.mock.calls).toEqual([
      ['plugin:paddock-shell|tray_fit_height', { height: 342 }],
      ['plugin:paddock-shell|tray_close'],
      ['plugin:paddock-shell|app_quit'],
      ['plugin:paddock-shell|window_focus', { label: 'main' }],
      ['plugin:paddock-shell|tray_close'],
    ]);
  });
});

describe('TauriBackend: errors', () => {
  it('turns a well-formed rejection into a CommandError', async () => {
    const { backend, invoke } = fake();
    invoke.mockRejectedValueOnce({ code: 'busy', message: 'no' });
    await expect(backend.whySlow()).rejects.toEqual({ code: 'busy', message: 'no' });
  });
  it('turns other rejections into internal errors', async () => {
    const { backend, invoke } = fake();
    invoke.mockRejectedValueOnce('plain text');
    await expect(backend.settings()).rejects.toEqual({ code: 'internal', message: 'plain text' });
    invoke.mockRejectedValueOnce(new Error('boom'));
    await expect(backend.settings()).rejects.toEqual({ code: 'internal', message: 'boom' });
  });

  it('has no timer: a request the shell has not answered stays pending, however long it takes (ER-19)', async () => {
    vi.useFakeTimers();
    try {
      const { invoke, listen } = fake();
      invoke.mockReturnValue(new Promise(() => undefined));
      const backend = new TauriBackend(invoke as unknown as InvokeFn, listen as unknown as ListenFn);
      let settled = false;
      const done = () => (settled = true);
      void backend.health().then(done, done);
      void backend.pickFolder().then(done, done);
      expect(vi.getTimerCount()).toBe(0);
      await vi.advanceTimersByTimeAsync(60 * 60_000);
      expect(settled).toBe(false);
    } finally {
      vi.useRealTimers();
    }
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
  it('the disk scan event has the name the shell emits', () => {
    expect(EVENTS.diskScan).toBe('dyno://disk-scan');
    expect(EVENTS.diskVolumes).toBe('dyno://disk-volumes');
  });


  it.each([
    ['onAlert', EVENTS.alert],
    ['onActivity', EVENTS.activity],
    ['onAgentStatus', EVENTS.agent],
    ['onDiskScan', EVENTS.diskScan],
    ['onDiskVolumes', EVENTS.diskVolumes],
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

describe('TauriBackend: widget layouts on the grid', () => {
  it('sends the revision with an edit and asks the app, not the agent, for a preview', async () => {
    const { backend, invoke } = fake();
    const edit = { dashboard: 'undo' } as never;
    await backend.editWidgetLayout(edit, 7);
    expect(invoke).toHaveBeenLastCalledWith('edit_widget_layout', { edit, baseRevision: 7 });
    await backend.editWidgetLayout(edit);
    expect(invoke).toHaveBeenLastCalledWith('edit_widget_layout', { edit, baseRevision: null });
    const request = { surface: 'dashboard', revision: 7, gesture: { toFirst: { id: 'cpu' } }, seq: 3 } as never;
    await backend.previewWidgetLayout(request);
    expect(invoke).toHaveBeenLastCalledWith('preview_widget_layout', { request });
  });
});
