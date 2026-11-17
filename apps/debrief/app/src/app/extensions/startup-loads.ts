/**
 * The code loads that start-up began on its own (a lazy command set's chunk, started when the window is idle or when a
 * gated key is read), kept so a test can wait for exactly those before it tears its module down (`testing/startup.ts`).
 * A chunk that is still loading at teardown is imported into a torn-down environment, and Vitest reports an
 * `EnvironmentTeardownError` against whichever file runs next (debrief.yml run 8170). The record changes nothing at run
 * time: it holds the promise the load already has and forgets it when it settles, success or failure.
 *
 * This is a test-support record that lives in product code, because the loads it records are the product's own. Specs
 * share one page (`isolate: false`), so the set is shared across tests: a reader takes what is recorded and clears it
 * (`takeStartupLoads`), so one load that never settles is charged to the test that started it, not to every later one.
 */
const inFlight = new Set<Promise<unknown>>();
/** Loads that began and have not settled, taken from the record or not: what the leak detector reads (`test-setup.ts`). */
let unsettled = 0;

/** Records a start-up load; returns the same promise. */
export function trackStartupLoad<T>(load: Promise<T>): Promise<T> {
  inFlight.add(load);
  unsettled += 1;
  const forget = (): void => {
    inFlight.delete(load);
    unsettled -= 1;
  };
  load.then(forget, forget);
  return load;
}

/** How many recorded loads have not settled yet, including ones a reader already took. Read-only. */
export function unsettledStartupLoads(): number {
  return unsettled;
}

/** Takes every recorded load and clears the record: a later reader never waits on a load an earlier test started. */
export function takeStartupLoads(): Promise<unknown>[] {
  const loads = [...inFlight];
  inFlight.clear();
  return loads;
}

/**
 * Resolves once the loads recorded so far, and any that begin while waiting, have settled. It never starts a load, never
 * rejects (a failed load is already swallowed by its caller, and `lazy-commands.spec.ts` asserts that loads succeed) and
 * waits on real promises only, so it also ends under fake timers.
 */
export async function startupLoadsSettled(): Promise<void> {
  for (let loads = takeStartupLoads(); loads.length > 0; loads = takeStartupLoads()) await Promise.allSettled(loads);
}
