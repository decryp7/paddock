import type { BackendEvent, CommandError, Config, RegisterRequest, RunnerConfig } from './api.types';
import { MockBackend, type MockOptions } from './mock-backend';

const NOW = new Date(2026, 9, 22, 12, 0, 0).getTime();

function make(options: MockOptions = {}): MockBackend {
  return new MockBackend({ now: () => NOW, ...options });
}

async function rejection(p: Promise<unknown>): Promise<CommandError> {
  try {
    await p;
  } catch (e) {
    return e as CommandError;
  }
  throw new Error('expected a rejection');
}

function collect(mock: MockBackend): { events: BackendEvent[]; stop: () => void } {
  const events: BackendEvent[] = [];
  const stop = mock.watch((e) => events.push(e));
  return { events, stop };
}

describe('MockBackend scenarios', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  it('demo has four runners and a config problem', async () => {
    mock = make();
    const s = await mock.status();
    expect(s.runners.map((r) => r.name)).toEqual(['build-win', 'ui-tests', 'docker-lab', 'legacy-ci']);
    expect(s.runners.map((r) => r.status)).toEqual(['busy', 'idle', 'crashLoop', 'paused']);
    expect(s.service.configProblems).toHaveLength(1);
    expect(mock.platform()).toBe('browser');
    expect(make({ platform: 'macos' }).platform()).toBe('macos');
  });

  it('empty has no runners and no problems', async () => {
    mock = make({ scenario: 'empty' });
    expect((await mock.status()).runners).toEqual([]);
    expect((await mock.settings()).problems).toEqual([]);
  });

  it('unavailable and notInstalled reject with serviceUnavailable until installed', async () => {
    for (const scenario of ['unavailable', 'notInstalled'] as const) {
      mock = make({ scenario });
      expect((await rejection(mock.status())).code).toBe('serviceUnavailable');
      expect((await rejection(mock.settings())).code).toBe('serviceUnavailable');
      expect((await rejection(mock.metrics(null, 1))).code).toBe('serviceUnavailable');
      expect((await rejection(mock.start('build-win'))).code).toBe('serviceUnavailable');
      const { events } = collect(mock);
      await mock.installService();
      expect(events.some((e) => e.kind === 'connection' && e.connection.state === 'connected')).toBe(true);
      expect((await mock.status()).runners).toHaveLength(4);
      mock.dispose();
    }
  });

  it('mismatch rejects with protocolMismatch', async () => {
    mock = make({ scenario: 'mismatch' });
    expect((await rejection(mock.status())).code).toBe('protocolMismatch');
  });

  it('watch delivers the connection first and unsubscribes', async () => {
    mock = make({ scenario: 'unavailable' });
    const { events, stop } = collect(mock);
    await Promise.resolve();
    expect(events[0]).toMatchObject({ kind: 'connection', connection: { state: 'unavailable' } });
    stop();
    mock.start('x').catch(() => undefined);
    mock = make({ scenario: 'mismatch' });
    const m = collect(mock);
    await Promise.resolve();
    expect(m.events[0]).toMatchObject({ connection: { state: 'mismatch' } });
    mock = make({ scenario: 'notInstalled' });
    const n = collect(mock);
    await Promise.resolve();
    expect(n.events[0]).toMatchObject({ connection: { state: 'unavailable', message: expect.stringContaining('not installed') } });
  });

  it('uninstalling makes the service unavailable', async () => {
    mock = make();
    const { events } = collect(mock);
    await mock.uninstallService();
    expect(events.at(-1)).toMatchObject({ kind: 'connection', connection: { state: 'unavailable' } });
    expect((await mock.serviceInstall()).installed).toBe(false);
  });

  it('applies latency with timers', async () => {
    vi.useFakeTimers();
    try {
      mock = make({ latencyMs: 50 });
      let done = false;
      void mock.autostart().then(() => (done = true));
      await vi.advanceTimersByTimeAsync(49);
      expect(done).toBe(false);
      await vi.advanceTimersByTimeAsync(2);
      expect(done).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('MockBackend runner actions', () => {
  let mock: MockBackend;
  beforeEach(() => {
    vi.useFakeTimers();
    mock = make();
  });
  afterEach(() => {
    mock.dispose();
    vi.useRealTimers();
  });
  const runner = async (name: string) => (await mock.status()).runners.find((r) => r.name === name)!;

  it('stop on a busy runner is pending; on an idle one it stops', async () => {
    await mock.stop('build-win');
    expect((await runner('build-win')).pending).toBe('stop');
    await mock.stop('ui-tests');
    expect((await runner('ui-tests')).status).toBe('stopped');
  });

  it('start boots a stopped runner and rejects a running one', async () => {
    expect((await rejection(mock.start('ui-tests'))).code).toBe('busy');
    await mock.stop('ui-tests');
    const { events } = collect(mock);
    await mock.start('ui-tests');
    expect((await runner('ui-tests')).status).toBe('starting');
    await vi.advanceTimersByTimeAsync(1600);
    expect((await runner('ui-tests')).status).toBe('idle');
    expect(events.filter((e) => e.kind === 'status').length).toBeGreaterThanOrEqual(2);
    const logs = await mock.logs('ui-tests', null, 5);
    expect(logs.at(-1)?.text).toContain('declare successfully');
    await mock.start('docker-lab');
    expect((await runner('docker-lab')).status).toBe('starting');
  });

  it('restart waits for a job, otherwise counts a restart', async () => {
    await mock.restart('build-win');
    expect((await runner('build-win')).pending).toBe('restart');
    const before = (await runner('ui-tests')).restarts24h;
    await mock.restart('ui-tests');
    const after = await runner('ui-tests');
    expect(after.restarts24h).toBe(before + 1);
    expect(after.status).toBe('starting');
  });

  it('drain and resume', async () => {
    await mock.drain('build-win');
    expect(await runner('build-win')).toMatchObject({ status: 'draining', pending: 'pause' });
    await mock.resume('build-win');
    expect(await runner('build-win')).toMatchObject({ status: 'busy', pending: null });
    await mock.drain('ui-tests');
    expect((await runner('ui-tests')).status).toBe('paused');
    await mock.resume('ui-tests');
    expect((await runner('ui-tests')).status).toBe('idle');
    expect((await rejection(mock.resume('ui-tests'))).code).toBe('badRequest');
  });

  it('unknown runners reject', async () => {
    expect((await rejection(mock.start('nope'))).code).toBe('unknownRunner');
    expect((await rejection(mock.revealLogs('nope'))).code).toBe('unknownRunner');
    await mock.revealLogs('ui-tests');
  });

  it('checkUpdate refreshes the latest version and rejects unmanaged binaries', async () => {
    await mock.checkUpdate('ui-tests');
    const r = await runner('ui-tests');
    expect(r.latestVersion).toBe('5.3.0');
    expect(r.updateTarget).toBe('5.3.0');
    expect(r.checkedAt).toBe(NOW);
    expect((await rejection(mock.checkUpdate('legacy-ci'))).code).toBe('badRequest');
    await mock.skipVersion('ui-tests', '5.3.0');
    await mock.checkUpdate('ui-tests');
    expect((await runner('ui-tests')).updateTarget).toBeNull();
  });

  it('runs the whole update flow and notifies', async () => {
    const { events } = collect(mock);
    await mock.applyUpdate('ui-tests');
    expect((await runner('ui-tests')).updatePhase.phase).toBe('downloading');
    expect((await rejection(mock.applyUpdate('ui-tests'))).code).toBe('busy');
    await vi.advanceTimersByTimeAsync(1800 * 7);
    const r = await runner('ui-tests');
    expect(r.updatePhase.phase).toBe('idle');
    expect(r.currentVersion).toBe('5.3.0');
    expect(r.lastUpdate?.outcome.result).toBe('updated');
    const phases = events.flatMap((e) => (e.kind === 'status' ? [e.status.runners.find((x) => x.name === 'ui-tests')?.updatePhase.phase] : []));
    expect(phases).toEqual(expect.arrayContaining(['downloading', 'verifying', 'draining', 'stopping', 'swapping', 'confirming']));
    expect(events.some((e) => e.kind === 'notification' && e.notification.kind === 'updated')).toBe(true);
  });

  it('applyUpdate rejections', async () => {
    expect((await rejection(mock.applyUpdate('legacy-ci'))).code).toBe('badRequest');
    expect((await rejection(mock.applyUpdate('build-win'))).message).toContain('up to date');
  });

  it('draining a busy runner waits 30 s and cancel is only allowed early', async () => {
    await mock.checkUpdate('build-win');
    await mock.applyUpdate('build-win');
    await vi.advanceTimersByTimeAsync(1800 * 2);
    expect((await runner('build-win')).status).toBe('draining');
    await mock.cancelUpdate('build-win');
    const r = await runner('build-win');
    expect(r.updatePhase.phase).toBe('idle');
    expect(r.status).toBe('busy');
    expect(r.lastUpdate?.outcome.result).toBe('cancelled');
    expect((await rejection(mock.cancelUpdate('build-win'))).code).toBe('badRequest');
  });

  it('cannot cancel while swapping', async () => {
    await mock.applyUpdate('ui-tests');
    await vi.advanceTimersByTimeAsync(1800 * 4);
    expect((await runner('ui-tests')).updatePhase.phase).toBe('swapping');
    expect((await rejection(mock.cancelUpdate('ui-tests'))).code).toBe('busy');
  });

  it('pins, validates and unpins versions', async () => {
    await mock.pinVersion('ui-tests', '5.2.0');
    let r = await runner('ui-tests');
    expect(r.updatePolicy.pinned).toBe('5.2.0');
    expect(r.updateTarget).toBeNull();
    await mock.pinVersion('ui-tests', '5.4.0-rc.1');
    expect((await runner('ui-tests')).updateTarget).toBe('5.4.0-rc.1');
    expect((await rejection(mock.pinVersion('ui-tests', 'latest'))).code).toBe('badRequest');
    await mock.pinVersion('ui-tests', null);
    r = await runner('ui-tests');
    expect(r.updatePolicy.pinned).toBeNull();
    expect(r.updateTarget).toBe('5.3.0');
    await mock.pinVersion('build-win', null);
    expect((await runner('build-win')).updateTarget).toBeNull();
    const cfg = (await mock.settings()).config.runners.find((c) => c.name === 'ui-tests');
    expect(cfg?.update?.pinned).toBeNull();
  });

  it('skipVersion is idempotent', async () => {
    await mock.skipVersion('ui-tests', '5.3.0');
    await mock.skipVersion('ui-tests', '5.3.0');
    expect((await runner('ui-tests')).updatePolicy.skipped).toEqual(['5.3.0']);
  });
});

describe('MockBackend data', () => {
  let mock: MockBackend;
  beforeEach(() => (mock = make()));
  afterEach(() => mock.dispose());

  it('pages logs and searches them', async () => {
    const tail = await mock.logs('build-win', null, 10);
    expect(tail).toHaveLength(10);
    const first = tail[0]!.seq;
    const after = await mock.logs('build-win', first, 3);
    expect(after.map((l) => l.seq)).toEqual([first + 1, first + 2, first + 3]);
    const hits = await mock.searchLogs('build-win', ' LEVEL=info ', 5);
    expect(hits.length).toBeGreaterThan(0);
    expect(hits.length).toBeLessThanOrEqual(5);
    expect(hits.every((l) => l.text.toLowerCase().includes('level=info'))).toBe(true);
    expect(await mock.searchLogs('build-win', '   ', 5)).toEqual([]);
  });

  it('builds activity with token-dependent outcomes', async () => {
    const withToken = await mock.activity('build-win', 7);
    expect(withToken.hasApiToken).toBe(true);
    expect(withToken.summary.days.length).toBeGreaterThanOrEqual(7);
    expect(withToken.jobDays.some((d) => d.success > 0)).toBe(true);
    expect(withToken.events.length).toBeLessThanOrEqual(200);
    const noToken = await mock.activity('legacy-ci', 30);
    expect(noToken.hasApiToken).toBe(false);
    expect(noToken.jobDays.every((d) => d.success === 0 && d.avgQueuedMs === null)).toBe(true);
    expect(noToken.summary.days.length).toBeGreaterThan(withToken.summary.days.length);
    const one = await mock.activity('ui-tests', 0);
    expect(one.summary.days).toHaveLength(1);
    expect(withToken.summary.busy.some((s) => s.to === NOW)).toBe(true);
  });

  it('returns machine and folder metrics, clamping the range', async () => {
    const all = await mock.metrics(null, 24);
    expect(all.machine.length).toBeGreaterThan(0);
    expect(all.folder).toEqual([]);
    const one = await mock.metrics('docker-lab', 100_000);
    expect(one.folder.length).toBeGreaterThan(0);
    await mock.metrics('docker-lab', 0);
    expect((await rejection(mock.metrics('nope', 1))).code).toBe('unknownRunner');
  });

  it('scans disks and cleans them', async () => {
    const d = await mock.disk('docker-lab');
    expect(d.scan.entries.length).toBeGreaterThan(0);
    expect(d.preview.freedBytes).toBeGreaterThan(0);
    expect(d.lastCleanup).not.toBeNull();
    expect((await mock.disk('legacy-ci')).policy.work_max_age_hours).toBe(168);
    expect((await mock.disk('ui-tests')).policy.automatic).toBeDefined();
    const events: BackendEvent[] = [];
    mock.watch((e) => events.push(e));
    const report = await mock.cleanDisk('docker-lab');
    expect(report.trigger).toBe('manual');
    expect(report.failures).toHaveLength(1);
    expect(events.some((e) => e.kind === 'status')).toBe(true);
    const status = (await mock.status()).runners.find((r) => r.name === 'docker-lab')!;
    expect(status.disk?.low).toBe(false);
    const act = await mock.activity('docker-lab', 7);
    expect(act.events[0]?.kind).toBe('cleanup');
  });

  it('gives run details only with a token and a current job', async () => {
    const d = await mock.runDetails('build-win');
    expect(d?.url).toContain('/acme/storefront/actions/runs/342');
    expect(await mock.runDetails('ui-tests')).toBeNull();
    await mock.clearApiToken('https://git.example.com');
    expect(await mock.runDetails('build-win')).toBeNull();
    expect(await mock.hasApiToken('https://git.example.com')).toBe(false);
    expect((await rejection(mock.setApiToken('https://git.example.com', 'short'))).code).toBe('badRequest');
    await mock.setApiToken('https://git.example.com', 'long-enough-token');
    expect(await mock.hasApiToken('https://git.example.com')).toBe(true);
  });
});

describe('MockBackend settings and runners', () => {
  let mock: MockBackend;
  beforeEach(() => {
    vi.useFakeTimers();
    mock = make();
  });
  afterEach(() => {
    mock.dispose();
    vi.useRealTimers();
  });

  it('saves settings into the runner snapshots and validates the stall threshold', async () => {
    const reply = await mock.settings();
    const config: Config = structuredClone(reply.config);
    config.runners[0]!.labels = ['changed'];
    config.runners[0]!.enabled = false;
    await mock.saveSettings(config);
    const r = (await mock.status()).runners[0]!;
    expect(r.labels).toEqual(['changed']);
    expect(r.enabled).toBe(false);
    config.defaults.watchdog.threshold = 0;
    expect((await rejection(mock.saveSettings(config))).message).toContain('stall threshold');
    config.defaults.watchdog.threshold = 3;
    config.runners = config.runners.slice(0, 1);
    await mock.saveSettings(config);
  });

  it('adds and removes runners', async () => {
    const conf: RunnerConfig = { name: 'extra', kind: 'gitea-runner', dir: 'C:\\x', labels: ['a'], session: 'interactive', logged_off: 'wait', enabled: true };
    await mock.addRunner(conf);
    expect((await rejection(mock.addRunner(conf))).code).toBe('badRequest');
    const added = (await mock.status()).runners.find((r) => r.name === 'extra');
    expect(added?.status).toBe('notRegistered');
    await mock.removeRunner('extra');
    expect((await mock.status()).runners.find((r) => r.name === 'extra')).toBeUndefined();
    expect((await rejection(mock.removeRunner('extra'))).code).toBe('unknownRunner');
  });

  it('registers a runner and validates the request', async () => {
    const ok: RegisterRequest = {
      runner: 'new-one',
      serverUrl: 'https://git.example.com',
      token: 'abcdefgh',
      displayName: null,
      labels: ['x'],
      dir: null,
      session: 'interactive',
      loggedOff: 'wait',
      rememberToken: false,
    };
    expect((await rejection(mock.register({ ...ok, runner: 'Bad Name' }))).message).toContain('a–z');
    expect((await rejection(mock.register({ ...ok, serverUrl: 'ftp://x' }))).message).toContain('https://');
    expect((await rejection(mock.register({ ...ok, token: 'short' }))).message).toContain('token');
    expect((await rejection(mock.register({ ...ok, runner: 'ui-tests' }))).message).toContain('exists already');
    await mock.register(ok);
    const r = (await mock.status()).runners.find((x) => x.name === 'new-one')!;
    expect(r.dir).toContain('new-one');
    expect(r.status).toBe('starting');
    await mock.register({ ...ok, runner: 'second', dir: 'D:\\here' });
    expect((await mock.status()).runners.find((x) => x.name === 'second')?.dir).toBe('D:\\here');
  });

  it('handles the app-level calls', async () => {
    expect((await mock.appUpdateStatus()).available).toBe(false);
    expect((await rejection(mock.checkAppUpdate())).code).toBe('notImplemented');
    expect((await rejection(mock.installAppUpdate())).code).toBe('notImplemented');
    expect(await mock.autostart()).toBe(true);
    await mock.setAutostart(false);
    expect(await mock.autostart()).toBe(false);
    expect(await mock.exportDiagnostics()).toMatch(/pitcrew-diagnostics-2026-10-22-12-00-00\.zip$/);
    const open = vi.spyOn(window, 'open').mockImplementation(() => null);
    await mock.openUrl('https://example.com/x');
    expect(open).toHaveBeenCalledWith('https://example.com/x', '_blank', 'noopener');
    expect((await rejection(mock.openUrl('javascript:alert(1)'))).code).toBe('badRequest');
    open.mockRestore();
    expect(await mock.serviceInstall()).toMatchObject({ installed: true, running: true });
  });
});

describe('MockBackend simulation', () => {
  it('ticks status, logs and notifications while watched, and stops when unwatched', async () => {
    vi.useFakeTimers();
    const mock = make({ simulate: true });
    try {
      const { events, stop } = collect(mock);
      await vi.advanceTimersByTimeAsync(5_000 * 36);
      const kinds = events.filter((e) => e.kind === 'notification').map((e) => (e.kind === 'notification' ? e.notification.kind : ''));
      expect(kinds).toEqual(expect.arrayContaining(['cleanedUp', 'serverUnreachable']));
      expect(events.filter((e) => e.kind === 'status').length).toBeGreaterThanOrEqual(36);
      const logs = await mock.logs('build-win', null, 3);
      expect(logs.some((l) => l.text.includes('[ci/build]'))).toBe(true);
      stop();
      const n = events.length;
      await vi.advanceTimersByTimeAsync(20_000);
      expect(events.length).toBe(n);
    } finally {
      mock.dispose();
      vi.useRealTimers();
    }
  });

  it('does not simulate outside the demo scenario', async () => {
    vi.useFakeTimers();
    const mock = make({ simulate: true, scenario: 'empty' });
    try {
      const { events } = collect(mock);
      await vi.advanceTimersByTimeAsync(20_000);
      expect(events.filter((e) => e.kind === 'status')).toHaveLength(0);
    } finally {
      mock.dispose();
      vi.useRealTimers();
    }
  });
});
