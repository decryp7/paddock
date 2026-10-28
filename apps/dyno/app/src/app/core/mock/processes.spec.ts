import { type AppSpec, APP_SPECS, buildProcesses, groupSamples, round, sampleProcess } from './processes';
import { osProfile } from './os';

const START = 1_700_000_000_000;
const spec = (over: Partial<AppSpec>): AppSpec => ({ group: 'g', groupName: 'G', name: 'g', exe: '/bin/g', user: 'alex', count: 1, cpu: 10, memory: 1000, disk: 100, ageSecs: 100, ...over });

describe('buildProcesses', () => {
  const specs = [spec({ group: 'sys', name: 'init', count: 1 }), spec({ group: 'app', name: 'app', count: 4, parent: 'sys', cpu: 20, memory: 4000, disk: 400 })];
  const procs = buildProcesses(START, specs, 1);

  it('expands counts into processes with unique pids; the first spec is the root', () => {
    expect(procs).toHaveLength(5);
    expect(new Set(procs.map((p) => p.pid)).size).toBe(5);
    expect(procs[0]).toMatchObject({ pid: 1, parent: null });
  });
  it('is deterministic', () => {
    expect(buildProcesses(START, specs, 1)).toEqual(procs);
  });
  it('splits totals unevenly, the first process doing most of the work', () => {
    const app = procs.filter((p) => p.group === 'app');
    expect(app.reduce((a, p) => a + p.cpu, 0)).toBeCloseTo(20);
    expect(app.reduce((a, p) => a + p.memory, 0)).toBeCloseTo(4000);
    expect(app[0]!.cpu).toBeGreaterThan(app[1]!.cpu);
  });
  it('parents the first process on the parent group, the rest on the first of their own group', () => {
    const app = procs.filter((p) => p.group === 'app');
    expect(app[0]!.parent).toBe(1);
    expect(app[1]!.parent).toBe(app[0]!.pid);
  });
  it('starts processes in the past relative to start', () => {
    for (const p of procs) expect(p.started).toBeLessThan(START);
  });
  it('ships a realistic catalogue on every OS with a root and valid parents', () => {
    for (const os of ['windows', 'macos', 'linux'] as const) {
      const prof = osProfile(os);
      const all = buildProcesses(START, prof.specs, prof.rootPid);
      const pids = new Set(all.map((p) => p.pid));
      expect(all[0]!.parent).toBeNull();
      expect(all[0]!.pid).toBe(prof.rootPid);
      for (const p of all) if (p.parent !== null) expect(pids.has(p.parent)).toBe(true);
      expect(all.length).toBeGreaterThan(100);
    }
    expect(APP_SPECS[0]!.parent).toBeUndefined();
  });
});

describe('sampleProcess', () => {
  const [p] = buildProcesses(START, [spec({ cpu: 10, swing: 0.5 })], 1);
  it('is deterministic per second', () => {
    expect(sampleProcess(p!, 100, null, 1)).toEqual(sampleProcess(p!, 100, null, 1));
  });
  it('stays within bounds and respects the CPU scale', () => {
    for (let t = 0; t < 60; t++) {
      const s = sampleProcess(p!, t, null, 1);
      expect(s.cpu).toBeGreaterThanOrEqual(0);
      expect(s.cpu).toBeLessThanOrEqual(100);
    }
    const avg = (scale: number) => Array.from({ length: 60 }, (_, t) => sampleProcess(p!, t, null, scale).cpu).reduce((a, b) => a + b, 0) / 60;
    expect(avg(2)).toBeGreaterThan(avg(1));
  });
  it('a cap limits the CPU', () => {
    for (let t = 0; t < 30; t++) expect(sampleProcess(p!, t, 2, 1).cpu).toBeLessThanOrEqual(2);
  });
  it('a suspended process uses nothing', () => {
    const s = sampleProcess({ ...p!, state: 'suspended' }, 5, null, 1);
    expect(s).toMatchObject({ cpu: 0, diskReadBps: 0, diskWriteBps: 0, state: 'suspended' });
  });
  it('an idle process is sleeping, a busy one running', () => {
    expect(sampleProcess({ ...p!, cpu: 0 }, 5, null, 1).state).toBe('sleeping');
    expect(sampleProcess(p!, 5, null, 1).state).toBe('running');
      });
  it('splits disk throughput 35/65', () => {
    const s = sampleProcess(p!, 5, null, 1);
    expect(s.diskWriteBps).toBeGreaterThanOrEqual(s.diskReadBps);
  });
});

describe('groupSamples', () => {
  const procs = buildProcesses(START, [spec({ group: 'a', groupName: 'A', count: 2 }), spec({ group: 'b', groupName: 'B', name: 'b' })], 1);
  const samples = procs.map((p) => sampleProcess(p, 10, null, 1));
  it('sums per group', () => {
    const groups = groupSamples(procs, samples);
    expect(groups.map((g) => g.id)).toEqual(['a', 'b']);
    const a = groups[0]!;
    expect(a.pids).toEqual([samples[0]!.pid, samples[1]!.pid]);
    expect(a.cpu).toBeCloseTo(samples[0]!.cpu + samples[1]!.cpu, 1);
    expect(a.memory).toBe(samples[0]!.memory + samples[1]!.memory);
    expect(a.diskBps).toBe(samples.slice(0, 2).reduce((x, s) => x + s.diskReadBps + s.diskWriteBps, 0));
    expect(a.name).toBe('A');
  });
  it('round rounds to digits', () => {
    expect(round(1.2345, 2)).toBe(1.23);
    expect(round(1.5, 0)).toBe(2);
  });
});
