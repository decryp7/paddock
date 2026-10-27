import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { BASELINES_REFRESH_MS, BaselinesStore } from './baselines.store';
import { LifespanStore } from './lifespan.store';
import { THROTTLE_REFRESH_MS, ThrottleStore } from './throttle.store';

const NOW = 1_700_000_000_000;

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return mock;
}

describe('ThrottleStore', () => {
  afterEach(() => vi.useRealTimers());

  it('loads the overview and keeps it when a later read fails', async () => {
    const mock = setup();
    const store = TestBed.inject(ThrottleStore);
    await store.load();
    expect(store.overview()!.reading.verdict).toBe('fine');
    expect(store.error()).toBeNull();
    vi.spyOn(mock, 'throttling').mockRejectedValue({ code: 'internal', message: 'No reading' });
    await store.load();
    expect(store.error()?.message).toBe('No reading');
    expect(store.overview()).not.toBeNull();
  });

  it('shows the error when the first read fails', async () => {
    setup(['throttling']);
    const store = TestBed.inject(ThrottleStore);
    await store.load();
    expect(store.overview()).toBeNull();
    expect(store.error()).not.toBeNull();
  });

  it('refreshes every 15 seconds until stopped, and stops when its injector is destroyed', async () => {
    vi.useFakeTimers();
    const mock = setup();
    const spy = vi.spyOn(mock, 'throttling');
    const store = TestBed.inject(ThrottleStore);
    await store.start();
    expect(spy).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(THROTTLE_REFRESH_MS * 2 + 10);
    expect(spy).toHaveBeenCalledTimes(3);
    store.stop();
    await vi.advanceTimersByTimeAsync(THROTTLE_REFRESH_MS * 3);
    expect(spy).toHaveBeenCalledTimes(3);
    await store.start();
    await store.start();
    TestBed.resetTestingModule();
    const n = spy.mock.calls.length;
    await vi.advanceTimersByTimeAsync(THROTTLE_REFRESH_MS * 3);
    expect(spy.mock.calls.length).toBe(n);
  });
});

describe('LifespanStore', () => {
  it('loads the forecasts once and reports a failure', async () => {
    setup();
    const store = TestBed.inject(LifespanStore);
    await store.load();
    expect(store.overview()!.forecasts.map((f) => f.subject)).toEqual(['ssd', 'battery']);
    TestBed.resetTestingModule();
    setup(['lifespan']);
    const failing = TestBed.inject(LifespanStore);
    await failing.load();
    expect(failing.overview()).toBeNull();
    expect(failing.error()).not.toBeNull();
  });
});

describe('BaselinesStore', () => {
  afterEach(() => vi.useRealTimers());

  it('loads the overview and reports a failure', async () => {
    setup();
    const store = TestBed.inject(BaselinesStore);
    await store.load();
    expect(store.overview()!.items.length).toBeGreaterThan(0);
    TestBed.resetTestingModule();
    setup(['baselines']);
    const failing = TestBed.inject(BaselinesStore);
    await failing.load();
    expect(failing.error()).not.toBeNull();
  });

  it('refreshes every minute until stopped and when its injector is destroyed', async () => {
    vi.useFakeTimers();
    const mock = setup();
    const spy = vi.spyOn(mock, 'baselines');
    const store = TestBed.inject(BaselinesStore);
    await store.start();
    await vi.advanceTimersByTimeAsync(BASELINES_REFRESH_MS * 2 + 10);
    expect(spy).toHaveBeenCalledTimes(3);
    store.stop();
    await vi.advanceTimersByTimeAsync(BASELINES_REFRESH_MS * 3);
    expect(spy).toHaveBeenCalledTimes(3);
    await store.start();
    TestBed.resetTestingModule();
    const n = spy.mock.calls.length;
    await vi.advanceTimersByTimeAsync(BASELINES_REFRESH_MS * 3);
    expect(spy.mock.calls.length).toBe(n);
  });
});
