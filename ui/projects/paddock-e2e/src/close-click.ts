import type { Locator, Page } from '@playwright/test';

/**
 * Runs a step that closes the page it acts on (a pop-out's Dock back, which asks the host window to close it; a pointer-up that docks a
 * dragged pop-out; a dialog window's Close) and waits for that page to close.
 *
 * Awaiting such a step plainly is a race: when the page goes away while Playwright is still performing it, the step rejects with "Target
 * page, context or browser has been closed" (Debrief's popout.e2e.ts:62, debrief.yml run 8338 job 42172, 1 of 151 runs; the click had
 * been delivered). Here that rejection counts as the step's expected effect, and only when the page did close:
 * - the page closes (during the step or after it): resolves;
 * - the step fails and the page stays open (no such control, not clickable in time): rejects with the step's own error;
 * - the step succeeds and the page stays open: rejects, "did not close".
 * Whether the step did what it was for is the caller's next assertion (the panel is back in the host), never this helper's.
 */
export async function untilItsPageCloses(page: Page, step: () => Promise<unknown>, options: { timeout?: number; what?: string } = {}): Promise<void> {
  // Settles with `null` once the page has closed, or with the wait's error; it never rejects, so a timeout that comes while the step is
  // still failing is not an unhandled rejection.
  const closed: Promise<Error | null> = page.isClosed()
    ? Promise.resolve(null)
    : page.waitForEvent('close', { timeout: options.timeout }).then(
        () => null,
        (error: Error) => error,
      );
  try {
    await step();
  } catch (error) {
    // The page may report its end before the close event has run: wait for it, bounded by the same timeout.
    await closed;
    if (page.isClosed()) return;
    throw error;
  }
  const notClosed = await closed;
  if (notClosed && !page.isClosed()) throw new Error(`${options.what ?? 'the step'} did not close its page: ${notClosed.message}`);
}

/** Clicks a control whose click closes the page it is on, through {@link untilItsPageCloses}. */
export function clickThatClosesItsPage(target: Locator, options: { timeout?: number } = {}): Promise<void> {
  return untilItsPageCloses(target.page(), () => target.click({ timeout: options.timeout }), {
    timeout: options.timeout,
    what: `the click on ${target}`,
  });
}
