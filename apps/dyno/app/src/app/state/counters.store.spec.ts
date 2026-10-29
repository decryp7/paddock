import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { MAX_PINNED, MAX_SETS } from '../features/counters/counters-model';
import { MAX_WIDGETS } from '../features/widgets/widget-model';
import { Notifier } from '../shared/notifier';
import { COUNTERS_POLL_MS, CountersStore } from './counters.store';
import { SettingsStore } from './settings.store';

const NOW = 1_700_000_000_000;
const CPU = '\\Processor Information(_Total)\\% Processor Time';
const QUEUE = '\\System\\Processor Queue Length';
const GONE = '\\Network Interface(USB Ethernet)\\Bytes Sent/sec';

async function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, os: 'windows', fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  const settings = TestBed.inject(SettingsStore);
  await settings.load();
  return { mock, settings, store: TestBed.inject(CountersStore), notifier: TestBed.inject(Notifier) };
}

describe('CountersStore', () => {
  afterEach(() => vi.useRealTimers());

  it('lists the objects and opens one; an unknown one says why', async () => {
    const { store } = await setup();
    await store.loadObjects();
    expect(store.objects()!.objects.map((o) => o.name)).toContain('Memory');
    await store.open('Network Interface');
    expect(store.opened()!.instances).toEqual(['Ethernet', 'Wi-Fi']);
    store.closeObject();
    expect(store.opened()).toBeNull();
    await store.open('Nope');
    expect(store.openError()!.message).toMatch(/no counter object/);
    expect(store.opened()).toBeNull();
  });

  it('reports a failed object list', async () => {
    const { store } = await setup(['counterObjects']);
    await store.loadObjects();
    expect(store.objects()).toBeNull();
    expect(store.objectsError()).not.toBeNull();
  });

  it('searches, goes back to the objects for an empty query and ignores a stale answer', async () => {
    const { store, mock } = await setup();
    await store.search('disk queue');
    expect(store.hits()!.map((h) => h.counter)).toContain('Avg. Disk Queue Length');
    await store.search('   ');
    expect(store.hits()).toBeNull();
    let release!: (h: never[]) => void;
    const slow = new Promise<never[]>((r) => (release = r));
    vi.spyOn(mock, 'counterSearch').mockReturnValueOnce(slow as never).mockResolvedValueOnce([{ path: '\\A\\b', object: 'A', counter: 'b', instance: null, unit: 'count' }]);
    const first = store.search('old');
    await store.search('new');
    release([]);
    await first;
    expect(store.hits()!.map((h) => h.path)).toEqual(['\\A\\b']);
  });

  it('a failed search shows the error and no hits', async () => {
    const { store } = await setup(['counterSearch']);
    await store.search('x');
    expect(store.hits()).toBeNull();
    expect(store.searchError()).not.toBeNull();
  });

  it('charts counters: readings and history arrive, and a counter that is gone says why', async () => {
    const { store } = await setup();
    await store.watch(CPU);
    await store.watch(GONE);
    expect(store.watching()).toEqual([CPU, GONE]);
    expect(store.readings()[CPU]!.value).toHaveProperty('available');
    expect(store.readings()[GONE]!.value).toHaveProperty('unavailable');
    expect(store.history()!.series).toHaveLength(2);
    expect(store.history()!.events.length).toBeGreaterThan(0);
    await store.unwatch(GONE);
    expect(store.watching()).toEqual([CPU]);
    await store.unwatch(CPU);
    expect(store.history()).toBeNull();
    expect(store.charted()).toBe(false);
  });

  it('changing the range asks again; at most 32 counters are charted; setWatching dedups', async () => {
    const { store, mock } = await setup();
    await store.watch(CPU);
    const spy = vi.spyOn(mock, 'counterHistory');
    await store.setMinutes(15);
    expect(spy).toHaveBeenCalledWith([CPU], 15);
    await store.setWatching(Array.from({ length: 40 }, (_, i) => `\\O\\c${i}`));
    expect(store.watching()).toHaveLength(32);
    await store.setWatching([CPU, CPU]);
    expect(store.watching()).toEqual([CPU]);
  });

  it('polling errors are kept and polling continues', async () => {
    const { store } = await setup(['counterValues']);
    await store.watch(CPU);
    expect(store.pollError()).not.toBeNull();
  });

  it('reads while something uses it and stops with the last user', async () => {
    vi.useFakeTimers();
    const { store, mock } = await setup();
    store.track([CPU]);
    const spy = vi.spyOn(mock, 'counterValues');
    const a = store.use();
    const b = store.use();
    await vi.advanceTimersByTimeAsync(COUNTERS_POLL_MS * 2 + 10);
    expect(spy.mock.calls.length).toBeGreaterThanOrEqual(3);
    a();
    a();
    await vi.advanceTimersByTimeAsync(COUNTERS_POLL_MS * 2);
    const n = spy.mock.calls.length;
    expect(n).toBeGreaterThanOrEqual(5);
    b();
    await vi.advanceTimersByTimeAsync(COUNTERS_POLL_MS * 3);
    expect(spy.mock.calls.length).toBe(n);
    expect(store.trail()[CPU]!.length).toBeGreaterThan(1);
  });

  it('track dedups, caps and does not change state when nothing changed', async () => {
    const { store } = await setup();
    store.track([CPU, CPU, QUEUE]);
    expect(store.tracked()).toEqual([CPU, QUEUE]);
    const same = store.tracked();
    store.track([CPU, QUEUE]);
    expect(store.tracked()).toBe(same);
    expect(store.wanted()).toEqual([CPU, QUEUE]);
  });

  it('pins and unpins through settings, so a pin survives a restart; the limit is announced', async () => {
    const { store, settings, notifier } = await setup();
    expect(store.pinned()).toContain(CPU);
    expect(await store.togglePin(QUEUE)).toBe(true);
    expect(settings.settings()!.counters!.pinned).toContain(QUEUE);
    expect(store.isPinned(QUEUE)).toBe(true);
    expect(await store.togglePin(QUEUE)).toBe(true);
    expect(store.isPinned(QUEUE)).toBe(false);
    for (let i = store.pinned().length; i < MAX_PINNED; i++) await store.togglePin(`\\O\\c${i}`);
    expect(store.pinned()).toHaveLength(MAX_PINNED);
    expect(await store.togglePin('\\O\\extra')).toBe(false);
    expect(notifier.recent().at(-1)!.text).toMatch(/up to 16/);
  });

  it('saves, applies, replaces and deletes sets, and offers undo', async () => {
    const { store, notifier } = await setup();
    expect(await store.saveSet('x')).toBe(false);
    await store.setWatching([CPU, QUEUE]);
    expect(await store.saveSet('  Load  ')).toBe(true);
    expect(store.sets().find((s) => s.name === 'Load')!.paths).toEqual([CPU, QUEUE]);
    await store.setWatching([CPU]);
    await store.saveSet('Load');
    expect(store.sets().filter((s) => s.name === 'Load')).toHaveLength(1);
    await store.setWatching([]);
    await store.applySet('Load');
    expect(store.watching()).toEqual([CPU]);
    await store.applySet('Nope');
    expect(store.watching()).toEqual([CPU]);
    expect(await store.deleteSet('Load')).toBe(true);
    expect(store.sets().some((s) => s.name === 'Load')).toBe(false);
    const toast = notifier.recent().at(-1)!;
    expect(toast.action!.label).toBe('Undo');
    toast.action!.run();
    await new Promise((r) => setTimeout(r));
    expect(store.sets().some((s) => s.name === 'Load')).toBe(true);
    expect(await store.saveSet('x'.repeat(49))).toBe(false);
  });

  it('refuses a new set beyond the limit but still replaces an existing one', async () => {
    const { store, notifier } = await setup();
    await store.setWatching([CPU]);
    for (let i = store.sets().length; i < MAX_SETS; i++) await store.saveSet(`set ${i}`);
    expect(store.sets()).toHaveLength(MAX_SETS);
    expect(await store.saveSet('one too many')).toBe(false);
    expect(notifier.recent().at(-1)!.text).toMatch(/up to 12/);
    expect(await store.saveSet('set 1')).toBe(true);
  });

  it('adds a counter to the dashboard as a graph widget, within the widget limit', async () => {
    const { store, settings, notifier } = await setup();
    const before = settings.settings()!.dashboard!.widgets.length;
    expect(await store.addToDashboard(QUEUE)).toBe(true);
    const added = settings.settings()!.dashboard!.widgets.at(-1)!;
    expect(added.kind).toBe('graph');
    expect(added.metric).toEqual({ perfCounter: QUEUE });
    expect(settings.settings()!.dashboard!.widgets).toHaveLength(before + 1);
    const full = settings.settings()!;
    await settings.save({ ...full, dashboard: { ...full.dashboard!, widgets: Array.from({ length: MAX_WIDGETS }, (_, i) => ({ ...added, id: `w${i}` })) } }, null);
    expect(await store.addToDashboard(QUEUE)).toBe(false);
    expect(notifier.recent().at(-1)!.text).toMatch(/up to 32/);
  });

  it('does nothing to settings that are not loaded', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW, os: 'windows' });
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    const store = TestBed.inject(CountersStore);
    expect(await store.togglePin(CPU)).toBe(false);
    expect(await store.addToDashboard(CPU)).toBe(false);
  });
});
