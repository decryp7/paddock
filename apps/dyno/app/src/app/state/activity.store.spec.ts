import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import type { ActivityEntry } from '../core/api.types';
import { Notifier } from '../shared/notifier';
import { ActivityStore, mergeEntry } from './activity.store';

const NOW = 1_700_000_000_000;
const entry = (id: string, at: number, extra: Partial<ActivityEntry> = {}): ActivityEntry => ({
  id, at, kind: 'manual', title: id, detail: '', undo: 'notUndoable', bytes: null, ...extra,
});

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(ActivityStore), notifier: TestBed.inject(Notifier) };
}

describe('mergeEntry', () => {
  it('replaces an entry with the same id in place', () => {
    const list = [entry('a', 3), entry('b', 2)];
    const out = mergeEntry(list, entry('b', 2, { title: 'new' }));
    expect(out.map((e) => e.title)).toEqual(['a', 'new']);
  });
  it('adds a new entry and sorts newest first', () => {
    const out = mergeEntry([entry('a', 3), entry('b', 1)], entry('c', 2));
    expect(out.map((e) => e.id)).toEqual(['a', 'c', 'b']);
  });
  it('caps at the limit', () => {
    expect(mergeEntry([entry('a', 3), entry('b', 2)], entry('c', 1), 2).map((e) => e.id)).toEqual(['a', 'b']);
  });
});

describe('ActivityStore', () => {
  it('load fills the entries', async () => {
    const { store } = setup();
    await store.load(5);
    expect(store.loaded()).toBe(true);
    expect(store.loading()).toBe(false);
    expect(store.entries().length).toBeGreaterThan(0);
    expect(store.entries().length).toBeLessThanOrEqual(5);
  });
  it('load records a failure', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'activity').mockRejectedValue({ code: 'busy', message: 'later' });
    await store.load();
    expect(store.error()).toEqual({ code: 'busy', message: 'later' });
    expect(store.loaded()).toBe(false);
    expect(store.loading()).toBe(false);
  });
  it('pushed entries arrive through the backend subscription', async () => {
    const { store, mock } = setup();
    await store.load();
    const before = store.entries().length;
    await mock.setProfile('gaming'); // logs an entry and emits it
    await Promise.resolve();
    expect(store.entries().length).toBe(before + 1);
  });
  it('undo marks the original undone, adds the result and toasts', async () => {
    const { store, notifier } = setup();
    await store.load(100);
    const undoable = store.entries().find((e) => e.undo === 'available')!;
    expect(await store.undo(undoable.id)).toBe(true);
    expect(store.entries().find((e) => e.id === undoable.id)?.undo).toBe('undone');
    expect(store.entries()[0]!.title).toContain('Undid');
    expect(store.undoing()).toEqual([]);
    expect(notifier.toasts().at(-1)?.text).toContain('Undid');
  });
  it('undo failure shows an error toast and clears the busy flag', async () => {
    const { store, notifier } = setup();
    expect(await store.undo('missing')).toBe(false);
    expect(store.undoing()).toEqual([]);
    expect(notifier.toasts().at(-1)).toMatchObject({ tone: 'bad' });
  });
  it('unsubscribes on destroy', () => {
    const mock = new MockBackend({ latencyMs: 0 });
    const stop = vi.fn();
    vi.spyOn(mock, 'onActivity').mockReturnValue(stop);
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    TestBed.inject(ActivityStore);
    TestBed.resetTestingModule();
    expect(stop).toHaveBeenCalled();
  });
});
