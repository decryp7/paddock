import type { Page } from '@playwright/test';

/**
 * Stops the page's live updates (the intervals and timers that refresh a table or a status) so an interaction is not
 * overtaken by one: a row menu opened, then the rows re-rendered under it; a Stop button pressed on a job that finished meanwhile.
 * The pinned clock (`pinnedClockTest`) is paused a second ahead, which fires what is due once; no timer or animation frame runs
 * after that until {@link resumeLive} or `page.clock.runFor`. Playwright's own actions and `expect` waits use real time and keep working.
 * Needs the pinned (or any installed) clock; a page without one throws, naming the fix.
 */
export async function pauseLive(page: Page): Promise<void> {
  const now = await page.evaluate(() => Date.now());
  try {
    await page.clock.pauseAt(new Date(now + 1000));
  } catch (e) {
    throw new Error(`pauseLive: the page clock is not installed or cannot be paused (${(e as Error).message.split('\n')[0]}). Use the app's e2e/test.ts (pinnedClockTest), or call page.clock.install first.`, { cause: e });
  }
}

/** Lets the page's live updates run again from where they stopped (the counterpart of {@link pauseLive}). */
export async function resumeLive(page: Page): Promise<void> {
  await page.clock.resume();
}

/** Runs `steps` with the live updates stopped, and lets them run again afterwards (also when `steps` throws). */
export async function withLivePaused<T>(page: Page, steps: () => Promise<T>): Promise<T> {
  await pauseLive(page);
  try {
    return await steps();
  } finally {
    await resumeLive(page);
  }
}
