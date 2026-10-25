import type { JobEvent } from '../api.types';
import { MockJobs } from './mock-jobs';

describe('MockJobs', () => {
  it('announces the steps and a finished end, and stops announcing after unsubscribe', async () => {
    const jobs = new MockJobs(0);
    const seen: JobEvent[] = [];
    const off = jobs.on((e) => seen.push(e));
    const out = await jobs.run('a', 'download', async (tick) => {
      await tick('downloading', 'Downloading', 'bytes', 5, 10);
      return 'done';
    });
    expect(out).toBe('done');
    expect(seen.map((e) => e.state)).toEqual(['running', 'finished']);
    expect(seen[0]?.progress).toEqual({ phase: 'downloading', label: 'Downloading', unit: 'bytes', done: 5, total: 10 });
    off();
    await jobs.run('b', 'check', async () => undefined);
    expect(seen).toHaveLength(2);
  });

  it('ends as cancelled when cancelled between steps and as failed when the work throws', async () => {
    const jobs = new MockJobs(0);
    const seen: JobEvent[] = [];
    jobs.on((e) => seen.push(e));
    await expect(
      jobs.run('c', 'download', async (tick) => {
        jobs.cancel('c');
        await tick('downloading', 'Downloading', 'bytes', 1, 2);
      }),
    ).rejects.toMatchObject({ code: 'cancelled' });
    expect(seen.at(-1)?.state).toBe('cancelled');
    await expect(jobs.run('d', 'check', () => Promise.reject(new Error('boom')))).rejects.toThrow('boom');
    expect(seen.at(-1)?.state).toBe('failed');
    // The cancel flag does not leak into a later job with the same id.
    await expect(jobs.run('c', 'download', async (tick) => tick('downloading', 'Downloading', 'bytes', 0, 1))).resolves.toBeUndefined();
  });
});
