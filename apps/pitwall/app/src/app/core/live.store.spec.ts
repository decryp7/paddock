import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from './backend';
import { BACKEND } from './backend';
import { LiveStore } from './live.store';
import { MockBackend } from './mock-backend';
import { Toasts } from './toasts';

function setup(backend: PitwallBackend) {
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(LiveStore), toasts: TestBed.inject(Toasts) };
}

const flush = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

describe('LiveStore', () => {
  it('loads the overview, events and downloads', async () => {
    const { store } = setup(new MockBackend());
    expect(store.memory()).toBeNull();
    expect(store.models()).toEqual([]);
    expect(store.queue()).toBeNull();
    const done = store.load();
    expect(store.loading()).toBe(true);
    await done;
    expect(store.loading()).toBe(false);
    expect(store.models().length).toBe(3);
    expect(store.pairings().length).toBe(1);
    expect(store.events().length).toBeGreaterThan(5);
    expect(store.downloads().length).toBe(2);
    expect(store.server()?.serving).toBe('serving');
    expect(store.paused()).toBe(false);
    expect(store.apps().length).toBe(5);
    expect(store.throughput()?.genTps).toBeGreaterThan(0);
  });

  it('shows a typed error with Retry and keeps nothing stale', async () => {
    const { store } = setup(new MockBackend({ failing: ['overview'] }));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.overview()).toBeNull();
  });

  it('folds pushed updates from one subscription', async () => {
    const mock = new MockBackend();
    const { store } = setup(mock);
    await store.load();
    store.connect();
    store.connect();
    mock.tick();
    expect(store.updates()).toBeGreaterThan(0);
    expect(store.tail().length).toBe(1);
    store.disconnect();
    const before = store.updates();
    mock.tick();
    expect(store.updates()).toBe(before);
  });

  it('runs quick actions with a busy flag and a confirmation', async () => {
    const mock = new MockBackend();
    const { store, toasts } = setup(mock);
    await store.load();
    store.connect();
    const pausing = store.setServing(true);
    expect(store.isBusy()('serving')).toBe(true);
    // A second click while busy is ignored.
    expect(await store.setServing(true)).toBeUndefined();
    await pausing;
    expect(store.paused()).toBe(true);
    await store.setServing(false);
    expect(store.paused()).toBe(false);
    expect(toasts.toasts().map((t) => t.message)).toContain('Serving resumed.');

    await store.releaseMemory();
    expect(store.models().map((m) => m.modelId)).toEqual(['qwen3.5-4b-q4_k_m', 'qwen3-embedding-0.6b-q8_0']);
    expect(toasts.toasts().some((t) => t.message.startsWith('Released'))).toBe(true);
    await store.releaseMemory();
    expect(toasts.toasts().some((t) => t.message.startsWith('Nothing to release'))).toBe(true);

    await store.applyPreset('lowMemory');
    expect(store.overview()!.preset).toBe('lowMemory');

    await store.cancelJob('job-41');
    expect(store.queue()!.running.some((j) => j.id === 'job-41')).toBe(false);
    await store.cancelJob('nope');
    expect(toasts.toasts().some((t) => t.tone === 'bad')).toBe(true);
    store.disconnect();
  });

  it('approves and denies pairings and drives models', async () => {
    const mock = new MockBackend();
    const { store } = setup(mock);
    await store.load();
    const p = store.pairings()[0]!;
    await store.approvePairing(p.id, p.permissions);
    expect(store.pairings()).toEqual([]);
    await store.denyPairing('gone');
    expect(store.pairings()).toEqual([]);

    store.connect();
    await store.loadModel('gemma-4-12b-it-q4_k_m');
    expect(store.models().some((m) => m.modelId === 'gemma-4-12b-it-q4_k_m')).toBe(true);
    await store.pinModel('gemma-4-12b-it-q4_k_m', true);
    expect(store.models().find((m) => m.modelId === 'gemma-4-12b-it-q4_k_m')?.pinned).toBe(true);
    await store.pinModel('gemma-4-12b-it-q4_k_m', false);
    await store.unloadModel('gemma-4-12b-it-q4_k_m');
    expect(store.models().some((m) => m.modelId === 'gemma-4-12b-it-q4_k_m')).toBe(false);
    store.disconnect();
  });

  it('denies a pairing and survives actions before the overview loaded', async () => {
    const { store } = setup(new MockBackend());
    await store.applyPreset('balanced');
    await store.approvePairing('pair-1', { models: null, maxPriority: 'batch', tools: false, extendedApis: false, cacheResponses: false, limits: { requestsPerMin: null, tokensPerDay: null, maxConcurrent: null, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null } });
    await store.load();
    expect(store.pairings()).toEqual([]);
    await flush();
  });

  it('denies pending pairings', async () => {
    const { store } = setup(new MockBackend());
    await store.load();
    await store.denyPairing('pair-1');
    expect(store.pairings()).toEqual([]);
  });
});
