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
  it('loads tuning and startup, with errors', async () => {
    const { store, mock } = setup();
    await store.loadTuning();
    await store.loadStartup();
    expect(store.tuningLoaded()).toBe(true);
    expect(store.tuning().length).toBeGreaterThan(0);
    expect(store.startupLoaded()).toBe(true);
    expect(store.startup().length).toBeGreaterThan(0);
    vi.spyOn(mock, 'tuning').mockRejectedValue({ code: 'busy', message: 't' });
    vi.spyOn(mock, 'startupEntries').mockRejectedValue({ code: 'busy', message: 's' });
    await store.loadTuning();
    await store.loadStartup();
    expect(store.tuningError()?.message).toBe('t');
    expect(store.startupError()?.message).toBe('s');
  });
  it('applying a tuning suggestion refreshes the list and the score, and offers Undo', async () => {
    const { store, notifier, activity } = setup();
    await store.loadTuning();
    await store.loadReport();
    const before = store.report()!.score;
    const t = store.tuning().find((x) => !x.applied)!;
    expect(await store.setTuning(t.id, true)).toBe(true);
    expect(store.tuning().find((x) => x.id === t.id)?.applied).toBe(true);
    expect(store.busy()).toEqual([]);
    expect(store.report()!.score).toBeGreaterThanOrEqual(before);
    expect(activity.entries()[0]?.title).toBe(t.title);
    const toast = notifier.toasts().at(-1)!;
    expect(toast.action?.label).toBe('Undo');
    toast.action!.run();
    await new Promise((r) => setTimeout(r, 10));
    expect(activity.entries().find((e) => e.title === t.title)?.undo).toBe('undone');
  });
  it('reverting has no Undo button', async () => {
    const { store, notifier } = setup();
    await store.loadTuning();
    const t = store.tuning().find((x) => x.applied);
    if (!t) await store.setTuning(store.tuning()[0]!.id, true);
    const id = (t ?? store.tuning()[0]!).id;
    expect(await store.setTuning(id, false)).toBe(true);
    expect(notifier.toasts().at(-1)?.action).toBeNull();
  });
  it('a failed tuning change toasts and frees the busy flag', async () => {
    const { store, notifier } = setup();
    expect(await store.setTuning('nope', true)).toBe(false);
    expect(store.busy()).toEqual([]);
    expect(notifier.toasts().at(-1)).toMatchObject({ tone: 'bad' });
  });
  it('marks the entry busy while the change is in flight', async () => {
    const { store, mock } = setup();
    let release!: () => void;
    vi.spyOn(mock, 'applyTuning').mockImplementation(() => new Promise((_r, rej) => (release = () => rej({ code: 'busy', message: 'b' }))));
    const p = store.setTuning('x', true);
    expect(store.busy()).toEqual(['x']);
    release();
    await p;
    expect(store.busy()).toEqual([]);
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
