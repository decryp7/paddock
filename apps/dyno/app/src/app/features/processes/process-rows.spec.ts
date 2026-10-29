import type { AppGroup, Measured, ProcessSample } from '../../core/api.types';
import { type Row, type RowInput, type Sort, buildRows, compareRows, matches } from './process-rows';

type SampleOver = Partial<Omit<ProcessSample, 'user' | 'cpu' | 'memory' | 'diskReadBps' | 'diskWriteBps' | 'started'>> & {
  user?: string | null;
  cpu?: number | Measured<number>;
  memory?: number;
  diskReadBps?: number;
  diskWriteBps?: number;
};
const num = (v: number | Measured<number>): Measured<number> => (typeof v === 'number' ? { value: v } : v);
const sample = (pid: number, name: string, extra: SampleOver = {}): ProcessSample => {
  const { user = 'alex', cpu = 1, memory = 100, diskReadBps = 0, diskWriteBps = 0, ...rest } = extra;
  return {
    pid, parent: null, name, exe: `/bin/${name}`, state: 'running',
    user: user === null ? { unavailable: { reason: 'notPermitted' } } : { value: user },
    cpu: num(cpu), memory: num(memory), diskReadBps: num(diskReadBps), diskWriteBps: num(diskWriteBps),
    started: { value: 1000 + pid }, owner: 'own', ...rest,
  };
};
const group = (id: string, name: string, pids: number[], cpu = 0, memory = 0): AppGroup => ({
  id, name, exe: null, pids, cpu: { value: cpu }, memory: { value: memory }, diskBps: { value: 0 }, partial: false,
});
const input = (over: Partial<RowInput>): RowInput => ({
  mode: 'groups', processes: [], groups: [], sort: { key: 'cpu', dir: 'desc' }, query: '', expanded: new Set(), ...over,
});
const names = (rows: Row[]) => rows.map((r) => r.name);

describe('matches', () => {
  const p = sample(42, 'Chrome', { user: 'Alex', exe: '/Apps/Google Chrome' });
  it('matches everything on an empty or blank query', () => {
    expect(matches(p, '')).toBe(true);
    expect(matches(p, '   ')).toBe(true);
  });
  it('matches name, user, exe case-insensitively and pid exactly', () => {
    expect(matches(p, 'chro')).toBe(true);
    expect(matches(p, 'ALEX')).toBe(true);
    expect(matches(p, 'google')).toBe(true);
    expect(matches(p, '42')).toBe(true);
    expect(matches(p, '4')).toBe(false);
    expect(matches(p, 'firefox')).toBe(false);
  });
  it('copes with a missing user or exe', () => {
    const q = sample(1, 'x', { user: null, exe: null });
    expect(matches(q, 'alex')).toBe(false);
    expect(matches(q, 'x')).toBe(true);
  });
});

describe('compareRows', () => {
  const row = (name: string, extra: Partial<Row> = {}): Row => ({
    key: name, kind: 'process', depth: 0, name, count: 1, cpu: 0, memory: 0, disk: 0, unavailable: { cpu: null, memory: null, disk: null }, partial: false, user: null, pid: null, ref: null,
    groupId: null, expandable: false, expanded: false, suspended: false, ...extra,
  });
  it('sorts numerically in both directions', () => {
    const lo = row('a', { cpu: 1 });
    const hi = row('b', { cpu: 5 });
    expect(compareRows(lo, hi, { key: 'cpu', dir: 'asc' })).toBeLessThan(0);
    expect(compareRows(lo, hi, { key: 'cpu', dir: 'desc' })).toBeGreaterThan(0);
  });
  it('sorts names and users case-insensitively, a missing user as empty', () => {
    expect(compareRows(row('apple'), row('Banana'), { key: 'name', dir: 'asc' })).toBeLessThan(0);
    expect(compareRows(row('a', { user: 'Zed' }), row('b', { user: null }), { key: 'user', dir: 'asc' })).toBeGreaterThan(0);
  });
  it('puts rows whose value is unavailable last in either direction', () => {
    const na = row('na', { cpu: null });
    const some = row('some', { cpu: 3 });
    for (const dir of ['asc', 'desc'] as const) {
      expect(compareRows(na, some, { key: 'cpu', dir })).toBeGreaterThan(0);
      expect(compareRows(some, na, { key: 'cpu', dir })).toBeLessThan(0);
    }
    expect(compareRows(row('b', { cpu: null }), row('a', { cpu: null }), { key: 'cpu', dir: 'asc' })).toBeGreaterThan(0);
  });
  it('breaks ties by name, then pid', () => {
    const sort: Sort = { key: 'cpu', dir: 'desc' };
    expect(compareRows(row('a'), row('b'), sort)).toBeLessThan(0);
    expect(compareRows(row('a', { pid: 2 }), row('a', { pid: 1 }), sort)).toBeGreaterThan(0);
    expect(compareRows(row('a'), row('a'), sort)).toBe(0);
  });
  it('sorts by pid, treating a group as 0', () => {
    expect(compareRows(row('a', { pid: null }), row('b', { pid: 3 }), { key: 'pid', dir: 'asc' })).toBeLessThan(0);
  });
});

describe('buildRows: flat', () => {
  const processes = [sample(1, 'b', { cpu: 5 }), sample(2, 'a', { cpu: 9 }), sample(3, 'c', { cpu: 1, state: 'suspended' })];
  it('lists processes sorted', () => {
    const rows = buildRows(input({ mode: 'flat', processes }));
    expect(names(rows)).toEqual(['a', 'b', 'c']);
    expect(rows.every((r) => r.depth === 0 && !r.expandable && r.kind === 'process')).toBe(true);
    expect(rows[2]).toMatchObject({ suspended: true, ref: { pid: 3, started: 1003 }, key: 'p3' });
  });
  it('sums disk and filters by the query', () => {
    const rows = buildRows(input({ mode: 'flat', processes: [sample(1, 'x', { diskReadBps: 3, diskWriteBps: 4 })], query: 'x' }));
    expect(rows[0]!.disk).toBe(7);
    expect(buildRows(input({ mode: 'flat', processes, query: 'zzz' }))).toEqual([]);
  });
});

describe('buildRows: values that were not read', () => {
  it('shows null with the reason, never 0, and a partial group says so', () => {
    const hidden = sample(1, 'sys', { cpu: { unavailable: { reason: 'needsAdmin' } } });
    const rows = buildRows(input({ mode: 'flat', processes: [hidden, sample(2, 'mine', { cpu: 4 })] }));
    expect(rows.map((r) => r.name)).toEqual(['mine', 'sys']);
    expect(rows[1]).toMatchObject({ cpu: null });
    expect(rows[1]!.unavailable.cpu).toContain('administrator');
    expect(rows[1]!.unavailable.memory).toBeNull();
    const g = { ...group('g', 'G', [1, 2], 4), partial: true };
    expect(buildRows(input({ processes: [hidden, sample(2, 'mine', { cpu: 4 })], groups: [g] }))[0]).toMatchObject({ partial: true, cpu: 4 });
  });
});

describe('buildRows: other users', () => {
  const other = { count: 3, cpu: { value: 4.5 }, memory: { value: 900 }, partial: true } as const;
  it('adds one row that is not expandable, last in every mode, with its note values', () => {
    const processes = [sample(1, 'a', { cpu: 50 })];
    for (const mode of ['groups', 'tree', 'flat'] as const) {
      const rows = buildRows(input({ mode, processes, groups: [group('a', 'A', [1], 50)], otherUsers: other }));
      const last = rows[rows.length - 1]!;
      expect(last).toMatchObject({ kind: 'other', name: 'Other users', count: 3, cpu: 4.5, memory: 900, disk: null, partial: true, expandable: false, expanded: false, ref: null });
      expect(last.unavailable.disk).toContain('Unavailable');
    }
  });
  it('shows an unreadable total as null with the reason and follows the search', () => {
    const none = { ...other, cpu: { unavailable: { reason: 'needsAdmin' } }, partial: true } as const;
    const rows = buildRows(input({ mode: 'flat', processes: [], otherUsers: none }));
    expect(rows[0]).toMatchObject({ kind: 'other', cpu: null });
    expect(rows[0]!.unavailable.cpu).toContain('administrator');
    expect(buildRows(input({ mode: 'flat', processes: [], otherUsers: other, query: 'other' })).length).toBe(1);
    expect(buildRows(input({ mode: 'flat', processes: [], otherUsers: other, query: 'zzz' }))).toEqual([]);
    expect(buildRows(input({ mode: 'flat', processes: [], otherUsers: null }))).toEqual([]);
  });
});

describe('buildRows: groups', () => {
  const processes = [
    sample(1, 'chrome', { cpu: 2, user: 'alex' }),
    sample(2, 'chrome', { cpu: 6 }),
    sample(3, 'code', { cpu: 1, user: 'bob' }),
    sample(4, 'svc', { state: 'suspended' }),
  ];
  const groups = [group('code', 'Code', [3], 1, 100), group('chrome', 'Chrome', [1, 2], 8, 200), group('svc', 'Services', [4], 0.5), group('gone', 'Gone', [99])];

  it('orders groups by the sort and collapses them by default', () => {
    const rows = buildRows(input({ processes, groups }));
    expect(names(rows)).toEqual(['Chrome', 'Code', 'Services']);
    expect(rows[0]).toMatchObject({ kind: 'group', key: 'gchrome', count: 2, expandable: true, expanded: false, user: null, pid: null, ref: null, groupId: 'chrome' });
  });
  it('a single-process group shows its user and cannot expand', () => {
    const code = buildRows(input({ processes, groups })).find((r) => r.name === 'Code')!;
    expect(code).toMatchObject({ user: 'bob', expandable: false, expanded: false });
  });
  it('skips groups with no matching members', () => {
    expect(names(buildRows(input({ processes, groups }))).includes('Gone')).toBe(false);
  });
  it('shows members under an expanded group, sorted', () => {
    const rows = buildRows(input({ processes, groups, expanded: new Set(['gchrome']) }));
    expect(names(rows)).toEqual(['Chrome', 'chrome', 'chrome', 'Code', 'Services']);
    expect(rows[1]).toMatchObject({ depth: 1, pid: 2 });
    expect(rows[2]).toMatchObject({ depth: 1, pid: 1 });
    expect(rows[0]!.expanded).toBe(true);
  });
  it('searching opens every group and keeps only matching members', () => {
    const rows = buildRows(input({ processes, groups, query: 'chrome' }));
    expect(rows.map((r) => r.pid)).toEqual([null, 2, 1]);
  });
  it('a one-member group never reports expanded', () => {
    expect(buildRows(input({ processes, groups, query: 'code' }))[0]!.expanded).toBe(false);
  });
  it('protected / suspended only when every member is', () => {
    const rows = buildRows(input({ processes, groups }));
    expect(rows.find((r) => r.name === 'Services')).toMatchObject({ suspended: true });
    expect(rows.find((r) => r.name === 'Chrome')).toMatchObject({ suspended: false });
  });
  it('sorts groups by name ascending', () => {
    expect(names(buildRows(input({ processes, groups, sort: { key: 'name', dir: 'asc' } })))).toEqual(['Chrome', 'Code', 'Services']);
  });
});

describe('buildRows: tree', () => {
  const processes = [
    sample(1, 'init', { parent: null, cpu: 0 }),
    sample(2, 'shell', { parent: 1, cpu: 1 }),
    sample(3, 'build', { parent: 2, cpu: 9 }),
    sample(4, 'editor', { parent: 2, cpu: 3 }),
    sample(5, 'orphan', { parent: 77, cpu: 2 }),
  ];
  it('opens the real roots and nests children by parent', () => {
    const rows = buildRows(input({ mode: 'tree', processes }));
    // init (root, open) > shell (collapsed) ; orphan is a root too
    expect(rows.map((r) => [r.name, r.depth])).toEqual([['orphan', 0], ['init', 0], ['shell', 1]]);
    expect(rows.find((r) => r.name === 'init')).toMatchObject({ expandable: true, expanded: true });
    expect(rows.find((r) => r.name === 'shell')).toMatchObject({ expandable: true, expanded: false });
    expect(rows.find((r) => r.name === 'orphan')).toMatchObject({ expandable: false, expanded: false });
  });
  it('expands a node on request and sorts siblings', () => {
    const rows = buildRows(input({ mode: 'tree', processes, expanded: new Set(['p2']) }));
    expect(rows.map((r) => r.name)).toEqual(['orphan', 'init', 'shell', 'build', 'editor']);
    expect(rows.find((r) => r.name === 'build')!.depth).toBe(2);
  });
  it('an explicit collapse (-key) closes a root', () => {
    const rows = buildRows(input({ mode: 'tree', processes, expanded: new Set(['-p1']) }));
    expect(rows.map((r) => r.name)).toEqual(['orphan', 'init']);
    expect(rows.find((r) => r.name === 'init')!.expanded).toBe(false);
  });
  it('searching opens everything and shows a match whose parent did not match as a root', () => {
    const rows = buildRows(input({ mode: 'tree', processes, query: 'build' }));
    expect(rows.map((r) => [r.name, r.depth])).toEqual([['build', 0]]);
    const kids = [sample(1, 'zeta', { exe: null }), sample(2, 'zeta-child', { parent: 1, exe: null }), sample(3, 'other', { parent: 2, exe: null })];
    const both = buildRows(input({ mode: 'tree', processes: kids, query: 'zeta' }));
    expect(both.map((r) => [r.name, r.depth])).toEqual([['zeta', 0], ['zeta-child', 1]]);
  });
  it('survives parent cycles without looping or duplicating', () => {
    const cyc = [sample(1, 'a', { parent: 2 }), sample(2, 'b', { parent: 1 })];
    // Nobody is a root, so nothing is shown, but it must terminate.
    expect(buildRows(input({ mode: 'tree', processes: cyc }))).toEqual([]);
    const selfRoot = [sample(1, 'a', { parent: null }), sample(2, 'b', { parent: 1 }), sample(3, 'c', { parent: 2 })];
    const withCycle = selfRoot.map((p) => (p.pid === 1 ? { ...p, parent: 3 } : p));
    expect(() => buildRows(input({ mode: 'tree', processes: withCycle, query: 'a' }))).not.toThrow();
    const rows = buildRows(input({ mode: 'tree', processes: selfRoot, expanded: new Set(['p2', 'p3']) }));
    expect(new Set(rows.map((r) => r.key)).size).toBe(rows.length);
  });
  it('a process that is its own parent is not a root and does not hang', () => {
    expect(buildRows(input({ mode: 'tree', processes: [sample(1, 'self', { parent: 1 })], query: 'self' }))).toBeDefined();
  });
  it('is empty without processes', () => {
    expect(buildRows(input({ mode: 'tree' }))).toEqual([]);
    expect(buildRows(input({ mode: 'groups' }))).toEqual([]);
  });
});
