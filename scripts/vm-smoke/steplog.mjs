// Step logging for the VM smoke runners (rule 56): every step logs "start <label>" before it runs and its status and elapsed seconds
// after, so a hung or slow step is visible in the log while it runs and afterwards.

/**
 * @param {object[]} steps  the result list, appended to
 * @param {(line: string) => void} log
 * @param {() => number} [now]  milliseconds, injectable for tests
 */
export function makeSteps(steps, log, now = Date.now) {
  /** Runs `fn` (sync or async) as the step `label` and records the step it returns. */
  const step = async (label, fn) => {
    const t0 = now();
    log(`start ${label}`);
    const s = await fn();
    steps.push(s);
    log(`${s.status.toUpperCase()}  ${s.title}: ${s.summary} (${Math.round((now() - t0) / 1000)} s)`);
    return s;
  };
  return { step };
}
