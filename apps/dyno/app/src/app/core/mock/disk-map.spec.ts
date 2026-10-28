import type { DiskScanStatus } from '../api.types';
import { MockBackend } from '../mock-backend';
import { MockTree, mockVolumes } from './disk-map';
import wire from '../disk-map.wire.json';

const NOW = 1_700_000_000_000;
const make = (o: { fail?: Set<string> } = {}) => new MockBackend({ latencyMs: 0, now: () => NOW, ...o });

/** Starts a scan and collects its events until the final one. */
async function runScan(b: MockBackend, target: Parameters<MockBackend['startDiskScan']>[0]): Promise<{ first: DiskScanStatus; events: DiskScanStatus[] }> {
  const events: DiskScanStatus[] = [];
  let done!: () => void;
  const finished = new Promise<void>((r) => (done = r));
  const off = b.onDiskScan((s) => {
    events.push(s);
    if (typeof s.state !== 'string' || s.state !== 'running') done();
  });
  const first = await b.startDiskScan(target);
  await finished;
  off();
  return { first, events };
}

const keys = (o: object): string[] => Object.keys(o).sort();

describe('the mock disk map', () => {
  it('lists internal, external and network volumes with their kind and capacity', async () => {
    const v = await make().diskVolumes();
    expect(v.map((x) => x.name)).toEqual(['Macintosh HD', 'Photos', 'Media']);
    expect(v.map((x) => (typeof x.location === 'string' ? x.location : Object.keys(x.location)[0]))).toEqual(['internal', 'external', 'network']);
    expect(v.every((x) => (x.capacity ?? 0) > (x.free ?? 0))).toBe(true);
    expect(v[2]!.wholeVolume).toBe(false);
    expect(mockVolumes()).toEqual(v);
  });

  it('has the same fields as the engine sends', async () => {
    const b = make();
    const [vol] = await b.diskVolumes();
    expect(keys(vol!)).toEqual(keys(wire.volumes[0]!));
    const { first, events } = await runScan(b, { volume: { id: '/' } });
    expect(keys(first)).toEqual(keys(wire.statuses[0]!));
    expect(keys(first.progress)).toEqual(keys(wire.statuses[0]!.progress));
    expect(events.length).toBeGreaterThan(2);
    const page = await b.diskChildren({ scan: first.scan, node: null, offset: 0, limit: 10, minFraction: 0 });
    expect(keys(page)).toEqual(keys(wire.children));
    expect(keys(page.node)).toEqual(keys(wire.children.node));
    expect(keys(page.other)).toEqual(keys(wire.children.other));
  });

  it('runs a scan as a job: facts first, then growing counts, then done', async () => {
    const b = make();
    const { first, events } = await runScan(b, { volume: { id: '/' } });
    expect(first.state).toBe('running');
    expect(events[0]!.phase).toBe('readingVolumeFacts');
    expect(events.every((e, i) => i === 0 || (e.seq > events[i - 1]!.seq && e.progress.bytes >= events[i - 1]!.progress.bytes))).toBe(true);
    const last = events.at(-1)!;
    expect(last.state).toBe('done');
    expect(last.progress.totalBytes).not.toBeNull();
    expect(last.progress.bytes).toBeGreaterThan(0);
    expect(last.progress.unreadable).toBe(1);
  });

  it('a folder and a network share have no known total (the count just grows)', async () => {
    const b = make();
    const folder = await runScan(b, { folder: { path: '/Users/alex' } });
    expect(folder.events.at(-1)!.progress.totalBytes).toBeNull();
    expect(folder.events.at(-1)!.name).toBe('alex');
    const share = await runScan(make(), { volume: { id: '/Volumes/Media' } });
    expect(share.events.at(-1)!.progress.totalBytes).toBeNull();
  });

  it('a dropped share ends the scan as a recoverable failure', async () => {
    const { events } = await runScan(make(), { volume: { id: '/Volumes/Media' } });
    expect(events.at(-1)!.state).toEqual({ failed: { failure: { volumeUnreachable: { detail: 'nas.local stopped answering' } } } });
  });

  it('starting the same target again returns the running scan; two at most run at once', async () => {
    const b = make();
    const a = await b.startDiskScan({ volume: { id: '/' } });
    const again = await b.startDiskScan({ volume: { id: '/' } });
    expect(again.scan).toBe(a.scan);
    await b.startDiskScan({ volume: { id: '/Volumes/Photos' } });
    await expect(b.startDiskScan({ folder: { path: '/Users/alex' } })).rejects.toMatchObject({ code: 'busy' });
  });

  it('cancel stops a scan at once and the partial tree stays readable', async () => {
    const b = make();
    const seen: DiskScanStatus[] = [];
    b.onDiskScan((s) => seen.push(s));
    const s = await b.startDiskScan({ volume: { id: '/' } });
    await b.cancelDiskScan(s.scan);
    await Promise.resolve();
    expect(seen.at(-1)!.state).toBe('cancelled');
    const count = seen.length;
    await b.cancelDiskScan(s.scan);
    expect(seen.length).toBe(count);
    expect((await b.diskScans())[0]!.state).toBe('cancelled');
    expect((await b.diskChildren({ scan: s.scan, node: null, offset: 0, limit: 5, minFraction: 0 })).children.length).toBeGreaterThan(0);
  });

  it('answers one level at a time, and children + other + loose add up to the folder', async () => {
    const b = make();
    const { first } = await runScan(b, { volume: { id: '/' } });
    const q = { scan: first.scan, node: null, offset: 0, limit: 3, minFraction: 0.05 };
    const top = await b.diskChildren(q);
    expect(top.children.length).toBeLessThanOrEqual(3);
    const all = await b.diskChildren({ ...q, limit: 256 });
    const sum = all.children.reduce((a, c) => a + c.bytes, 0) + all.other.bytes + all.loose.bytes;
    expect(sum).toBe(all.node.bytes);
    expect(all.other.count).toBeGreaterThan(0);
    expect(all.children.map((c) => c.bytes)).toEqual([...all.children.map((c) => c.bytes)].sort((x, y) => y - x));
    const paged = [];
    let offset: number | null = 0;
    while (offset !== null) {
      const p = await b.diskChildren({ ...q, limit: 2, offset });
      paged.push(...p.children);
      offset = p.nextOffset;
    }
    expect(paged.map((c) => c.id)).toEqual(all.children.map((c) => c.id));
    const folder = all.children.find((c) => c.hasChildren)!;
    const level2 = await b.diskChildren({ scan: first.scan, node: folder.id, offset: 0, limit: 256, minFraction: 0 });
    expect(level2.node.id).toBe(folder.id);
    expect(level2.children.reduce((a, c) => a + c.bytes, 0) + level2.other.bytes + level2.loose.bytes).toBe(folder.bytes);
  });

  it('marks an unreadable folder and a mount point', async () => {
    const b = make();
    const { first } = await runScan(b, { volume: { id: '/' } });
    const kinds = new Set<string>();
    const walk = async (node: number | null): Promise<void> => {
      const p = await b.diskChildren({ scan: first.scan, node, offset: 0, limit: 256, minFraction: 0 });
      for (const c of p.children) {
        kinds.add(c.kind);
        if (c.kind === 'folder') await walk(c.id);
      }
    };
    await walk(null);
    expect([...kinds].sort()).toEqual(['file', 'folder', 'mountPoint', 'unreadable']);
  });

  it('gives the full path of a node', async () => {
    const b = make();
    const { first } = await runScan(b, { volume: { id: '/Volumes/Photos' } });
    const top = await b.diskChildren({ scan: first.scan, node: null, offset: 0, limit: 1, minFraction: 0 });
    expect(await b.diskPath(first.scan, 0)).toBe('/Volumes/Photos');
    expect(await b.diskPath(first.scan, top.children[0]!.id)).toMatch(/^\/Volumes\/Photos\/\w+/);
  });

  it('refuses what the engine refuses', async () => {
    const b = make();
    await expect(b.startDiskScan({ volume: { id: '/nope' } })).rejects.toMatchObject({ code: 'notFound' });
    await expect(b.startDiskScan({ folder: { path: 'relative' } })).rejects.toMatchObject({ code: 'badRequest' });
    await expect(b.diskChildren({ scan: 99, node: null, offset: 0, limit: 1, minFraction: 0 })).rejects.toMatchObject({ code: 'notFound' });
    await expect(b.cancelDiskScan(99)).rejects.toMatchObject({ code: 'notFound' });
    await expect(b.diskPath(99, 0)).rejects.toMatchObject({ code: 'notFound' });
    const { first } = await runScan(b, { volume: { id: '/' } });
    await expect(b.diskChildren({ scan: first.scan, node: null, offset: 0, limit: 1, minFraction: 2 })).rejects.toMatchObject({ code: 'badRequest' });
    await expect(b.diskChildren({ scan: first.scan, node: 9999, offset: 0, limit: 1, minFraction: 0 })).rejects.toMatchObject({ code: 'notFound' });
    await expect(b.diskPath(first.scan, 9999)).rejects.toMatchObject({ code: 'notFound' });
  });

  it('honours the fail option like every mock call', async () => {
    await expect(make({ fail: new Set(['diskVolumes']) }).diskVolumes()).rejects.toMatchObject({ code: 'internal' });
  });

  it('stops listening when unsubscribed', async () => {
    const b = make();
    const seen: unknown[] = [];
    b.onDiskScan((s) => seen.push(s))();
    await b.startDiskScan({ volume: { id: '/' } });
    await Promise.resolve();
    expect(seen).toEqual([]);
  });

  it('a mock with latency spreads the steps over time', async () => {
    vi.useFakeTimers();
    try {
      const b = new MockBackend({ latencyMs: 10, now: () => NOW });
      const seen: DiskScanStatus[] = [];
      b.onDiskScan((s) => seen.push(s));
      const started = b.startDiskScan({ volume: { id: '/' } });
      await vi.advanceTimersByTimeAsync(10);
      await started;
      expect(seen.length).toBe(1);
      await vi.advanceTimersByTimeAsync(5000);
      expect(seen.at(-1)!.state).toBe('done');
    } finally {
      vi.useRealTimers();
    }
  });

  it('a tree handles a missing folder and an empty one', () => {
    const t = new MockTree({ name: 'x', children: [] }, NOW);
    expect(t.children({ node: 5, offset: 0, limit: 1, minFraction: 0 })).toBeNull();
    expect(t.children({ node: null, offset: 0, limit: 1, minFraction: 0 })!.children).toEqual([]);
    expect(t.path(5, '/')).toBeNull();
  });
});
