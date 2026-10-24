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
    expect(all.usage).toEqual([]);
    expect(all.machine.at(-1)?.runners.memBytes).toBeGreaterThan(0);
    expect(all.machine.every((m) => m.runners.cpuPermille <= m.cpuPermille)).toBe(true);
    const one = await mock.metrics('docker-lab', 100_000);
    expect(one.folder.length).toBeGreaterThan(0);
    expect(one.usage.length).toBeGreaterThan(0);
    expect((await mock.status()).runners.find((r) => r.name === 'docker-lab')?.usage).toBeNull();
    expect((await mock.status()).runners.find((r) => r.name === 'build-win')?.usage?.processes).toBe(23);
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

describe('MockBackend runner configuration', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  it('reads a commented file with its form view', async () => {
    mock = make();
    const c = await mock.runnerConfig('ui-tests');
    expect(c.path).toContain('config.yaml');
    expect(c.text).toContain('# Execute how many tasks');
    expect(c.view.settings.capacity).toBe(1);
    expect(c.view.containerRunner).toBe(false);
    expect((await mock.runnerConfig('docker-lab')).view.containerRunner).toBe(true);
    expect(c.backups).toBeGreaterThan(0);
    expect(c.restartPending).toBe(false);
  });

  it('previews without writing and flags bad values', async () => {
    mock = make();
    const c = await mock.runnerConfig('ui-tests');
    const p = await mock.previewRunnerConfig('ui-tests', { mode: 'form', settings: { ...c.view.settings, capacity: 0 } });
    expect(p.issues[0]).toMatchObject({ severity: 'error', path: 'runner.capacity' });
    expect((await mock.runnerConfig('ui-tests')).text).toBe(c.text);
    const raw = await mock.previewRunnerConfig('ui-tests', { mode: 'raw', text: 'runner:\n\tx: 1' });
    expect(raw.issues[0]!.line).toBe(2);
  });

  it('saves with a backup and a fresh fingerprint, restarting after the job when it runs', async () => {
    mock = make();
    const c = await mock.runnerConfig('build-win');
    const saved = await mock.saveRunnerConfig('build-win', { mode: 'form', settings: { ...c.view.settings, capacity: 2 } }, c.fingerprint);
    expect(saved).toMatchObject({ restart: 'afterJob', changedKeys: ['runner.capacity'] });
    expect(saved.backup).toMatch(/^config\.yaml\.bak-20261022-\d{6}$/);
    expect(saved.fingerprint).not.toBe(c.fingerprint);
    const after = await mock.runnerConfig('build-win');
    expect(after.text).toContain('capacity: 2');
    expect(after.restartPending).toBe(true);
    expect((await mock.status()).runners.find((r) => r.name === 'build-win')!.pending).toBe('restart');
    const events = (await mock.activity('build-win', 7)).events;
    expect(events[0]).toMatchObject({ kind: 'configSaved', restart: true });
  });

  it('plans no restart for comments, and the next start for a stopped runner', async () => {
    mock = make();
    const c = await mock.runnerConfig('legacy-ci');
    const comment = await mock.saveRunnerConfig('legacy-ci', { mode: 'raw', text: `# note\n${c.text}` }, c.fingerprint);
    expect(comment.restart).toBe('notNeeded');
    const c2 = await mock.runnerConfig('docker-lab');
    const stopped = await mock.saveRunnerConfig('docker-lab', { mode: 'form', settings: { ...c2.view.settings, capacity: 2 } }, c2.fingerprint);
    expect(stopped.restart).toBe('notRunning');
  });

  it('restarts an idle runner at once and records the label check', async () => {
    mock = make();
    vi.useFakeTimers();
    try {
      const c = await mock.runnerConfig('ui-tests');
      const saved = await mock.saveRunnerConfig('ui-tests', { mode: 'form', settings: { ...c.view.settings, labels: [...c.view.settings.labels, 'extra:host'] } }, c.fingerprint);
      expect(saved.restart).toBe('afterJob');
      await vi.advanceTimersByTimeAsync(2_000);
      const events = (await mock.activity('ui-tests', 7)).events;
      expect(events.some((e) => e.kind === 'labelsVerified' && e.labels.includes('extra'))).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('refuses a stale base and a file with errors, and names the runner for unknown ones', async () => {
    mock = make();
    const c = await mock.runnerConfig('ui-tests');
    expect((await rejection(mock.saveRunnerConfig('ui-tests', { mode: 'raw', text: c.text }, 'old'))).code).toBe('busy');
    const bad = await rejection(mock.saveRunnerConfig('ui-tests', { mode: 'form', settings: { ...c.view.settings, capacity: 99 } }, c.fingerprint));
    expect(bad.code).toBe('badRequest');
    expect(bad.message).toContain('runner.capacity');
    expect((await rejection(mock.runnerConfig('nope'))).code).toBe('unknownRunner');
  });

  it('has config, label and process events in the history', async () => {
    mock = make();
    const kinds = async (n: string) => (await mock.activity(n, 7)).events.map((e) => e.kind);
    expect(await kinds('build-win')).toEqual(expect.arrayContaining(['configSaved', 'labelsVerified', 'processStopped']));
    expect(await kinds('docker-lab')).toContain('labelsMismatch');
    const job = (await mock.activity('build-win', 7)).summary.recentJobs[0]!;
    expect(job.peakCpuPermille).toBeGreaterThan(0);
    expect(job.peakMemBytes).toBeGreaterThan(0);
  });
});

describe('MockBackend processes', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  it('lists the runner, a job’s children and three leftovers that match the snapshot', async () => {
    mock = make();
    const l = await mock.runnerProcesses('build-win');
    expect(l.idle).toBe(false);
    expect(l.cleanup).toBe(true);
    expect(l.keep).toEqual({ amount: 10, unit: 'minutes' });
    expect(l.entries.find((e) => e.isRunner)?.pid).toBe(l.runnerPid);
    const left = l.entries.filter((e) => e.leftover);
    expect(left.map((e) => e.leftover!.fate.kind)).toEqual(['stopsAt', 'stopsAt', 'never']);
    expect(left[0]!.leftover!.rule).toBe('*GradleDaemon*');
    expect(l.entries.some((e) => e.command.includes('[redacted]'))).toBe(true);
    expect(l.entries.some((e) => e.job?.running)).toBe(true);
    const snap = (await mock.status()).runners.find((r) => r.name === 'build-win')!;
    expect(snap.leftovers).toBe(left.length);
  });

  it('has an empty list for a runner that is not running', async () => {
    mock = make();
    expect(await mock.runnerProcesses('docker-lab')).toMatchObject({ entries: [], runnerPid: null });
    expect((await mock.runnerProcesses('ui-tests')).idle).toBe(true);
  });

  it('shows leftovers as off when automatic cleanup is off', async () => {
    mock = make();
    const s = await mock.settings();
    s.config.defaults.processes.cleanup = false;
    await mock.saveSettings(s.config);
    const l = await mock.runnerProcesses('build-win');
    expect(l.cleanup).toBe(false);
    expect(l.entries.filter((e) => e.leftover).map((e) => e.leftover!.fate.kind)).toEqual(['off', 'off', 'never']);
  });

  it('keeps a leftover longer and refuses what cannot be kept', async () => {
    mock = make();
    const l = await mock.runnerProcesses('build-win');
    const e = l.entries.find((x) => x.leftover?.fate.kind === 'stopsAt')!;
    const at = (e.leftover!.fate as { at: number }).at;
    await mock.keepProcess('build-win', e.pid, e.startedAt!, { amount: 1, unit: 'hours' });
    const after = (await mock.runnerProcesses('build-win')).entries.find((x) => x.pid === e.pid)!;
    expect((after.leftover!.fate as { at: number }).at).toBe(at + 3_600_000);
    const never = l.entries.find((x) => x.leftover?.fate.kind === 'never')!;
    expect((await rejection(mock.keepProcess('build-win', never.pid, never.startedAt!, { amount: 1, unit: 'hours' }))).code).toBe('badRequest');
    expect((await rejection(mock.keepProcess('build-win', 1, 1, { amount: 1, unit: 'hours' }))).code).toBe('badRequest');
    expect((await rejection(mock.keepProcess('build-win', e.pid, e.startedAt!, { amount: 0, unit: 'hours' }))).message).toContain('at least 1');
  });

  it('stops a process, drops its row, logs it, and refuses strangers and the runner without consent', async () => {
    mock = make();
    const l = await mock.runnerProcesses('build-win');
    const e = l.entries.find((x) => x.leftover?.rule === '*GradleDaemon*')!;
    await mock.stopProcess('build-win', e.pid, e.startedAt, false);
    const after = await mock.runnerProcesses('build-win');
    expect(after.entries.some((x) => x.pid === e.pid)).toBe(false);
    expect((await mock.status()).runners.find((r) => r.name === 'build-win')!.leftovers).toBe(2);
    expect((await mock.activity('build-win', 1)).events[0]).toMatchObject({ kind: 'processStopped', by: 'user', pid: e.pid });
    const gone = await rejection(mock.stopProcess('build-win', e.pid, e.startedAt, false));
    expect(gone).toMatchObject({ code: 'badRequest' });
    expect(gone.message).toContain('not part of this runner’s process tree');
    expect((await rejection(mock.stopProcess('build-win', l.runnerPid!, null, false))).message).toContain('runner process itself');
  });

  it('restarts the runner when the user ends its process', async () => {
    mock = make();
    const l = await mock.runnerProcesses('ui-tests');
    await mock.stopProcess('ui-tests', l.runnerPid!, null, true);
    expect((await mock.status()).runners.find((r) => r.name === 'ui-tests')!.status).toBe('starting');
  });

  it('validates the process policy on save', async () => {
    mock = make();
    const base = (await mock.settings()).config;
    const bad = (f: (c: typeof base) => void) => {
      const c = structuredClone(base);
      f(c);
      return rejection(mock.saveSettings(c));
    };
    expect((await bad((c) => (c.defaults.processes.keep.amount = 0))).message).toContain('at least 1');
    expect((await bad((c) => (c.defaults.processes.keep = { amount: 31, unit: 'days' }))).message).toContain('30 days');
    expect((await bad((c) => (c.defaults.processes.stop_timeout_secs = 0))).message).toContain('1 to 300');
    expect((await bad((c) => c.defaults.processes.rules.push({ pattern: ' ', keep: 'never' }))).message).toContain('needs a pattern');
    expect((await bad((c) => c.defaults.processes.rules.push({ pattern: 'x', keep: { amount: 0, unit: 'hours' } }))).message).toContain('rule "x"');
    expect((await bad((c) => (c.runners[0]!.processes = { keep: { amount: 0, unit: 'hours' } }))).message).toContain('build-win: ');
    const ok = structuredClone(base);
    ok.runners[0]!.processes = { cleanup: false, rules: [{ pattern: 'x', keep: 'never' }] };
    await mock.saveSettings(ok);
    expect((await mock.runnerProcesses('build-win')).cleanup).toBe(false);
  });
});

describe('MockBackend insights, storage, Debrief and channels', () => {
  let mock: MockBackend;
  beforeEach(() => {
    mock = make();
  });
  afterEach(() => mock.dispose());

  it('answers insights for a range, clamped to 1-90 days', async () => {
    const r = await mock.insights(30);
    expect(r.insights.rangeDays).toBe(30);
    expect(r.insights.repos[0]!.days).toHaveLength(30);
    expect((await mock.insights(500)).insights.rangeDays).toBe(90);
    expect((await mock.insights(0)).insights.rangeDays).toBe(1);
  });

  it('previews and prunes the container engine, and refuses an empty choice or a running job', async () => {
    expect((await mock.engineReport()).engine).toBe('docker');
    const p = await mock.previewEnginePrune({ kinds: ['danglingImages', 'buildCache'], olderThanDays: 0 });
    expect(p.items).toHaveLength(2);
    expect(p.totalBytes).toBeGreaterThan(0);
    expect((await rejection(mock.previewEnginePrune({ kinds: [], olderThanDays: 0 }))).code).toBe('badRequest');
    expect((await rejection(mock.pruneEngine({ kinds: ['volumes'], olderThanDays: 0 }))).code).toBe('busy');
    const quiet = make({ scenario: 'demo' });
    (quiet as unknown as { runners: { currentJob: unknown }[] }).runners.forEach((r) => (r.currentJob = null));
    const res = await quiet.pruneEngine({ kinds: ['danglingImages'], olderThanDays: 0 });
    expect(res.freed[0]![0]).toBe('danglingImages');
    quiet.dispose();
  });

  it('reports, previews and trims the cache', async () => {
    const info = await mock.cacheReport('ui-tests');
    expect(info.entries).toBeGreaterThan(0);
    const plan = await mock.previewCacheClean('ui-tests', { maxMb: 0, maxAgeDays: 20 });
    expect(plan.entries.every(([, why]) => why === 'old')).toBe(true);
    expect((await rejection(mock.previewCacheClean('ui-tests', { maxMb: 0, maxAgeDays: 0 }))).code).toBe('badRequest');
    const done = await mock.cleanCache('ui-tests', { maxMb: 0, maxAgeDays: 20 });
    expect(done.failures).toEqual([]);
    expect((await mock.cacheReport('ui-tests')).bytes).toBeLessThan(info.bytes);
    expect((await rejection(mock.cleanCache('build-win', { maxMb: 1, maxAgeDays: 0 }))).code).toBe('busy');
  });

  it('hands a job to Debrief and says so when it is not installed', async () => {
    expect((await mock.debriefStatus()).cliName).toBe('debrief');
    expect(await mock.openInDebrief('build-win', 'run 7', 1, 2)).toMatchObject({ result: 'opened' });
    expect((await rejection(mock.openInDebrief('build-win', 'x', 1, null))).code).toBe('badRequest');
    (mock as unknown as { debrief: boolean }).debrief = false;
    expect(await mock.openInDebrief('build-win', 'x', null, null)).toMatchObject({ result: 'notInstalled' });
    expect((await mock.debriefStatus()).installed).toBe(false);
  });

  it('keeps channels and their secrets apart and validates them', async () => {
    expect((await mock.channels()).channels).toEqual([]);
    const hook = {
      id: 'ch-1',
      name: 'Hook',
      enabled: false,
      events: ['diskLow'],
      minSeverity: 'info' as const,
      transport: { type: 'webhook' as const, url: 'https://example.org/h', secretKey: 'channel-ch-1' },
    };
    await mock.saveChannels({ channels: [hook] });
    expect((await mock.channels()).channels).toHaveLength(1);
    expect(await mock.hasChannelSecret('channel-ch-1')).toBe(false);
    await mock.setChannelSecret('channel-ch-1', 'tok');
    expect(await mock.hasChannelSecret('channel-ch-1')).toBe(true);
    await mock.clearChannelSecret('channel-ch-1');
    expect(await mock.hasChannelSecret('channel-ch-1')).toBe(false);
    expect((await rejection(mock.setChannelSecret('k', ''))).code).toBe('badRequest');
    expect(await mock.testChannel('ch-1')).toContain('Sent');
    expect((await rejection(mock.testChannel('nope'))).code).toBe('badRequest');
    expect((await rejection(mock.saveChannels({ channels: [hook, hook] }))).message).toContain('unique');
    expect((await rejection(mock.saveChannels({ channels: [{ ...hook, transport: { type: 'webhook', url: 'ftp://x', secretKey: null } }] }))).message).toContain('http');
    const many = Array.from({ length: 9 }, (_, i) => ({ ...hook, id: `ch-${i}` }));
    expect((await rejection(mock.saveChannels({ channels: many }))).message).toContain('at most 8');
  });
});
