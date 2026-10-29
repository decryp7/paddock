import { TestBed } from '@angular/core/testing';
import type { JobEvent } from './api.types';
import { BACKEND } from './backend';
import { applyJobEvent, JobsService, STILL_WORKING_MS, type JobRow } from './jobs';
import { MockBackend } from './mock-backend';

const started = (jobId: number): JobEvent => ({ kind: 'started', jobId, title: `Job ${jobId}` });
const progress = (jobId: number, over: Partial<Extract<JobEvent, { kind: 'progress' }>> = {}): JobEvent => ({
  kind: 'progress',
  jobId,
  phase: 'Loading the weights',
  done: 1,
  total: null,
  unit: 'items',
  canStopWaiting: false,
  elapsedSecs: 1,
  cancellable: true,
  notCancellableReason: null,
  ...over,
});

describe('applyJobEvent', () => {
  it('adds a row on started, labelled "Starting" until the first progress', () => {
    const rows = applyJobEvent([], started(2), 100);
    expect(rows).toEqual([expect.objectContaining({ id: 2, title: 'Job 2', phase: 'Starting', done: 0, total: null, lastEventAt: 100 })]);
  });

  it('updates the phase and count, and a count never goes backwards within a phase', () => {
    let rows: JobRow[] = applyJobEvent([], started(1), 0);
    rows = applyJobEvent(rows, progress(1, { done: 5, elapsedSecs: 5 }), 1);
    rows = applyJobEvent(rows, progress(1, { done: 3, elapsedSecs: 4 }), 2);
    expect(rows[0]!).toMatchObject({ done: 5, elapsedSecs: 5, lastEventAt: 2 });
  });

  it('starts the count again in a new phase and takes the unit and total of that phase', () => {
    let rows: JobRow[] = applyJobEvent([], started(1), 0);
    rows = applyJobEvent(rows, progress(1, { phase: 'Downloading', unit: 'bytes', done: 900, total: 1000 }), 1);
    rows = applyJobEvent(rows, progress(1, { phase: 'Installing', unit: 'items', done: 1, total: null }), 2);
    expect(rows[0]!).toMatchObject({ phase: 'Installing', unit: 'items', done: 1, total: null });
  });

  it('carries the reason Cancel is off and the Stop waiting offer', () => {
    const rows = applyJobEvent(applyJobEvent([], started(1), 0), progress(1, { cancellable: false, notCancellableReason: 'The OS is asking.', canStopWaiting: true }), 1);
    expect(rows[0]!).toMatchObject({ cancellable: false, notCancellableReason: 'The OS is asking.', canStopWaiting: true });
  });

  it('shows a job whose started event was missed, and keeps jobs apart', () => {
    let rows = applyJobEvent([], progress(7), 0);
    rows = applyJobEvent(rows, started(3), 0);
    expect(rows.map((r) => r.id)).toEqual([3, 7]);
  });

  it.each(['done', 'failed', 'cancelled'] as const)('removes the row on %s', (kind) => {
    const rows = applyJobEvent([], started(1), 0);
    const end: JobEvent =
      kind === 'done' ? { kind, jobId: 1, result: null } : kind === 'failed' ? { kind, jobId: 1, error: { code: 'internal', message: 'x' } } : { kind, jobId: 1, stoppedWaiting: false };
    expect(applyJobEvent(rows, end, 1)).toEqual([]);
  });
});

describe('JobsService', () => {
  afterEach(() => vi.useRealTimers());

  it('follows a job the backend runs, from start to end, and says Still working only after a quiet spell', async () => {
    vi.useFakeTimers();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: new MockBackend() }] });
    const jobs = TestBed.inject(JobsService);
    const backend = TestBed.inject(BACKEND) as MockBackend;
    backend.holdJobs = true;
    const pending = backend.runBenchmark('qwen3.5-4b-q4_k_m');
    expect(jobs.jobs().length).toBe(1);
    const job = jobs.jobs()[0]!;
    expect(jobs.stalled(job)).toBe(false);
    await vi.advanceTimersByTimeAsync(STILL_WORKING_MS);
    expect(jobs.stalled(jobs.jobs()[0]!)).toBe(true);
    await jobs.cancel(job.id);
    await expect(pending).rejects.toMatchObject({ code: 'cancelled' });
    expect(jobs.any()).toBe(false);
  });

  it('cancel marks the job as cancelling; when it does not take effect Stop waiting is offered and ends the job', async () => {
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: new MockBackend() }] });
    const jobs = TestBed.inject(JobsService);
    const backend = TestBed.inject(BACKEND) as MockBackend;
    backend.holdJobs = true;
    void backend.loadModel('gemma-4-12b-it-q4_k_m').catch(() => undefined);
    const id = jobs.jobs()[0]!.id;
    const asked = jobs.cancel(id);
    expect(jobs.jobs()[0]!.cancelling).toBe(true);
    await asked;
    expect(jobs.any()).toBe(false);

    void backend.loadModel('gemma-4-12b-it-q4_k_m').catch(() => undefined);
    const stuck = jobs.jobs()[0]!.id;
    backend.refuseCancel = true;
    await jobs.cancel(stuck);
    expect(jobs.jobs()[0]).toMatchObject({ cancelling: true, canStopWaiting: true });
    await jobs.stopWaiting(stuck);
    expect(jobs.any()).toBe(false);
  });

  it('a refused Cancel (a step that cannot be interrupted) leaves the job running and not cancelling', async () => {
    const events: ((e: JobEvent) => void)[] = [];
    const backend = { jobEvents: (l: (e: JobEvent) => void) => (events.push(l), () => undefined), jobCancel: () => Promise.resolve(false), jobStopWaiting: () => Promise.reject(new Error('x')) };
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
    const jobs = TestBed.inject(JobsService);
    events[0]!(started(5));
    await jobs.cancel(5);
    expect(jobs.jobs()[0]).toMatchObject({ id: 5, cancelling: false });
    await jobs.stopWaiting(5);
    expect(jobs.any()).toBe(true);
  });
});
