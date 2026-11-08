import { afterEach, beforeEach, vi } from 'vitest';

/** What a component fixture offers to settle: Angular's `ComponentFixture` has both. */
export interface Settleable {
  detectChanges(): void;
  whenStable(): Promise<unknown>;
}

/** Every timer a spec can wait on: faked together, so no spec waits on real time (ADR 0041, C71). */
const FAKED = ['Date', 'setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'requestAnimationFrame', 'cancelAnimationFrame'] as const;

/** One animation frame; timers due at or before it fire when the clock is moved by it. */
const FRAME_MS = 16;

/**
 * Installs the fake clock for the describe it is called in (`now` is the wall clock the spec pins), restored after each test.
 * Replies of the mock backend that are scheduled with a zero delay, and animation frames, run when `settle` moves the clock.
 */
export function useSettleClock(now: number): void {
  beforeEach(() => vi.useFakeTimers({ now, toFake: [...FAKED] }));
  afterEach(() => vi.useRealTimers());
}

/** The most frames one round moves the clock while it waits for Angular to be stable: a bound on a loop, not a delay. */
const MAX_FRAMES = 40;

/**
 * Lets the work a user action started finish. Angular's scheduler and the mock backend's zero-delay replies run on the faked
 * timers, so `whenStable` would never resolve by itself: each round starts it, moves the fake clock one frame at a time until it
 * resolves (the timers due run), then renders. It waits on the condition, never on real time; `rounds` is the most it looks.
 */
export async function settle(f: Settleable, rounds = 6): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    let stable = false;
    const waiting = f.whenStable().then(() => {
      stable = true;
    });
    for (let frame = 0; frame < MAX_FRAMES && !stable; frame++) await vi.advanceTimersByTimeAsync(FRAME_MS);
    await waiting;
    f.detectChanges();
  }
}

/**
 * Awaits a promise whose work waits on the fake clock (a mock reply with a latency): moves the clock in steps until it settles.
 * `limitMs` is the most fake time it moves, a bound on a loop and not a delay.
 */
export async function untilResolved<T>(promise: Promise<T>, limitMs = 120_000, stepMs = 100): Promise<T> {
  let done = false;
  const tracked = promise.finally(() => {
    done = true;
  });
  for (let at = 0; at < limitMs && !done; at += stepMs) await vi.advanceTimersByTimeAsync(stepMs);
  return tracked;
}
