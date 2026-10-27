import type { CommandError } from './api.types';
import type { PitwallBackend } from './backend';

/** How long a call may take before its panel shows an error with Retry instead of waiting forever. */
export const DEFAULT_TIMEOUT_MS = 15_000;

/** Calls that legitimately take long (a model load starts an engine and may wait for the weights). */
const SLOW: Partial<Record<keyof PitwallBackend, number>> = {
  loadModel: 180_000,
  runBenchmark: 300_000,
  searchModels: 30_000,
  repoFiles: 30_000,
  appUpdateStatus: 30_000,
  checkAppUpdate: 30_000,
  // The OS asks the user to approve (UAC, Login Items) and the server then starts.
  installService: 150_000,
  uninstallService: 150_000,
  installAppUpdate: 600_000,
};

export function timeoutError(method: string, ms: number): CommandError {
  return {
    code: 'serverUnavailable',
    message: `The Pitwall server did not answer ${method} within ${Math.round(ms / 1000)} seconds. It may still be starting; try again.`,
  };
}

/**
 * Wraps a backend so that every call returning a promise rejects with a typed error when it takes too long: no panel
 * waits on the server for ever (suite lesson: a first IPC call in a new window can go unanswered). Streaming calls
 * (`chat`, `subscribe`) return functions and pass through untouched. The wrapped call keeps running; its late result is ignored.
 */
export function withTimeout(inner: PitwallBackend, timeoutMs: number = DEFAULT_TIMEOUT_MS): PitwallBackend {
  return new Proxy(inner, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver) as unknown;
      if (typeof value !== 'function') return value;
      return (...args: unknown[]) => {
        const result = (value as (...a: unknown[]) => unknown).apply(target, args);
        if (!(result instanceof Promise)) return result;
        const ms = SLOW[prop as keyof PitwallBackend] ?? timeoutMs;
        return new Promise((resolve, reject) => {
          const timer = setTimeout(() => reject(timeoutError(String(prop), ms)), ms);
          result.then(
            (v) => {
              clearTimeout(timer);
              resolve(v);
            },
            (e: unknown) => {
              clearTimeout(timer);
              reject(e);
            },
          );
        });
      };
    },
  });
}
