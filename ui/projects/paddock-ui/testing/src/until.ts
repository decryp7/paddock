/** Options of {@link until}. */
export interface UntilOptions {
  /** What the wait is for, in words. The failure names it, so a red run says which condition never came. Required. */
  what: string;
  /**
   * One round of letting pending work run before the condition is read again: `() => settle(fixture)`, `() => flush()`, or
   * (under fake timers) `() => vi.advanceTimersByTimeAsync(0)`. Default: one turn of the event loop. The wait never sleeps for a
   * fixed time; it counts rounds, so a loaded runner gets as many turns as a quiet one.
   */
  settle?: () => Promise<unknown>;
  /** Most rounds before it fails. Default 5000: a few seconds of event-loop turns, far more than any wait that is going to end. */
  maxRounds?: number;
}

/** The turn of the event loop `until` waits on by default. */
const turn = (): Promise<void> => new Promise<void>((resolve) => setTimeout(resolve, 0));

/**
 * Waits until `condition` holds (rule 49, ER-12): it returns `true`, or it stops throwing, like an `expect`. Replaces
 * `await flush(DEBOUNCE_MS + 20)` and every other fixed wait that guesses how long async work takes. Between reads it runs
 * `settle` (default: one event-loop turn), so a debounce, a lazy import or an engine call gets turns until it is done. Fails after
 * `maxRounds` with `what` and the last thrown error as the cause. It checks once before the first round, so a condition that already
 * holds returns at once.
 */
export async function until(condition: () => boolean | void | Promise<boolean | void>, options: UntilOptions): Promise<void> {
  if (!options.what) throw new Error('until(): say what the wait is for ({ what })');
  const settle = options.settle ?? turn;
  const rounds = options.maxRounds ?? 5000;
  let last: unknown;
  for (let round = 0; round <= rounds; round++) {
    try {
      if ((await condition()) !== false) return;
      last = undefined;
    } catch (e) {
      last = e;
    }
    if (round < rounds) await settle();
  }
  throw new Error(`until(): ${options.what} did not happen within ${rounds} rounds`, { cause: last });
}
