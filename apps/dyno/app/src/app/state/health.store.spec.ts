import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';
import { HealthStore } from './health.store';

const NOW = 1_700_000_000_000;

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(HealthStore), notifier: TestBed.inject(Notifier), activity: TestBed.inject(ActivityStore) };
}

describe('HealthStore', () => {
  it('loads the report with last week', async () => {
    const { store } = setup();
    await store.loadReport();
    expect(store.report()?.score).toBeGreaterThan(0);
    expect(store.weekly()).not.toBeNull();
  });
  it('a failing weekly report does not fail the report', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'weeklyReport').mockRejectedValue(new Error('x'));
    await store.loadReport();
    expect(store.report()).not.toBeNull();
    expect(store.weekly()).toBeNull();
    expect(store.reportError()).toBeNull();
  });
  it('a failing report sets reportError', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'health').mockRejectedValue({ code: 'internal', message: 'h' });
    await store.loadReport();
    expect(store.reportError()).toEqual({ code: 'internal', message: 'h' });
    expect(store.report()).toBeNull();
  });
  it('loads startup entries, with errors', async () => {
    const { store, mock } = setup();
    await store.loadStartup();
    expect(store.startupLoaded()).toBe(true);
    expect(store.startup().length).toBeGreaterThan(0);
    vi.spyOn(mock, 'startupOverview').mockRejectedValue({ code: 'busy', message: 's' });
    await store.loadStartup();
    expect(store.startupError()?.message).toBe('s');
  });
  it('keeps what the engine could not read, so the page can say the list is partial', async () => {
    const { store, mock } = setup();
    const real = mock.startupOverview.bind(mock);
    vi.spyOn(mock, 'startupOverview').mockImplementation(async (f) => ({ ...(await real(f)), unavailable: ['Login items: need administrator rights'] }));
    await store.loadStartup();
    expect(store.startupUnavailable()).toEqual(['Login items: need administrator rights']);
  });
  it('asks the engine to filter the startup list and keeps the counts', async () => {
    const { store } = setup();
    await store.loadStartup();
    const all = store.startup().length;
    expect(store.startupCounts().total).toBe(all);
    const { os, signed } = store.startupCounts();
    expect(os).toBeGreaterThan(0);
    await store.loadStartup({ hideOs: true, hideSigned: false });
    expect(store.startup().length).toBe(all - os);
    expect(store.startupCounts()).toEqual({ total: all, os, signed });
    await store.loadStartup({ hideOs: true, hideSigned: true });
    expect(store.startup().length).toBe(all - os - signed);
  });
});
