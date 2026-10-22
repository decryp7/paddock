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
    vi.spyOn(mock, 'startupEntries').mockRejectedValue({ code: 'busy', message: 's' });
    await store.loadStartup();
    expect(store.startupError()?.message).toBe('s');
  });
  it('changes a startup entry in place', async () => {
    const { store, notifier, activity } = setup();
    await store.loadStartup();
    const e = store.startup()[0]!;
    expect(await store.setStartup(e.id, 'disabled')).toBe(true);
    expect(store.startup()[0]!.state).toBe('disabled');
    expect(activity.entries()[0]?.title).toContain('Disabled');
    expect(notifier.toasts().at(-1)?.tone).toBe('ok');
    expect(store.busy()).toEqual([]);
  });
  it('a failed startup change keeps the list', async () => {
    const { store, notifier } = setup();
    await store.loadStartup();
    const snapshot = structuredClone(store.startup());
    expect(await store.setStartup('ghost', 'disabled')).toBe(false);
    expect(store.startup()).toEqual(snapshot);
    expect(notifier.toasts().at(-1)?.tone).toBe('bad');
  });
});
