import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { MAX_WEEKS_AGO, ReportsStore } from './reports.store';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(ReportsStore), notifier: TestBed.inject(Notifier) };
}

describe('ReportsStore', () => {
  it('loads the last seven days and remembers where the newest week ends', async () => {
    const { store } = setup();
    await store.load();
    expect(store.report()?.nothingToReport).toBe(false);
    expect(store.latestEnd()).toBe(store.report()!.to);
    expect(store.loading()).toBe(false);
  });

  it('selects an older week, clamped to what the engine keeps, and keeps the newest end', async () => {
    const { store, mock } = setup();
    await store.load();
    const end = store.latestEnd();
    const spy = vi.spyOn(mock, 'weeklyReport');
    await store.select(2);
    expect(spy).toHaveBeenLastCalledWith(2);
    expect(store.weeksAgo()).toBe(2);
    expect(store.report()!.to).toBeLessThan(end!);
    expect(store.latestEnd()).toBe(end);
    await store.select(400);
    expect(spy).toHaveBeenLastCalledWith(MAX_WEEKS_AGO);
    await store.select(-3.7);
    expect(spy).toHaveBeenLastCalledWith(0);
  });

  it('does not reload the week already shown', async () => {
    const { store, mock } = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'weeklyReport');
    await store.select(0);
    expect(spy).not.toHaveBeenCalled();
  });

  it('an empty week is a report, not an error', async () => {
    const { store } = setup();
    await store.select(6);
    expect(store.error()).toBeNull();
    expect(store.report()?.nothingToReport).toBe(true);
  });

  it('keeps the engine message when loading fails and recovers on retry', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'weeklyReport').mockRejectedValueOnce({ code: 'busy', message: 'later' });
    await store.load();
    expect(store.error()).toEqual({ code: 'busy', message: 'later' });
    expect(store.report()).toBeNull();
    expect(store.loading()).toBe(false);
    spy.mockRestore();
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.report()).not.toBeNull();
  });

  it('ignores a slow answer for a week the person already left', async () => {
    const { store, mock } = setup();
    let release!: () => void;
    const real = mock.weeklyReport.bind(mock);
    vi.spyOn(mock, 'weeklyReport').mockImplementationOnce(
      (w) =>
        new Promise((resolve) => {
          release = () => void real(w).then(resolve);
        }),
    );
    const slow = store.select(3);
    await store.select(1);
    release();
    await slow;
    expect(store.weeksAgo()).toBe(1);
    expect(store.report()!.score).toBe(73);
  });

  it('exports the shown week in the chosen format and toasts the file name', async () => {
    const { store, mock, notifier } = setup();
    await store.select(2);
    await store.export('html');
    expect(mock.savedReports).toEqual([{ format: 'html', weeksAgo: 2 }]);
    expect(store.saved()?.path).toMatch(/\.html$/);
    expect(notifier.recent().at(-1)?.text).toBe('Saved dyno-weekly-report.html');
    expect(store.exporting()).toBeNull();
  });

  it('a cancelled save dialog changes nothing, and a failed one keeps the message', async () => {
    const { store, mock, notifier } = setup();
    vi.spyOn(mock, 'saveReport').mockResolvedValueOnce(null);
    await store.export('markdown');
    expect(store.saved()).toBeNull();
    expect(notifier.recent()).toHaveLength(0);
    vi.spyOn(mock, 'saveReport').mockRejectedValueOnce({ code: 'accessDenied', message: 'no' });
    await store.export('markdown');
    expect(store.saveError()).toEqual({ code: 'accessDenied', message: 'no' });
    expect(store.exporting()).toBeNull();
  });

  it('ignores a second export while one is open', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'saveReport');
    await Promise.all([store.export('markdown'), store.export('html')]);
    expect(spy).toHaveBeenCalledTimes(1);
  });
});
