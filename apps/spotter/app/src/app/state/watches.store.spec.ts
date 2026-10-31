import { TestBed } from '@angular/core/testing';
import { provideShellTesting, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { flush, makeMock } from '../../testing/test-helpers';
import { WatchesStore } from './watches.store';

function setup(options: Parameters<typeof makeMock>[0] = {}, backend = makeMock(options)) {
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, ...provideShellTesting()] });
  return { backend, store: TestBed.inject(WatchesStore), notices: () => shellPorts().notices };
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

  it('turns successful actions into messages and failures into error messages', async () => {
    const { backend, store, notices } = setup();
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
    expect(shellPorts().messages()).toEqual([
      'Checking every watch',
      'Check finished',
      'Watch paused',
      'Watch resumed',
      'Marked as seen',
      'Recorded 1.0.0 as installed',
      'Installed version cleared',
    ]);
    expect(await store.remove('nope')).toBe(false);
    expect(notices().at(-1)).toMatchObject({ level: 'error', message: 'That did not work. That watch no longer exists.' });
    expect(await store.remove('w-1')).toBe(true);
    expect(backend.calls).toContain('deleteWatch');
  });

  it('marks all unseen watches as seen with one message', async () => {
    const { store } = setup();
    store.start();
    await flush();
    const n = store.unseen().length;
    await store.acknowledgeAll();
    expect(store.unseen()).toHaveLength(0);
    expect(shellPorts().messages()).toEqual([`Marked ${n} watches as seen`]);
  });

  it('says "Marked as seen" for a single watch and reports one error when one fails', async () => {
    const { backend, store, notices } = setup();
    store.start();
    await flush();
    for (const w of store.unseen().slice(1)) await backend.acknowledge(w.id, null);
    expect(store.unseen()).toHaveLength(1);
    await store.acknowledgeAll();
    expect(shellPorts().messages().at(-1)).toBe('Marked as seen');
    vi.spyOn(backend, 'acknowledge').mockRejectedValue(new Error('x'));
    await backend.resetAcknowledgement('w-1');
    const before = notices().length;
    await store.acknowledgeAll();
    expect(notices().length).toBe(before + 1);
    expect(notices().at(-1)?.level).toBe('error');
  });

  it('reports downloads and their failures', async () => {
    const { backend, store, notices } = setup();
    const v = (await backend.getWatch('w-1')).history[0]!;
    await store.download('w-1', v.version);
    expect(notices().at(-1)?.level).toBe('success');
    expect(notices().at(-1)?.message).toMatch(/^Download finished\. .*/);
    expect(notices().at(-1)?.message).toContain(v.name);
    await store.download('w-1', 'gone');
    expect(notices().at(-1)).toMatchObject({ level: 'error', message: 'Download failed. That release is gone.' });
  });

  it('a download the engine cancelled is an info message and keeps the part, not a failure', async () => {
    const { backend, store, notices } = setup();
    vi.spyOn(backend, 'downloadRelease').mockRejectedValue({ code: 'cancelled', message: 'Cancelled.', problems: [] });
    await store.download('w-1', '1.0');
    expect(notices().at(-1)).toMatchObject({ level: 'info' });
    expect(notices().at(-1)?.message).toMatch(/^Download cancelled\. /);
    expect(notices().some((n) => n.level === 'error')).toBe(false);
  });

  it('a long download is never given up on: it ends when the engine says so', async () => {
    const { backend, store, notices } = setup();
    let finish: (path: string) => void = () => undefined;
    vi.spyOn(backend, 'downloadRelease').mockReturnValue(new Promise<string>((r) => (finish = r)));
    const run = store.download('w-1', '1.0');
    await flush(30);
    expect(notices(), 'nothing is reported while it runs').toEqual([]);
    finish('/d/file.zip');
    await run;
    expect(notices().at(-1)).toMatchObject({ level: 'success', message: 'Download finished. /d/file.zip' });
  });

  it('checking every watch asks the backend for one check of all and reports it', async () => {
    const { backend, store } = setup();
    await store.load();
    const spy = vi.spyOn(backend, 'checkNow');
    expect(await store.checkNow(null)).toBe(true);
    expect(spy).toHaveBeenCalledWith(null);
    expect(shellPorts().messages()).toEqual(['Checking every watch']);
  });

  it('a check the engine cancelled says so, and one that fails is an error', async () => {
    const { backend, store, notices } = setup();
    await store.load();
    const spy = vi.spyOn(backend, 'checkNow').mockRejectedValueOnce({ code: 'cancelled', message: 'Cancelled.', problems: [] });
    expect(await store.checkNow('w-1')).toBe(false);
    expect(spy).toHaveBeenCalledWith('w-1');
    expect(notices().at(-1)).toMatchObject({ level: 'info', message: 'Check cancelled' });
    spy.mockRejectedValue({ code: 'unavailable', message: 'down', problems: [] });
    expect(await store.checkNow('w-1')).toBe(false);
    expect(notices().at(-1)).toMatchObject({ level: 'error', message: 'That did not work. down' });
  });
});
