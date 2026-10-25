import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { flush, makeMock } from '../../testing/test-helpers';
import { DashboardStore, EVENTS_PAGE, RELOAD_DEBOUNCE_MS } from './dashboard.store';

function setup() {
  const backend = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  return { backend, store: TestBed.inject(DashboardStore) };
}

describe('DashboardStore', () => {
  afterEach(() => vi.useRealTimers());

  it('loads the dashboard and the first events page', async () => {
    const { store } = setup();
    expect(store.dashboard().value).toBeNull();
    const p = store.load();
    expect(store.dashboard().loading).toBe(true);
    await p;
    expect(store.dashboard().value?.range).toBe('30d');
    expect(store.events().value).toHaveLength(EVENTS_PAGE);
    expect(store.moreEvents()).toBe(true);
  });

  it('changes range and reloads only the dashboard', async () => {
    const { backend, store } = setup();
    await store.setRange('90d');
    expect(store.range()).toBe('90d');
    expect(store.dashboard().value?.bucketsMs).toHaveLength(90);
    expect(backend.calls).toEqual(['dashboard']);
  });

  it('keeps the last value next to the error when a reload fails', async () => {
    const { backend, store } = setup();
    await store.load();
    vi.spyOn(backend, 'dashboard').mockRejectedValueOnce({ code: 'unavailable', message: 'down', problems: [] });
    await store.loadDashboard();
    expect(store.dashboard().error?.message).toBe('down');
    expect(store.dashboard().value).not.toBeNull();
    await store.loadDashboard();
    expect(store.dashboard().error).toBeNull();
  });

  it('pages older events and stops when the last page is short', async () => {
    const { store } = setup();
    await store.loadEvents();
    const first = store.events().value!;
    await store.loadMoreEvents();
    const all = store.events().value!;
    expect(all.length).toBeGreaterThan(first.length);
    expect(all.slice(0, first.length)).toEqual(first);
    expect(new Set(all.map((e) => `${e.atMs}${e.kind}${e.watchId}${e.version}`)).size).toBe(all.length);
    for (let i = 0; i < 20 && store.moreEvents(); i++) await store.loadMoreEvents();
    expect(store.moreEvents()).toBe(false);
  });

  it('starts paging from the beginning when there are no events yet', async () => {
    const { store } = setup();
    await store.loadMoreEvents();
    expect(store.events().value!.length).toBeGreaterThan(0);
  });

  it('keeps events and reports the error when paging fails', async () => {
    const { backend, store } = setup();
    await store.loadEvents();
    const before = store.events().value;
    vi.spyOn(backend, 'events').mockRejectedValueOnce(new Error('boom'));
    await store.loadMoreEvents();
    expect(store.events().error?.message).toBe('boom');
    expect(store.events().value).toEqual(before);
  });

  it('collects bursts of pushes into one reload', async () => {
    vi.useFakeTimers();
    const { backend, store } = setup();
    store.scheduleReload();
    store.scheduleReload();
    store.scheduleReload();
    await vi.advanceTimersByTimeAsync(RELOAD_DEBOUNCE_MS - 1);
    expect(backend.calls).toEqual([]);
    await vi.advanceTimersByTimeAsync(2);
    expect(backend.calls.sort()).toEqual(['dashboard', 'events']);
    vi.useRealTimers();
    await flush();
  });
});
