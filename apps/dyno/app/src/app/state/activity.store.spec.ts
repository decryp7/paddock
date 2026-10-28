import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import type { ActivityEntry } from '../core/api.types';
import { ActivityStore, mergeEntry } from './activity.store';

const NOW = 1_700_000_000_000;
const entry = (id: string, at: number, extra: Partial<ActivityEntry> = {}): ActivityEntry => ({
  id, at, kind: 'system', title: id, detail: '', ...extra,
});

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(ActivityStore) };
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
    mock.log('system', 'A note', 'Pushed to the subscribers.');
    await Promise.resolve();
    expect(store.entries().length).toBe(before + 1);
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
