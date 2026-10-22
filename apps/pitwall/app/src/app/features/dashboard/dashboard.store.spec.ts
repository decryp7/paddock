import { TestBed } from '@angular/core/testing';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import type { History, HistoryPoint, HistoryRange, PitwallEvent } from '../../core/api.types';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { DashboardStore, withTail } from './dashboard.store';

const HOUR = 3_600_000;
const NOW = 1_800_000_000_000;
const point = (at: number): HistoryPoint => ({ at, ramUsed: 1, ramTotal: 2, vramUsed: null, vramTotal: null, pitwallRss: 0, cpuPercent: 0, promptTps: 0, genTps: 0, p50Ms: null, p95Ms: null, requests: 0 });
const event = (id: number, at: number): PitwallEvent => ({ id, at, kind: 'modelLoaded', severity: 'info', message: `e${id}`, modelId: null, appId: null, reason: null });
const history = (range: HistoryRange, points: HistoryPoint[], events: PitwallEvent[] = []): History => ({ range, stepMs: 60_000, points, requestsByApp: [], requestsByModel: [], events });

describe('withTail', () => {
  it('returns the history points for ranges other than 1h', () => {
    const h = history('24h', [point(1)]);
    expect(withTail(h, [point(5)])).toBe(h.points);
  });
  it('returns the history points when there is no tail or nothing newer', () => {
    const h = history('1h', [point(NOW)]);
    expect(withTail(h, [])).toBe(h.points);
    expect(withTail(h, [point(NOW - 1), point(NOW)])).toBe(h.points);
  });
  it('appends only newer tail points and drops points older than an hour before the newest', () => {
    const h = history('1h', [point(NOW - HOUR), point(NOW - 1000), point(NOW)]);
    const out = withTail(h, [point(NOW - 500), point(NOW + 2000), point(NOW + HOUR - 500)]);
    expect(out.map((p) => p.at)).toEqual([NOW - 1000, NOW, NOW + 2000, NOW + HOUR - 500].filter((a) => a >= NOW + HOUR - 500 - HOUR));
    expect(out[out.length - 1]!.at).toBe(NOW + HOUR - 500);
  });
  it('treats an empty history as ending at time zero', () => {
    const out = withTail(history('1h', []), [point(10), point(20)]);
    expect(out.map((p) => p.at)).toEqual([10, 20]);
  });
});

function setup(backend: PitwallBackend = new MockBackend({ now: () => NOW })) {
  TestBed.configureTestingModule({ providers: [DashboardStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(DashboardStore), live: TestBed.inject(LiveStore), backend };
}
const patchLive = (live: unknown, partial: object) => patchState(live as WritableStateSource<object>, partial);

describe('DashboardStore', () => {
  it('starts on 1 h with nothing loaded', () => {
    const { store } = setup();
    expect(store.range()).toBe('1h');
    expect(store.history()).toBeNull();
    expect(store.points()).toEqual([]);
    expect(store.chartEvents()).toEqual([]);
    expect(store.loading()).toBe(false);
    expect(store.error()).toBeNull();
  });

  it('selects a range, loads its history and caches it per range', async () => {
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'history');
    const p = store.select('24h');
    expect(store.range()).toBe('24h');
    expect(store.loading()).toBe(true);
    await p;
    expect(store.loading()).toBe(false);
    expect(store.history()!.range).toBe('24h');
    expect(store.points().length).toBeGreaterThan(0);
    await store.select('7d');
    await store.select('24h');
    expect(store.history()!.range).toBe('24h');
    expect(spy).toHaveBeenCalledTimes(3);
    expect(Object.keys(store.histories()).sort()).toEqual(['24h', '7d']);
  });

  it('shows a cached range at once while it refreshes', async () => {
    const { store } = setup();
    await store.select('7d');
    await store.select('1h');
    const refresh = store.select('7d');
    expect(store.history()).not.toBeNull();
    await refresh;
  });

  it('records the error and keeps the cached history when a refresh fails', async () => {
    const backend = new MockBackend({ now: () => NOW });
    const { store } = setup(backend);
    await store.select('1h');
    const before = store.history();
    vi.spyOn(backend, 'history').mockRejectedValueOnce({ code: 'serverUnavailable', message: 'down' });
    await store.select('1h');
    expect(store.error()).toEqual({ code: 'serverUnavailable', message: 'down' });
    expect(store.loading()).toBe(false);
    expect(store.history()).toBe(before);
    await store.select('1h');
    expect(store.error()).toBeNull();
  });

  it('reports a failing history with no data', async () => {
    const { store } = setup(new MockBackend({ failing: ['history'] }));
    await store.select('30d');
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.history()).toBeNull();
    expect(store.loading()).toBe(false);
  });

  it('ignores the outcome of a failed request for a range that is no longer selected', async () => {
    const backend = new MockBackend({ now: () => NOW });
    const { store } = setup(backend);
    let reject!: (e: unknown) => void;
    vi.spyOn(backend, 'history').mockImplementationOnce(() => new Promise((_, r) => (reject = r)));
    const slow = store.select('7d');
    await store.select('1h');
    reject({ code: 'serverUnavailable', message: 'late' });
    await slow;
    expect(store.range()).toBe('1h');
    expect(store.error()).toBeNull();
    expect(store.loading()).toBe(false);
  });

  it('caches a late result for the old range without ending the new range loading', async () => {
    const backend = new MockBackend({ now: () => NOW });
    const { store } = setup(backend);
    let resolveSlow!: (h: History) => void;
    let resolveFast!: (h: History) => void;
    vi.spyOn(backend, 'history')
      .mockImplementationOnce(() => new Promise((r) => (resolveSlow = r)))
      .mockImplementationOnce(() => new Promise((r) => (resolveFast = r)));
    const slow = store.select('7d');
    const fast = store.select('24h');
    resolveSlow(history('7d', [point(1)]));
    await slow;
    expect(store.histories()['7d']!.points.length).toBe(1);
    expect(store.loading()).toBe(true);
    resolveFast(history('24h', [point(2)]));
    await fast;
    expect(store.loading()).toBe(false);
  });

  it('continues the 1 h chart with the live tail', async () => {
    const { store, live } = setup();
    patchLive(live, { tail: [] });
    await store.select('1h');
    const last = store.history()!.points.at(-1)!.at;
    const base = store.points().length;
    patchLive(live, { tail: [point(last + 1000), point(last + 2000)] });
    expect(store.points().length).toBeGreaterThanOrEqual(base + 2);
    expect(store.points().at(-1)!.at).toBe(last + 2000);
  });

  it('does not add the tail to longer ranges', async () => {
    const { store, live } = setup();
    await store.select('24h');
    const base = store.points().length;
    patchLive(live, { tail: [point(store.history()!.points.at(-1)!.at + 1000)] });
    expect(store.points().length).toBe(base);
  });

  it('adds live events that are newer than the history and not already in it', async () => {
    const { store, live } = setup();
    await store.select('1h');
    const h = store.history()!;
    const last = h.points.at(-1)!.at;
    const known = h.events[0] ?? event(10_000, last - 1);
    patchLive(live, { events: [event(1, last + 5), event(2, last - 5), { ...known, at: last + 9 }] });
    const ids = store.chartEvents().map((e) => e.id);
    expect(ids).toContain(1);
    expect(ids).not.toContain(2);
    expect(ids.filter((i) => i === known.id).length).toBe(1);
    expect(ids.length).toBe(h.events.length + (h.events.some((e) => e.id === known.id) ? 1 : 2));
  });
});
