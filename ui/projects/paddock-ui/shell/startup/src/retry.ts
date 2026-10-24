/** How a start-up call is retried (`retryUntilAnswered`). */
export interface RetryOptions {
  /** How long one attempt may take before another is sent (ms). */
  readonly timeoutMs: number;
  /** Attempts in all; after the last one the call waits for whichever attempt answers first, without a deadline. */
  readonly attempts: number;
  /** Calls `fn` once after `ms` unless the returned function is called first (default `setTimeout`; tests pass a manual one). */
  readonly after?: (fn: () => void, ms: number) => () => void;
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
  const after =
    options.after ??
    ((fn: () => void, ms: number) => {
      const handle = setTimeout(fn, ms);
      return () => clearTimeout(handle);
    });
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    let sent = 0;
    let stop: (() => void) | null = null;
    const finish = (): boolean => {
      if (settled) return false;
      settled = true;
      stop?.();
      stop = null;
      return true;
    };
    const attempt = () => {
      stop = null;
      if (settled || sent >= options.attempts) return;
      sent++;
      let p: Promise<T>;
      try {
        p = call();
      } catch (e) {
        p = Promise.reject(e);
      }
      p.then(
        (v) => void (finish() && resolve(v)),
        (e: unknown) => void (finish() && reject(e)),
      );
      // The next attempt, unless this one (or an earlier one) answers first.
      if (!settled && sent < options.attempts) stop = after(attempt, options.timeoutMs);
    };
    attempt();
  });
}
