import { JOB_SNAPSHOT_FIXTURES } from '../generated/jobs-fixtures';
import { isActiveJob, isFinishedJob, JOBS_EVENT, parseJobSnapshot, parseJobSnapshots } from './wire';

describe('the jobs wire model', () => {
  it('names the event as paddock_jobs::tauri::EVENT does', () => {
    expect(JOBS_EVENT).toBe('paddock://jobs');
  });

  it('decodes every snapshot shape Rust wrote (round trip)', () => {
    const jobs = parseJobSnapshots(JOB_SNAPSHOT_FIXTURES);
    expect(jobs).toHaveLength(JOB_SNAPSHOT_FIXTURES.length);
    expect(jobs.map((j) => j.state)).toEqual(['queued', 'running', 'running', 'running', 'done', 'failed', 'cancelled', 'cancelled', 'done']);
    const [queued, finding, download, writing, , failed, user, deadline, quiet] = jobs;
    expect(queued?.subject).toBe('3 folders');
    expect(finding).toMatchObject({ state: 'running', phase: { id: 'find', label: 'Finding files', unit: 'items', total: null, done: 12345 } });
    expect(download).toMatchObject({ state: 'running', phase: { unit: 'bytes', total: 118 * 1024 * 1024 } });
    expect(download?.etaMs).toBeGreaterThan(0);
    expect(writing).toMatchObject({ cancellable: false, notCancellableReason: 'Writing the database' });
    expect(failed).toMatchObject({ state: 'failed', message: 'The folder is not readable' });
    expect(user).toMatchObject({ reason: 'user' });
    expect(deadline).toMatchObject({ reason: 'deadline' });
    // The flag Rust wrote for a quiet job survives the parser; the other jobs do not carry it.
    expect(quiet).toMatchObject({ state: 'done', quietOnSuccess: true });
    expect(jobs.filter((j) => j.quietOnSuccess).map((j) => j.id)).toEqual([9]);
    expect(jobs.filter(isActiveJob).map((j) => j.id)).toEqual([1, 2, 3, 4]);
    expect(jobs.filter(isFinishedJob).map((j) => j.id)).toEqual([5, 6, 7, 8, 9]);
  });

  const good = { id: 1, kind: 'scan', title: 'Scan', seq: 1, elapsedMs: 0, cancellable: true, state: 'queued' };

  it('reads quietOnSuccess when it is true, leaves it out otherwise and refuses a non-boolean', () => {
    expect(parseJobSnapshot({ ...good, quietOnSuccess: true })).toMatchObject({ quietOnSuccess: true });
    expect(parseJobSnapshot({ ...good, quietOnSuccess: false })).not.toHaveProperty('quietOnSuccess');
    expect(parseJobSnapshot(good)).not.toHaveProperty('quietOnSuccess');
    expect(parseJobSnapshot({ ...good, quietOnSuccess: 'yes' })).toBeNull();
  });

  it('fills absent optional fields and drops unknown ones', () => {
    expect(parseJobSnapshot({ ...good, extra: 'x' })).toEqual({ ...good, subject: null, etaMs: null, notCancellableReason: null });
    expect(parseJobSnapshot({ ...good, state: 'running' })).toMatchObject({ state: 'running', phase: null });
    expect(parseJobSnapshot({ ...good, state: 'running', phase: null })).toMatchObject({ phase: null });
    expect(parseJobSnapshot({ ...good, state: 'running', phase: { id: 'a', label: 'A', unit: 'bytes', done: 3 } })).toMatchObject({
      phase: { id: 'a', label: 'A', unit: 'bytes', total: null, done: 3 },
    });
  });

  it.each([
    ['not an object', 'x'],
    ['an array', [good]],
    ['null', null],
    ['a negative id', { ...good, id: -1 }],
    ['a fractional seq', { ...good, seq: 1.5 }],
    ['a numeric kind', { ...good, kind: 3 }],
    ['a numeric title', { ...good, title: 3 }],
    ['a numeric subject', { ...good, subject: 3 }],
    ['a missing elapsedMs', { ...good, elapsedMs: undefined }],
    ['a text etaMs', { ...good, etaMs: '1' }],
    ['a text cancellable', { ...good, cancellable: 'yes' }],
    ['a numeric reason', { ...good, notCancellableReason: 1 }],
    ['an unknown state', { ...good, state: 'paused' }],
    ['a failed job without message', { ...good, state: 'failed' }],
    ['a cancelled job with an unknown reason', { ...good, state: 'cancelled', reason: 'bored' }],
    ['a phase that is not an object', { ...good, state: 'running', phase: 'x' }],
    ['a phase with an unknown unit', { ...good, state: 'running', phase: { id: 'a', label: 'A', unit: 'files', done: 0 } }],
    ['a phase without done', { ...good, state: 'running', phase: { id: 'a', label: 'A', unit: 'items' } }],
    ['a phase with a text total', { ...good, state: 'running', phase: { id: 'a', label: 'A', unit: 'items', total: '2', done: 0 } }],
    ['a phase with a numeric id', { ...good, state: 'running', phase: { id: 1, label: 'A', unit: 'items', done: 0 } }],
    ['a phase with a numeric label', { ...good, state: 'running', phase: { id: 'a', label: 1, unit: 'items', done: 0 } }],
  ])('rejects %s', (_what, value) => {
    expect(parseJobSnapshot(value)).toBeNull();
  });

  it('keeps the valid entries of a list and treats a non-list as empty', () => {
    expect(parseJobSnapshots([good, 'junk', { ...good, id: 2 }]).map((j) => j.id)).toEqual([1, 2]);
    expect(parseJobSnapshots({ jobs: [] })).toEqual([]);
  });

  it('keeps a failed message and a cancel reason', () => {
    expect(parseJobSnapshot({ ...good, state: 'failed', message: 'no' })).toMatchObject({ message: 'no' });
    expect(parseJobSnapshot({ ...good, state: 'done' })).toMatchObject({ state: 'done' });
  });
});
