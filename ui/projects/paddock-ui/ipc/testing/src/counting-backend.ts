import { isSignal, type Provider, type ProviderToken } from '@angular/core';
import type { RecordedCall } from './mock-calls';

/**
 * Counting seams for budget tests (the bulk-API redesign's G3, PRINCIPLES "Bulk APIs"): N items cost one call, not N. Two levels,
 * each with a one-line assertion after Django's `assertNumQueries`:
 *
 * - `countingInvoke(invoke)` counts the IPC call itself, under an app's `tauri-backend.ts` (which takes its `invoke` or bridge as a
 *   constructor argument): the real cost of a backend method, including a `Promise.all(ids.map((id) => invoke(...)))` inside it.
 *   Assert with `expectInvokes`.
 * - `countingBackend(inner)` counts calls on the backend port that stores and components inject (`BACKEND`, `DebriefBackend`, ...),
 *   provided through the app's own DI token with `provideCountingBackend`, so the code under test runs unchanged and nothing patches
 *   `invoke`. Assert with `expectBackendCalls`. It counts port calls, not IPC: a port method that makes N invokes counts once here.
 *
 * A counter belongs to the test that made it, so tests that run side by side never share a count. A budget (`{ max }`) needs at least
 * one call, because zero is what work sent to another instance (or never started) looks like; `{ max, allowNone: '<why>' }` says that
 * zero is right.
 */

/** A budget: at least one and at most `max` calls; `allowNone` (a reason) lets zero pass. */
export interface Budget {
  readonly max: number;
  readonly allowNone?: string;
}

/** Checks the calls named `names` against `expected`; throws a readable error. `what` names one call ("backend call"). */
function check(expected: number | Budget, names: readonly string[], what: string): void {
  const got = names.length;
  const list = names.join(', ') || 'none';
  if (typeof expected === 'number') {
    if (got !== expected) throw new Error(`expected ${expected} ${what}${expected === 1 ? '' : 's'}, got ${got}: ${list}`);
    return;
  }
  if (expected.allowNone !== undefined && expected.allowNone.trim() === '') throw new Error('allowNone needs a reason: say why making no call is right here');
  if (got === 0 && expected.allowNone === undefined) {
    throw new Error(`expected at least one ${what}, got 0: the work used another instance than the counted one, or made no call; if none is right, say why with { max, allowNone: '<reason>' }`);
  }
  if (got > expected.max) throw new Error(`expected at most ${expected.max} ${what}s, got ${got}: ${list}`);
}

/** Whether `value` is an Observable-like object (it has `subscribe`): passed through as is, never wrapped. */
function isSubscribable(value: object): boolean {
  return typeof (value as { subscribe?: unknown }).subscribe === 'function';
}

/** The backend port, counted (`countingBackend`). */
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

/**
 * Wraps `inner` so every method call on it (and on its nested plain objects, as `jobs.cancel`) is recorded, then passed through with
 * `this` as the inner object. Signals (`signal`, `computed`, `input`) are reads, not calls: they pass through uncounted. Subscribables
 * (Observables) pass through unwrapped. Getters run on the inner object.
 */
export function countingBackend<B extends object>(inner: B): CountingBackend<B> {
  const calls: RecordedCall[] = [];
  const wrapped = new WeakMap<object, object>();
  const wrap = <T extends object>(target: T, prefix: string): T => {
    const known = wrapped.get(target);
    if (known) return known as T;
    const proxy = new Proxy(target, {
      get(t, prop) {
        const value = Reflect.get(t, prop, t) as unknown;
        if (typeof prop !== 'string') return value;
        if (typeof value === 'function') {
          if (isSignal(value)) return value;
          return (...args: unknown[]) => {
            calls.push({ method: prefix + prop, args });
            return (value as (...a: unknown[]) => unknown).apply(t, args);
          };
        }
        if (typeof value === 'object' && value !== null && !(value instanceof Promise) && !isSubscribable(value)) return wrap(value, `${prefix}${prop}.`);
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
 * Runs `run` and asserts how many backend port calls it made (of `method`, when given): exactly `expected`, or a `Budget`. Returns
 * what `run` returned. Calls made before `run` started do not count. The error lists the calls, so a per-item loop is plain to see.
 */
export async function expectBackendCalls<B, T>(counter: CountingBackend<B>, expected: number | Budget, run: () => T | Promise<T>, method?: string): Promise<T> {
  const start = counter.calls.length;
  const value = await run();
  const made = counter.calls.slice(start).filter((c) => method === undefined || c.method === method);
  check(expected, made.map((c) => c.method), method === undefined ? 'backend call' : `${method} call`);
  return value;
}

/** One IPC call: the command name and its arguments. */
export interface RecordedInvoke {
  readonly command: string;
  readonly args: unknown;
}

/** The IPC call, counted (`countingInvoke`). */
export interface CountingInvoke<F> {
  /** The counted `invoke`: pass it (or a bridge holding it) to the app's tauri-backend. */
  readonly invoke: F;
  /** Every invoke so far, in order. */
  readonly calls: RecordedInvoke[];
  /** How many invokes so far, of `command` or of all. */
  count(command?: string): number;
  /** Forgets the invokes so far. */
  reset(): void;
}

/** Wraps an `invoke` function (Tauri's `invoke(cmd, args)` or a fake of it) so every call is recorded, then passed through. */
export function countingInvoke<F extends (command: string, args?: never) => unknown>(invoke: F): CountingInvoke<F> {
  const calls: RecordedInvoke[] = [];
  const counted = ((command: string, args?: unknown) => {
    calls.push({ command, args });
    return (invoke as (c: string, a?: unknown) => unknown)(command, args);
  }) as unknown as F;
  return {
    invoke: counted,
    calls,
    count: (command) => (command === undefined ? calls.length : calls.filter((c) => c.command === command).length),
    reset: () => void calls.splice(0),
  };
}

/** Runs `run` and asserts how many invokes it made (of `command`, when given): exactly `expected`, or a `Budget`. Returns its value. */
export async function expectInvokes<F, T>(counter: CountingInvoke<F>, expected: number | Budget, run: () => T | Promise<T>, command?: string): Promise<T> {
  const start = counter.calls.length;
  const value = await run();
  const made = counter.calls.slice(start).filter((c) => command === undefined || c.command === command);
  check(expected, made.map((c) => c.command), command === undefined ? 'invoke' : `${command} invoke`);
  return value;
}
