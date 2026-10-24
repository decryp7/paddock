import { retryUntilAnswered } from './retry';

/** A call whose attempts are answered by hand. */
function manual<T>() {
  const pending: { resolve: (v: T) => void; reject: (e: unknown) => void }[] = [];
  const call = vi.fn(() => new Promise<T>((resolve, reject) => pending.push({ resolve, reject })));
  return { call, pending };
}

/** A repeating timer run by hand (no sleeps). */
function timers() {
  const t = {
    fn: null as (() => void) | null,
    stopped: false,
    every: (fn: () => void) => {
      t.fn = fn;
      return () => void (t.stopped = true);
    },
    fire: () => t.fn?.(),
  };
  return t;
}

describe('retryUntilAnswered', () => {
  it('answers with the first reply and sends nothing more', async () => {
    const m = manual<number>();
    const t = timers();
    const result = retryUntilAnswered(m.call, { timeoutMs: 10, attempts: 3, every: t.every });
    m.pending[0]!.resolve(7);
    await expect(result).resolves.toBe(7);
    t.fire();
    expect(m.call).toHaveBeenCalledTimes(1);
    expect(t.stopped).toBe(true);
  });

  it('sends again when an attempt goes unanswered (a lost first IPC), up to the limit', async () => {
    const m = manual<number>();
    const t = timers();
    const result = retryUntilAnswered(m.call, { timeoutMs: 10, attempts: 3, every: t.every });
    t.fire();
    t.fire();
    expect(m.call).toHaveBeenCalledTimes(3);
    expect(t.stopped).toBe(false);
    t.fire();
    expect(m.call).toHaveBeenCalledTimes(3);
    expect(t.stopped, 'no more attempts: the timer stops').toBe(true);
    // The second attempt answers; the late first one does not change the result.
    m.pending[1]!.resolve(2);
    m.pending[0]!.resolve(1);
    await expect(result).resolves.toBe(2);
  });

  it('keeps waiting without a deadline after the last attempt (a paused webview resumes later)', async () => {
    const m = manual<string>();
    const t = timers();
    let done = false;
    const result = retryUntilAnswered(m.call, { timeoutMs: 10, attempts: 2, every: t.every }).then((v) => ((done = true), v));
    t.fire();
    await Promise.resolve();
    expect(done).toBe(false);
    m.pending[0]!.resolve('late');
    await expect(result).resolves.toBe('late');
  });

  it('rejects on the first error reply (the backend answered) and on a call that throws', async () => {
    const m = manual<number>();
    const t = timers();
    const result = retryUntilAnswered(m.call, { timeoutMs: 10, attempts: 3, every: t.every });
    m.pending[0]!.reject(new Error('no'));
    await expect(result).rejects.toThrow('no');
    m.pending[0]!.resolve(1);
    t.fire();
    const thrown = retryUntilAnswered(
      () => {
        throw new Error('sync');
      },
      { timeoutMs: 10, attempts: 1, every: t.every },
    );
    await expect(thrown).rejects.toThrow('sync');
  });

  it('uses real timers by default', async () => {
    vi.useFakeTimers();
    try {
      const m = manual<number>();
      const result = retryUntilAnswered(m.call, { timeoutMs: 100, attempts: 2 });
      await vi.advanceTimersByTimeAsync(100);
      expect(m.call).toHaveBeenCalledTimes(2);
      m.pending[1]!.reject('the second attempt failed');
      await expect(result).rejects.toBe('the second attempt failed');
      m.pending[0]!.reject('ignored');
    } finally {
      vi.useRealTimers();
    }
  });
});
