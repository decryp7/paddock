import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import type { LiveUpdate } from '../core/api.types';
import { LiveStore, SPARK_CAPACITY, appendSeries, push, seriesFromHistory } from './live.store';

const NOW = 1_700_000_000_000;
const empty = () => ({ cpu: [], memory: [], diskRead: [], diskWrite: [], netRx: [], netTx: [], gpu: [] });

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(LiveStore) };
}

describe('push', () => {
  it('appends without mutating', () => {
    const a = [1, 2];
    const b = push(a, 3);
    expect(b).toEqual([1, 2, 3]);
    expect(a).toEqual([1, 2]);
  });
  it('keeps only the last SPARK_CAPACITY values', () => {
    let list: number[] = [];
    for (let i = 0; i < SPARK_CAPACITY + 5; i++) list = push(list, i);
    expect(list).toHaveLength(SPARK_CAPACITY);
    expect(list[0]).toBe(5);
    expect(list.at(-1)).toBe(SPARK_CAPACITY + 4);
  });
});

describe('appendSeries', () => {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  it('folds one update into every series', () => {
    const u = mock.liveUpdate('summary');
    const s = appendSeries(empty(), u);
    expect(s.cpu).toEqual([u.system.cpu.total]);
    expect(s.memory[0]).toBeCloseTo((u.system.memory.used / u.system.memory.total) * 100);
    expect(s.diskRead).toEqual([u.system.diskIo.readBps]);
    expect(s.netRx[0]).toBeCloseTo(u.system.network.reduce((a, n) => a + n.rxBps, 0));
    expect(s.gpu[0]).toBe('available' in u.system.gpu ? u.system.gpu.available.utilization : 0);
  });
  it('uses 0 for an unavailable GPU and an unknown memory total', () => {
    const u = structuredClone(mock.liveUpdate('summary')) as LiveUpdate;
    u.system.gpu = { unavailable: 'none' };
    u.system.memory.total = 0;
    const s = appendSeries(empty(), u);
    expect(s.gpu).toEqual([0]);
    expect(s.memory).toEqual([0]);
  });
});

describe('LiveStore', () => {
  it('starts disconnected with the agent starting', () => {
    const { store } = setup();
    expect(store.update()).toBeNull();
    expect(store.connected()).toBe(false);
    expect(store.system()).toBeNull();
    expect(store.overhead()).toBeNull();
    expect(store.agent().state).toBe('starting');
  });
  it('receive stores the update and grows the series', () => {
    const { store, mock } = setup();
    store.receive(mock.liveUpdate('summary'));
    store.receive(mock.liveUpdate('summary'));
    expect(store.connected()).toBe(true);
    expect(store.system()?.at).toBe(Math.floor(NOW / 1000) * 1000);
    expect(store.overhead()).not.toBeNull();
    expect(store.series().cpu).toHaveLength(2);
  });
  it('subscribes on init to live updates and the agent status, and unsubscribes on destroy', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
    const stopLive = vi.fn();
    const stopAgent = vi.fn();
    const onLive = vi.spyOn(mock, 'onLive').mockImplementation((_d, l) => {
      l(mock.liveUpdate('summary'));
      return stopLive;
    });
    vi.spyOn(mock, 'onAgentStatus').mockImplementation((l) => {
      l({ state: 'running', message: null });
      return stopAgent;
    });
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    const store = TestBed.inject(LiveStore);
    expect(onLive).toHaveBeenCalledWith('summary', expect.any(Function));
    expect(store.connected()).toBe(true);
    expect(store.agent().state).toBe('running');
    TestBed.resetTestingModule();
    expect(stopLive).toHaveBeenCalled();
    expect(stopAgent).toHaveBeenCalled();
  });
  it('setAgent replaces the status', () => {
    const { store } = setup();
    store.setAgent({ state: 'stopped', message: 'gone' } as never);
    expect(store.agent()).toEqual({ state: 'stopped', message: 'gone' });
  });
  it('seeds the sparklines from history once, and live samples that arrived first win', async () => {
    const { store, mock } = setup();
    const history = await mock.history('hour', null);
    store.seed(history.points);
    expect(store.series().cpu.length).toBeGreaterThan(1);
    expect(store.series().cpu.length).toBeLessThanOrEqual(SPARK_CAPACITY);
    const seeded = store.series().cpu.length;
    store.seed(history.points.slice(0, 2));
    expect(store.series().cpu).toHaveLength(seeded);
    TestBed.resetTestingModule();
    const { store: live, mock: m2 } = setup();
    live.receive(m2.liveUpdate('summary'));
    live.seed(history.points);
    expect(live.series().cpu).toHaveLength(1);
  });
  it('turns history points into series, keeping only the last minute of them', async () => {
    const { mock } = setup();
    const points = (await mock.history('day', null)).points;
    const series = seriesFromHistory(points);
    expect(series.cpu).toEqual(points.slice(-SPARK_CAPACITY).map((p) => p.cpu));
    expect(series.gpu.every((v) => typeof v === 'number')).toBe(true);
  });
});
