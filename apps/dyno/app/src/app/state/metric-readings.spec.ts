import { Injector, runInInjectionContext, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import type { ReadingQuery } from '../core/generated/widgets';
import { MockBackend } from '../core/mock-backend';
import { LiveStore } from './live.store';
import { MetricReadings, type NamedQuery } from './metric-readings';

const q = (metric: ReadingQuery['metric'], warning: number | null = null): ReadingQuery => ({ metric, thresholds: { warning, critical: null } });
const named = (key: string, query: ReadingQuery): NamedQuery => ({ key, query });

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
    const out = runInInjectionContext(TestBed.inject(Injector), () => TestBed.inject(MetricReadings).follow(() => [named('a', q('cpuTotal', 0)), named('b', q('fan'))]));
    TestBed.tick();
    await vi.waitFor(() => expect(out().size).toBe(2));
    expect(spy).toHaveBeenCalledWith([q('cpuTotal', 0), q('fan')]);
    expect(out().get('a')!.tone).toBe('warn');
    expect(out().get('b')!.value).toBeNull();
  });

  it('asks again on a new live update and when the queries change, and sends nothing for none', async () => {
    const { mock, live } = setup();
    const spy = vi.spyOn(mock, 'metricReadings');
    const queries = signal<NamedQuery[]>([]);
    const out = runInInjectionContext(TestBed.inject(Injector), () => TestBed.inject(MetricReadings).follow(() => queries()));
    TestBed.tick();
    await Promise.resolve();
    expect(spy).not.toHaveBeenCalled();
    queries.set([named('m', q('memory'))]);
    TestBed.tick();
    await vi.waitFor(() => expect(out().size).toBe(1));
    const calls = spy.mock.calls.length;
    live.receive(mock.liveUpdate('summary'));
    TestBed.tick();
    await vi.waitFor(() => expect(spy.mock.calls.length).toBeGreaterThan(calls));
  });

  it('keeps the last readings when the engine does not answer', async () => {
    const { mock } = setup();
    const out = runInInjectionContext(TestBed.inject(Injector), () => TestBed.inject(MetricReadings).follow(() => [named('c', q('cpuTotal'))]));
    TestBed.tick();
    await vi.waitFor(() => expect(out().size).toBe(1));
    vi.spyOn(mock, 'metricReadings').mockRejectedValue(new Error('down'));
    TestBed.inject(LiveStore).receive(mock.liveUpdate('summary'));
    TestBed.tick();
    await Promise.resolve();
    expect(out().size).toBe(1);
  });

  it('matches an answer to the widget that asked, whatever the order of the list now', async () => {
    const { mock } = setup();
    const spy = vi.spyOn(mock, 'metricReadings');
    const list = signal<NamedQuery[]>([named('first', q('cpuTotal')), named('second', q('fan'))]);
    const out = runInInjectionContext(TestBed.inject(Injector), () => TestBed.inject(MetricReadings).follow(() => list()));
    TestBed.tick();
    await vi.waitFor(() => expect(out().size).toBe(2));
    list.set([named('second', q('fan')), named('first', q('cpuTotal'))]);
    TestBed.tick();
    await vi.waitFor(() => expect(spy.mock.lastCall![0].map((x) => x.metric)).toEqual(['fan', 'cpuTotal']));
    await vi.waitFor(() => expect(out().get('second')!.value).toBeNull());
    expect(out().get('first')!.value).not.toBeNull();
  });
});
