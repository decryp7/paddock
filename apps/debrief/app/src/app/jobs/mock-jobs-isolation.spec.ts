import { LeakDetector, registerSingleton } from '@decryp7/paddock-ui/testing';
import { tinyBackend } from '../../testing/helpers';
import { endMockJobs } from '../../testing/jobs';
import { mockNotifications } from '../notifications/mock-notifications';
import { MockJobCancelled, mockJobs, mockJobsLeftover, mockSteps, resetMockJobs } from './mock-jobs';

// Spec files share one window and one module graph (the Angular builder runs Vitest with `isolate: false`), so the mock
// jobs service is shared by every spec a worker runs. An import job one spec left running (its fake timers were thrown
// away when it ended) used to hold the one import slot forever: the next spec's profile save queued behind it and its
// "re-import did not finish" wait timed out, depending on which spec files a worker happened to run together.
// test-setup.ts gives every test a fresh mock jobs service (`resetMockJobs`) and registers it with the leak detector, which
// names a test that leaves jobs behind. A spec that starts jobs ends them itself (`endMockJobs`). Both halves are proved here
// within one test each, so no test leaves a job on purpose.
describe('mock jobs between tests', () => {
  afterEach(() => vi.useRealTimers());

  it('resetMockJobs gives a fresh service: no job, ids start again, the old one is dropped', () => {
    const old = mockJobs();
    old.start({ kind: 'import', title: 'Left running' }, () => new Promise(() => undefined));
    expect(old.activeCount()).toBe(1);
    expect(old.nextId()).toBe(2);
    resetMockJobs();
    const fresh = mockJobs();
    expect(fresh).not.toBe(old);
    expect(fresh.activeCount()).toBe(0);
    expect(fresh.nextId()).toBe(1);
    expect(mockJobsLeftover()).toBeNull();
  });

  it('the leak detector names a test that leaves a job running, and resets the service', () => {
    // A local detector instance (the page-wide one belongs to the global detector).
    const remove = registerSingleton({ name: 'mock jobs (probe)', leftover: mockJobsLeftover, reset: resetMockJobs });
    try {
      const detector = new LeakDetector();
      detector.begin();
      mockJobs().start({ kind: 'import', title: 'Left running' }, () => new Promise(() => undefined));
      detector.snapshot(vi);
      const found = detector.finish(false);
      // The page-wide entry of the same service reports it too (the first reader resets it): either name carries the count.
      expect(found.some((f) => f.kind === 'singleton' && f.message.includes('holding state: 1 queued or running job(s)'))).toBe(true);
      expect(mockJobsLeftover()).toBeNull();
    } finally {
      remove();
    }
  });

  it('endMockJobs ends what a test started, so nothing is left to name', () => {
    mockJobs().start({ kind: 'export', title: 'Started' }, () => new Promise(() => undefined));
    endMockJobs();
    expect(mockJobsLeftover()).toBeNull();
  });

  it("a re-import is not queued behind an import job left running before a reset", async () => {
    vi.useFakeTimers();
    mockJobs().start({ kind: 'import', title: 'Left running' }, () => new Promise(() => undefined));
    resetMockJobs();
    const backend = tinyBackend();
    const save = backend.saveProfile('name = "Next"\n');
    await vi.advanceTimersByTimeAsync(0);
    const id = await save;
    let state = 'waiting';
    void mockJobs()
      .wait(id)
      .then((o) => (state = o.state));
    // The re-import takes 250 ms of (fake) time; a minute is plenty when nothing blocks the import slot.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(state).toBe('done');
  });

  it('a replaced instance stops ticking and forgets who listened', async () => {
    vi.useFakeTimers();
    const old = mockJobs();
    const heard = vi.fn();
    await old.onChange(heard);
    old.start({ kind: 'import', title: 'Left running' }, () => new Promise(() => undefined));
    heard.mockClear();
    resetMockJobs();
    await vi.advanceTimersByTimeAsync(5000);
    expect(heard).not.toHaveBeenCalled();
    expect(mockJobs()).not.toBe(old);
  });

  // jobs.spec 'asks for attention' failed on main when a Save-problem-report job of an earlier spec finished late and
  // posted "Finished: ..." into the notification centre the next test had just reset.
  it('a job of a replaced instance that finishes later posts no notification', async () => {
    let finish: () => void = () => undefined;
    const old = mockJobs();
    const job = old.start(
      { kind: 'export', title: 'Left running' },
      () => new Promise<{ value: null; result: { type: 'file'; path: string } }>((resolve) => (finish = () => resolve({ value: null, result: { type: 'file', path: '/x.zip' } }))),
    );
    resetMockJobs();
    finish();
    await job.done;
    expect((await mockNotifications().list()).notices).toEqual([]);
  });

  it('a job step still waiting when the instance is replaced leaves no timer and ends quietly', async () => {
    vi.useFakeTimers();
    const job = mockJobs().start({ kind: 'import', title: 'Stepping' }, async (ctx) => {
      await mockSteps(ctx, 1000, 10);
      return { value: null };
    });
    const outcome = job.done.then(() => null, (e: unknown) => e);
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBeGreaterThan(0);
    resetMockJobs();
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBe(0);
    expect(await outcome).toBeInstanceOf(MockJobCancelled);
  });
});
