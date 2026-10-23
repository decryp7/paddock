import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';
import { HealthStore } from './health.store';
import { TuningStore } from './tuning.store';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(TuningStore), notifier: TestBed.inject(Notifier), activity: TestBed.inject(ActivityStore), health: TestBed.inject(HealthStore) };
}

describe('TuningStore', () => {
  it('loads the catalogue, and an error keeps a Retry', async () => {
    const { store, mock } = setup();
    await store.load();
    expect(store.loaded()).toBe(true);
    expect(store.items().length).toBeGreaterThan(0);
    vi.spyOn(mock, 'tuning').mockRejectedValue({ code: 'busy', message: 't' });
    await store.load();
    expect(store.error()?.message).toBe('t');
  });

  it('an engine without tuning gives an empty list, not an error with a Retry', async () => {
    const mock = new MockBackend({ latencyMs: 0 });
    vi.spyOn(mock, 'tuning').mockRejectedValue({ code: 'unsupported', message: 'Tuning is not built yet.' });
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    const store = TestBed.inject(TuningStore);
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.loaded()).toBe(true);
    expect(store.items()).toEqual([]);
  });

  it('applying refreshes the list and the health score, logs it and offers Undo', async () => {
    const { store, notifier, activity, health } = setup();
    await store.load();
    await health.loadReport();
    const before = health.report()!.score;
    const t = store.items().find((x) => !x.applied && !x.blocked)!;
    expect(await store.apply(t.id)).toBe(true);
    expect(store.items().find((x) => x.id === t.id)?.applied).toBe(true);
    expect(store.busy()).toEqual([]);
    expect(health.report()!.score).toBeGreaterThanOrEqual(before);
    expect(activity.entries()[0]?.title).toBe(t.title);
    const toast = notifier.toasts().at(-1)!;
    expect(toast.action?.label).toBe('Undo');
    toast.action!.run();
    await new Promise((r) => setTimeout(r, 10));
    expect(activity.entries().find((e) => e.title === t.title)?.undo).toBe('undone');
  });

  it('reverting has no Undo button, and a setting changed outside needs force', async () => {
    const { store, notifier, mock } = setup();
    await store.load();
    const id = store.items().find((x) => !x.applied && !x.blocked)!.id;
    await store.apply(id);
    mock.changeTuningOutside(id);
    expect(await store.revert(id)).toBe(false);
    expect(notifier.toasts().at(-1)).toMatchObject({ tone: 'bad' });
    expect(await store.revert(id, true)).toBe(true);
    expect(notifier.toasts().at(-1)?.action).toBeNull();
  });

  it('a refused change toasts and frees the busy flag', async () => {
    const { store, notifier } = setup();
    await store.load();
    const admin = store.items().find((x) => x.needsAdmin && !x.applied)!;
    expect(await store.apply(admin.id)).toBe(false);
    expect(await store.apply('nope')).toBe(false);
    expect(store.busy()).toEqual([]);
    expect(notifier.toasts().at(-1)).toMatchObject({ tone: 'bad' });
  });

  it('marks the entry busy while the change is in flight', async () => {
    const { store, mock } = setup();
    let release!: () => void;
    vi.spyOn(mock, 'applyTuning').mockImplementation(() => new Promise((_r, rej) => (release = () => rej({ code: 'busy', message: 'b' }))));
    const p = store.apply('x');
    expect(store.busy()).toEqual(['x']);
    release();
    await p;
    expect(store.busy()).toEqual([]);
  });
});
