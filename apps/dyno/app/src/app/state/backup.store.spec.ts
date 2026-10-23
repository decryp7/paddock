import { TestBed } from '@angular/core/testing';
import { BackupReport } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { BACKUP_POLL_LIMIT, BACKUP_POLL_MS, BackupStore } from './backup.store';

const NOW = 1_700_000_000_000;

function setup(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(BackupStore) };
}
const report = (checking: boolean): BackupReport => ({ statuses: [], checkedAt: checking ? null : NOW, checking, loginPathUsed: true });

describe('BackupStore', () => {
  afterEach(() => vi.useRealTimers());

  it('loads the last report and derives the hero and statuses', async () => {
    const { store } = setup();
    await store.load();
    expect(store.statuses()).toHaveLength(3);
    expect(store.hero()).toBe('Restic backup is out of date'.replace('Restic', 'restic'));
    expect(store.busy()).toBe(false);
  });

  it('says nothing is found as a normal, empty report', async () => {
    const { store } = setup({ backup: 'none' });
    await store.load();
    expect(store.report()?.statuses).toEqual([]);
    expect(store.error()).toBeNull();
    expect(store.hero()).toBe('No backup tool found');
  });

  it('keeps the engine message and recovers on retry', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'backup').mockRejectedValueOnce({ code: 'busy', message: 'later' });
    await store.load();
    expect(store.error()).toEqual({ code: 'busy', message: 'later' });
    await store.load();
    expect(store.error()).toBeNull();
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it('checks now: busy meanwhile, one at a time, the new report afterwards', async () => {
    const { store, mock } = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'refreshBackup');
    const first = store.check();
    expect(store.busy()).toBe(true);
    await store.check();
    await first;
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.busy()).toBe(false);
    expect(store.report()?.checkedAt).toBe(NOW);
  });

  it('keeps the last report when a check fails and says why', async () => {
    const { store, mock } = setup();
    await store.load();
    vi.spyOn(mock, 'refreshBackup').mockRejectedValueOnce({ code: 'timeout', message: 'slow' });
    await store.check();
    expect(store.checkError()?.message).toBe('slow');
    expect(store.statuses()).toHaveLength(3);
    expect(store.busy()).toBe(false);
    await store.check();
    expect(store.checkError()).toBeNull();
  });

  it('follows a check the engine runs in the background until it is done', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'backup').mockResolvedValueOnce(report(true)).mockResolvedValueOnce(report(true)).mockResolvedValueOnce(report(false));
    const done = store.load();
    await vi.advanceTimersByTimeAsync(0);
    expect(store.busy()).toBe(true);
    await vi.advanceTimersByTimeAsync(BACKUP_POLL_MS);
    expect(store.busy()).toBe(true);
    await vi.advanceTimersByTimeAsync(BACKUP_POLL_MS);
    await done;
    expect(spy).toHaveBeenCalledTimes(3);
    expect(store.busy()).toBe(false);
  });

  it('gives up after about 90 s with an error the page can retry', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    vi.spyOn(mock, 'backup').mockResolvedValue(report(true));
    const done = store.load();
    await vi.advanceTimersByTimeAsync(BACKUP_POLL_MS * (BACKUP_POLL_LIMIT + 1));
    await done;
    expect(BACKUP_POLL_MS * BACKUP_POLL_LIMIT).toBe(90_000);
    expect(store.error()?.code).toBe('timeout');
    // a check the person asked for reports the same on its own channel
    vi.spyOn(mock, 'refreshBackup').mockResolvedValue(report(true));
    const check = store.check();
    await vi.advanceTimersByTimeAsync(BACKUP_POLL_MS * (BACKUP_POLL_LIMIT + 1));
    await check;
    expect(store.checkError()?.code).toBe('timeout');
    expect(store.checking()).toBe(false);
  });
});
