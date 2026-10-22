import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { flush, makeMock } from '../../testing/test-helpers';
import { ToastStore } from './toast.store';
import { WatchesStore } from './watches.store';

function setup(backend = makeMock()) {
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  return { backend, store: TestBed.inject(WatchesStore), toasts: TestBed.inject(ToastStore) };
}

describe('WatchesStore', () => {
  afterEach(() => vi.restoreAllMocks());

  it('loads the list once on start and follows pushes', async () => {
    const { backend, store } = setup();
    store.start();
    store.start();
    expect(store.loading()).toBe(true);
    await flush();
    expect(backend.calls.filter((c) => c === 'listWatches')).toHaveLength(1);
    expect(store.loaded()).toBe(true);
    expect(store.watches()).toHaveLength(8);
    await backend.setPaused('w-1', true);
    expect(store.byId('w-1')?.paused).toBe(true);
    expect(store.byId('missing')).toBeNull();
  });

  it('keeps the error when loading fails and recovers on retry', async () => {
    const { backend, store } = setup();
    const spy = vi.spyOn(backend, 'listWatches').mockRejectedValueOnce({ code: 'unavailable', message: 'down', problems: [] });
    await store.load();
    expect(store.error()?.message).toBe('down');
    expect(store.loaded()).toBe(true);
    expect(store.loading()).toBe(false);
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.watches()).toHaveLength(8);
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it('survives a failed subscription', async () => {
    const { backend, store } = setup();
    vi.spyOn(backend, 'onState').mockRejectedValue(new Error('no events'));
    store.start();
    await flush();
    expect(store.watches()).toHaveLength(8);
  });

  it('derives counts: unseen, attention, checking', async () => {
    const { store } = setup();
    await store.load();
    expect(store.unseen().length).toBeGreaterThan(0);
    expect(store.attention().map((w) => w.name)).toEqual(expect.arrayContaining(['Orbit server', 'Orbit API image', 'Vendor download page']));
    expect(store.checking()).toBe(false);
  });

  it('filters by query, filter and sorts', async () => {
    const { store } = setup();
    await store.load();
    store.setQuery('  ORBIT ');
    expect(store.visible().map((w) => w.name)).toEqual(['Orbit API image', 'Orbit server']);
    store.setQuery('');
    store.setFilter('paused');
    expect(store.visible().map((w) => w.name)).toEqual(['Nightly docs feed']);
    store.setFilter('attention');
    expect(store.visible()).toHaveLength(3);
    store.setFilter('new');
    expect(store.visible().every((w) => w.unseen)).toBe(true);
    store.setFilter('all');
    store.setSort('latest');
    const latest = store.visible().map((w) => w.latest?.firstSeenMs ?? 0);
    expect(latest).toEqual([...latest].sort((a, b) => b - a));
    store.setSort('checked');
    const checked = store.visible().map((w) => w.lastCheckMs ?? 0);
    expect(checked).toEqual([...checked].sort((a, b) => b - a));
    store.setSort('name');
    expect(store.visible()[0]!.name).toBe('Acme Installer');
  });

  it('searches the latest version text too', async () => {
    const { store } = setup();
    await store.load();
    const v = store.watches()[0]!.latest!.version;
    store.setQuery(v);
    expect(store.visible().map((w) => w.id)).toContain(store.watches()[0]!.id);
  });

  it('turns successful actions into toasts and failures into error toasts', async () => {
    const { backend, store, toasts } = setup();
    await store.load();
    expect(await store.checkNow(null)).toBe(true);
    expect(await store.checkNow('w-1')).toBe(true);
    expect(await store.setPaused('w-1', true)).toBe(true);
    expect(await store.setPaused('w-1', false)).toBe(true);
    expect(await store.acknowledge('w-1', null)).toBe(true);
    expect(await store.setInstalled('w-1', '1.0.0')).toBe(true);
    expect(await store.setInstalled('w-1', null)).toBe(true);
    const latest = (await backend.getWatch('w-1')).history[0]!.version;
    expect(await store.openRelease('w-1', latest)).toBe(true);
    expect(toasts.toasts().map((t) => t.title)).toEqual(['Watch resumed', 'Marked as seen', 'Recorded 1.0.0 as installed', 'Installed version cleared']);
    expect(await store.remove('nope')).toBe(false);
    const last = toasts.toasts().at(-1)!;
    expect(last).toMatchObject({ tone: 'danger', title: 'That did not work', body: 'That watch no longer exists.' });
    expect(await store.remove('w-1')).toBe(true);
    expect(backend.calls).toContain('deleteWatch');
  });

  it('marks all unseen watches as seen with one toast', async () => {
    const { store, toasts } = setup();
    store.start();
    await flush();
    const n = store.unseen().length;
    await store.acknowledgeAll();
    expect(store.unseen()).toHaveLength(0);
    expect(toasts.toasts().at(-1)?.title).toBe(`Marked ${n} watches as seen`);
  });

  it('says "Marked as seen" for a single watch and stays quiet when one fails', async () => {
    const { backend, store, toasts } = setup();
    store.start();
    await flush();
    for (const w of store.unseen().slice(1)) await backend.acknowledge(w.id, null);
    expect(store.unseen()).toHaveLength(1);
    await store.acknowledgeAll();
    expect(toasts.toasts().at(-1)?.title).toBe('Marked as seen');
    vi.spyOn(backend, 'acknowledge').mockRejectedValue(new Error('x'));
    await backend.resetAcknowledgement('w-1');
    const before = toasts.toasts().length;
    await store.acknowledgeAll();
    expect(toasts.toasts().length).toBe(before + 1);
    expect(toasts.toasts().at(-1)?.tone).toBe('danger');
  });

  it('reports downloads and their failures', async () => {
    const { backend, store, toasts } = setup();
    const v = (await backend.getWatch('w-1')).history[0]!;
    await store.download('w-1', v.version);
    expect(toasts.toasts().at(-1)).toMatchObject({ title: 'Download finished' });
    expect(toasts.toasts().at(-1)?.body).toContain(v.name);
    await store.download('w-1', 'gone');
    expect(toasts.toasts().at(-1)).toMatchObject({ tone: 'danger', title: 'Download failed', body: 'That release is gone.' });
  });
});
