import { readValue } from './measured';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import type { Alert, BackupReport, BootReport, FinderQuery, FinderResult, JobOutput, JobSpec, ProcessSample, SleepReport, SoftwareInventory, UsageReport } from './api.types';
import { MockBackend, type MockOptions, applyMockPlatform, mockOptionsFromUrl } from './mock-backend';

const NOW = 1_700_000_000_000;
const make = (o: MockOptions = {}) => new MockBackend({ latencyMs: 0, now: () => NOW, ...o });

/** Starts a job and plays it to its end the way the shell's jobs store sees it (every status the sink hears); a failed job rejects with its message, a cancelled one resolves `null`. */
async function runJob(b: MockBackend, spec: JobSpec): Promise<JobOutput | null> {
  const id = await b.startJob(spec);
  for (let i = 0; i < 200; i++) {
    const s = b.jobStatuses().find((j) => j.id === id)!;
    if (s.state === 'done') return b.jobResult(id);
    if (s.state === 'cancelled') return null;
    if (s.state === 'failed') throw { code: 'internal', message: s.message };
    await new Promise((r) => setTimeout(r, 0));
  }
  throw new Error('the job never ended');
}
const proc = (b: MockBackend, name: string): ProcessSample => b.liveUpdate('processes').processes!.find((p) => p.name === name)!;
const ref = (p: ProcessSample) => ({ pid: p.pid, started: readValue(p.started) });

describe('mockOptionsFromUrl', () => {
  it('reads jobstepms as a whole number of milliseconds and ignores anything else', () => {
    expect(mockOptionsFromUrl('?jobstepms=600000').jobStepMs).toBe(600000);
    expect(mockOptionsFromUrl('?jobstepms=0').jobStepMs).toBe(0);
    for (const bad of ['-1', '2.5', 'slow', '']) expect(mockOptionsFromUrl(`?jobstepms=${bad}`)).not.toHaveProperty('jobStepMs');
  });

  it('reads jobsteps as a whole number from 1 to 100 and ignores anything else', () => {
    expect(mockOptionsFromUrl('?jobsteps=6').jobSteps).toBe(6);
    for (const bad of ['0', '101', '2.5', 'many', '']) expect(mockOptionsFromUrl(`?jobsteps=${bad}`)).not.toHaveProperty('jobSteps');
    expect(mockOptionsFromUrl('')).not.toHaveProperty('jobSteps');
  });

  it('a mock made with jobSteps keeps its jobs running past the first step', async () => {
    const b = new MockBackend({ latencyMs: 0, jobSteps: 3 });
    expect(b.jobSteps).toBe(3);
    const id = await b.startJob({ rescanIssues: { query: { days: 14 } } });
    expect(b.jobStatuses().find((j) => j.id === id)).toMatchObject({ state: 'running' });
    expect(new MockBackend({ latencyMs: 0 }).jobSteps).toBe(1);
  });

  it('defaults to a macOS laptop with latency', () => {
    expect(mockOptionsFromUrl('')).toEqual({ machine: 'laptop', os: 'macos', latencyMs: 150, fail: new Set(), widgetStorage: true, widgets: { popup: {}, agentStopped: false } });
  });
  it.each(['windows', 'macos', 'linux'] as const)('?mock=%s picks the OS', (os) => {
    expect(mockOptionsFromUrl(`?mock=${os}`).os).toBe(os);
  });
  it('accepts desktop and combinations, case-insensitively', () => {
    expect(mockOptionsFromUrl('?mock=desktop').machine).toBe('desktop');
    expect(mockOptionsFromUrl('?mock=Linux,DESKTOP')).toMatchObject({ os: 'linux', machine: 'desktop' });
    expect(mockOptionsFromUrl('?mock=bogus').os).toBe('macos');
  });
  it('reads fail and latency', () => {
    const o = mockOptionsFromUrl('?fail=health,whySlow&latency=0');
    expect([...o.fail!]).toEqual(['health', 'whySlow']);
    expect(o.latencyMs).toBe(0);
    expect(mockOptionsFromUrl('?latency=abc').latencyMs).toBe(150);
  });
});

describe('applyMockPlatform', () => {
  it('sets data-platform from the URL', () => {
    const root = document.createElement('html');
    applyMockPlatform('?mock=windows', root);
    expect(root.dataset['platform']).toBe('windows');
  });
  it('can show another OS chrome than the machine the mock imitates (screenshots)', () => {
    const root = document.createElement('html');
    applyMockPlatform('?mock=windows&chrome=macos', root);
    expect(root.dataset['platform']).toBe('macos');
    const other = document.createElement('html');
    applyMockPlatform('?mock=windows&chrome=bogus', other);
    expect(other.dataset['platform']).toBe('windows');
  });
  it('defaults to macOS and never overrides what the shell set', () => {
    const root = document.createElement('html');
    applyMockPlatform('', root);
    expect(root.dataset['platform']).toBe('macos');
    applyMockPlatform('?mock=linux', root);
    expect(root.dataset['platform']).toBe('macos');
  });
});

describe('OS differences', () => {
  it('disks follow the OS', async () => {
    const mounts = async (os: 'windows' | 'macos' | 'linux') => make({ os }).liveUpdate('summary').system.disks.map((d) => d.mount);
    expect(await mounts('windows')).toEqual(['C:\\', 'D:\\']);
    expect(await mounts('macos')).toEqual(['/']);
    expect(await mounts('linux')).toEqual(['/', '/mnt/data']);
  });
  it('process names follow the OS', () => {
    const win = make({ os: 'windows' }).liveUpdate('processes').processes!.map((p) => p.name);
    const mac = make({ os: 'macos' }).liveUpdate('processes').processes!.map((p) => p.name);
    expect(win).toContain('rustc.exe');
    expect(mac).toContain('rustc');
    expect(mac).not.toContain('rustc.exe');
  });
  it('processDetails: no affinity on macOS, a full mask elsewhere', async () => {
    const mac = make({ os: 'macos' });
    expect((await mac.processDetails(ref(proc(mac, 'rustc')))).affinity).toBeNull();
    const lin = make({ os: 'linux' });
    expect((await lin.processDetails(ref(proc(lin, 'rustc')))).affinity).toBe(2 ** 16 - 1);
  });
  it('windows wording is localised for the other OSes', async () => {
    const win = await make({ os: 'windows' }).anomalies();
    const mac = await make({ os: 'macos' }).anomalies();
    expect(JSON.stringify(win)).toMatch(/\.exe/);
    expect(JSON.stringify(mac)).not.toMatch(/\.exe/);
  });
  it('whySlow reflects the OS wording', async () => {
    const d = await make({ os: 'macos' }).whySlow();
    expect(JSON.stringify(d)).not.toMatch(/Drive C:|\.exe/);
  });
  it('history disk trends name only the disks the machine has off Windows', async () => {
    const h = await make({ os: 'macos' }).history('week', null);
    expect(h.diskTrends.map((t) => t.disk)).toEqual(['Macintosh HD']);
    const w = await make({ os: 'windows' }).history('week', null);
    expect(w.diskTrends.map((t) => t.disk)).toEqual(['C:', 'D:']);
  });
  it('the day of history holds the alerts and the network change the dashboard lists', async () => {
    const h = await make({ os: 'macos' }).history('day', null);
    expect(h.events.map((e) => e.kind)).toEqual(expect.arrayContaining(['alert', 'network']));
  });
  it('a desktop has no battery, temperatures or fans', () => {
    const s = make({ machine: 'desktop' }).liveUpdate('summary').system;
    expect('unavailable' in s.battery).toBe(true);
    expect('unavailable' in s.temperatures).toBe(true);
    expect('unavailable' in s.fans).toBe(true);
    const l = make({ machine: 'laptop', os: 'windows' }).liveUpdate('summary').system;
    expect('available' in l.battery && 'available' in l.temperatures && 'available' in l.fans).toBe(true);
    expect('unavailable' in make({ machine: 'laptop', os: 'macos' }).liveUpdate('summary').system.fans).toBe(true);
  });
});

describe('live data', () => {
  it('is deterministic for a frozen clock and includes processes only on request', () => {
    const a = make().liveUpdate('summary');
    expect(a).toEqual(make().liveUpdate('summary'));
    expect(a.processes).toBeNull();
    expect(a.groups).toBeNull();
    const full = make().liveUpdate('processes');
    expect(full.processes!.length).toBeGreaterThan(100);
    expect(full.groups!.length).toBeGreaterThan(5);
    expect(a.topCpu.length).toBeLessThanOrEqual(8);
  });
  it('reports a plausible system sample', () => {
    const { system, pressure } = make().liveUpdate('summary');
    expect(system.cpu.total).toBeGreaterThan(0);
    expect(system.cpu.perCore).toHaveLength(14);
    expect(system.cpu.coreKinds.filter((k) => k === 'efficiency')).toHaveLength(4);
    expect(system.cpu.coreKinds.slice(0, 4)).toEqual(['efficiency', 'efficiency', 'efficiency', 'efficiency']);
    expect(system.cpu.coreFrequencyMhz).toHaveLength(14);
    expect(system.memory.used).toBeLessThan(system.memory.total);
    expect(['normal', 'elevated', 'critical']).toContain(pressure);
  });
  it('tick pushes to listeners, trimming the process list for summary listeners', () => {
    vi.useFakeTimers();
    try {
      const b = make();
      const summary = vi.fn();
      const processes = vi.fn();
      const stopSummary = b.onLive('summary', summary);
      b.tick();
      expect(summary.mock.calls.at(-1)![0].processes).toBeNull();
      const stopProcesses = b.onLive('processes', processes);
      b.tick();
      expect(summary.mock.calls.at(-1)![0].processes).toBeNull();
      expect(processes.mock.calls.at(-1)![0].processes).not.toBeNull();
      stopSummary();
      stopProcesses();
    } finally {
      vi.useRealTimers();
    }
  });
  it('onLive delivers a first update asynchronously, then on the timer; unsubscribing stops both', () => {
    vi.useFakeTimers();
    try {
      const b = make();
      const l = vi.fn();
      const stop = b.onLive('summary', l);
      expect(l).not.toHaveBeenCalled();
      vi.advanceTimersByTime(0);
      expect(l).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(2000);
      expect(l).toHaveBeenCalledTimes(3);
      stop();
      vi.advanceTimersByTime(5000);
      expect(l).toHaveBeenCalledTimes(3);
      expect(vi.getTimerCount()).toBe(0);
      const gone = vi.fn();
      b.onLive('summary', gone)();
      vi.advanceTimersByTime(10);
      expect(gone).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
  it('agent status: running soon after subscribing, pushable, unsubscribable', () => {
    vi.useFakeTimers();
    try {
      const b = make();
      const l = vi.fn();
      const stop = b.onAgentStatus(l);
      vi.advanceTimersByTime(0);
      expect(l).toHaveBeenCalledWith({ state: 'running', message: null });
      b.pushAgentStatus({ state: 'stopped', message: 'x' } as never);
      expect(l).toHaveBeenLastCalledWith({ state: 'stopped', message: 'x' });
      stop();
      b.pushAgentStatus({ state: 'running', message: null });
      expect(l).toHaveBeenCalledTimes(2);
      const never = vi.fn();
      b.onAgentStatus(never)();
      vi.advanceTimersByTime(0);
      expect(never).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
  it('pushAlert reaches listeners and the log', async () => {
    const b = make();
    const l = vi.fn();
    const stop = b.onAlert(l);
    const a: Alert = { id: 'z', at: NOW, kind: 'lowDisk', severity: 'warning', title: 'Low', detail: '' };
    b.pushAlert(a);
    expect(l).toHaveBeenCalledWith(a);
    expect((await b.alerts(1))[0]!.id).toBe('z');
    stop();
    b.pushAlert({ ...a, id: 'y' });
    expect(l).toHaveBeenCalledTimes(1);
  });
});

describe('errors and the fail option', () => {
  it('rejects named requests with an internal error', async () => {
    const b = make({ fail: new Set(['health', 'whySlow']) });
    await expect(b.health()).rejects.toEqual({ code: 'internal', message: 'The mock was asked to fail “health”.' });
    await expect(b.whySlow()).rejects.toMatchObject({ code: 'internal' });
    await expect(b.settings()).resolves.toBeDefined();
  });
  it('replies after the simulated latency', async () => {
    vi.useFakeTimers();
    try {
      const b = new MockBackend({ latencyMs: 100, now: () => NOW });
      const done = vi.fn();
      void b.settings().then(done);
      await vi.advanceTimersByTimeAsync(99);
      expect(done).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1);
      expect(done).toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
  it('adds, replaces and deletes one alert rule without touching the other settings', async () => {
    const b = make();
    const rule = { id: 'hot', name: 'Hot', metric: 'cpuTemp', op: 'above', limit: 90, forSecs: 30, severity: 'warning', enabled: true } as never;
    const added = await b.upsertAlertRule(rule);
    expect(added.customAlerts?.map((r) => r.id)).toContain('hot');
    const renamed = await b.upsertAlertRule({ ...(rule as object), name: 'Hotter' } as never);
    expect(renamed.customAlerts?.filter((r) => r.id === 'hot')).toHaveLength(1);
    expect(renamed.customAlerts?.find((r) => r.id === 'hot')?.name).toBe('Hotter');
    const gone = await b.deleteAlertRule('hot');
    expect(gone.customAlerts?.some((r) => r.id === 'hot')).toBe(false);
    expect(gone.sampling).toEqual(added.sampling);
  });
  it('replies are copies: mutating them does not change the mock', async () => {
    const b = make();
    const s = await b.settings();
    s.launchAtLogin = !s.launchAtLogin;
    expect((await b.settings()).launchAtLogin).not.toBe(s.launchAtLogin);
  });
});

describe('process actions', () => {
  it('lockers match by path and validate it', async () => {
    const b = make();
    await expect(b.lockers('  ')).rejects.toMatchObject({ code: 'badRequest' });
    expect((await b.lockers('/Users/alex/dev/dyno/target')).map((l) => l.name)).toContain('rustc');
    expect((await b.lockers('/x/Chrome/Cookies'))[0]!.name).toBe('Google Chrome');
    expect((await b.lockers('/var/some.log'))[0]!.name).toBe('dyno-agent');
    expect(await b.lockers('/nothing/here')).toEqual([]);
  });
  it('openLocation fails for processes without a file', async () => {
    const b = make();
    const kernel = b.liveUpdate('processes').processes!.find((p) => p.exe === null)!;
    await expect(b.openLocation(ref(kernel))).rejects.toMatchObject({ code: 'unsupported' });
    await expect(b.openLocation(ref(proc(b, 'node')))).resolves.toBeUndefined();
  });
  it('processDetails shows parents, a signature and system-process denials', async () => {
    const b = make({ os: 'windows' });
    const details = await b.processDetails(ref(proc(b, 'rustc.exe')));
    expect(details.parents.length).toBeGreaterThan(0);
    expect(details.signature).toBe('unsigned');
    expect(details.commandLine[0]).toMatch(/rustc/);
    const chrome = await b.processDetails(ref(proc(b, 'chrome.exe')));
    expect(chrome.signature).toEqual({ signed: 'Google LLC' });
    expect(chrome.commandLine).toContain('--type=renderer');
    const sys = b.liveUpdate('processes').processes!.find((p) => p.exe === null)!;
    const sd = await b.processDetails(ref(sys));
    expect('unavailable' in sd.openFiles).toBe(true);
    expect(sd.cwd).toBeNull();
  });
});

describe('snapshots', () => {
  it('lists, takes and diffs snapshots', async () => {
    const b = make();
    expect((await b.snapshots()).map((s) => s.id)).toEqual(['snap-2', 'snap-1']);
    const taken = await b.takeSnapshot('  ');
    expect(taken.label).toBe('Snapshot');
    expect((await b.takeSnapshot('After')).label).toBe('After');
    expect((await b.snapshots())[0]!.label).toBe('After');
    const diff = await b.diffSnapshots('snap-1', 'snap-2');
    expect(diff.before.id).toBe('snap-1');
    expect(diff.added.length).toBeGreaterThan(0);
    expect(diff.removed.length).toBeGreaterThanOrEqual(0);
    // Order of the arguments does not matter.
    expect((await b.diffSnapshots('snap-2', 'snap-1')).before.id).toBe('snap-1');
    await expect(b.diffSnapshots('snap-1', 'zzz')).rejects.toEqual({ code: 'notFound', message: 'That snapshot was deleted.' });
  });
});

describe('diagnosis, history, activity', () => {
  it('whySlow blames the build while it runs, and points at the task manager', async () => {
    const d = await make().whySlow();
    expect(d.causes[0]!.kind).toBe('cpuHog');
    expect(d.causes[0]!.hints.some((h) => h.link === 'taskManager')).toBe(true);
  });
  it.each([
    ['hour', 30_000, 3_600_000],
    ['day', 300_000, 86_400_000],
    ['week', 1_800_000, 7 * 86_400_000],
    ['month', 7_200_000, 30 * 86_400_000],
  ] as const)('history(%s) spans its range at the right resolution', async (range, step, span) => {
    const h = await make().history(range, null);
    expect(h.range).toBe(range);
    expect(h.resolutionMs).toBe(step);
    expect(h.to - h.from).toBe(span);
    expect(h.points.length).toBe(span / step);
    expect(h.points.at(-1)!.at).toBe(h.to);
    expect(h.to % step).toBe(0);
    expect(h.diskTrends.length).toBeGreaterThan(0);
  });
  it('history honours an end time', async () => {
    const end = NOW - 3 * 86_400_000;
    const h = await make().history('day', end);
    expect(h.to).toBeLessThanOrEqual(end);
    expect(end - h.to).toBeLessThan(h.resolutionMs);
  });
  it('activity honours the limit', async () => {
    const b = make();
    expect(await b.activity(2)).toHaveLength(2);
    expect(await b.activity(-3)).toEqual([]);
    expect((await b.alerts(1)).length).toBe(1);
    expect(await b.alerts(-1)).toEqual([]);
  });
});

describe('health, tuning, startup', () => {
  it('health grades the score', async () => {
    const h = await make().health();
    expect(h.score).toBe(100 - h.reasons.reduce((a, r) => a + r.impact, 0));
    const grade = h.score >= 80 ? 'good' : h.score >= 60 ? 'fair' : 'poor';
    expect(h.grade).toBe(grade);
  });
});

describe('reports, settings, plugins, tray', () => {
  it('weeklyReport covers the last week', async () => {
    const r = await make().weeklyReport(0);
    expect(r.to - r.from).toBe(7 * 86_400_000);
    expect(r.topApps).toHaveLength(5);
    expect(r.highlights.length).toBeGreaterThan(0);
    expect(r.nothingToReport).toBe(false);
    expect(r.issues.length).toBeGreaterThan(0);
  });
  it('weeklyReport: an older week ends a week earlier, a week with no history is empty, too far back is refused', async () => {
    const b = make();
    const now = await b.weeklyReport(0);
    const before = await b.weeklyReport(1);
    expect(before.to).toBe(now.from);
    const empty = await b.weeklyReport(6);
    expect(empty).toMatchObject({ nothingToReport: true, score: null, issues: [] });
    await expect(b.weeklyReport(53)).rejects.toMatchObject({ code: 'badRequest' });
  });
  it('saveReport records the format and the week and answers the path', async () => {
    const b = make();
    expect(await b.saveReport('html', 2)).toEqual({ path: '/Users/me/dyno-weekly-report.html', bytes: 2_048 });
    expect((await b.saveReport('markdown', 0))!.path).toMatch(/\.md$/);
    expect(b.savedReports).toEqual([
      { format: 'html', weeksAgo: 2 },
      { format: 'markdown', weeksAgo: 0 },
    ]);
  });
  it('issues: a scored window, leaderboards and sources; the window is clamped', async () => {
    const b = make();
    const o = await b.issues({ days: 14 });
    expect(o.daily).toHaveLength(14);
    expect(o.score).toBe(o.daily[13]!.score);
    expect(o.apps[0]!.name).toBe('Mail');
    expect(o.total).toBe(o.issues.length);
    expect(o.issues[0]!.at).toBeGreaterThan(o.issues[1]!.at);
    expect((await b.issues({ days: 7 })).total).toBeLessThan(o.total);
    expect((await b.issues({ days: 999 })).days).toBe(90);
    expect(await runJob(b, { rescanIssues: { query: { days: 14 } } })).toMatchObject({ issues: { days: 14 } });
    expect(await b.saveIssues(14)).toMatchObject({ count: 13 });
    expect(b.savedIssues).toEqual([14]);
  });
  it('whySlow names the apps behind a cause and how long it looked', async () => {
    const d = await make().whySlow();
    expect(d.windowSecs).toBeGreaterThan(0);
    expect(d.causes[0]!.involved[0]!.share).toMatch(/CPU/);
  });
  it('saveSettings validates and persists', async () => {
    const b = make();
    const s = structuredClone(await b.settings());
    s.sampling.intervalMs = 249;
    await expect(b.saveSettings(s)).rejects.toMatchObject({ code: 'badRequest', message: expect.stringContaining('250 ms') });
    s.sampling.intervalMs = 500;
    s.webApi.port = 80;
    await expect(b.saveSettings(s)).rejects.toMatchObject({ message: expect.stringContaining('1024 and 65535') });
    s.webApi.port = 70000;
    await expect(b.saveSettings(s)).rejects.toMatchObject({ code: 'badRequest' });
    s.webApi.port = 8080;
    await expect(b.saveSettings(s)).resolves.toMatchObject({ webApi: { port: 8080 } });
    expect((await b.settings()).sampling.intervalMs).toBe(500);
    expect(b.liveUpdate('summary').overhead.samplingIntervalMs).toBe(500);
  });
  it('lists plugins and handles the tray commands', async () => {
    const b = make();
    expect((await b.plugins()).length).toBeGreaterThan(0);
    await expect(b.showMainWindow()).resolves.toBeUndefined();
    await expect(b.hideTray()).resolves.toBeUndefined();
    await expect(b.fitTray(300)).resolves.toBeUndefined();
    await expect(b.quitApp()).resolves.toBeUndefined();
  });
});

describe('mock CPU layout', () => {
  it('is uniform with ?mock=uniform and hybrid with ?mock=hybrid on other systems', async () => {
    const uniform = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, cpu: 'uniform' });
    expect(uniform.liveUpdate('summary').system.cpu.coreKinds).toEqual([]);
    const win = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, os: 'windows', cpu: 'hybrid' });
    const cpu = win.liveUpdate('summary').system.cpu;
    expect(cpu.coreKinds).toHaveLength(16);
    expect(cpu.coreKinds[0]).toBe('performance');
    expect(mockOptionsFromUrl('?mock=linux,hybrid').cpu).toBe('hybrid');
    expect(mockOptionsFromUrl('?mock=uniform').cpu).toBe('uniform');
  });

  it('history carries per-core buckets in the CPU order', async () => {
    const b = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
    const h = await b.history('day', null);
    expect(h.corePoints).toHaveLength(h.points.length);
    expect(h.corePoints[0]!.cores).toHaveLength(14);
    const c = h.corePoints[5]!.cores[0]!;
    expect(c.min).toBeLessThanOrEqual(c.avg);
    expect(c.avg).toBeLessThanOrEqual(c.max);
  });

});

describe('mock extras and backup health', () => {
  const NOW = new Date(2026, 9, 22, 14, 30).getTime();
  const fresh = (o: ConstructorParameters<typeof MockBackend>[0] = {}) => new MockBackend({ latencyMs: 0, now: () => NOW, ...o });

  it('changes: newest first over three days, narrowed by the window', async () => {
    const b = fresh();
    const t = await b.changes({ hours: 0 });
    expect(t.changes.length).toBeGreaterThan(8);
    expect(t.changes.every((c, i) => i === 0 || c.at <= t.changes[i - 1]!.at)).toBe(true);
    expect(t.changes.every((c) => c.at >= NOW - 72 * 3_600_000)).toBe(true);
    expect(new Set(t.changes.map((c) => c.kind)).size).toBeGreaterThanOrEqual(5);
    const day = await b.changes({ hours: 24 });
    expect(day.changes.length).toBeLessThan(t.changes.length);
    const week = await b.changes({ hours: 168 });
    expect(week.changes.length).toBeGreaterThan(t.changes.length);
    expect(week.watchingSince).toBeLessThan(NOW);
    const around = await b.changes({ hours: 24, around: NOW - 3 * 24 * 3_600_000 });
    expect(around.to).toBeLessThan(NOW);
  });

  it('sleep, boot, usage, GPU and software answer realistic data', async () => {
    const b = fresh();
    const { sleep } = (await runJob(b, 'sleepBlockers')) as { sleep: SleepReport };
    expect(sleep.blockers.map((x) => x.name)).toContain('coreaudiod');
    expect(sleep.wakes.length).toBeGreaterThan(2);
    const { boot } = (await runJob(b, 'bootAnalysis')) as { boot: BootReport };
    expect(boot.boots).toHaveLength(8);
    expect(boot.slowest[0]!.secs).toBeGreaterThan(boot.slowest[1]!.secs);
    expect(boot.verdict).toBeTruthy();
    const { appUsage: usage } = (await runJob(b, 'appUsage')) as { appUsage: UsageReport };
    expect(usage.apps.length).toBeGreaterThan(5);
    expect((await b.gpuProcesses()).processes[0]!.gpuPercent).toBeGreaterThan(0);
    const { software: sw } = (await runJob(b, 'softwareInventory')) as { software: SoftwareInventory };
    expect(sw.items.length).toBeGreaterThanOrEqual(40);
  });

  it('saveInventoryReport remembers the format; pickFolder answers a sample folder', async () => {
    const b = fresh();
    expect(await b.saveInventoryReport('markdown')).toMatchObject({ path: expect.stringMatching(/\.md$/) });
    expect(await b.saveInventoryReport('json')).toMatchObject({ path: expect.stringMatching(/\.json$/) });
    expect(b.savedInventories).toEqual(['markdown', 'json']);
    expect(await b.pickFolder()).toBe('/Users/alex/Projects');
    expect(b.pickedFolders).toBe(1);
  });

  it('findFiles job: modes, the size floor and a folder that must be absolute', async () => {
    const b = fresh();
    const find = async (query: FinderQuery) => ((await runJob(b, { findFiles: { query } })) as { finder: FinderResult }).finder;
    const both = await find({ folder: '/Users/alex/Projects/', mode: 'both', minBytes: 50 * 1024 ** 2 });
    expect(both.folder).toBe('/Users/alex/Projects');
    expect(both.large).toHaveLength(8);
    expect(both.duplicates).toHaveLength(3);
    expect(both.limit).toBeNull();
    expect(both.wastedBytes).toBe(both.duplicates.reduce((a, g) => a + g.wastedBytes, 0));
    expect(both.duplicates[0]!.files[0]!.modified).toBeLessThan(both.duplicates[0]!.files[1]!.modified);
    expect((await find({ folder: '/x', mode: 'large', minBytes: 0 })).duplicates).toEqual([]);
    expect((await find({ folder: '/x', mode: 'duplicates', minBytes: 0 })).large).toEqual([]);
    const big = await find({ folder: '/x', mode: 'both', minBytes: 1024 ** 3 });
    expect(big.large.every((f) => f.bytes >= 1024 ** 3)).toBe(true);
    // A blank folder is refused at the start; a relative one fails the job with what to fix.
    await expect(b.startJob({ findFiles: { query: { folder: ' ', mode: 'both', minBytes: 0 } } })).rejects.toMatchObject({ code: 'badRequest' });
    await expect(runJob(b, { findFiles: { query: { folder: 'relative/path', mode: 'both', minBytes: 0 } } })).rejects.toMatchObject({ message: 'Give the full path of the folder.' });
  });

  describe('jobs (ER-17 to ER-19)', () => {
    const rescan: JobSpec = { rescanIssues: { query: { days: 14 } } };
    const statusOf = (b: MockBackend, id: number) => b.jobStatuses().find((j) => j.id === id)!;

    it('a start answers at once with the id of a running job; the same work answers the same job; another search is busy', async () => {
      const b = fresh();
      b.jobSteps = 3;
      b.jobStepMs = 50;
      const first = await b.startJob(rescan);
      expect(statusOf(b, first)).toMatchObject({ state: 'running', phase: null, kind: 'rescan-issues' });
      expect(await b.startJob(rescan)).toBe(first);
      await expect(b.startJob({ rescanIssues: { query: { days: 7 } } })).rejects.toMatchObject({ code: 'busy' });
      // Different kinds run side by side.
      expect(await b.startJob('refreshBackup')).not.toBe(first);
    });

    it('pushes every status to the sink in order: queued, running, a phase whose count only grows, done', async () => {
      const heard: SuJobSnapshot[] = [];
      const b = fresh({ jobs: { emit: (j: SuJobSnapshot) => heard.push(j) } });
      b.jobSteps = 4;
      const id = await b.startJob({ fileHash: { source: { path: '/Applications/Tool.app/tool' } } });
      // The steps are chained timers: wait until the job has ended, not for a fixed time (rule 49 (a)).
      await vi.waitFor(() => expect(heard.filter((j) => j.id === id).at(-1)?.state).toBe('done'));
      const mine = heard.filter((j) => j.id === id);
      expect(mine.map((j) => j.state)).toEqual(['queued', 'running', 'running', 'running', 'running', 'done']);
      expect(mine.every((j, i) => i === 0 || j.seq > mine[i - 1]!.seq)).toBe(true);
      const counts = mine.flatMap((j) => (j.state === 'running' && j.phase ? [j.phase.done] : []));
      expect(counts).toHaveLength(3);
      expect(counts.every((c, i) => i === 0 || c >= counts[i - 1]!)).toBe(true);
      expect(mine[2]).toMatchObject({ phase: { id: 'hash', unit: 'bytes', total: 48_231_424 } });
      expect(await b.jobResult(id)).toMatchObject({ fileHash: { bytes: 48_231_424 } });
    });

    it('Cancel ends a job as cancelled, it has no result, and a new search can start', async () => {
      const b = fresh();
      b.jobSteps = 5;
      b.jobStepMs = 50;
      const id = await b.startJob({ findFiles: { query: { folder: '/x', mode: 'both', minBytes: 0 } } });
      expect(b.cancelJob(id)).toBe(true);
      expect(statusOf(b, id).state).toBe('cancelled');
      await expect(b.jobResult(id)).rejects.toMatchObject({ code: 'notFound' });
      expect(b.cancelJob(id)).toBe(false);
      expect(b.cancelJob(999)).toBe(false);
      expect(await b.startJob({ findFiles: { query: { folder: '/y', mode: 'both', minBytes: 0 } } })).not.toBe(id);
    });

    it('a stalled mock reports the first status and nothing more', async () => {
      const b = fresh();
      b.jobsStalled = true;
      const id = await b.startJob('sleepBlockers');
      await new Promise((r) => setTimeout(r, 20));
      expect(statusOf(b, id)).toMatchObject({ state: 'running', phase: null, seq: 2 });
    });

    it('the four tool reads and the backup check hand over their reports; a check of the backups sets the time only when done', async () => {
      const b = fresh();
      expect(await runJob(b, 'sleepBlockers')).toMatchObject({ sleep: { blockers: expect.any(Array) } });
      expect(await runJob(b, 'bootAnalysis')).toMatchObject({ boot: { boots: expect.any(Array) } });
      expect(await runJob(b, 'appUsage')).toMatchObject({ appUsage: { apps: expect.any(Array) } });
      expect(await runJob(b, 'softwareInventory')).toMatchObject({ software: { items: expect.any(Array) } });
      b.jobSteps = 2;
      b.jobStepMs = 5;
      const before = (await b.backup()).checkedAt;
      const id = await b.startJob('refreshBackup');
      expect((await b.backup()).checkedAt).toBe(before);
      await new Promise((r) => setTimeout(r, 40));
      expect(statusOf(b, id).state).toBe('done');
      expect((await b.backup()).checkedAt).toBe(NOW);
    });

    it('only the last few finished jobs are kept', async () => {
      const b = fresh();
      for (let i = 0; i < 20; i++) await runJob(b, { fileHash: { source: { path: `/a/${i}` } } });
      expect(b.jobStatuses()).toHaveLength(16);
    });
  });

  it('maintenance: open inside a window, closed outside, always open when off', async () => {
    const b = fresh();
    // 14:30 on a Thursday: the weeknight window (22:00 to 02:00) is closed and opens tonight.
    const closed = await b.maintenance();
    expect(closed).toMatchObject({ enabled: true, openNow: false, closesAt: null });
    expect(new Date(closed.nextOpensAt!).getHours()).toBe(22);
    const late = fresh({ now: () => new Date(2026, 9, 22, 23, 15).getTime() });
    const open = await late.maintenance();
    expect(open).toMatchObject({ openNow: true, nextOpensAt: null });
    expect(new Date(open.closesAt!).getHours()).toBe(2);
    const s = structuredClone(await b.settings());
    s.maintenance.enabled = false;
    await b.saveSettings(s);
    expect(await b.maintenance()).toEqual({ enabled: false, openNow: true, closesAt: null, nextOpensAt: null });
  });

  it('saveSettings refuses a bad maintenance window or backup setting with the engine sentences', async () => {
    const b = fresh();
    const s = structuredClone(await b.settings());
    s.maintenance.windows[0]!.days = [];
    await expect(b.saveSettings(s)).rejects.toMatchObject({ message: 'Pick at least one day for each window.' });
    s.maintenance.windows[0]!.days = ['mon'];
    s.maintenance.windows[0]!.endMinute = s.maintenance.windows[0]!.startMinute;
    await expect(b.saveSettings(s)).rejects.toMatchObject({ message: expect.stringContaining('different time') });
    s.maintenance.windows = [];
    await expect(b.saveSettings(s)).rejects.toMatchObject({ message: expect.stringContaining('Add a window') });
    s.maintenance.enabled = false;
    s.backup.staleAfterHours = 0;
    await expect(b.saveSettings(s)).rejects.toMatchObject({ message: expect.stringContaining('2160') });
    s.backup = { alertWhenStale: true, staleAfterHours: 24, overrides: [{ tool: 'restic', dirs: ['relative'] }] };
    await expect(b.saveSettings(s)).rejects.toMatchObject({ message: expect.stringContaining('absolute') });
    s.backup.overrides[0]!.dirs = ['/opt/restic'];
    await expect(b.saveSettings(s)).resolves.toMatchObject({ backup: { staleAfterHours: 24 } });
  });

  it('backup: a mix of verdicts, nothing found as a variant, and a check that stamps the time', async () => {
    const b = fresh();
    const r = await b.backup();
    expect(r.statuses.map((s) => [s.tool, s.state])).toEqual([
      ['timeMachine', 'healthy'],
      ['restic', 'stale'],
      ['borg', 'cannotCheck'],
    ]);
    expect(r.statuses[0]!.nextStep).toBeNull();
    expect(r.statuses[1]!.nextStep).toContain('never starts a backup');
    expect(r.checkedAt).toBe(NOW - 40 * 60_000);
    const checked = (await runJob(b, 'refreshBackup')) as { backup: BackupReport };
    expect(checked.backup.checkedAt).toBe(NOW);
    expect((await b.backup()).checkedAt).toBe(NOW);
    const none = await fresh({ backup: 'none' }).backup();
    expect(none.statuses).toEqual([]);
  });

  it('the URL option backup=none selects the empty variant', () => {
    expect(mockOptionsFromUrl('?backup=none').backup).toBe('none');
    expect(mockOptionsFromUrl('?latency=0').backup).toBeUndefined();
  });

});

describe('hardware: throttling, lifespan and baselines', () => {
  it('answers the throttling overview: fine now, a few episodes in the last 30 days', async () => {
    const t = await make().throttling();
    expect(t.reading.verdict).toBe('fine');
    expect(t.current).toBeNull();
    expect(t.episodes.length).toBeGreaterThan(0);
    expect(t.episodes.every((e) => e.start > NOW - 30 * 86_400_000)).toBe(true);
    expect(t.minutes30d).toBeGreaterThan(0);
    expect(t.inputs.length).toBeGreaterThan(0);
  });
  it('a laptop has a disk and a battery forecast, a desktop only the disk', async () => {
    const laptop = await make().lifespan();
    expect(laptop.forecasts.map((f) => f.subject)).toEqual(['ssd', 'battery']);
    expect(laptop.forecasts.every((f) => 'remaining' in (f.outcome as object))).toBe(true);
    expect((await make({ machine: 'desktop' }).lifespan()).forecasts.map((f) => f.subject)).toEqual(['ssd']);
  });
  it('answers the baselines with unusual items first and says what is usual', async () => {
    const b = await make().baselines();
    expect(b.learning).toEqual({ ready: { days: 34 } });
    expect(b.items[0]!.judgement.state).toEqual({ higher: 'strong' });
    expect(b.items[0]!.current).toBe('71%');
    expect(b.items[0]!.judgement.usual).toBe('14%');
    expect(b.items.some((i) => i.judgement.state === 'usual')).toBe(true);
  });
  it('fails when asked to', async () => {
    const b = make({ fail: new Set(['throttling', 'lifespan', 'baselines']) });
    await expect(b.throttling()).rejects.toBeDefined();
    await expect(b.lifespan()).rejects.toBeDefined();
    await expect(b.baselines()).rejects.toBeDefined();
  });
  it('the weekly report carries the hardware sentences, and an empty week has none', async () => {
    const b = make();
    expect((await b.weeklyReport(0)).hardware.length).toBe(2);
    expect((await b.weeklyReport(5)).hardware).toEqual([]);
  });
});

describe('MockBackend: the helper', () => {
  it('reads the state it starts in from ?helper= and ignores anything else', async () => {
    expect(mockOptionsFromUrl('?helper=foreign').helper).toBe('foreign');
    expect(mockOptionsFromUrl('?helper=root')).not.toHaveProperty('helper');
    expect((await make().helperStatus()).state).toBe('off');
    for (const [variant, state] of [['silent', 'notRunning'], ['foreign', 'foreign'], ['outdated', 'outdated'], ['ready', 'ready'], ['notIncluded', 'notIncluded']] as const) {
      expect((await make({ helper: variant }).helperStatus()).state, variant).toBe(state);
    }
  });

  it('turning on ends ready, turning off ends off, and the helper job runs its phases with an uncancellable prompt', async () => {
    const b = make({ jobSteps: 4 });
    b.jobStepMs = 1;
    const id = await b.startJob({ helper: { action: 'turnOn' } });
    const seen = new Set<string>();
    for (let i = 0; i < 400 && b.jobStatuses().find((j) => j.id === id)!.state !== 'done'; i++) {
      const s = b.jobStatuses().find((j) => j.id === id)! as SuJobSnapshot & { phase?: { id: string } | null; cancellable: boolean };
      if (s.phase) seen.add(s.phase.id);
      if (s.phase?.id === 'prompt') expect(s.cancellable).toBe(false);
      await new Promise((r) => setTimeout(r, 1));
    }
    expect([...seen]).toContain('prompt');
    expect((await b.jobResult(id)) as JobOutput).toMatchObject({ helper: { state: 'ready' } });
    expect((await b.helperStatus()).state).toBe('ready');
    expect(await runJob(make({ helper: 'ready' }), { helper: { action: 'turnOff' } })).toMatchObject({ helper: { state: 'off' } });
  });

  it('a declined prompt fails the job with the engine\'s words and changes nothing', async () => {
    const b = make({ fail: new Set(['helperJob']) });
    await expect(runJob(b, { helper: { action: 'turnOn' } })).rejects.toMatchObject({ message: expect.stringContaining('declined') });
    expect((await b.helperStatus()).state).toBe('off');
  });
});
