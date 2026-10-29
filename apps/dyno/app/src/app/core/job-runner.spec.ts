import { TestBed } from '@angular/core/testing';
import type { JobStatus } from './api.types';
import { BACKEND } from './backend';
import { JOB_POLL_MS, JOB_STALLED_MS, JobRunner, type JobProgress, toSnapshot } from './job-runner';
import { MockBackend } from './mock-backend';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, runner: TestBed.inject(JobRunner) };
}

const status = (over: Partial<JobStatus> = {}): JobStatus => ({
  id: 1,
  kind: 'findFiles',
  title: 'Finding files',
  subject: '/x',
  state: { running: { phase: null } },
  seq: 1,
  elapsedMs: 0,
  etaMs: null,
  cancellable: true,
  notCancellableReason: null,
  ...over,
});

describe('JobRunner', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('reports a phase with its own growing count from the first moment and resolves with the output', async () => {
    const { mock, runner } = setup();
    mock.jobPolls = 4;
    const seen: JobProgress[] = [];
    const run = runner.run({ findFiles: { query: { folder: '/x', mode: 'both', minBytes: 0 } } }, (p) => seen.push(p));
    await vi.advanceTimersByTimeAsync(0);
    // The very first thing the page can show is the job itself, "Starting", not an empty box.
    expect(seen[0]!.snapshot).toMatchObject({ state: 'running', title: 'Finding files', subject: '/x' });
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS * 5);
    const counts = seen.flatMap((p) => (p.snapshot.state === 'running' && p.snapshot.phase ? [p.snapshot.phase.done] : []));
    expect(counts.length).toBeGreaterThanOrEqual(3);
    expect(counts.every((c, i) => i === 0 || c >= counts[i - 1]!)).toBe(true);
    expect(counts.at(-1)).toBeGreaterThan(counts[0]!);
    expect(await run.result).toMatchObject({ finder: { folder: '/x' } });
    expect(seen.at(-1)!.snapshot.state).toBe('done');
    expect(vi.getTimerCount()).toBe(0);
  });

  it('never times a running job out, and says "still working" when nothing changes', async () => {
    const { mock, runner } = setup();
    const running = status({ seq: 5 });
    vi.spyOn(mock, 'startJob').mockResolvedValue(running);
    vi.spyOn(mock, 'jobs').mockResolvedValue([running]);
    const seen: JobProgress[] = [];
    let settled = false;
    const run = runner.run('refreshBackup', (p) => seen.push(p));
    void run.result.then(
      () => (settled = true),
      () => (settled = true),
    );
    await vi.advanceTimersByTimeAsync(JOB_STALLED_MS - JOB_POLL_MS);
    expect(seen.at(-1)!.stalled).toBe(false);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS * 2);
    expect(seen.at(-1)!.stalled).toBe(true);
    // Ten minutes later it is still running: no error, nothing settled.
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(settled).toBe(false);
    expect(seen.at(-1)!.snapshot.state).toBe('running');
    // A change in the job clears the warning.
    const moved = status({ seq: 6, state: { running: { phase: { id: 'tools', label: 'Checking backup tools', unit: 'items', done: 1, total: 3 } } } });
    vi.mocked(mock.jobs).mockResolvedValue([moved]);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    expect(seen.at(-1)!.stalled).toBe(false);
    vi.mocked(mock.jobs).mockResolvedValue([{ ...moved, seq: 7, state: 'done' }]);
    vi.spyOn(mock, 'jobResult').mockResolvedValue({ backup: { statuses: [], checkedAt: 1, checking: false, loginPathUsed: true } });
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    expect(await run.result).toMatchObject({ backup: { checkedAt: 1 } });
  });

  it('Cancel asks the engine to stop that job and the run ends with null', async () => {
    const { mock, runner } = setup();
    mock.jobPolls = 50;
    const cancel = vi.spyOn(mock, 'cancelJob');
    const run = runner.run({ rescanIssues: { query: { days: 14 } } }, () => undefined);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    await run.cancel();
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(cancel.mock.calls[0]![0]).toBe(1);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS);
    expect(await run.result).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('a failed job rejects with the engine message, not a timeout', async () => {
    const { mock, runner } = setup();
    mock.jobPolls = 2;
    const run = runner.run({ findFiles: { query: { folder: 'relative', mode: 'both', minBytes: 0 } } }, () => undefined);
    const caught = run.result.catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(JOB_POLL_MS * 3);
    expect(await caught).toEqual({ code: 'internal', message: 'Give the full path of the folder.' });
  });

  it('a start the engine refuses, or a job it forgot, rejects with a clear message', async () => {
    const { mock, runner } = setup();
    vi.spyOn(mock, 'startJob').mockRejectedValueOnce({ code: 'busy', message: 'Finding files is already running; wait for it or cancel it.' });
    await expect(runner.run('refreshBackup', () => undefined).result).rejects.toMatchObject({ code: 'busy' });
    vi.spyOn(mock, 'startJob').mockResolvedValueOnce(status());
    vi.spyOn(mock, 'jobs').mockResolvedValueOnce([]);
    await expect(runner.run('refreshBackup', () => undefined).result).rejects.toMatchObject({ message: expect.stringContaining('lost track') });
  });

  it('maps every engine state onto the shared job card', () => {
    expect(toSnapshot(status({ state: 'queued' }))).toMatchObject({ state: 'queued', id: 1 });
    expect(toSnapshot(status({ state: 'done' })).state).toBe('done');
    expect(toSnapshot(status({ state: 'cancelled' }))).toMatchObject({ state: 'cancelled', reason: 'user' });
    expect(toSnapshot(status({ state: { failed: { message: 'no' } } }))).toMatchObject({ state: 'failed', message: 'no' });
    expect(toSnapshot(status({ cancellable: false, notCancellableReason: 'reading' }))).toMatchObject({ cancellable: false, notCancellableReason: 'reading' });
  });
});
