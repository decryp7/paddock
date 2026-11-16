import { settleUntil } from './helpers';

describe('settleUntil', () => {
  it('returns once the condition holds, however many turns that takes', async () => {
    let ready = false;
    setTimeout(() => (ready = true), 30);
    await settleUntil(undefined, () => expect(ready).toBe(true));
    expect(ready).toBe(true);
  });

  it('fails with the condition’s own error when it never holds', async () => {
    await expect(settleUntil(undefined, () => expect('never').toBe('ready'), 50)).rejects.toThrow(/expected 'never' to be 'ready'/);
  });

  it('leaves no timer of its own behind when it times out in the middle of a settle (leak at helpers.ts, 7 of 300 runs)', async () => {
    // Every zero-delay timer settle() arms is made to take 20 ms, so the 5 ms timeout always lands while one is pending.
    const real = globalThis.setTimeout;
    const pending = new Set<unknown>();
    vi.spyOn(globalThis, 'setTimeout').mockImplementation(((fn: () => void, ms?: number, ...rest: unknown[]) => {
      if (ms !== 0) return real(fn, ms, ...rest);
      const handle = real(() => {
        pending.delete(handle);
        fn();
      }, 20);
      pending.add(handle);
      return handle;
    }) as typeof setTimeout);
    await expect(settleUntil(undefined, () => expect('never').toBe('ready'), 5)).rejects.toThrow(/Timed out|expected 'never' to be 'ready'/);
    expect(pending.size).toBe(0);
  });
});
