/**
 * The one clock every mock backend's timer runs on (browser development and tests only; the real backends never
 * use it). Mock features driven by time (live tail ticks and alerts, slow imports, simulated latency, update
 * checks) all go through it: each mock takes a `clock` option (default {@link systemMockClock}) and hands it to the
 * helpers it creates, so a unit test can pass its own, and Playwright's `page.clock` (which patches the page's
 * timers before the app starts) drives every mock feature at once in simulated time.
 */
export type MockTimer = ReturnType<typeof globalThis.setTimeout>;

export interface MockClock {
  /** Wall-clock milliseconds (like `Date.now()`). */
  now(): number;
  /** A monotonic reading for measuring durations (like `performance.now()`). */
  elapsed(): number;
  setTimeout(fn: () => void, ms: number): MockTimer;
  clearTimeout(timer: MockTimer | undefined): void;
  setInterval(fn: () => void, ms: number): MockTimer;
  clearInterval(timer: MockTimer | undefined): void;
  /** Resolves after `ms`. */
  sleep(ms: number): Promise<void>;
}

/**
 * The page's own timers, looked up at every call (never captured when the module loads), so whatever the page's
 * globals are at that moment (real, or Playwright's or Vitest's fake timers) is what runs.
 *
 * Only mocks may use it: the test setup stops it after every test (`stopSystemMockClock`) and, until the next test, refuses new
 * timers and lets sleeps return at once. Product code that used it would silently stop working between tests.
 */
export const systemMockClock: MockClock = {
  now: () => Date.now(),
  elapsed: () => globalThis.performance?.now() ?? Date.now(),
  setTimeout: (fn, ms) => {
    if (halted) return refused(); // between tests: nothing new starts
    const handle = globalThis.setTimeout(() => {
      pending.delete(handle);
      fn();
    }, ms);
    pending.set(handle, { clear: globalThis.clearTimeout, wake: null });
    return handle;
  },
  clearTimeout: (t) => {
    pending.delete(t);
    globalThis.clearTimeout(t);
  },
  setInterval: (fn, ms) => {
    if (halted) return refused();
    const handle = globalThis.setInterval(fn, ms);
    pending.set(handle, { clear: globalThis.clearInterval, wake: null });
    return handle;
  },
  clearInterval: (t) => {
    pending.delete(t);
    globalThis.clearInterval(t);
  },
  sleep: (ms) =>
    new Promise<void>((resolve) => {
      if (halted) return resolve(); // a loop of sleeps ends at once once the clock is stopped
      const handle = globalThis.setTimeout(() => {
        pending.delete(handle);
        resolve();
      }, ms);
      pending.set(handle, { clear: globalThis.clearTimeout, wake: resolve });
    }),
};

/** Set by `stopSystemMockClock` until `resumeSystemMockClock`: nothing new starts, sleeps return at once. */
let halted = false;
let warned = false;
/** The test the clock was stopped after (named in the warning). */
let stoppedAfter = 'an unnamed test';

/** A timer asked for between tests: refused, with one warning per pause (a mock that keeps starting timers after its test ended). */
function refused(): 0 {
  if (!warned) {
    warned = true;
    const frame = (new Error().stack ?? '').split('\n').slice(2).map((l) => l.trim()).find((l) => l.length > 0 && !l.includes('mock/mock-clock.ts'))?.replace(/^at /, '') ?? 'unknown caller';
    const err = (globalThis as unknown as { process?: { stderr?: { write(s: string): unknown } } }).process?.stderr;
    err?.write(`[mock-clock] a timer was asked for after "${stoppedAfter}" and refused: something still runs after its test ended, at ${frame}\n`);
  }
  return 0;
}

/** The timers of `systemMockClock` still running, by handle. */
const pending = new Map<unknown, { clear: (handle: never) => void; wake: (() => void) | null }>();

/**
 * Stops every timer `systemMockClock` still has running and lets the sleeps waiting on them go (tests: none outlives the test
 * that started it; the mock's own `dispose` is not reached by a spec that ends with a mock still ticking).
 */
export function stopSystemMockClock(afterTest = 'an unnamed test'): void {
  stoppedAfter = afterTest;
  halted = true;
  for (const [handle, t] of [...pending]) {
    pending.delete(handle);
    try {
      t.clear(handle as never);
    } catch {
      /* a clear that belonged to fake timers no longer installed: nothing to stop */
    }
    t.wake?.();
  }
}

/** Lets `systemMockClock` run again (before every test). */
export function resumeSystemMockClock(): void {
  halted = false;
  warned = false;
}
