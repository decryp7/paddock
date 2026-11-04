import { TestBed } from '@angular/core/testing';
import { BackupReport } from '../core/api.types';
import { BACKEND } from '../core/backend';
import type { MockBackend } from '../core/mock-backend';
import { jobsProviders, mockWithJobs, pump } from '../../testing/job-test-support';
import { BACKUP_POLL_MS, BackupStore } from './backup.store';

const NOW = 1_700_000_000_000;

function setup(options: ConstructorParameters<typeof MockBackend>[0] = {}) {
  const { mock, jobs } = mockWithJobs({ now: () => NOW, ...options });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, jobsProviders(jobs)] });
  return { mock, store: TestBed.inject(BackupStore) };
}
const report = (checking: boolean): BackupReport => ({ statuses: [], checkedAt: checking ? null : NOW, checking, loginPathUsed: true, via: 'local' });

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

  it('checks now as a job: busy meanwhile, one at a time, the new report afterwards', async () => {
    const { store, mock } = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'startJob');
    const first = store.check();
    expect(store.busy()).toBe(true);
    await store.check();
    await first;
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy).toHaveBeenCalledWith('refreshBackup');
    expect(store.busy()).toBe(false);
    expect(store.checkJob()).toBeNull();
    expect(store.report()?.checkedAt).toBe(NOW);
  });

  it('keeps the last report when a check fails and says why', async () => {
    const { store, mock } = setup();
    await store.load();
    vi.spyOn(mock, 'startJob').mockRejectedValueOnce({ code: 'busy', message: 'Checking backups is already running; wait for it or cancel it.' });
    await store.check();
    expect(store.checkError()?.message).toContain('already running');
    expect(store.statuses()).toHaveLength(3);
    expect(store.busy()).toBe(false);
    await store.check();
    expect(store.checkError()).toBeNull();
  });

  describe('a check that takes a while', () => {
    it('shows which tool it is on, and "still working" rather than an error, however long it takes', async () => {
      vi.useFakeTimers();
      const { store, mock } = setup();
      await store.load();
      mock.jobSteps = 4;
      mock.jobStepMs = 500;
      const done = store.check();
      await pump(500);
      expect(store.checkJob()?.snapshot).toMatchObject({ title: 'Checking backups', state: 'running', phase: { id: 'tools', total: 3 } });
      await pump(3000);
      await done;
      expect(store.checking()).toBe(false);
      expect(store.checkJob()).toBeNull();
    });

    it('never gives up on a check that goes quiet: "still working" after the shell\'s threshold, no error', async () => {
      vi.useFakeTimers();
      const { store, mock } = setup();
      await store.load();
      mock.jobsStalled = true;
      const done = store.check();
      await pump(0);
      expect(store.checkJob()?.stalled).toBe(false);
      await pump(10 * 60_000);
      expect(store.checkError()).toBeNull();
      expect(store.checking()).toBe(true);
      expect(store.checkJob()?.stalled).toBe(true);
      void done;
    });

    it('Cancel stops the check at the engine and the earlier report stays', async () => {
      vi.useFakeTimers();
      const { store, mock } = setup();
      await store.load();
      const before = store.report();
      mock.jobSteps = 10;
      mock.jobStepMs = 500;
      const cancel = vi.spyOn(mock, 'cancelJob');
      const done = store.check();
      await pump(500);
      await store.cancelCheck();
      expect(cancel).toHaveBeenCalledTimes(1);
      await pump(0);
      await done;
      expect(store.checking()).toBe(false);
      expect(store.checkError()).toBeNull();
      expect(store.report()).toBe(before);
    });
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

  it('keeps following a check the engine runs in the background for as long as it takes, with no error', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    vi.spyOn(mock, 'backup').mockResolvedValueOnce(report(true)).mockResolvedValue(report(true));
    const done = store.load();
    await vi.advanceTimersByTimeAsync(BACKUP_POLL_MS * 600);
    expect(store.error()).toBeNull();
    expect(store.busy()).toBe(true);
    vi.mocked(mock.backup).mockResolvedValue(report(false));
    await vi.advanceTimersByTimeAsync(BACKUP_POLL_MS);
    await done;
    expect(store.busy()).toBe(false);
    expect(store.error()).toBeNull();
  });
});
