import type { Provider, ProviderToken } from '@angular/core';
import type { RecordedCall } from './mock-calls';

/**
 * A counting seam for budget tests (the bulk-API redesign's G3, PRINCIPLES "Bulk APIs"): the backend an app's stores and components
 * inject, wrapped so every call is recorded. Provided through the app's own DI token (`BACKEND`, `DebriefBackend`, ...), so the code
 * under test runs unchanged and nothing patches `invoke`. `expectBackendCalls` is the one-line assertion, after Django's
 * `assertNumQueries`: N items cost one call, not N.
 *
 * A counter belongs to the test that made it (one per TestBed), so tests that run side by side never share a count. Calls on nested
 * objects (`backend.jobs.cancel`) are counted as `jobs.cancel`; members that are not functions pass through uncounted.
 */
export interface CountingBackend<B> {
  /** The wrapped backend: provide or pass this to the code under test. */
  readonly backend: B;
  /** Every call so far, in order. */
  readonly calls: RecordedCall[];
  /** How many calls so far, of `method` (a dotted path for nested objects) or of all. */
  count(method?: string): number;
  /** Forgets the calls so far. */
  reset(): void;
}

/** Wraps `inner` so every call on it (and on its nested objects) is recorded, then passed through with `this` as `inner`. */
export function countingBackend<B extends object>(inner: B): CountingBackend<B> {
  const calls: RecordedCall[] = [];
  const wrapped = new WeakMap<object, object>();
  const wrap = <T extends object>(target: T, prefix: string): T => {
    const known = wrapped.get(target);
    if (known) return known as T;
    const proxy = new Proxy(target, {
      get(t, prop, receiver) {
        const value = Reflect.get(t, prop, receiver) as unknown;
        if (typeof prop !== 'string') return value;
        if (typeof value === 'function') {
          return (...args: unknown[]) => {
            calls.push({ method: prefix + prop, args });
            return (value as (...a: unknown[]) => unknown).apply(t, args);
          };
        }
        if (typeof value === 'object' && value !== null && !(value instanceof Promise)) return wrap(value, `${prefix}${prop}.`);
        return value;
      },
    });
    wrapped.set(target, proxy);
    return proxy;
  };
  return {
    backend: wrap(inner, ''),
    calls,
    count: (method) => (method === undefined ? calls.length : calls.filter((c) => c.method === method).length),
    reset: () => void calls.splice(0),
  };
}

/** `providers` for `TestBed.configureTestingModule` that give `token` the counted `inner`, and the counter to assert on. */
export function provideCountingBackend<B extends object>(token: ProviderToken<B>, inner: B): { providers: Provider[]; counter: CountingBackend<B> } {
  const counter = countingBackend(inner);
  return { providers: [{ provide: token, useValue: counter.backend }], counter };
}

/**
 * Runs `run` and asserts how many backend calls it made (of `method`, when given): exactly `expected`, or at most `expected.max`.
 * Returns what `run` returned. Calls made before `run` started do not count. The error lists the calls, so a per-item loop is
 * plain to see.
 */
export async function expectBackendCalls<B, T>(counter: CountingBackend<B>, expected: number | { max: number }, run: () => T | Promise<T>, method?: string): Promise<T> {
  const start = counter.calls.length;
  const value = await run();
  const made = counter.calls.slice(start).filter((c) => method === undefined || c.method === method);
  const what = method === undefined ? 'backend call' : `${method} call`;
  const limit = typeof expected === 'number' ? expected : expected.max;
  const ok = typeof expected === 'number' ? made.length === expected : made.length <= expected.max;
  if (!ok) {
    const want = typeof expected === 'number' ? `${limit} ${what}${limit === 1 ? '' : 's'}` : `at most ${limit} ${what}s`;
    throw new Error(`expected ${want}, got ${made.length}: ${made.map((c) => c.method).join(', ') || 'none'}`);
  }
  return value;
}
