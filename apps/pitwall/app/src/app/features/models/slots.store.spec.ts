import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend, type MockOptions } from '../../core/mock-backend';
import { FIT_DELAY_MS, SlotsStore, contextSteps, nearestStep } from './slots.store';

function setup(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [SlotsStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(SlotsStore), backend };
}

const MODEL = 'qwen3.5-9b-q4_k_m';

describe('context steps', () => {
  it('offers sizes up to the trained maximum and finds the nearest', () => {
    expect(contextSteps(32768)).toEqual([2048, 4096, 8192, 16384, 32768]);
    expect(contextSteps(null).at(-1)).toBe(131072);
    expect(contextSteps(1024)).toEqual([1024]);
    expect(contextSteps(100)).toEqual([512]);
    expect(nearestStep([2048, 4096, 8192], 8000)).toBe(2);
    expect(nearestStep([2048, 4096, 8192], 1)).toBe(0);
  });
});

describe('SlotsStore', () => {
  afterEach(() => vi.useRealTimers());

  it('asks the engine and keeps the table', async () => {
    const { store } = setup();
    const done = store.query(MODEL, 8192, 2);
    expect(store.loading()).toBe(true);
    await done;
    expect(store.fit()?.choices.length).toBeGreaterThan(0);
    expect(store.fit()?.slots).toBe(2);
    expect(store.loading()).toBe(false);
  });

  it('shows an error and recovers on the next question', async () => {
    const { store } = setup(new MockBackend({ failing: ['modelFit'] } as MockOptions));
    await store.query(MODEL, null, null);
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.fit()).toBeNull();
    TestBed.resetTestingModule();
    const ok = setup();
    await ok.store.query(MODEL, null, null);
    expect(ok.store.error()).toBeNull();
  });

  it('keeps only the latest of several quick questions', async () => {
    vi.useFakeTimers();
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'modelFit');
    store.schedule(MODEL, 8192, 1);
    store.schedule(MODEL, 8192, 2);
    store.schedule(MODEL, 8192, 4);
    await vi.advanceTimersByTimeAsync(FIT_DELAY_MS + 5);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.fit()?.slots).toBe(4);
  });

  it('ignores the answer of a superseded question', async () => {
    const { store } = setup();
    const first = store.query(MODEL, 8192, 1);
    const second = store.query(MODEL, 8192, 8);
    await Promise.all([first, second]);
    expect(store.fit()?.slots).toBe(8);
  });

  it('resets, cancelling a pending question', async () => {
    vi.useFakeTimers();
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'modelFit');
    store.schedule(MODEL, 8192, 1);
    store.reset();
    await vi.advanceTimersByTimeAsync(FIT_DELAY_MS + 5);
    expect(spy).not.toHaveBeenCalled();
    expect(store.fit()).toBeNull();
    expect(store.modelId()).toBeNull();
  });
});
