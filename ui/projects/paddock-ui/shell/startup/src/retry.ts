/** How a start-up call is retried (`retryUntilAnswered`). */
export interface RetryOptions {
  /** How long one attempt may take before another is sent (ms). */
  readonly timeoutMs: number;
  /** Attempts in all; after the last one the call waits for whichever attempt answers first, without a deadline. */
  readonly attempts: number;
  /** Timer (tests pass a manual one). */
  readonly setTimer?: (fn: () => void, ms: number) => unknown;
}

/** Per-attempt timeout and attempts of the start-up calls (ADR 0058): 4 s, 4 attempts. */
export const STARTUP_RETRY: RetryOptions = { timeoutMs: 4000, attempts: 4 };

/**
 * Calls `call`, and calls it again whenever an attempt has not answered within `timeoutMs`, up to `attempts` calls: the
 * first IPC of a fresh webview can go unanswered and the content process can be paused for seconds. The first answer of
 * any attempt wins (an earlier attempt that answers late still counts); a rejection is an answer too (the backend
 * replied: retrying would not help) and rejects. There is no overall deadline (rule 12: a running start is never given
 * up on). Only idempotent reads may use this.
 */
export function retryUntilAnswered<T>(call: () => Promise<T>, options: RetryOptions): Promise<T> {
  const setTimer = options.setTimer ?? ((fn: () => void, ms: number) => setTimeout(fn, ms));
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    let sent = 0;
    const attempt = () => {
      if (settled) return;
      sent++;
      let p: Promise<T>;
      try {
        p = call();
      } catch (e) {
        p = Promise.reject(e);
      }
      p.then(
        (v) => {
          if (settled) return;
          settled = true;
          resolve(v);
        },
        (e: unknown) => {
          if (settled) return;
          settled = true;
          reject(e);
        },
      );
      if (sent < options.attempts) setTimer(attempt, options.timeoutMs);
    };
    attempt();
  });
}
