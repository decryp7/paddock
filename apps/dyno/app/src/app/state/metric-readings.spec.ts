import { Injector, runInInjectionContext, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import type { ReadingQuery } from '../core/generated/widgets';
import { MockBackend } from '../core/mock-backend';
import { LiveStore } from './live.store';
import { MetricReadings } from './metric-readings';

const q = (metric: ReadingQuery['metric'], warning: number | null = null): ReadingQuery => ({ metric, thresholds: { warning, critical: null } });

function setup() {
  const mock = new MockBackend({ latencyMs: 0 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  const live = TestBed.inject(LiveStore);
  live.receive(mock.liveUpdate('summary'));
  return { mock, live };
}

describe('MetricReadings', () => {
  it('asks the engine for the queries and answers in their order, with the engine tone', async () => {
    const { mock } = setup();
    const spy = vi.spyOn(mock, 'metricReadings');
    const out = runInInjectionContext(TestBed.inject(Injector), () => TestBed.inject(MetricReadings).follow(() => [q('cpuTotal', 0), q('fan')]));
    TestBed.tick();
    await vi.waitFor(() => expect(out()).toHaveLength(2));
    expect(spy).toHaveBeenCalledWith([q('cpuTotal', 0), q('fan')]);
    expect(out()[0]!.tone).toBe('warn');
    expect(out()[1]!.value).toBeNull();
  });

  it('asks again on a new live update and when the queries change, and sends nothing for none', async () => {
    const { mock, live } = setup();
    const spy = vi.spyOn(mock, 'metricReadings');
    const queries = signal<ReadingQuery[]>([]);
    const out = runInInjectionContext(TestBed.inject(Injector), () => TestBed.inject(MetricReadings).follow(() => queries()));
    TestBed.tick();
    await Promise.resolve();
    expect(spy).not.toHaveBeenCalled();
    queries.set([q('memory')]);
    TestBed.tick();
    await vi.waitFor(() => expect(out()).toHaveLength(1));
    const calls = spy.mock.calls.length;
    live.receive(mock.liveUpdate('summary'));
    TestBed.tick();
    await vi.waitFor(() => expect(spy.mock.calls.length).toBeGreaterThan(calls));
  });

  it('keeps the last readings when the engine does not answer', async () => {
    const { mock } = setup();
    const out = runInInjectionContext(TestBed.inject(Injector), () => TestBed.inject(MetricReadings).follow(() => [q('cpuTotal')]));
    TestBed.tick();
    await vi.waitFor(() => expect(out()).toHaveLength(1));
    vi.spyOn(mock, 'metricReadings').mockRejectedValue(new Error('down'));
    TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
    TestBed.tick();
    await Promise.resolve();
    expect(out()).toHaveLength(1);
  });
});
