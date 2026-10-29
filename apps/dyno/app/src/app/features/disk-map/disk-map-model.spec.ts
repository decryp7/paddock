import type { DiskChildren, DiskNode, RingNode, DiskScanStatus, ScanProgress, Volume } from '../../core/api.types';
import { mockVolumes } from '../../core/mock/disk-map';
import {
  capacityStrip,
  failureText,
  filterRows,
  jobCount,
  jobFraction,
  levelRows,
  jobRate,
  nothingReadable,
  revealLabel,
  outcome,
  sharePercent,
  shareText,
  sortRows,
  stoppedAt,
  chainTo,
  nodeOfChartId,
  toChartTree,
  findRingNode,
  volumeFree,
  volumeGroup,
  volumeMeta,
} from './disk-map-model';

const GB = 1024 ** 3;
const progress = (o: Partial<ScanProgress> = {}): ScanProgress => ({ bytes: 0, items: 0, folders: 0, totalBytes: null, unreadable: 0, ...o });
const node = (id: number, name: string, bytes: number, o: Partial<DiskNode> = {}): DiskNode => ({ id, name, kind: 'folder', bytes, logicalBytes: bytes, items: 3, newestChange: 1000 + id, complete: true, hasChildren: true, ...o });
const level = (): DiskChildren => ({
  node: node(0, 'Home', 100),
  children: [node(1, 'b', 60), node(2, 'a', 30, { kind: 'file', hasChildren: false }), node(3, 'c', 5, { complete: false })],
  other: { count: 2, bytes: 4, logicalBytes: 4, items: 2 },
  loose: { count: 1, bytes: 1, logicalBytes: 1, items: 1 },
  shown: 3,
  offset: 0,
  nextOffset: null,
});
const [internal, external, network] = mockVolumes() as [Volume, Volume, Volume];

describe('volumes', () => {
  it('groups and describes them', () => {
    expect([internal, external, network].map(volumeGroup)).toEqual(['internal', 'external', 'network']);
    expect(volumeMeta(internal)).toBe('APFS · internal');
    expect(volumeMeta(external)).toBe('EXFAT · USB');
    expect(volumeMeta(network)).toBe('SMB network volume');
    expect(volumeMeta({ ...external, location: { external: { bus: null } } })).toBe('EXFAT · external');
  });

  it('says what it cannot know', () => {
    expect(volumeFree({ ...network, reachable: false })).toBe('Not answering');
    expect(volumeFree({ ...internal, free: null })).toBe('Free space not reported');
    expect(volumeFree(network)).toMatch(/free on the server$/);
    expect(volumeFree(internal)).toMatch(/ free$/);
  });
});

describe('shares and progress', () => {
  it('never divides by zero or exceeds 100', () => {
    expect(sharePercent(5, 0)).toBe(0);
    expect(sharePercent(NaN, 10)).toBe(0);
    expect(sharePercent(20, 10)).toBe(100);
    expect(shareText(0, 10)).toBe('0%');
    expect(shareText(1, 1000)).toBe('<1%');
    expect(shareText(55, 100)).toBe('55%');
  });

  it('counts with a total or just grows', () => {
    expect(jobCount(progress({ items: 1, bytes: 2 * GB }))).toBe('1 item · 2 GB counted');
    expect(jobCount(progress({ items: 1200, bytes: GB, totalBytes: 4 * GB }))).toBe('1,200 items · 1 GB of 4 GB counted');
    expect(jobFraction(progress({ totalBytes: null }))).toBeNull();
    expect(jobFraction(progress({ bytes: 1, totalBytes: 4 }))).toBe(0.25);
    expect(jobFraction(progress({ bytes: 9, totalBytes: 4 }))).toBe(1);
    expect(stoppedAt(progress({ bytes: 56, totalBytes: 100 }))).toBe('Stopped by you at 56%');
    expect(stoppedAt(progress({ bytes: GB }))).toBe('Stopped by you after 1 GB');
  });
});

describe('failures and access', () => {
  it('words each failure', () => {
    expect(failureText({ volumeUnreachable: { detail: 'nas.local stopped answering' } }, 'Media')).toMatchObject({ kind: 'network', title: 'Scan stopped: nas.local stopped answering' });
    expect(failureText({ cannotStart: { reason: 'Gone.' } }, 'X')).toMatchObject({ kind: 'start', detail: 'Gone.' });
    expect(failureText({ internal: { detail: 'Oops' } }, 'X')).toMatchObject({ kind: 'internal', detail: 'Oops' });
    expect(outcome({ failed: { failure: { internal: { detail: '' } } } })).toBe('failed');
    expect(outcome('done')).toBe('done');
  });

  it('shows the whole no-access state only when the engine says so and nothing was counted', () => {
    const s = (state: DiskScanStatus['state'], fda: boolean, p: Partial<ScanProgress>) => ({ state, needsFullDiskAccess: fda, progress: progress(p) }) as DiskScanStatus;
    expect(nothingReadable(s('done', true, {}))).toBe(true);
    expect(nothingReadable(s('done', false, {}))).toBe(false);
    expect(nothingReadable(s('done', true, { bytes: 5 }))).toBe(false);
    expect(nothingReadable(s('running', true, {}))).toBe(false);
    expect(nothingReadable(null)).toBe(false);
  });
});

describe('rate and labels', () => {
  const st = (o: Partial<DiskScanStatus>) => o as DiskScanStatus;
  it('shows a rate and a time left only when the engine knows them', () => {
    expect(jobRate(st({ itemsPerSecond: null, etaSeconds: null }))).toBeNull();
    expect(jobRate(st({ itemsPerSecond: 1850, etaSeconds: null }))).toBe('1,850 items/s');
    expect(jobRate(st({ itemsPerSecond: 1850, etaSeconds: 40 }))).toMatch(/^1,850 items\/s · about .+ left$/);
  });

  it('names the file manager per system', () => {
    expect(revealLabel('macos')).toBe('Reveal in Finder');
    expect(revealLabel('windows')).toBe('Show in Explorer');
    expect(revealLabel('linux')).toBe('Show in folder');
    expect(revealLabel('browser')).toBe('Show in folder');
  });
});

describe('capacity strip', () => {
  const used = internal.capacity! - internal.free!;
  it('keeps the uncounted part as a ghost until the end and shows only the hidden space the engine sends', () => {
    const running = capacityStrip(internal, progress({ bytes: used / 2 }), 'running')!;
    expect(running.notCounted).toBeCloseTo(used / 2);
    expect(running.hidden).toBe(0);
    const done = capacityStrip(internal, progress({ bytes: used / 2 }), 'done', 123)!;
    expect(done.notCounted).toBe(0);
    expect(done.hidden).toBe(123);
    expect(capacityStrip(internal, progress({ bytes: used / 2 }), 'done')!.hidden).toBe(0);
    const stopped = capacityStrip(internal, progress({ bytes: used / 2 }), 'cancelled')!;
    expect(stopped.hidden).toBe(0);
    expect(stopped.notCounted).toBeCloseTo(used / 2);
  });

  it('flags clones', () => {
    expect(capacityStrip(internal, progress({ bytes: used * 1.2 }), 'done', 0)!.overCounted).toBe(true);
  });

  it('has no ghost for a share, and nothing known without capacity', () => {
    expect(capacityStrip(network, progress({ bytes: 5 * GB }), 'done')!.hidden).toBe(0);
    expect(capacityStrip({ ...internal, capacity: null }, progress(), 'done')).toBeNull();
    expect(capacityStrip(null, progress(), 'done')).toBeNull();
    const nofree = capacityStrip({ ...internal, free: null }, progress({ bytes: 1 }), 'running')!;
    expect(nofree.used).toBeNull();
    expect(nofree.width.free).toBe(0);
  });

  it('keeps the widths within the capacity', () => {
    const c = capacityStrip(internal, progress({ bytes: used / 4 }), 'running')!;
    expect(c.width.counted + c.width.notCounted + c.width.hidden + c.width.free).toBeCloseTo(100, 5);
  });
});

describe('level rows', () => {
  it('makes a row per child with its share and whether it opens', () => {
    const rows = levelRows(level());
    expect(rows.map((r) => [r.node.name, r.share, r.drillable, r.rank])).toEqual([
      ['b', '60%', true, 0],
      ['a', '30%', false, 1],
      ['c', '5%', true, 2],
    ]);
  });

  it('sorts and filters without touching the input', () => {
    const rows = levelRows(level());
    expect(sortRows(rows, 'name', 'asc').map((r) => r.node.name)).toEqual(['a', 'b', 'c']);
    expect(sortRows(rows, 'bytes', 'asc').map((r) => r.node.name)).toEqual(['c', 'a', 'b']);
    expect(sortRows(rows, 'newestChange', 'desc').map((r) => r.node.name)).toEqual(['c', 'a', 'b']);
    expect(rows.map((r) => r.node.name)).toEqual(['b', 'a', 'c']);
    expect(filterRows(rows, ' B ').map((r) => r.node.name)).toEqual(['b']);
    expect(filterRows(rows, '')).toHaveLength(3);
  });
});

const folded0 = { count: 0, bytes: 0, logicalBytes: 0, items: 0 };
const ring = (n: DiskNode, children: RingNode[] = [], o: Partial<RingNode> = {}): RingNode => ({ node: n, children, other: folded0, loose: folded0, ...o });
const rings = (): RingNode =>
  ring(node(0, 'Home', 100), [
    ring(node(1, 'b', 60), [ring(node(10, 'deep', 40)), ring(node(11, 'file', 15, { kind: 'file', hasChildren: false }))], { loose: { count: 1, bytes: 5, logicalBytes: 5, items: 1 } }),
    ring(node(2, 'a', 30, { kind: 'file', hasChildren: false })),
    ring(node(3, 'c', 5, { complete: false })),
  ], { other: { count: 2, bytes: 4, logicalBytes: 4, items: 2 }, loose: { count: 1, bytes: 1, logicalBytes: 1, items: 1 } });

describe('chart tree from the rings answer', () => {
  it('nests the levels, folds the small items and says which folders have more inside', () => {
    const tree = toChartTree(rings());
    expect(tree).toMatchObject({ id: 'n0', label: 'Home', value: 100 });
    expect(tree.children!.map((k) => [k.id, k.kind])).toEqual([['n1', 'item'], ['n2', 'item'], ['n3', 'item'], ['n0-other', 'other'], ['n0-loose', 'other']]);
    const b = tree.children![0]!;
    expect(b.children!.map((c) => c.id)).toEqual(['n10', 'n11', 'n1-loose']);
    expect(b.children![0]!.hasMore).toBe(true);
    expect(b.children![1]!.hasMore).toBe(false);
    expect(b.hasMore).toBe(false);
    expect(tree.children![2]!.hasMore).toBe(true);
    expect(tree.children![2]!.detail).toContain('incomplete');
    expect(tree.children![3]!.label).toBe('2 smaller items');
    expect(tree.children!.reduce((a, k) => a + k.value, 0)).toBe(100);
  });

  it('leaves out folded and loose parts that are empty', () => {
    const l = ring(node(0, 'Home', 100), [ring(node(1, 'b', 100))]);
    expect(toChartTree(l).children).toHaveLength(1);
    expect(toChartTree(ring(node(0, 'Zero', 0))).children).toEqual([]);
  });

  it('puts hidden space and the ghost at the root only, counted in its value', () => {
    const tree = toChartTree(rings(), [], { hidden: 20, pending: 10 });
    expect(tree.value).toBe(130);
    expect(tree.children!.slice(-2).map((c) => [c.id, c.kind, c.value])).toEqual([['hidden', 'hidden', 20], ['pending', 'pending', 10]]);
    expect(toChartTree(rings(), [], { hidden: 0, pending: 0 }).value).toBe(100);
    expect(toChartTree(rings(), [{ id: 9, name: 'Up', bytes: 500 }], { hidden: 20, pending: 10 }).children![0]!.value).toBe(100);
  });

  it('wraps the centre in its enclosing folders so the chart can go back up', () => {
    const tree = toChartTree(rings(), [{ id: 7, name: 'Root', bytes: 900 }, { id: 8, name: 'Mid', bytes: 300 }]);
    expect([tree.id, tree.label, tree.value]).toEqual(['n7', 'Root', 900]);
    expect(tree.children!.map((c) => c.id)).toEqual(['n8']);
    expect(tree.children![0]!.children!.map((c) => c.id)).toEqual(['n0']);
    expect(toChartTree(rings(), [{ id: 7, name: 'Odd', bytes: 1 }]).value).toBe(100);
  });

  it('finds the folders to open to reach a node, and a node anywhere', () => {
    expect(chainTo(rings(), 10)!.map((n) => n.id)).toEqual([1, 10]);
    expect(chainTo(rings(), 1)!.map((n) => n.id)).toEqual([1]);
    expect(chainTo(rings(), 999)).toBeNull();
    expect(findRingNode(rings(), 10)!.name).toBe('deep');
    expect(findRingNode(rings(), 0)!.name).toBe('Home');
    expect(findRingNode(rings(), 999)).toBeNull();
  });

  it('reads node numbers back from chart ids', () => {
    expect(nodeOfChartId('n12')).toBe(12);
    expect(['hidden', 'pending', 'n1-other', '']).toSatisfy((a: string[]) => a.every((x) => nodeOfChartId(x) === null));
  });
});
