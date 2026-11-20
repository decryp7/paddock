import { expect, test } from '@playwright/test';
import type { BrowserContext, Page } from '@playwright/test';
import { clickThatClosesItsPage, untilItsPageCloses } from '../src/index.ts';

// A control whose click closes the window it is in (a pop-out's Dock back, which asks the host window to close it). If the window goes away
// while Playwright is still performing the click, `locator.click` rejects with "Target page, context or browser has been closed" (Debrief's
// popout.e2e.ts:62, debrief.yml run 8338 job 42172, 1 of 151 runs). That moment cannot be scheduled from outside Playwright, so these
// tests hold the click open on a disabled button and close the window from its host meanwhile: the same rejection, every time.

/** A window opened by the host page, holding one button; `close()` closes it from the host, as Debrief's host does (`win.close()`). */
async function popupWith(page: Page, context: BrowserContext, button: string): Promise<{ popup: Page; closeFromHost: () => Promise<void> }> {
  await page.goto('data:text/html,<p>host</p>');
  const opened = context.waitForEvent('page');
  await page.evaluate(() => {
    const w = window.open('', '_blank');
    (window as unknown as { closePopup: () => void }).closePopup = () => w?.close();
  });
  const popup = await opened;
  await popup.setContent(`<button ${button}>Dock back</button>`);
  return { popup, closeFromHost: () => page.evaluate(() => (window as unknown as { closePopup: () => void }).closePopup()) };
}

test('a window closed while its click is being performed is a closed window, not a failed step', async ({ page, context }) => {
  const { popup, closeFromHost } = await popupWith(page, context, 'disabled');
  const clicking = clickThatClosesItsPage(popup.getByRole('button', { name: 'Dock back' }));
  await closeFromHost();
  await clicking;
  expect(popup.isClosed()).toBe(true);
});

test('a click whose window the host closes after it passes', async ({ page, context }) => {
  const { popup } = await popupWith(page, context, 'onclick="opener.closePopup()"');
  await clickThatClosesItsPage(popup.getByRole('button', { name: 'Dock back' }));
  expect(popup.isClosed()).toBe(true);
});

// The precondition (ER-33): without the helper the same sequence fails a plain click, so the first test proves something.
test('a plain click rejects when its window is closed while it is being performed', async ({ page, context }) => {
  const { popup, closeFromHost } = await popupWith(page, context, 'disabled');
  const clicking = popup
    .getByRole('button', { name: 'Dock back' })
    .click()
    .then(
      () => null,
      (e: Error) => e,
    );
  await closeFromHost();
  expect(String(await clicking)).toContain('has been closed');
});

// Any step, not only a click: an in-page step (a dispatched pointer-up, an evaluate) that is still running when the host closes the window.
test('an in-page step still running when its window closes is a closed window, not a failed step', async ({ page, context }) => {
  const { popup, closeFromHost } = await popupWith(page, context, '');
  const stepping = untilItsPageCloses(popup, () => popup.evaluate(() => new Promise(() => {})));
  await closeFromHost();
  await stepping;
  expect(popup.isClosed()).toBe(true);
});

// Positive controls: the helper does not turn every failure into a pass.
test('a click that leaves its window open fails', async ({ page, context }) => {
  const { popup } = await popupWith(page, context, 'onclick="void 0"');
  const error = await clickThatClosesItsPage(popup.getByRole('button', { name: 'Dock back' }), { timeout: 1_000 }).then(
    () => null,
    (e: Error) => e,
  );
  expect(String(error)).toMatch(/did not close/);
  expect(popup.isClosed()).toBe(false);
});

test('a click that cannot happen fails with its own error while the window stays open', async ({ page, context }) => {
  const { popup } = await popupWith(page, context, 'onclick="opener.closePopup()"');
  const error = await clickThatClosesItsPage(popup.getByRole('button', { name: 'No such button' }), { timeout: 1_000 }).then(
    () => null,
    (e: Error) => e,
  );
  expect(String(error)).toContain('locator.click');
  expect(popup.isClosed()).toBe(false);
});
