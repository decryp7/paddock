import { TestBed } from '@angular/core/testing';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { JobsBackend, MemoryJobsBackend, type Unlisten } from './jobs-backend';
import { finishedNotice, JOB_NOTICE_KINDS, JobsStore } from './jobs.store';
import { provideShellJobs } from './provide-jobs';
import { jobsBed, phase, settle, snap } from './spec-helpers';

/** A backend whose list and listen answer when the test says so. */
class ControlledBackend extends JobsBackend {
  list: { resolve: (v: readonly SuJobSnapshot[]) => void; reject: (e: unknown) => void } | null = null;
  handler: ((j: SuJobSnapshot) => void) | null = null;
  unlisten = vi.fn();
  listenResult: Promise<Unlisten> = Promise.resolve(this.unlisten);
  cancelResult: Promise<boolean> = Promise.resolve(true);
  snapshot(): Promise<readonly SuJobSnapshot[]> {
    return new Promise((resolve, reject) => (this.list = { resolve, reject }));
  }
  cancel(): Promise<boolean> {
    return this.cancelResult;
  }
  listen(handler: (j: SuJobSnapshot) => void): Promise<Unlisten> {
    this.handler = handler;
    return this.listenResult;
  }
}

describe('JobsStore', () => {
  it('loads the list: idle, loading, loaded; active first (oldest first), then finished (newest first)', async () => {
    const backend = new ControlledBackend();
    jobsBed({ backend });
    const store = TestBed.inject(JobsStore);
    expect(store.requestStatus()).toBe('loading');
    expect(store.isLoading()).toBe(true);
    backend.list?.resolve([snap(1, 1, 'done'), snap(2, 1, 'queued'), snap(3, 4), snap(4, 1, 'failed')]);
    await settle();
    expect(store.requestStatus()).toBe('loaded');
    expect(store.isLoaded()).toBe(true);
    expect(store.list().map((j) => j.id)).toEqual([2, 3, 4, 1]);
    expect(store.activeCount()).toBe(2);
    expect(store.active().map((j) => j.id)).toEqual([2, 3]);
    expect(store.lead()?.id).toBe(3);
  });

  it('shows a failed load as an error and loads again on retry', async () => {
    const backend = new ControlledBackend();
    jobsBed({ backend });
    const store = TestBed.inject(JobsStore);
    backend.list?.reject(new Error('the engine is gone'));
    await settle();
    expect(store.requestStatus()).toEqual({ error: 'the engine is gone' });
    expect(store.requestError()).toBe('the engine is gone');
    const again = store.load();
    expect(store.requestError()).toBeNull();
    backend.list?.resolve([snap(1, 1)]);
    await again;
    expect(store.requestStatus()).toBe('loaded');
    expect(store.jobs()).toHaveLength(1);
  });

  it.each([
    ['a string', 'refused', 'refused'],
    ['an error-like object', { message: 'denied' }, 'denied'],
    ['anything else', 42, 'The jobs service did not answer.'],
  ])('reports %s as the error message', async (_what, error, text) => {
    const backend = new ControlledBackend();
    jobsBed({ backend });
    const store = TestBed.inject(JobsStore);
    backend.list?.reject(error);
    await settle();
    expect(store.requestError()).toBe(text);
  });

  it('keeps the highest seq per job: an event during the load is not lost', async () => {
    const backend = new ControlledBackend();
    jobsBed({ backend });
    const store = TestBed.inject(JobsStore);
    await settle();
    backend.handler?.(snap(1, 5, 'running', { phase: phase('Reading', 50) }));
    backend.list?.resolve([snap(1, 3), snap(2, 1, 'queued')]);
    await settle();
    expect(store.jobs().map((j) => [j.id, j.seq])).toEqual([
      [1, 5],
      [2, 1],
    ]);
    backend.handler?.(snap(1, 4));
    expect(store.jobs()[0]?.seq).toBe(5);
  });

  it('posts a notice when a job ends, once, but never for the jobs of the first list', async () => {
    const { backend, notices } = jobsBed();
    backend.emit(snap(9, 3, 'failed'));
    const store = TestBed.inject(JobsStore);
    await settle();
    expect(store.jobs()).toHaveLength(1);
    backend.emit(snap(1, 1));
    backend.emit(snap(1, 2, 'done', { title: 'Exporting report.html' }));
    backend.emit(snap(1, 2, 'done', { title: 'Exporting report.html' }));
    backend.emit(snap(2, 1));
    backend.emit(snap(2, 2, 'failed', { title: 'Scanning', message: 'The folder is not readable' }));
    backend.emit(snap(3, 1));
    backend.emit(snap(3, 2, 'cancelled', { reason: 'user' }));
    backend.emit(snap(4, 2, 'cancelled', { title: 'Researching', reason: 'deadline' }));
    backend.emit(snap(4, 3, 'cancelled', { title: 'Researching', reason: 'deadline' }));
    expect(notices).toEqual([
      { level: 'success', kind: JOB_NOTICE_KINDS.finished, message: 'Exporting report.html: done' },
      { level: 'error', kind: JOB_NOTICE_KINDS.failed, message: 'Scanning failed: The folder is not readable' },
      { level: 'warning', kind: JOB_NOTICE_KINDS.failed, message: 'Researching stopped: the time limit was reached' },
    ]);
    expect(finishedNotice(snap(5, 1, 'queued'))).toBeNull();
  });

  it('does not announce the success of a quiet job, but its failure', () => {
    expect(finishedNotice({ ...snap(6, 2, 'done'), quietOnSuccess: true })).toBeNull();
    expect(finishedNotice({ ...snap(6, 2, 'failed', { message: 'The share is offline' }), quietOnSuccess: true })).toMatchObject({ level: 'error' });
    expect(finishedNotice(snap(6, 2, 'done'))).toMatchObject({ level: 'success' });
  });

  it('shows "Still working" for a running job without events, and only while it is quiet', async () => {
    const { backend, clock } = jobsBed({ stillWorkingAfterMs: 3_000 });
    const store = TestBed.inject(JobsStore);
    await settle();
    expect(clock.ticking).toBe(0);
    backend.emit(snap(1, 1, 'queued'));
    backend.emit(snap(2, 1, 'running', { phase: phase('Reading', 1) }));
    await settle();
    expect(clock.ticking).toBe(1);
    expect(clock.periods()[0]).toBe(1_000);
    clock.advance(2_999);
    expect(store.stalledIds().size).toBe(0);
    clock.advance(1);
    expect([...store.stalledIds()]).toEqual([2]);
    backend.emit(snap(2, 2, 'running', { phase: phase('Reading', 2) }));
    expect(store.stalledIds().size).toBe(0);
    clock.advance(10_000);
    expect([...store.stalledIds()]).toEqual([2]);
    backend.emit(snap(2, 3, 'done'));
    await settle();
    expect(store.stalledIds().size).toBe(0);
    expect(clock.ticking).toBe(0);
  });

  it('ticks at least every 250 ms when "Still working" comes early', async () => {
    const { backend, clock } = jobsBed({ stillWorkingAfterMs: 100 });
    TestBed.inject(JobsStore);
    await settle();
    backend.emit(snap(1, 1));
    await settle();
    expect(clock.periods()[0]).toBe(250);
  });

  it('cancels through the backend; a failed request is a notice', async () => {
    const backend = new ControlledBackend();
    const { notices } = jobsBed({ backend });
    const store = TestBed.inject(JobsStore);
    expect(await store.cancel(1)).toBe(true);
    backend.cancelResult = Promise.reject(new Error('denied'));
    expect(await store.cancel(1)).toBe(false);
    expect(notices).toEqual([{ level: 'error', kind: JOB_NOTICE_KINDS.failed, message: 'The job could not be cancelled: denied' }]);
  });

  it('follows a job to its end; while someone waits, its end is not a notice', async () => {
    const { backend, notices } = jobsBed();
    const store = TestBed.inject(JobsStore);
    await settle();
    backend.emit(snap(1, 1));
    const following = store.follow(1);
    const waitingForUnknown = store.follow(7);
    backend.emit(snap(1, 2, 'done'));
    expect((await following.finished).state).toBe('done');
    expect(notices).toEqual([]);

    backend.emit(snap(2, 1));
    store.follow(2).stop();
    backend.emit(snap(2, 2, 'done'));
    expect(notices).toHaveLength(1);

    const already = store.follow(1);
    expect((await already.finished).seq).toBe(2);
    backend.emit(snap(7, 1, 'failed'));
    expect((await waitingForUnknown.finished).state).toBe('failed');
  });

  it('settles a waiter from the list when the job ended before the list came', async () => {
    const backend = new ControlledBackend();
    jobsBed({ backend });
    const store = TestBed.inject(JobsStore);
    const following = store.follow(3);
    backend.list?.resolve([snap(3, 4, 'done')]);
    expect((await following.finished).id).toBe(3);
  });

  it('stops listening when the window goes, also when listening started late', async () => {
    const backend = new ControlledBackend();
    jobsBed({ backend });
    TestBed.inject(JobsStore);
    await settle();
    TestBed.resetTestingModule();
    expect(backend.unlisten).toHaveBeenCalledTimes(1);

    const late = new ControlledBackend();
    let answer: (u: Unlisten) => void = () => undefined;
    late.listenResult = new Promise((r) => (answer = r));
    jobsBed({ backend: late });
    TestBed.inject(JobsStore);
    TestBed.resetTestingModule();
    answer(late.unlisten);
    await settle();
    expect(late.unlisten).toHaveBeenCalledTimes(1);
  });

  it('keeps working without events when listening fails', async () => {
    const backend = new ControlledBackend();
    backend.listenResult = Promise.reject(new Error('no event bridge'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    jobsBed({ backend });
    const store = TestBed.inject(JobsStore);
    backend.list?.resolve([snap(1, 1)]);
    await settle();
    expect(store.jobs()).toHaveLength(1);
    expect(warn).toHaveBeenCalledWith('[paddock-jobs] cannot listen for job events: no event bridge');
    warn.mockRestore();
  });
});

describe('provideShellJobs', () => {
  it.each([0, -1, Number.NaN, Number.POSITIVE_INFINITY])('refuses a "Still working" delay of %s', (ms) => {
    expect(() => provideShellJobs({ backend: () => new MemoryJobsBackend(), stillWorkingAfterMs: ms })).toThrow(RangeError);
  });

  it('defaults the delay to 10 s', async () => {
    const { JOBS_CONFIG } = await import('./jobs-config');
    TestBed.configureTestingModule({ providers: [provideShellJobs({ backend: () => new MemoryJobsBackend() })] });
    expect(TestBed.inject(JOBS_CONFIG).stillWorkingAfterMs).toBe(10_000);
  });
});
