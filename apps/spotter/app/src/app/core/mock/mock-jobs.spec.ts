import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { MockJobs } from './mock-jobs';

describe('MockJobs', () => {
  it('announces queued, each step and the end as snapshots with numeric ids and rising seq', async () => {
    const jobs = new MockJobs(0);
    const seen: SuJobSnapshot[] = [];
    jobs.connect((s) => seen.push(s));
    const out = await jobs.run('sp.download', 'Downloading', async (tick) => {
      await tick('downloading', 'Downloading', 'bytes', 5, 10);
      return 'done';
    });
    expect(out).toBe('done');
    expect(seen.map((s) => s.state)).toEqual(['queued', 'running', 'done']);
    expect(seen.every((s) => s.id === 1 && s.kind === 'sp.download' && s.title === 'Downloading' && s.cancellable)).toBe(true);
    expect(seen.map((s) => s.seq)).toEqual([1, 2, 3]);
    expect(seen[1]).toMatchObject({ state: 'running', phase: { id: 'downloading', label: 'Downloading', unit: 'bytes', done: 5, total: 10 } });
    const second: SuJobSnapshot[] = [];
    jobs.connect((s) => second.push(s));
    await jobs.run('sp.check', 'Checking', async (tick) => tick('checking', 'Checking', 'count', 1, null));
    expect(second[0]?.id, 'ids count up from 1').toBe(2);
    expect(second[1]).toMatchObject({ phase: { unit: 'items', total: null } });
    expect(seen).toHaveLength(3);
  });

  it('ends as cancelled when cancelled between steps and as failed when the work throws', async () => {
    const jobs = new MockJobs(0);
    const seen: SuJobSnapshot[] = [];
    jobs.connect((s) => seen.push(s));
    await expect(
      jobs.run('sp.download', 'Downloading', async (tick) => {
        jobs.cancel(1);
        await tick('downloading', 'Downloading', 'bytes', 1, 2);
      }),
    ).rejects.toMatchObject({ code: 'cancelled' });
    expect(seen.at(-1)).toMatchObject({ state: 'cancelled', reason: 'user' });
    await expect(jobs.run('sp.check', 'Checking', () => Promise.reject(new Error('boom')))).rejects.toThrow('boom');
    expect(seen.at(-1)).toMatchObject({ state: 'failed', message: 'boom' });
    // The cancel flag does not leak into a later job.
    await expect(jobs.run('sp.download', 'Downloading', async (tick) => tick('downloading', 'Downloading', 'bytes', 0, 1))).resolves.toBeUndefined();
    expect(seen.at(-1)?.state).toBe('done');
  });
});
