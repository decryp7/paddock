import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { BenchStore } from './bench.store';

const NOW = 1_700_000_000_000;

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(BenchStore) };
}

describe('BenchStore', () => {
  afterEach(() => vi.useRealTimers());

  it('loads the saved runs and nothing is running', async () => {
    const { store } = setup();
    await store.load();
    expect(store.overview()?.runs.length).toBe(3);
    expect(store.overview()?.running).toBeNull();
  });

  it('starting is refused without a kind and reported inline', async () => {
    const { store } = setup();
    await store.start({ kinds: [], label: 'x' });
    expect(store.error()?.code).toBe('badRequest');
  });

  it('a run is polled until it finishes and then lands in the history', async () => {
    vi.useFakeTimers();
    const { store } = setup();
    await store.start({ kinds: ['cpu', 'memory'], label: 'Mine' });
    expect(store.overview()?.running?.kinds).toEqual(['cpu', 'memory']);
    expect(store.busy()).toBe(false);
    await vi.advanceTimersByTimeAsync(4000);
    expect(store.overview()?.running).toBeNull();
    expect(store.overview()?.runs[0]?.label).toBe('Mine');
    store.stop();
  });

  it('a second start while one runs is refused with busy', async () => {
    vi.useFakeTimers();
    const { store } = setup();
    await store.start({ kinds: ['cpu'], label: 'a' });
    await store.start({ kinds: ['cpu'], label: 'b' });
    expect(store.error()?.code).toBe('busy');
    await store.cancel();
    expect(store.overview()?.running).toBeNull();
    store.stop();
  });

  it('compares two runs and clears the comparison; deleting an unknown run reports it', async () => {
    const { store } = setup();
    await store.load();
    await store.compare('r2', 'r3');
    expect(store.comparison()?.deltas.some((d) => d.verdict === 'better')).toBe(true);
    store.clearComparison();
    expect(store.comparison()).toBeNull();
    await store.remove('r1');
    expect(store.overview()?.runs.length).toBe(2);
    await store.remove('nope');
    expect(store.error()?.code).toBe('notFound');
    await store.compare('nope', 'r3');
    expect(store.error()?.code).toBe('notFound');
  });

  it('a failing load stops polling and shows the error', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(['benchmarks']) });
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    const store = TestBed.inject(BenchStore);
    await store.load();
    expect(store.error()).not.toBeNull();
    expect(store.overview()).toBeNull();
  });
});
