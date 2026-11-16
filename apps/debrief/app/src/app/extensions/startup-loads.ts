/**
 * The code loads that start-up began on its own (a lazy command set's chunk, started when the window is idle or when a
 * gated key is read), kept so a test can wait for exactly those before it tears its module down (`testing/startup.ts`).
 * A chunk that is still loading at teardown is imported into a torn-down environment, and Vitest reports an
 * `EnvironmentTeardownError` against whichever file runs next (debrief.yml run 8170). The record changes nothing at run
 * time: it holds the promise the load already has and forgets it when it settles, success or failure.
 */
const inFlight = new Set<Promise<unknown>>();

/** Records a start-up load; returns the same promise. */
export function trackStartupLoad<T>(load: Promise<T>): Promise<T> {
  inFlight.add(load);
  const forget = (): void => void inFlight.delete(load);
  load.then(forget, forget);
  return load;
}

/**
 * Resolves once no recorded load is in flight, including loads that begin while waiting. It never starts a load, never
 * rejects (a failed load is already swallowed by its caller, and `lazy-commands.spec.ts` asserts that loads succeed) and
 * waits on real promises only, so it also ends under fake timers.
 */
export async function startupLoadsSettled(): Promise<void> {
  while (inFlight.size > 0) await Promise.allSettled([...inFlight]);
}
