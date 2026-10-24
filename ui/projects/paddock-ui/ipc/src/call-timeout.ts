import type { CommandError } from '../generated/ipc';

/**
 * Call timeouts for request/reply commands (plan X28; Pitwall's Proxy wrapper, Pitcrew's per-command table).
 *
 * **Rule 12: the UI never times out a running job.** A command that starts a job answers at once and the job reports
 * through events, so it needs no timeout; a command that waits for a long result must be named in `untimed` (or given a
 * generous limit in `limits`), never left to the default. `callTimeoutProblems` finds names in the policy that the backend
 * does not have (a typo would leave a call on the default).
 */

/** How long an ordinary request/reply call may take before the panel shows an error with Retry. */
export const DEFAULT_CALL_TIMEOUT_MS = 15_000;

/** The timers the wrapper uses (injectable for tests). */
export interface Timers {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

const GLOBAL_TIMERS: Timers = {
  setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
  clearTimeout: (handle) => globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>),
};

/** The policy of one backend. */
export interface CallTimeoutPolicy<B> {
  /** What the backend is called in the message ("The Pitwall server"). */
  readonly subject: string;
  /** The limit of calls not named in `limits` (default {@link DEFAULT_CALL_TIMEOUT_MS}). */
  readonly defaultMs?: number;
  /** Methods that legitimately take longer (or must be quicker), in milliseconds. */
  readonly limits?: Partial<Record<keyof B, number>>;
  /** Methods that are never timed out: they start a job (events report it) or wait on the user (a dialog, an OS prompt). */
  readonly untimed?: readonly (keyof B)[];
  /** Replaces the timers (tests). */
  readonly timers?: Timers;
}

/** The error a call that did not answer in time becomes. */
export function timeoutError(subject: string, method: string, ms: number): CommandError {
  return {
    code: 'timeout',
    message: `${subject} did not answer ${method} within ${Math.round(ms / 1000)} seconds. It may still be starting or busy; try again.`,
    retryable: true,
  };
}

/** Runs `work`, rejecting with `error` when it does not settle in `ms`; the late result is ignored. */
export function withTimeout<T>(work: Promise<T>, ms: number, error: () => CommandError, timers: Timers = GLOBAL_TIMERS): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const handle = timers.setTimeout(() => reject(error()), ms);
    work.then(
      (v) => {
        timers.clearTimeout(handle);
        resolve(v);
      },
      (e: unknown) => {
        timers.clearTimeout(handle);
        reject(e);
      },
    );
  });
}

/** The time one method gets, or `null` when it is never timed out. */
export function limitFor<B>(policy: CallTimeoutPolicy<B>, method: keyof B): number | null {
  if (policy.untimed?.includes(method)) return null;
  return policy.limits?.[method] ?? policy.defaultMs ?? DEFAULT_CALL_TIMEOUT_MS;
}

/**
 * Wraps a backend so that every method returning a promise rejects with a typed `timeout` error when it takes too long.
 * Methods that return anything else (a stream's unsubscribe function, a value) pass through, and so does every method in
 * `untimed`. The wrapped call keeps running; its late result is ignored. There is no per-method table to forget: a new
 * method gets the default.
 */
export function withCallTimeout<B extends object>(inner: B, policy: CallTimeoutPolicy<B>): B {
  const timers = policy.timers ?? GLOBAL_TIMERS;
  return new Proxy(inner, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver) as unknown;
      if (typeof value !== 'function') return value;
      return (...args: unknown[]) => {
        const result = (value as (...a: unknown[]) => unknown).apply(target, args);
        if (!(result instanceof Promise)) return result;
        const ms = limitFor(policy, prop as keyof B);
        if (ms === null) return result;
        return withTimeout(result, ms, () => timeoutError(policy.subject, String(prop), ms), timers);
      };
    },
  });
}

/** What is wrong with a policy for a backend whose methods are `methods`: names that are not methods, and limits that are not positive. */
export function callTimeoutProblems<B>(methods: readonly string[], policy: CallTimeoutPolicy<B>): string[] {
  const known = new Set(methods);
  const problems: string[] = [];
  for (const name of policy.untimed ?? []) if (!known.has(String(name))) problems.push(`untimed names ${String(name)}, which is not a method`);
  for (const [name, ms] of Object.entries(policy.limits ?? {})) {
    if (!known.has(name)) problems.push(`limits names ${name}, which is not a method`);
    if (typeof ms !== 'number' || !(ms > 0)) problems.push(`the limit of ${name} must be a positive number of milliseconds`);
  }
  if (policy.defaultMs !== undefined && !(policy.defaultMs > 0)) problems.push('defaultMs must be a positive number of milliseconds');
  return problems;
}
