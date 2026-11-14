import { type Failable } from './mock-options';
import { type MockClock, systemMockClock } from './mock-clock';

export interface MockFailureOptions {
  /** The first N calls of each named command reject with a simulated error. */
  fail: Partial<Record<Failable, number>>;
  /** Answer after the mock's usual delay (default) or at once (a test's `latency: false`). */
  latency?: boolean;
  clock?: MockClock;
}

/** The delay of a simulated failure: the mock's default answer delay. */
const FAILURE_DELAY_MS = 60;

/**
 * Failure injection for `?mockFail=<command>:N`, applied only where the UI receives the backend (`provideBackend`): the first
 * N calls of a named command reject with a plain message string, like a failing Tauri command. The mock backends read the raw
 * mock behind it, never this wrapper, so they cannot use up failures meant for the UI.
 */
export function withMockFailures<T extends object>(backend: T, options: MockFailureOptions): T {
  const left: Partial<Record<string, number>> = { ...options.fail };
  const clock = options.clock ?? systemMockClock;
  const latency = options.latency ?? true;
  return new Proxy(backend, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver);
      if (typeof prop !== 'string' || typeof value !== 'function' || !((left[prop] ?? 0) > 0)) return value;
      return (...args: unknown[]) => {
        const remaining = left[prop] ?? 0;
        if (remaining <= 0) return (value as (...a: unknown[]) => unknown).apply(target, args);
        left[prop] = remaining - 1;
        const message = `Simulated ${prop} failure`;
        return new Promise<never>((_, reject) => (latency ? clock.setTimeout(() => reject(message), FAILURE_DELAY_MS) : reject(message)));
      };
    },
  });
}
