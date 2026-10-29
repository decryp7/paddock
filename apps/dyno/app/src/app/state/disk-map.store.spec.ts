import { TestBed } from '@angular/core/testing';
import { patchState } from '@ngrx/signals';
import { unprotected } from '@ngrx/signals/testing';
import type { DiskScanStatus, Volume } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { DiskMapStore, FOLD_BELOW, PAGE, RINGS, RING_MAX_NODES, RING_MIN_SHARE } from './disk-map.store';

const NOW = 1_700_000_000_000;

function setup(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(DiskMapStore) };
}
/** Lets the mock's scheduled scan steps run (its timers are faked, so a test decides when a scan moves). */
const advance = () => vi.advanceTimersByTimeAsync(0);

describe('DiskMapStore', () => {
  beforeEach(() => vi.useFakeTimers({ now: NOW, toFake: ['setTimeout', 'clearTimeout', 'Date'] }));
  afterEach(() => vi.useRealTimers());

  it('lists the volumes and selects the internal one', async () => {
    const { store } = setup();
    await store.loadVolumes();
    expect(store.volumesLoaded()).toBe(true);
    expect(store.volumes().map((v) => v.name)).toEqual(['Macintosh HD', 'Photos', 'Media']);
    expect(store.selectedVolume()?.name).toBe('Macintosh HD');
    expect(store.status()).toBeNull();
    expect(store.capacity()?.counted).toBe(0);
  });

  it('keeps the engine message when the volumes cannot be listed, and recovers', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'diskVolumes').mockRejectedValueOnce({ code: 'internal', message: 'No list.' });
    await store.loadVolumes();
    expect(store.volumesError()).toEqual({ code: 'internal', message: 'No list.' });
    await store.loadVolumes();
    expect(store.volumesError()).toBeNull();
  });

  it('starts a scan, shows its root level at once and follows the pushed status', async () => {
    const { store } = setup();
    await store.loadVolumes();
    expect(await store.scan({ volume: { id: '/' } })).toBe(true);
    expect(store.outcome()).toBe('running');
    expect(store.trail()).toEqual([{ node: null, name: 'Macintosh HD', bytes: expect.any(Number) }]);
    expect(store.level()?.node.name).toBe('Macintosh HD');
    await advance();
    expect(store.outcome()).toBe('done');
    expect(store.status()!.progress.bytes).toBeGreaterThan(0);
    expect(store.rows().map((r) => r.node.name)).toContain('Users');
    expect(store.capacity()!.hidden).toBeGreaterThanOrEqual(0);
  });

  it('ignores a status that is older than the one it has, and one from another scan', async () => {
    const { store } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    const done = store.status()!;
    store.receive({ ...done, seq: done.seq - 1, state: 'running' });
    expect(store.outcome()).toBe('done');
    store.receive({ ...done, scan: 99, seq: 99, state: 'cancelled' });
    expect(store.status()!.scan).toBe(done.scan);
  });

  it('drills in, goes up, goes to a crumb and to the root; a file does not open', async () => {
    const { store } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    const users = store.level()!.children.find((c) => c.name === 'Users')!;
    await store.open(users);
    expect(store.centre()?.name).toBe('Users');
    const alex = store.level()!.children.find((c) => c.name === 'alex')!;
    await store.open(alex);
    expect(store.trail().map((c) => c.name)).toEqual(['Macintosh HD', 'Users', 'alex']);
    const before = store.level();
    await store.open({ ...alex, kind: 'file' });
    await store.open({ ...alex, hasChildren: false });
    expect(store.level()).toBe(before);
    await store.up();
    expect(store.centre()?.name).toBe('Users');
    await store.open(alex);
    await store.root();
    expect(store.trail()).toHaveLength(1);
    await store.up();
    await store.goTo(5);
    expect(store.trail()).toHaveLength(1);
  });

  it('cancel leaves a usable tree', async () => {
    const { store } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await store.cancel();
    await advance();
    expect(store.outcome()).toBe('cancelled');
    await store.refresh();
    expect(store.rows().length).toBeGreaterThan(0);
    await store.open(store.level()!.children[0]!);
    expect(store.levelError()).toBeNull();
  });

  it('asks the table for everything and the chart for the folded view', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    const spy = vi.spyOn(mock, 'diskChildren');
    await store.setView('table');
    expect(spy).toHaveBeenLastCalledWith(expect.objectContaining({ minFraction: 0, limit: PAGE }));
    await store.setView('table');
    expect(spy).toHaveBeenCalledTimes(1);
    await store.setView('chart');
    expect(spy).toHaveBeenLastCalledWith(expect.objectContaining({ minFraction: FOLD_BELOW }));
  });

  it('reads the next page and appends it', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    const first = store.level()!;
    await store.loadMore();
    expect(store.level()).toBe(first);
    const spy = vi.spyOn(mock, 'diskChildren').mockResolvedValueOnce({ ...first, children: [{ ...first.children[0]!, id: 900, name: 'more' }], nextOffset: null });
    patchState(unprotected(store), { level: { ...first, nextOffset: first.children.length } });
    await store.loadMore();
    expect(spy).toHaveBeenCalledWith(expect.objectContaining({ offset: first.children.length }));
    expect(store.level()!.children.at(-1)!.name).toBe('more');
    expect(store.level()!.nextOffset).toBeNull();
  });

  it('shows a failed level read and recovers on refresh', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    vi.spyOn(mock, 'diskChildren').mockRejectedValueOnce({ code: 'notFound', message: 'Gone.' });
    await store.refresh();
    expect(store.levelError()).toEqual({ code: 'notFound', message: 'Gone.' });
    await store.refresh();
    expect(store.levelError()).toBeNull();
  });

  it('reports a scan that could not start, and a share that dropped', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    vi.spyOn(mock, 'startDiskScan').mockRejectedValueOnce({ code: 'busy', message: 'Two scans are already running.' });
    expect(await store.scan({ volume: { id: '/' } })).toBe(false);
    expect(store.startError()?.message).toContain('Two scans');
    store.dismissStartError();
    expect(store.startError()).toBeNull();
    await store.selectVolume('/Volumes/Media');
    await store.rescan();
    await advance();
    expect(store.outcome()).toBe('failed');
    expect(store.status()!.progress.bytes).toBeGreaterThan(0);
    expect(store.rows().length).toBeGreaterThan(0);
  });

  it('does not start a second scan while one is starting', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    const spy = vi.spyOn(mock, 'startDiskScan');
    const first = store.scan({ volume: { id: '/' } });
    expect(await store.scan({ volume: { id: '/' } })).toBe(false);
    await first;
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it('restores the newest kept scan of the selected volume and switches between volumes', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    store.reset();
    expect(store.status()).toBeNull();
    await store.loadVolumes();
    await store.restore();
    expect(store.outcome()).toBe('done');
    expect(store.level()?.node.name).toBe('Macintosh HD');
    await store.selectVolume('/Volumes/Photos');
    expect(store.status()).toBeNull();
    expect(store.volume()?.name).toBe('Photos');
    await store.selectVolume('/');
    expect(store.outcome()).toBe('done');
    vi.spyOn(mock, 'diskScans').mockRejectedValueOnce(new Error('x'));
    await store.restore();
    expect(store.outcome()).toBe('done');
  });

  it('chooses a folder through the OS dialog, and does nothing when none is chosen', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    vi.spyOn(mock, 'pickFolder').mockResolvedValueOnce(null);
    expect(await store.chooseFolder()).toBe(false);
    vi.spyOn(mock, 'pickFolder').mockResolvedValueOnce('/Users/alex');
    expect(await store.chooseFolder()).toBe(true);
    expect(store.selectedVolumeId()).toBeNull();
    expect(store.status()!.target).toEqual({ folder: { path: '/Users/alex' } });
    vi.spyOn(mock, 'pickFolder').mockRejectedValueOnce({ code: 'internal', message: 'No dialog.' });
    expect(await store.chooseFolder()).toBe(false);
    expect(store.startError()?.message).toBe('No dialog.');
  });

  it('gives the path of a node and nothing before a scan', async () => {
    const { store } = setup();
    await store.loadVolumes();
    expect(await store.pathOf(1)).toBeNull();
    await store.scan({ volume: { id: '/' } });
    await advance();
    const users = store.level()!.children.find((c) => c.name === 'Users')!;
    expect(await store.pathOf(users.id)).toBe('/Users');
  });

  it('cancel without a scan does nothing, and a refused cancel is reported', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    await store.cancel();
    await store.scan({ volume: { id: '/' } });
    vi.spyOn(mock, 'cancelDiskScan').mockRejectedValueOnce({ code: 'notFound', message: 'No such scan.' });
    await store.cancel();
    expect(store.startError()?.message).toBe('No such scan.');
  });

  it('rescans the same target, and a folder when no volume is known', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    const spy = vi.spyOn(mock, 'startDiskScan');
    await store.scan({ volume: { id: '/' } });
    await advance();
    await store.rescan();
    expect(spy).toHaveBeenLastCalledWith({ volume: { id: '/' } });
    const status: DiskScanStatus = store.status()!;
    expect(status.seq).toBeGreaterThan(0);
  });

  it('reveals a node, and reports a refusal', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    expect(await store.reveal(1)).toBe(false);
    await store.scan({ volume: { id: '/' } });
    await advance();
    const spy = vi.spyOn(mock, 'diskReveal');
    expect(await store.reveal(1)).toBe(true);
    expect(spy).toHaveBeenCalledWith(store.status()!.scan, 1);
    spy.mockRejectedValueOnce({ code: 'unsupported', message: 'No file manager.' });
    expect(await store.reveal(1)).toBe(false);
    expect(store.startError()?.message).toBe('No file manager.');
  });

  it('opens the Full Disk Access pane, and reports when it cannot', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'openFullDiskAccessSettings');
    await store.openFullDiskAccessSettings();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.startError()).toBeNull();
    spy.mockRejectedValueOnce({ code: 'unsupported', message: 'macOS only.' });
    await store.openFullDiskAccessSettings();
    expect(store.startError()?.message).toBe('macOS only.');
  });

  it('reads the refused folders again and shows the scan running', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    expect(await store.readAgain()).toBe(false);
    await store.scan({ volume: { id: '/' } });
    await advance();
    const scan = store.status()!.scan;
    const start = vi.spyOn(mock, 'startDiskScan');
    expect(await store.readAgain()).toBe(true);
    expect(store.outcome()).toBe('running');
    expect(store.status()!.scan).toBe(scan);
    expect(start).not.toHaveBeenCalled();
    await advance();
    expect(store.outcome()).toBe('done');
    vi.spyOn(mock, 'retryDiskScan').mockRejectedValueOnce({ code: 'badRequest', message: 'Nothing to read again.' });
    expect(await store.readAgain()).toBe(false);
    expect(store.startError()?.message).toBe('Nothing to read again.');
  });

  it('reads the rings under the centre in one request, and nothing else for the chart', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    await advance();
    const spy = vi.spyOn(mock, 'diskRings');
    const kids = vi.spyOn(mock, 'diskChildren');
    await store.refresh();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy).toHaveBeenCalledWith({ scan: store.status()!.scan, node: null, rings: RINGS, minShare: RING_MIN_SHARE, maxNodes: RING_MAX_NODES });
    expect(kids).toHaveBeenCalledTimes(1);
    const users = store.rings()!.children.find((c) => c.node.name === 'Users')!;
    expect(users.children.length).toBeGreaterThan(0);
    expect(store.rings()!.node.id).toBe(store.rootId());
    expect(RING_MAX_NODES).toBeLessThanOrEqual(2000);
  });

  it('keeps the list when the rings cannot be read', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    vi.spyOn(mock, 'diskRings').mockRejectedValue({ code: 'internal', message: 'x' });
    await store.scan({ volume: { id: '/' } });
    await advance();
    expect(store.rings()).toBeNull();
    expect(store.ringsError()).toEqual({ code: 'internal', message: 'x' });
    expect(store.rows().length).toBeGreaterThan(0);
    expect(store.levelError()).toBeNull();
    vi.mocked(mock.diskRings).mockRestore();
    await store.refresh();
    expect(store.ringsError()).toBeNull();
    expect(store.rings()).not.toBeNull();
  });

  it('opens a folder on an outer ring through every folder on the way, and goes back by the trail', async () => {
    const { store } = setup();
    await store.loadVolumes();
    await store.scan({ volume: { id: '/' } });
    await advance();
    await advance();
    const users = store.rings()!.children.find((c) => c.node.name === 'Users')!;
    const alex = users.children.find((c) => c.node.name === 'alex')!.node;
    await store.openTo(alex.id);
    expect(store.trail().map((c) => c.name)).toEqual(['Macintosh HD', 'Users', 'alex']);
    expect(store.level()!.node.id).toBe(alex.id);
    expect(store.rings()!.node.id).toBe(alex.id);
    expect(store.ancestors().map((a) => a.name)).toEqual(['Macintosh HD', 'Users']);
    expect(store.ancestors()[0]!.id).toBe(store.rootId());
    expect(store.ancestors().every((a) => a.bytes > 0)).toBe(true);
    await store.openTo(alex.id);
    expect(store.trail()).toHaveLength(3);
    await store.openTo(987654);
    expect(store.trail()).toHaveLength(3);
    await store.goTo(0);
    expect(store.level()!.node.id).toBe(store.rootId());
    await store.openTo(store.rootId()!);
    expect(store.trail()).toHaveLength(1);
  });

  it('asks the table for no rings', async () => {
    const { store, mock } = setup();
    await store.loadVolumes();
    await store.setView('table');
    await store.scan({ volume: { id: '/' } });
    await advance();
    const spy = vi.spyOn(mock, 'diskRings');
    await store.refresh();
    expect(spy).not.toHaveBeenCalled();
    expect(store.rings()).toBeNull();
  });

  it('follows the volumes the engine pushes, keeping the choice while its volume is there', async () => {
    const { store, mock } = setup();
    let push!: (v: Volume[]) => void;
    vi.spyOn(mock, 'onDiskVolumes').mockImplementation((l) => {
      push = l;
      return () => undefined;
    });
    TestBed.inject(DiskMapStore);
    await store.loadVolumes();
    const [hd, photos, media] = store.volumes() as [Volume, Volume, Volume];
    await store.selectVolume(photos.id);
    store.volumesChanged([hd, photos, { ...media, reachable: false }]);
    expect(store.volumes()[2]!.reachable).toBe(false);
    expect(store.selectedVolumeId()).toBe(photos.id);
    store.volumesChanged([hd, media]);
    expect(store.selectedVolumeId()).toBe(hd.id);
    expect(store.volumes()).toHaveLength(2);
    store.volumesChanged([]);
    expect(store.selectedVolumeId()).toBeNull();
    void push;
  });
});
