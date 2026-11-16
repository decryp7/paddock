import type { Locator, Page } from '@playwright/test';

/** The first look-ahead of a pause, in milliseconds. Measured 2026-11-16 (Playwright 1.64.1, idle machine, 40 pauses each): the round trip from reading the page's time to the end of `pauseAt` took 0.6 ms median and 3.9 ms at most in Chromium, 1.4 ms and 2.9 ms in WebKit; 250 ms is about 60 times that. */
const PAUSE_AHEAD_MS = 250;
/** How many times a pause that lands in the past is tried again, each with twice the look-ahead (250, 500, 1000 ms). */
const PAUSE_ATTEMPTS = 3;

/**
 * Stops the page's live updates (the intervals and timers that refresh a table or a status) so a step is not overtaken by one: a Stop
 * button pressed on a job that finished meanwhile, a control that moves when an item appears beside it.
 *
 * What it does to the page clock (the pinned one from `pinnedClockTest`, or any other: Playwright installs a clock on the first
 * `pauseAt` when there is none, so this never throws for a missing one):
 * - It moves the clock forward by the look-ahead (250 ms, doubled up to 1 s when the round trip was slower than that) and pauses it there. Every
 *   timer due within those milliseconds fires once on the way (a toast's alert, a debounce, a mock's delayed reply), so the page is
 *   not in the state it was in a moment before.
 * - Repeated pauses add up: each one moves the clock again.
 * - From then on no timer or animation frame runs until {@link resumeLive} or `page.clock.runFor`. The page does not draw while the
 *   clock is paused, so pause only around the step that must not be overtaken (a click, a focus and a key), never around an
 *   assertion that waits for the page to draw, and not before the page has booted. {@link withLivePaused} resumes for you.
 * - After the resume the clock runs again from where it stopped; a live update that fell due meanwhile fires then, so a result that
 *   depends on no update arriving after the step (a menu that stays open) is not made safe by this.
 */
export async function pauseLive(page: Page): Promise<void> {
  let ahead = PAUSE_AHEAD_MS;
  for (let attempt = 1; ; attempt++) {
    const now = await page.evaluate(() => Date.now());
    try {
      await page.clock.pauseAt(new Date(now + ahead));
      return;
    } catch (e) {
      // The page clock moved past the target while the call was on its way (a slow round trip): look further ahead.
      if (attempt >= PAUSE_ATTEMPTS || !/past/i.test((e as Error).message)) throw e;
      ahead *= 2;
    }
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
