/**
 * Unit tests of a live mock feed (a tail, a poll, a ticking clock). A test that asserts a value while the feed keeps
 * changing it is a race against the timer (Debrief's events-grid live tail, run 8191; the Spotter Today clock, run 8154;
 * the Dyno row menu, run 8161; Debrief's start-up settle, run 8170). The e2e kit has `pauseLive` and `withLivePaused`;
 * this is the same for unit tests.
 *
 * No second clock: `holdableTimers` wraps the timers a mock already takes (Debrief's `MockClock`, `ManualClock`, an app's
 * own timer option) and, while held, keeps every timer that comes due from running; what came due runs once on release.
 * `holdLive()` / `releaseLive()` / `withLiveHeld(fn)` hold every wrapped source of the test at once.
 *
 * ```ts
 * const clock = holdableTimers(new ManualClock());          // or holdableTimers(systemMockClock)
 * const backend = new MockBackend({ liveMs: 25, clock });
 * await tail.toggle();
 * await clock.advance(100);                                   // the feed ticks while the test says so
 * await withLiveHeld(async () => expect(grid.total()).toBe(await settledTotal())); // nothing moves under the assertion
 * ```
 */

import { registerSingleton } from './leaks';

/** The timer members a mock feed uses; any clock with these can be wrapped. */
export interface TimerSource {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  setInterval(fn: () => void, ms: number): unknown;
  clearInterval(handle: unknown): void;
}

/** The control added to a wrapped source. */
export interface Holdable {
  /** True while its timers are kept from running. */
  readonly held: boolean;
  /** Keeps due timers from running (they wait; a repeated interval waits once). */
  hold(): void;
  /** Runs what came due while held, once each, and lets timers run again. */
  release(): void;
}

export type HeldTimers<T extends TimerSource> = T & Holdable;

const sources = new Set<Holdable>();
let holds = 0;

/** Wraps `inner` (not changed) so its timers can be held. Registered for `holdLive`; a source made while held starts held. */
export function holdableTimers<T extends TimerSource>(inner: T): HeldTimers<T> {
  // The wrapper's prototype is the wrapped source, so `advance()` and the other members of a ManualClock still work.
  const wrapped = Object.create(inner) as HeldTimers<T>;
  let held = false;
  const waiting = new Map<unknown, () => void>();
  const gate = (key: { handle: unknown }, fn: () => void, repeat: boolean) => () => {
    if (!held) return fn();
    // A repeated timer waits once; a one-shot waits and runs once. Both are forgotten if their timer is cleared.
    if (waiting.has(key) && repeat) return;
    waiting.set(key, () => {
      waiting.delete(key);
      fn();
    });
  };
  const make = (start: (fn: () => void, ms: number) => unknown, repeat: boolean) => (fn: () => void, ms: number) => {
    const key = { handle: undefined as unknown };
    key.handle = start.call(inner, gate(key, fn, repeat), ms);
    handles.set(key.handle, key);
    return key.handle;
  };
  const handles = new Map<unknown, { handle: unknown }>();
  const clear = (stop: (h: unknown) => void) => (handle: unknown) => {
    const key = handles.get(handle);
    if (key) {
      waiting.delete(key);
      handles.delete(handle);
    }
    stop.call(inner, handle);
  };
  Object.defineProperties(wrapped, {
    setTimeout: { value: make(inner.setTimeout, false) },
    setInterval: { value: make(inner.setInterval, true) },
    clearTimeout: { value: clear(inner.clearTimeout) },
    clearInterval: { value: clear(inner.clearInterval) },
    held: { get: () => held },
    hold: {
      value: () => {
        held = true;
      },
    },
    release: {
      value: () => {
        held = false;
        for (const run of [...waiting.values()]) run();
      },
    },
  });
  sources.add(wrapped);
  if (holds > 0) wrapped.hold();
  return wrapped;
}

/** Holds every wrapped source (and any made until the matching `releaseLive`). Nests. */
export function holdLive(): void {
  holds++;
  for (const s of sources) s.hold();
}

/** Undoes one `holdLive`; the last one lets the feeds run again and runs what came due. */
export function releaseLive(): void {
  if (holds === 0) throw new Error('releaseLive() without a holdLive(): hold the feed first, or use withLiveHeld(fn), which pairs them.');
  if (--holds > 0) return;
  for (const s of sources) s.release();
}

/** Runs `fn` with every live feed held, and releases them when it ends (also when it throws). */
export async function withLiveHeld<R>(fn: () => R | Promise<R>): Promise<R> {
  holdLive();
  try {
    return await fn();
  } finally {
    releaseLive();
  }
}

/** How many holds are open (for the kit's own check). */
export function liveHoldCount(): number {
  return holds;
}

/** Forgets every source and hold (the kit calls it after each test, so one test's hold never reaches the next). */
export function resetLive(): void {
  holds = 0;
  sources.clear();
}

// A hold a test left open is named by the leak detector and reset before the next test (the kit never carries one over).
registerSingleton({ name: 'live holds', leftover: () => (holds > 0 ? `${holds} holdLive() without its releaseLive()` : null), reset: resetLive });
