import { TestBed } from '@angular/core/testing';
import type { JobEvent } from './api.types';
import { BACKEND } from './backend';
import fixtureJson from './testdata/job-events.json';
import { parseJobEvent } from './job-wire';
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

  it('ignores an event of a kind it does not know instead of crashing', () => {
    const rows = applyJobEvent([], started(1), 0);
    expect(applyJobEvent(rows, { kind: 'paused', jobId: 1 } as unknown as JobEvent, 1)).toEqual(rows);
  });

  it('a progress event clears the local cancelling flag, so the real phase shows again', () => {
    let rows: JobRow[] = applyJobEvent([], started(1), 0);
    rows = rows.map((r) => ({ ...r, cancelling: true }));
    rows = applyJobEvent(rows, progress(1, { phase: 'Cancel did not take effect', canStopWaiting: true }), 1);
    expect(rows[0]).toMatchObject({ cancelling: false, canStopWaiting: true });
  });

  it('parses every event of the shared wire fixture (also checked by a Rust test)', () => {
    const fixture = fixtureJson as unknown[];
    const parsed = fixture.map(parseJobEvent);
    expect(parsed.every((e) => e !== null)).toBe(true);
    expect(parsed).toEqual(fixture);
    expect(new Set(fixture.map((e) => (e as { kind: string }).kind))).toEqual(new Set(['started', 'progress', 'done', 'failed', 'cancelled']));
  });

  it('parseJobEvent drops anything without a known kind and a numeric job id', () => {
    for (const bad of [null, 'x', 4, {}, { kind: 'progress' }, { kind: 'paused', jobId: 1 }, { kind: 'done', jobId: '1' }, { kind: 'done', jobId: Number.NaN }]) {
      expect(parseJobEvent(bad)).toBeNull();
    }
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
    expect(jobs.jobs()[0]).toMatchObject({ cancelling: false, canStopWaiting: true });
    await jobs.stopWaiting(stuck);
    expect(jobs.any()).toBe(false);
  });

  it('late events of a finished job are ignored, and a Cancel the backend does not know removes the orphaned row', async () => {
    const events: ((e: JobEvent) => void)[] = [];
    const backend = { jobEvents: (l: (e: JobEvent) => void) => (events.push(l), () => undefined), jobCancel: () => Promise.resolve(false), jobStopWaiting: () => Promise.resolve(false) };
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
    const jobs = TestBed.inject(JobsService);
    events[0]!(started(8));
    events[0]!({ kind: 'done', jobId: 8, result: null });
    events[0]!(progress(8));
    expect(jobs.any()).toBe(false);
    events[0]!(started(9));
    await jobs.cancel(9);
    expect(jobs.any()).toBe(false);
  });

  it('a refused Cancel (a step that cannot be interrupted) leaves the job running and not cancelling', async () => {
    const events: ((e: JobEvent) => void)[] = [];
    const backend = { jobEvents: (l: (e: JobEvent) => void) => (events.push(l), () => undefined), jobCancel: () => Promise.resolve(false), jobStopWaiting: () => Promise.reject(new Error('x')) };
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
    const jobs = TestBed.inject(JobsService);
    events[0]!(started(5));
    events[0]!(progress(5, { cancellable: false, notCancellableReason: 'The OS is asking.' }));
    await jobs.cancel(5);
    expect(jobs.jobs()[0]).toMatchObject({ id: 5, cancelling: false });
    await jobs.stopWaiting(5);
    expect(jobs.any()).toBe(true);
  });
});
