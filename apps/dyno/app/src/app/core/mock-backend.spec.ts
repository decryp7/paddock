import type { Alert, ProcessSample } from './api.types';
import { MockBackend, type MockOptions, applyMockPlatform, mockOptionsFromUrl } from './mock-backend';

const NOW = 1_700_000_000_000;
const make = (o: MockOptions = {}) => new MockBackend({ latencyMs: 0, now: () => NOW, ...o });
const proc = (b: MockBackend, name: string): ProcessSample => b.liveUpdate('processes').processes!.find((p) => p.name === name)!;
const ref = (p: ProcessSample) => ({ pid: p.pid, started: p.started });

describe('mockOptionsFromUrl', () => {
  it('defaults to a macOS laptop with latency', () => {
    expect(mockOptionsFromUrl('')).toEqual({ machine: 'laptop', os: 'macos', latencyMs: 150, fail: new Set() });
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
  it('appInfo reports the OS', async () => {
    for (const os of ['windows', 'macos', 'linux'] as const) expect((await make({ os }).appInfo()).os).toBe(os);
    expect((await make().appInfo()).updatesAvailable).toBe(false);
  });
  it('disks follow the OS', async () => {
    const mounts = async (os: 'windows' | 'macos' | 'linux') => make({ os }).liveUpdate('summary').system.disks.map((d) => d.mount);
    expect(await mounts('windows')).toEqual(['C:\\', 'D:\\']);
    expect(await mounts('macos')).toEqual(['/', '/Volumes/T7']);
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
  it('history disk trends use the OS mount points off Windows', async () => {
    const h = await make({ os: 'macos' }).history('week', null);
    expect(h.diskTrends[0]!.disk).toBe('/');
    const w = await make({ os: 'windows' }).history('week', null);
    expect(w.diskTrends[0]!.disk).not.toBe('/');
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
  it('replies are copies: mutating them does not change the mock', async () => {
    const b = make();
    const s = await b.settings();
    s.theme = s.theme === 'dark' ? 'light' : 'dark';
    expect((await b.settings()).theme).not.toBe(s.theme);
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
    expect(await b.rescanIssues({ days: 14 })).toMatchObject({ days: 14 });
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
    await expect(b.trayOpenMain()).resolves.toBeUndefined();
    await expect(b.trayQuit()).resolves.toBeUndefined();
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
    const sleep = await b.sleepBlockers();
    expect(sleep.blockers.map((x) => x.name)).toContain('coreaudiod');
    expect(sleep.wakes.length).toBeGreaterThan(2);
    const boot = await b.bootAnalysis();
    expect(boot.boots).toHaveLength(8);
    expect(boot.slowest[0]!.secs).toBeGreaterThan(boot.slowest[1]!.secs);
    expect(boot.verdict).toBeTruthy();
    const usage = await b.appUsage();
    expect(usage.apps.length).toBeGreaterThan(5);
    expect((await b.gpuProcesses()).processes[0]!.gpuPercent).toBeGreaterThan(0);
    const sw = await b.softwareInventory();
    expect(sw.items.length).toBeGreaterThanOrEqual(40);
  });

  it('saveInventoryReport remembers the format; pickFolder answers a sample folder', async () => {
    const b = fresh();
    expect(await b.saveInventoryReport('markdown')).toMatchObject({ path: expect.stringMatching(/\.md$/) });
    expect(await b.saveInventoryReport('json')).toMatchObject({ path: expect.stringMatching(/\.json$/) });
    expect(b.savedInventories).toEqual(['markdown', 'json']);
    expect(await b.pickFolder()).toBe('/Users/sam/Projects');
    expect(b.pickedFolders).toBe(1);
  });

  it('findFiles: modes, the size floor and a folder that must be absolute', async () => {
    const b = fresh();
    const both = await b.findFiles({ folder: '/Users/sam/Projects/', mode: 'both', minBytes: 50 * 1024 ** 2 });
    expect(both.folder).toBe('/Users/sam/Projects');
    expect(both.large).toHaveLength(8);
    expect(both.duplicates).toHaveLength(3);
    expect(both.wastedBytes).toBe(both.duplicates.reduce((a, g) => a + g.wastedBytes, 0));
    expect(both.duplicates[0]!.files[0]!.modified).toBeLessThan(both.duplicates[0]!.files[1]!.modified);
    expect((await b.findFiles({ folder: '/x', mode: 'large', minBytes: 0 })).duplicates).toEqual([]);
    expect((await b.findFiles({ folder: '/x', mode: 'duplicates', minBytes: 0 })).large).toEqual([]);
    const big = await b.findFiles({ folder: '/x', mode: 'both', minBytes: 1024 ** 3 });
    expect(big.large.every((f) => f.bytes >= 1024 ** 3)).toBe(true);
    await expect(b.findFiles({ folder: 'relative/path', mode: 'both', minBytes: 0 })).rejects.toMatchObject({ code: 'badRequest' });
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
    const checked = await b.refreshBackup();
    expect(checked.checkedAt).toBe(NOW);
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
