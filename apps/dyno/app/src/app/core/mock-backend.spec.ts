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
    const { system, pressure, activeActions, profile, rulesPaused } = make().liveUpdate('summary');
    expect(system.cpu.total).toBeGreaterThan(0);
    expect(system.cpu.perCore).toHaveLength(14);
    expect(system.cpu.coreKinds.filter((k) => k === 'efficiency')).toHaveLength(4);
    expect(system.cpu.coreKinds.slice(0, 4)).toEqual(['efficiency', 'efficiency', 'efficiency', 'efficiency']);
    expect(system.cpu.coreFrequencyMhz).toHaveLength(14);
    expect(system.memory.used).toBeLessThan(system.memory.total);
    expect(['normal', 'elevated', 'critical']).toContain(pressure);
    expect(activeActions).toHaveLength(1);
    expect(profile).toBe('normal');
    expect(rulesPaused).toBe(false);
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
    await expect(b.rules()).resolves.toBeDefined();
  });
  it('replies after the simulated latency', async () => {
    vi.useFakeTimers();
    try {
      const b = new MockBackend({ latencyMs: 100, now: () => NOW });
      const done = vi.fn();
      void b.rules().then(done);
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
  it('act sets priority, is undoable, and shows up in processDetails', async () => {
    const b = make();
    const p = proc(b, 'node');
    const entry = await b.act(ref(p), { setPriority: 'idle' });
    expect(entry).toMatchObject({ kind: 'manual', undo: 'available' });
    expect((await b.processDetails(ref(p))).priority).toBe('idle');
    expect((await b.activity(1))[0]!.id).toBe(entry.id);
  });
  it('act: suspend/resume, affinity, efficiency, CPU cap', async () => {
    const b = make();
    const p = proc(b, 'node');
    await b.act(ref(p), 'suspend');
    expect(proc(b, 'node')).toMatchObject({ state: 'suspended', cpu: 0 });
    expect((await b.act(ref(p), 'resume')).title).toMatch(/Resumed/);
    expect(proc(b, 'node').state).not.toBe('suspended');
    await b.act(ref(p), { setAffinity: 3 });
    expect((await b.processDetails(ref(p))).affinity).toBeNull(); // macOS
    const lin = make({ os: 'linux' });
    const lp = proc(lin, 'node');
    await lin.act(ref(lp), { setAffinity: 3 });
    expect((await lin.processDetails(ref(lp))).affinity).toBe(3);
    await b.act(ref(p), { setEfficiencyMode: true });
    expect((await b.processDetails(ref(p))).efficiencyMode).toBe(true);
    await b.act(ref(p), { setEfficiencyMode: false });
    expect((await b.processDetails(ref(p))).efficiencyMode).toBe(false);
    expect((await b.act(ref(p), { setCpuCap: 0.5 })).title).toMatch(/Capped/);
    expect(proc(b, 'node').cpu).toBeLessThanOrEqual(0.5);
    expect((await b.act(ref(p), { setCpuCap: null })).title).toMatch(/Removed the CPU cap/);
  });
  it('end removes the process and is not undoable', async () => {
    const b = make();
    const p = proc(b, 'node');
    const before = b.liveUpdate('processes').processes!.length;
    expect((await b.act(ref(p), 'end')).undo).toBe('notUndoable');
    expect(b.liveUpdate('processes').processes!.length).toBe(before - 1);
    await expect(b.act(ref(p), 'end')).rejects.toEqual({ code: 'notFound', message: 'The process has exited.' });
    await expect(b.processDetails(ref(p))).rejects.toMatchObject({ code: 'notFound' });
  });
  it('protected processes refuse', async () => {
    const b = make();
    const p = b.liveUpdate('processes').processes!.find((x) => x.protectedReason)!;
    await expect(b.act(ref(p), 'end')).rejects.toMatchObject({ code: 'protected' });
  });
  it('a stale start time is treated as a different process', async () => {
    const b = make();
    const p = proc(b, 'node');
    await expect(b.act({ pid: p.pid, started: p.started + 1 }, 'end')).rejects.toMatchObject({ code: 'notFound' });
  });
  it('actOnGroup affects every member', async () => {
    const b = make();
    const members = b.liveUpdate('processes').groups!.find((g) => g.id === 'node')!.pids.length;
    const entry = await b.actOnGroup('node', 'end');
    expect(entry.title).toBe(`Ended node (${members})`);
    expect(b.liveUpdate('processes').processes!.some((p) => p.name === 'node')).toBe(false);
  });
  it('actOnGroup: unknown and protected groups', async () => {
    const b = make();
    await expect(b.actOnGroup('nope', 'end')).rejects.toEqual({ code: 'notFound', message: 'That app is no longer running.' });
    await expect(b.actOnGroup('dyno', 'end')).rejects.toMatchObject({ code: 'protected' });
  });
  it('group CPU caps are shared across the members', async () => {
    const b = make();
    await b.actOnGroup('chrome', { setCpuCap: 4 });
    const g = b.liveUpdate('processes').groups!.find((x) => x.id === 'chrome')!;
    expect(g.cpu).toBeLessThanOrEqual(4.5);
  });
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
    const sys = b.liveUpdate('processes').processes!.find((p) => p.protectedReason && p.protectedReason !== 'Protected: Dyno itself')!;
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
  it('whySlow blames the build while it runs, and offers a fix', async () => {
    const d = await make().whySlow();
    expect(d.causes[0]!.kind).toBe('cpuHog');
    expect(d.causes[0]!.suggestions.some((s) => typeof s.action === 'object' && 'process' in s.action)).toBe(true);
  });
  it('whySlow is calm when the build is gone, and without cargo has no process fix', async () => {
    const b = make();
    await b.actOnGroup('cargo', 'end');
    const d = await b.whySlow();
    expect(d.causes[0]!.kind).toBe('nothingWrong');
  });
  it('whySlow without cargo but with rustc still diagnoses', async () => {
    const b = make();
    const cargo = proc(b, 'cargo');
    await b.act(ref(cargo), 'end');
    const d = await b.whySlow();
    expect(d.causes[0]!.kind).toBe('cpuHog');
    expect(d.causes[0]!.suggestions).toHaveLength(1);
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
  it('undo marks the entry undone, logs a new one and emits both', async () => {
    const b = make();
    const seen = vi.fn();
    b.onActivity(seen);
    const entry = await b.act(ref(proc(b, 'node')), { setPriority: 'idle' });
    const undone = await b.undo(entry.id);
    expect(undone.title).toBe(`Undid: ${entry.title}`);
    expect(undone.undo).toBe('notUndoable');
    expect((await b.activity(10)).find((e) => e.id === entry.id)!.undo).toBe('undone');
    await Promise.resolve();
    expect(seen.mock.calls.map((c) => c[0].id)).toContain(entry.id);
  });
  it('undo of a rule action releases the active action', async () => {
    const b = make();
    const rule = (await b.activity(100)).find((e) => e.kind === 'rule' && e.undo === 'available');
    if (rule) {
      const undone = await b.undo(rule.id);
      expect(undone.kind).toBe('release');
    }
  });
  it('undo errors', async () => {
    const b = make();
    await expect(b.undo('nope')).rejects.toEqual({ code: 'notFound', message: 'That activity is no longer in the log.' });
    const e = await b.act(ref(proc(b, 'node')), 'end');
    await expect(b.undo(e.id)).rejects.toEqual({ code: 'badRequest', message: 'It cannot be undone.' });
    const expired = (await b.activity(100)).find((x) => x.undo === 'expired');
    if (expired) await expect(b.undo(expired.id)).rejects.toMatchObject({ message: expect.stringContaining('no longer') });
  });
  it('releaseAction drops the rule action once', async () => {
    const b = make();
    const target = b.liveUpdate('summary').activeActions[0]!.target;
    const entry = await b.releaseAction(target);
    expect(entry.kind).toBe('release');
    expect(b.liveUpdate('summary').activeActions).toEqual([]);
    await expect(b.releaseAction(target)).rejects.toEqual({ code: 'notFound', message: 'That action was already released.' });
  });
});

describe('rules and profiles', () => {
  it('has the demo rules, profiles and Windows-vs-other protected names', async () => {
    const rs = await make().rules();
    expect(rs.profile).toBe('normal');
    expect(rs.rules.length).toBeGreaterThan(0);
    expect(rs.protected).toContain('Spotify');
    expect((await make({ os: 'windows' }).rules()).protected).toContain('Spotify.exe');
  });
  it('setProfile switches, logs, and rejects unknown ids', async () => {
    const b = make();
    expect((await b.setProfile('gaming')).profile).toBe('gaming');
    expect(b.liveUpdate('summary').profile).toBe('gaming');
    expect((await b.activity(1))[0]!.title).toMatch(/Switched to the .* profile/);
    await expect(b.setProfile('nonsense' as never)).rejects.toEqual({ code: 'badRequest', message: 'Unknown profile.' });
  });
  it('the meeting profile makes the build step aside', async () => {
    const b = make();
    const cpu = () => b.liveUpdate('summary').topCpu.find((g) => g.id === 'cargo')!.cpu;
    const normal = cpu();
    await b.setProfile('meeting');
    expect(cpu()).toBeLessThan(normal);
  });
  it('pauseRules releases active actions and boosts the build; resume restores', async () => {
    const b = make();
    const cpu = () => b.liveUpdate('summary').topCpu.find((g) => g.id === 'cargo')!.cpu;
    const normal = cpu();
    expect((await b.pauseRules(true)).paused).toBe(true);
    expect(b.liveUpdate('summary').activeActions).toEqual([]);
    expect(b.liveUpdate('summary').rulesPaused).toBe(true);
    expect(cpu()).toBeGreaterThan(normal);
    expect((await b.pauseRules(false)).paused).toBe(false);
    expect((await b.activity(1))[0]!.title).toBe('Resumed rules');
  });
  it('saveRules validates every rule', async () => {
    const b = make();
    const rs = await b.rules();
    const rule = rs.rules[0]!;
    await expect(b.saveRules({ ...rs, rules: [{ ...rule, name: ' ' }] })).rejects.toEqual({ code: 'badRequest', message: 'Every rule needs a name.' });
    await expect(b.saveRules({ ...rs, rules: [{ ...rule, condition: { ...rule.condition, cpuAbove: 0 } }] })).rejects.toMatchObject({ message: expect.stringContaining('between 1 and 100%') });
    await expect(b.saveRules({ ...rs, rules: [{ ...rule, condition: { ...rule.condition, cpuAbove: 101 } }] })).rejects.toMatchObject({ code: 'badRequest' });
    await expect(b.saveRules({ ...rs, rules: [{ ...rule, ladder: [] }] })).rejects.toMatchObject({ message: expect.stringContaining('at least one step') });
    const saved = await b.saveRules({ ...rs, protected: ['X'], builtInProtected: ['ignored'] });
    expect(saved.protected).toEqual(['X']);
    expect(saved.builtInProtected).not.toEqual(['ignored']);
    expect((await b.rules()).protected).toEqual(['X']);
  });
});

describe('cleanup', () => {
  it('lists categories and marks the scheduled ones', async () => {
    const b = make();
    const cats = await b.cleanupCategories();
    expect(cats.length).toBeGreaterThan(2);
    const settings = await b.settings();
    for (const c of cats) expect(c.scheduled).toBe(settings.cleanup.categories.includes(c.id));
  });
  it('scans all categories or the requested ones', async () => {
    const b = make();
    const all = await b.scanCleanup([]);
    expect(all.results.length).toBe((await b.cleanupCategories()).length);
    expect(all.totalBytes).toBe(all.results.reduce((a, r) => a + r.bytes, 0));
    const ids = (await b.cleanupCategories()).slice(0, 1).map((c) => c.id);
    const one = await b.scanCleanup(ids);
    expect(one.results.map((r) => r.category)).toEqual(ids);
  });
  it('runs a quarantine cleanup, freeing bytes and logging an undoable entry', async () => {
    const b = make();
    const scan = await b.scanCleanup([]);
    const items = scan.results.flatMap((r) => r.items).filter((i) => !i.path.includes('Cache')).slice(0, 3);
    const quarantineBefore = (await b.quarantine()).length;
    const out = await b.runCleanup(scan.id, items.map((i) => i.id), 'quarantine');
    expect(out.deleted).toBe(items.length);
    expect(out.freedBytes).toBe(items.reduce((a, i) => a + i.bytes, 0));
    expect(out.quarantineId).toBeTruthy();
    expect((await b.quarantine()).length).toBe(quarantineBefore + 1);
    const entry = (await b.activity(1))[0]!;
    expect(entry).toMatchObject({ kind: 'cleanup', undo: 'available', bytes: out.freedBytes });
    // A scan can be used once.
    await expect(b.runCleanup(scan.id, [], 'quarantine')).rejects.toEqual({ code: 'notFound', message: 'That scan expired. Scan again.' });
  });
  it('recycle-bin cleanups are not quarantined or undoable', async () => {
    const b = make();
    const scan = await b.scanCleanup([]);
    const item = scan.results.flatMap((r) => r.items)[0]!;
    const out = await b.runCleanup(scan.id, [item.id], 'recycleBin');
    expect(out.quarantineId).toBeNull();
    expect((await b.activity(1))[0]!.undo).toBe('notUndoable');
  });
  it('restoreQuarantine restores once', async () => {
    const b = make();
    const q = (await b.quarantine())[0]!;
    await b.restoreQuarantine(q.id);
    expect(await b.quarantine()).toEqual([]);
    await expect(b.restoreQuarantine(q.id)).rejects.toMatchObject({ code: 'notFound' });
  });
  it('diskUsage returns a tree', async () => {
    const root = await make().diskUsage(null);
    expect(root.bytes).toBeGreaterThan(0);
  });
});

describe('health, tuning, startup', () => {
  it('health grades the score', async () => {
    const h = await make().health();
    expect(h.score).toBe(100 - h.reasons.reduce((a, r) => a + r.impact, 0));
    const grade = h.score >= 80 ? 'good' : h.score >= 60 ? 'fair' : 'poor';
    expect(h.grade).toBe(grade);
  });
  it('applying a tuning suggestion changes the list, the score and the log; applying twice fails', async () => {
    const b = make();
    const before = await b.health();
    const t = (await b.tuning()).find((x) => !x.applied)!;
    const entry = await b.applyTuning(t.id);
    expect(entry).toMatchObject({ kind: 'tuning', undo: 'available', title: t.title });
    expect((await b.tuning()).find((x) => x.id === t.id)!.applied).toBe(true);
    expect((await b.health()).score).toBeGreaterThanOrEqual(before.score);
    await expect(b.applyTuning(t.id)).rejects.toEqual({ code: 'badRequest', message: 'It is already applied.' });
    const reverted = await b.revertTuning(t.id);
    expect(reverted.title).toBe(`Reverted: ${t.title}`);
    await expect(b.revertTuning(t.id)).rejects.toEqual({ code: 'badRequest', message: 'It is not applied.' });
    await expect(b.applyTuning('nope')).rejects.toMatchObject({ code: 'notFound' });
  });
  it('tuning that needs administrator rights is refused, and a setting changed outside Dyno needs a forced revert', async () => {
    const b = make();
    const list = await b.tuning();
    const admin = list.find((x) => x.needsAdmin && !x.applied)!;
    expect(admin.blocked).toContain('administrator rights');
    expect(list.find((x) => x.id === 'powerPlan')!.preview).toEqual(['Activate the power plan Balanced']);
    await expect(b.applyTuning(admin.id)).rejects.toMatchObject({ code: 'accessDenied' });
    await b.applyTuning('powerPlan');
    b.changeTuningOutside('powerPlan');
    expect((await b.tuning()).find((x) => x.id === 'powerPlan')!.changedOutside).toBe(true);
    await expect(b.revertTuning('powerPlan')).rejects.toMatchObject({ code: 'badRequest', message: expect.stringContaining('Something else changed') });
    expect((await b.revertTuning('powerPlan', true)).title).toContain('Reverted');
    expect((await b.tuning()).find((x) => x.id === 'powerPlan')!.changedOutside).toBe(false);
    b.changeTuningOutside('powerPlan'); // not applied: nothing to flag
    expect((await b.tuning()).find((x) => x.id === 'powerPlan')!.changedOutside).toBe(false);
  });
  it('startup entries change state with the right wording', async () => {
    const b = make();
    const e = (await b.startupEntries())[0]!;
    expect((await b.setStartupState(e.id, 'disabled')).title).toBe(`Disabled ${e.name} at sign-in`);
    expect((await b.setStartupState(e.id, 'enabled')).title).toBe(`Enabled ${e.name} at sign-in`);
    expect((await b.setStartupState(e.id, { delayed: 30 })).title).toBe(`Delayed by 30 s: ${e.name} at sign-in`);
    expect((await b.startupEntries())[0]!.state).toEqual({ delayed: 30 });
    await expect(b.setStartupState('nope', 'enabled')).rejects.toEqual({ code: 'notFound', message: 'That startup entry was removed.' });
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
    expect(empty).toMatchObject({ nothingToReport: true, score: null, issues: [], changes: [] });
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

  it('rules answers a copy, and switching a rule off releases what it had in effect', async () => {
    const b = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
    const set = await b.rules();
    set.paused = true;
    expect((await b.rules()).paused).toBe(false);
    expect(b.liveUpdate('summary').activeActions).toHaveLength(1);
    set.paused = false;
    set.rules = set.rules.map((r) => ({ ...r, enabled: false }));
    await b.saveRules(set);
    expect((await b.rules()).rules.every((r) => !r.enabled)).toBe(true);
    expect(b.liveUpdate('summary').activeActions).toHaveLength(0);
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
