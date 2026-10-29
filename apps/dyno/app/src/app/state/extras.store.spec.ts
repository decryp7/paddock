import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { JOB_POLL_MS } from '../core/job-runner';
import { freezeJobs } from '../../testing/job-test-support';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { ExtrasStore } from './extras.store';
import { SettingsStore } from './settings.store';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(ExtrasStore), notifier: TestBed.inject(Notifier), settings: TestBed.inject(SettingsStore) };
}

describe('ExtrasStore: read-only tabs', () => {
  it('reads a tab the first time it opens and not again until asked', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'sleepBlockers');
    await store.ensure('sleep');
    await store.ensure('sleep');
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.sleep().data?.blockers.length).toBeGreaterThan(0);
    await store.reload('sleep');
    expect(spy).toHaveBeenCalledTimes(2);
  });

  it('loads every tab through its own request', async () => {
    const { store } = setup();
    for (const tab of ['changes', 'sleep', 'boot', 'apps', 'gpu', 'inventory', 'maintenance'] as const) await store.ensure(tab);
    expect(store.changes().data?.changes.length).toBeGreaterThan(0);
    expect(store.boot().data?.boots).toHaveLength(8);
    expect(store.apps().data?.apps.length).toBeGreaterThan(0);
    expect(store.gpu().data?.processes.length).toBeGreaterThan(0);
    expect(store.software().data?.items.length).toBeGreaterThanOrEqual(40);
    expect(store.maintenance().data?.enabled).toBe(true);
    await store.ensure('files');
    expect(store.finder().data).toBeNull();
  });

  it('keeps the engine message per tab, recovers on retry and leaves other tabs alone', async () => {
    const { store, mock } = setup();
    await store.ensure('gpu');
    const spy = vi.spyOn(mock, 'bootAnalysis').mockRejectedValueOnce({ code: 'timeout', message: 'slow' });
    await store.ensure('boot');
    expect(store.boot().error).toEqual({ code: 'timeout', message: 'slow' });
    expect(store.boot().loading).toBe(false);
    expect(store.gpu().error).toBeNull();
    await store.ensure('boot');
    expect(store.boot().error).toBeNull();
    expect(store.boot().data).not.toBeNull();
    // a later failure keeps what was shown
    spy.mockRejectedValueOnce({ code: 'busy', message: 'again' });
    await store.reload('boot');
    expect(store.boot().error?.message).toBe('again');
    expect(store.boot().data).not.toBeNull();
  });

  it('a slower answer for a request that was replaced does not overwrite the newer one', async () => {
    const { store, mock } = setup();
    let release!: () => void;
    const real = mock.changes.bind(mock);
    vi.spyOn(mock, 'changes').mockImplementationOnce((q) => new Promise((res) => (release = () => res(real(q)))));
    const slow = store.loadChanges();
    await store.setHours(24);
    const fresh = store.changes().data!.changes.length;
    release();
    await slow;
    expect(store.changes().data!.changes.length).toBe(fresh);
    expect(store.hours()).toBe(24);
    await store.setHours(24);
  });

  it('does not start a second read of a tab that is loading', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'appUsage');
    await Promise.all([store.ensure('apps'), store.ensure('apps')]);
    expect(spy).toHaveBeenCalledTimes(1);
  });
});

describe('ExtrasStore: file finder', () => {
  it('does nothing when the folder dialog is cancelled, and searches the chosen folder', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'pickFolder').mockResolvedValueOnce(null);
    await store.chooseFolder();
    expect(store.folder()).toBeNull();
    await store.search();
    expect(store.finder().data).toBeNull();
    await store.chooseFolder();
    expect(store.folder()).toBe('/Users/sam/Projects');
    expect(store.finder().data?.large).toHaveLength(8);
  });

  it('shows a dialog failure as the finder error', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'pickFolder').mockRejectedValueOnce({ code: 'internal', message: 'no dialog' });
    await store.chooseFolder();
    expect(store.finder().error?.message).toBe('no dialog');
  });

  it('searches again for a new mode or size, and not for the same one', async () => {
    const { store, mock } = setup();
    await store.chooseFolder();
    const spy = vi.spyOn(mock, 'startJob');
    await store.setMode('both');
    await store.setMinMib(100);
    expect(spy).not.toHaveBeenCalled();
    await store.setMode('duplicates');
    expect(store.finder().data?.large).toEqual([]);
    await store.setMinMib(1024);
    expect(spy).toHaveBeenCalledTimes(2);
    expect(store.finder().data?.duplicates.map((g) => g.bytesEach)).toEqual([expect.any(Number)]);
  });

});

describe('ExtrasStore: the file finder as a job', () => {
  afterEach(() => vi.useRealTimers());

  async function choose(mock: MockBackend, store: InstanceType<typeof ExtrasStore>, polls: number) {
    mock.jobPolls = polls;
    const picking = store.chooseFolder();
    await vi.advanceTimersByTimeAsync(0);
    return picking;
  }

  it('shows the walk and then each phase with its own count, from the first moment, and never goes backwards', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    mock.jobPolls = 5;
    const counts: number[] = [];
    const picking = store.chooseFolder();
    for (let i = 0; i < 8; i++) {
      await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
      const s = store.finderJob()?.snapshot;
      if (s?.state === 'running') expect(s.title).toBe('Finding files');
      if (s?.state === 'running' && s.phase) counts.push(s.phase.done);
    }
    await picking;
    expect(counts.length).toBeGreaterThanOrEqual(3);
    expect(counts.every((c, i) => i === 0 || c >= counts[i - 1]!)).toBe(true);
    expect(store.finder().loading).toBe(false);
    expect(store.finderJob()).toBeNull();
    expect(store.finder().data?.large).toHaveLength(8);
  });

  it('shows "still working" and keeps going far past the old 160 s limit, with no error', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    const picking = choose(mock, store, 6);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    freezeJobs(mock);
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(store.finder().error).toBeNull();
    expect(store.finder().loading).toBe(true);
    expect(store.finderJob()?.stalled).toBe(true);
    void picking;
  });

  it('Cancel tells the engine to stop, keeps the earlier result and offers Search again', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    await choose(mock, store, 1);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS * 2);
    const earlier = store.finder().data;
    expect(earlier).not.toBeNull();
    mock.jobPolls = 10;
    const cancel = vi.spyOn(mock, 'cancelJob');
    const searching = store.setMode('large');
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    await store.cancelSearch();
    expect(cancel).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    await searching;
    expect(store.finderCancelled()).toBe(true);
    expect(store.finder()).toMatchObject({ loading: false, error: null, data: earlier });
    expect(store.finderJob()).toBeNull();
    mock.jobPolls = 1;
    await store.search();
    expect(store.finderCancelled()).toBe(false);
    expect(store.finder().data).not.toBeNull();
  });

  it('a new mode or size stops the search in progress and starts the new one; only the newer answer is shown', async () => {
    vi.useFakeTimers();
    const { store, mock } = setup();
    mock.jobPolls = 6;
    const start = vi.spyOn(mock, 'startJob');
    const cancel = vi.spyOn(mock, 'cancelJob');
    void store.chooseFolder();
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    mock.jobPolls = 1;
    const second = store.setMode('duplicates');
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS * 4);
    await second;
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(start).toHaveBeenCalledTimes(2);
    expect(store.finder().data?.large).toEqual([]);
    expect(store.finder().data?.duplicates.length).toBeGreaterThan(0);
    expect(store.finderCancelled()).toBe(false);
  });

  it('shows the engine message when the job fails, and the limit sentence comes with the result', async () => {
    const { store, mock } = setup();
    await store.chooseFolder();
    vi.spyOn(mock, 'pickFolder').mockResolvedValueOnce('relative/path');
    await store.chooseFolder();
    expect(store.finder().error?.message).toBe('Give the full path of the folder.');
    expect(store.finderJob()).toBeNull();
    vi.spyOn(mock, 'jobResult').mockResolvedValueOnce({ finder: { ...store.finder().data!, limit: { limit: 'files', message: 'Stopped after 500000 files and folders. Choose a smaller folder to see everything in it.' } } });
    vi.spyOn(mock, 'pickFolder').mockResolvedValueOnce('/Users/sam/Projects');
    await store.chooseFolder();
    expect(store.finder().data?.limit?.message).toContain('Choose a smaller folder');
  });
});

describe('ExtrasStore: inventory report and maintenance', () => {
  it('exports once at a time and remembers where it was saved', async () => {
    const { store, mock, notifier } = setup();
    const spy = vi.spyOn(mock, 'saveInventoryReport');
    await Promise.all([store.exportReport('markdown'), store.exportReport('markdown')]);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(store.saved()?.path).toMatch(/dyno-inventory\.md$/);
    expect(notifier.recent().at(-1)?.text).toContain('dyno-inventory.md');
  });

  it('a cancelled save changes nothing and a failed one is kept', async () => {
    const { store, mock } = setup();
    spyOn(mock, 'saveInventoryReport').mockResolvedValueOnce(null);
    await store.exportReport('json');
    expect(store.saved()).toBeNull();
    expect(store.saveError()).toBeNull();
    spyOn(mock, 'saveInventoryReport').mockRejectedValueOnce({ code: 'accessDenied', message: 'read-only folder' });
    await store.exportReport('json');
    expect(store.saveError()?.message).toBe('read-only folder');
    expect(store.exporting()).toBe(false);
  });

  it('saves the windows through the settings and reads the status again', async () => {
    const { store, settings } = setup();
    await store.ensure('maintenance');
    await settings.load();
    const next = structuredClone(settings.settings()!.maintenance);
    next.enabled = false;
    expect(await store.saveMaintenance(next)).toBe(true);
    expect(settings.settings()!.maintenance.enabled).toBe(false);
    expect(store.maintenance().data?.enabled).toBe(false);
    expect(store.maintenanceError()).toBeNull();
  });

  it('loads the settings itself when they are not there yet', async () => {
    const { store, settings } = setup();
    const next = { enabled: false, windows: [] };
    expect(await store.saveMaintenance(next)).toBe(true);
    expect(settings.settings()?.maintenance).toEqual(next);
  });

  it('shows the engine sentence when it refuses the windows', async () => {
    const { store, settings } = setup();
    await settings.load();
    const bad = { enabled: true, windows: [{ days: [], startMinute: 60, endMinute: 120 }] };
    expect(await store.saveMaintenance(bad)).toBe(false);
    expect(store.maintenanceError()?.message).toBe('Pick at least one day for each window.');
    expect(store.maintenanceSaving()).toBe(false);
  });

  it('cannot save when the settings cannot be read', async () => {
    const { store } = setup(['settings']);
    expect(await store.saveMaintenance({ enabled: false, windows: [] })).toBe(false);
    expect(store.maintenanceError()).not.toBeNull();
  });
});

function spyOn<T extends object, K extends keyof T>(o: T, k: K) {
  return vi.spyOn(o as never, k as never) as unknown as ReturnType<typeof vi.spyOn> & { mockResolvedValueOnce(v: unknown): void; mockRejectedValueOnce(v: unknown): void };
}
