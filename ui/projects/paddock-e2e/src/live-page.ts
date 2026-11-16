import type { Locator, Page } from '@playwright/test';

/**
 * Stops the page's live updates (the intervals and timers that refresh a table or a status) so an interaction is not
 * overtaken by one: a row menu opened, then the rows re-rendered under it; a Stop button pressed on a job that finished meanwhile.
 * The pinned clock (`pinnedClockTest`) is paused 5 s ahead of the page's own time (time keeps running until the pause lands, so the target lies ahead; 5 s is a margin for a slow round trip on a loaded runner, not a wait), which fires what is due on the way once each; no timer or animation frame runs
 * after that until {@link resumeLive} or `page.clock.runFor`. Playwright's own actions and `expect` waits use real time and keep working.
 * Needs the pinned (or any installed) clock; a page without one throws, naming the fix.
 *
 * The page does not draw while the clock is paused (its timers and animation frames are the fake ones), so pause only around the steps
 * that must not be overtaken (a focus and a key), never around an assertion that waits for the page to draw, and not before the page has
 * booted: use {@link withLivePaused} and put the waits after it.
 */
export async function pauseLive(page: Page): Promise<void> {
  const now = await page.evaluate(() => Date.now());
  try {
    await page.clock.pauseAt(new Date(now + 5000));
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

/**
 * Clicks `target` with the live updates stopped, then lets them run again, also when the click fails. A control that moves when a timer
 * adds something beside it (a status item to its right, a toast stack that reorders on a new alert) can move between mousedown and
 * mouseup, which then hit different elements, and the browser fires no click (Debrief, CI runs 8136, 8004, 7941).
 */
export async function clickWithLivePaused(page: Page, target: Locator): Promise<void> {
  await withLivePaused(page, () => target.click());
}
