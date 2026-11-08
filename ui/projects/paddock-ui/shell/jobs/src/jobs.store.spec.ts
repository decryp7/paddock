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

  describe('a failed read followed by job events', () => {
    /** Every snapshot() call waits for the test to answer it; `calls` counts them. */
    class QueuedBackend extends ControlledBackend {
      reads: { resolve: (v: readonly SuJobSnapshot[]) => void; reject: (e: unknown) => void }[] = [];
      override snapshot(): Promise<readonly SuJobSnapshot[]> {
        return new Promise((resolve, reject) => this.reads.push({ resolve, reject }));
      }
    }
    const start = async (stillWorkingAfterMs = 10_000) => {
      const backend = new QueuedBackend();
      const bed = jobsBed({ backend, stillWorkingAfterMs });
      const store = TestBed.inject(JobsStore);
      await vi.waitFor(() => expect(backend.reads).toHaveLength(1));
      backend.reads[0]?.reject(new Error('agent unavailable'));
      await vi.waitFor(() => expect(store.requestError()).toBe('agent unavailable'));
      return { ...bed, backend, store };
    };

    it('clears the error and lists the jobs the event missed, once the re-read succeeds', async () => {
      const { backend, store } = await start();
      backend.handler?.(snap(1, 3, 'running', { phase: phase('Running benchmarks', 1) }));
      expect(backend.reads).toHaveLength(2);
      expect(store.requestError()).toBe('agent unavailable');
      backend.reads[1]?.resolve([snap(1, 2), snap(2, 1)]);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      expect(store.jobs().map((j) => [j.id, j.seq])).toEqual([
        [1, 3],
        [2, 1],
      ]);
    });

    it('starts one re-read for many events during a slow one, and never hides the error meanwhile', async () => {
      const { backend, store } = await start();
      for (let seq = 1; seq <= 5; seq++) backend.handler?.(snap(1, seq));
      expect(backend.reads).toHaveLength(2);
      expect(store.requestError()).toBe('agent unavailable');
      expect(store.isLoading()).toBe(false);
      backend.reads[1]?.resolve([snap(1, 1)]);
      await vi.waitFor(() => expect(store.requestError()).toBeNull());
    });

    it('keeps the error for a read that keeps failing, and re-reads at most once per time limit', async () => {
      const { backend, store, clock } = await start(3_000);
      backend.handler?.(snap(1, 1));
      backend.reads[1]?.reject(new Error('still down'));
      await vi.waitFor(() => expect(store.requestError()).toBe('still down'));
      for (let seq = 2; seq <= 6; seq++) backend.handler?.(snap(1, seq));
      expect(backend.reads).toHaveLength(2);
      clock.advance(3_000);
      backend.handler?.(snap(1, 7));
      expect(backend.reads).toHaveLength(3);
      expect(store.requestError()).toBe('still down');
      expect(store.jobs()).toHaveLength(1);
    });

    /** Fails read `n` and waits until the store has armed its next retry. */
    const failRead = async (backend: QueuedBackend, n: number, clock: { ticking: number }, message = 'still down') => {
      backend.reads[n]?.reject(new Error(message));
      await vi.waitFor(() => expect(clock.ticking).toBe(1));
    };

    it('retries on its own with a growing delay (2 s, 4 s, 8 s, then every 15 s) and keeps the error up without loading', async () => {
      const { backend, store, clock } = await start();
      expect(clock.ticking).toBe(1);
      const steps: [number, number][] = [
        [2_000, 2],
        [4_000, 3],
        [8_000, 4],
        [15_000, 5],
        [15_000, 6],
      ];
      for (const [delay, reads] of steps) {
        clock.advance(delay - 1);
        expect(backend.reads).toHaveLength(reads - 1);
        clock.advance(1);
        expect(backend.reads).toHaveLength(reads);
        expect(store.isLoading()).toBe(false);
        expect(store.requestError()).not.toBeNull();
        await failRead(backend, reads - 1, clock);
        expect(store.requestError()).toBe('still down');
      }
    });

    it('stops retrying once a read succeeds: the error clears and no timer is left', async () => {
      const { backend, store, clock } = await start();
      clock.advance(2_000);
      expect(backend.reads).toHaveLength(2);
      backend.reads[1]?.resolve([snap(1, 1, 'done')]);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      expect(clock.ticking).toBe(0);
      clock.advance(120_000);
      expect(backend.reads).toHaveLength(2);
    });

    it('stops retrying once an event re-read succeeds: no timer is left and no read follows', async () => {
      const { backend, store, clock } = await start();
      expect(clock.ticking).toBe(1);
      // Queued, not running: a running job keeps the "Still working" tick on the same clock.
      backend.handler?.(snap(1, 1, 'queued'));
      expect(backend.reads).toHaveLength(2);
      backend.reads[1]?.resolve([snap(1, 1, 'queued')]);
      await vi.waitFor(() => expect(store.requestStatus()).toBe('loaded'));
      expect(clock.ticking).toBe(0);
      clock.advance(120_000);
      expect(backend.reads).toHaveLength(2);
    });

    it('does not set the same error again while the reads keep failing alike (the alert is not announced twice)', async () => {
      const { backend, store, clock } = await start();
      const first = store.requestStatus();
      clock.advance(2_000);
      await failRead(backend, 1, clock, 'agent unavailable');
      expect(store.requestStatus()).toBe(first);
      clock.advance(4_000);
      await failRead(backend, 2, clock);
      expect(store.requestStatus()).toEqual({ error: 'still down' });
    });

    it('starts the delays again after a manual Retry that fails', async () => {
      const { backend, store, clock } = await start();
      clock.advance(2_000);
      await failRead(backend, 1, clock);
      clock.advance(4_000);
      await failRead(backend, 2, clock);
      void store.load();
      expect(clock.ticking).toBe(0);
      expect(backend.reads).toHaveLength(4);
      await failRead(backend, 3, clock);
      clock.advance(1_999);
      expect(backend.reads).toHaveLength(4);
      clock.advance(1);
      expect(backend.reads).toHaveLength(5);
    });

    it('does not start a second read when the timer fires during an event re-read', async () => {
      const { backend, clock } = await start();
      backend.handler?.(snap(1, 1));
      expect(backend.reads).toHaveLength(2);
      clock.advance(2_000);
      expect(backend.reads).toHaveLength(2);
    });

    it('leaves no timer and starts no read after the store is destroyed', async () => {
      const { backend, clock } = await start();
      expect(clock.ticking).toBe(1);
      TestBed.resetTestingModule();
      expect(clock.ticking).toBe(0);
      clock.advance(120_000);
      expect(backend.reads).toHaveLength(1);
    });

    it('does not arm a timer when a read fails after the store was destroyed', async () => {
      const { backend, clock } = await start();
      clock.advance(2_000);
      expect(backend.reads).toHaveLength(2);
      TestBed.resetTestingModule();
      // Fake timers for the flush only: advancing them by 0 runs every promise continuation of the failed read (not a count of turns).
      vi.useFakeTimers();
      try {
        backend.reads[1]?.reject(new Error('late'));
        await vi.advanceTimersByTimeAsync(0);
      } finally {
        vi.useRealTimers();
      }
      expect(clock.ticking).toBe(0);
    });

    it('drops a stale read that finishes after a newer one (Retry)', async () => {
      const { backend, store } = await start();
      backend.handler?.(snap(1, 1));
      const retry = store.load();
      expect(backend.reads).toHaveLength(3);
      backend.reads[2]?.resolve([snap(1, 5), snap(2, 1)]);
      await retry;
      backend.reads[1]?.reject(new Error('late failure'));
      // A macrotask: every promise continuation of the late read has run once it fires (not a count of turns).
      await new Promise<void>((resolve) => setTimeout(resolve));
      expect(store.requestStatus()).toBe('loaded');
      expect(store.jobs().map((j) => j.seq)).toEqual([5, 1]);
    });
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
