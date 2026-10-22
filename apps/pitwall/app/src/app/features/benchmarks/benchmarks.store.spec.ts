import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { BenchmarksStore } from './benchmarks.store';

const NOW = 1_800_000_000_000;
const QWEN = 'qwen3-8b-instruct-q4_k_m';

describe('BenchmarksStore', () => {
  function setup(backend: PitwallBackend = new MockBackend({ now: () => NOW })) {
    TestBed.configureTestingModule({ providers: [BenchmarksStore, { provide: BACKEND, useValue: backend }] });
    return { store: TestBed.inject(BenchmarksStore), toasts: TestBed.inject(Toasts) };
  }

  it('loads results and models', async () => {
    const { store } = setup();
    const p = store.load();
    expect(store.loading()).toBe(true);
    await p;
    expect(store.loading()).toBe(false);
    expect(store.results()).toHaveLength(7);
    expect(store.models().length).toBeGreaterThan(0);
  });

  it('sorts newest first, offers chat models only, maps names and picks the newest result per model', async () => {
    const { store } = setup();
    await store.load();
    const ats = store.sorted().map((r) => r.at);
    expect(ats).toEqual([...ats].sort((a, b) => b - a));
    expect(store.runnable().every((m) => m.capabilities.includes('chat'))).toBe(true);
    expect(store.runnable().map((m) => m.id)).not.toContain('nomic-embed-text-v1.5-f16');
    expect(store.names().get(QWEN)).toBe('Qwen3 8B Instruct');
    const latest = store.latest().get(QWEN)!;
    expect(latest.build).toBe('cuda-13 · b11669');
    expect(latest.at).toBe(NOW - 2 * 86_400_000);
  });

  it('is empty on a fresh install', async () => {
    const { store } = setup(new MockBackend({ empty: true }));
    await store.load();
    expect(store.results()).toEqual([]);
    expect(store.latest().size).toBe(0);
  });

  it('reports a load error', async () => {
    const { store } = setup(new MockBackend({ failing: ['benchmarks'] }));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.loading()).toBe(false);
    expect(store.results()).toEqual([]);
  });

  it('runs a benchmark: marks it running, prepends the result and toasts the speed', async () => {
    const { store, toasts } = setup();
    await store.load();
    const p = store.run(QWEN);
    expect(store.isRunning(QWEN)).toBe(true);
    expect(store.isRunning('other')).toBe(false);
    await p;
    expect(store.isRunning(QWEN)).toBe(false);
    expect(store.results()).toHaveLength(8);
    expect(store.results()[0]).toMatchObject({ modelId: QWEN, at: NOW });
    expect(store.latest().get(QWEN)!.at).toBe(NOW);
    expect(toasts.toasts().at(-1)).toMatchObject({ tone: 'ok' });
    expect(toasts.toasts().at(-1)!.message).toMatch(/^Qwen3 8B Instruct: [\d.]+ tok\/s\.$/);
  });

  it('ignores a second run of the same model while one is in flight', async () => {
    const { store } = setup();
    await store.load();
    const first = store.run(QWEN);
    await store.run(QWEN);
    await first;
    expect(store.results()).toHaveLength(8);
  });

  it('reports a failing run, keeps the results and frees the model', async () => {
    const { store, toasts } = setup(new MockBackend({ failing: ['runBenchmark'] }));
    await store.load();
    await store.run(QWEN);
    expect(store.results()).toHaveLength(7);
    expect(store.isRunning(QWEN)).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
  });

  it('uses the model id in the toast for a model it does not list', async () => {
    const be = new MockBackend({ now: () => NOW });
    const { store, toasts } = setup(be);
    // Not loaded yet: names are empty, so the id is shown.
    await store.run(QWEN);
    expect(toasts.toasts().at(-1)!.message.startsWith(`${QWEN}: `)).toBe(true);
  });

  it('reports an unknown model as not found', async () => {
    const { store, toasts } = setup();
    await store.load();
    await store.run('ghost');
    expect(toasts.toasts().at(-1)).toMatchObject({ tone: 'bad', message: 'No model ghost.' });
  });
});
