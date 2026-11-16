import { expect, Page, test } from '@playwright/test';
import { openAppWith } from './mock-url';
import { viewport } from './mock-url';

// ADR 0072: the live-tail grid follows the newest events, holds on request, and never takes the view from the user. Each case runs in
// both sort orders and in Chromium and WebKit (branch CI runs Chromium only; WebKit runs locally and in the full run), on Playwright's
// pinned clock: `mockPageStaggerMs` answers the pages of one request at different times and the live ticks (every 50 ms) land between.
const URL = '?mockTotal=5000&mockLive=1&mockLiveRate=20&mockLiveMs=50&mockLiveRotateMs=0&mockLiveAlertMs=0&mockPageStaggerMs=10';
const SCALED = '?mockTotal=3000000&mockLive=1&mockLiveRate=20&mockLiveMs=50&mockLiveRotateMs=0&mockLiveAlertMs=0&mockPageStaggerMs=10';

type Order = 'oldest first' | 'newest first';
const ORDERS: Order[] = ['oldest first', 'newest first'];

async function open(page: Page, order: Order, search = URL): Promise<void> {
  await page.clock.install();
  await openAppWith(page, search);
  if (order === 'newest first') await page.getByRole('button', { name: /Oldest first/ }).click();
  await page.locator('debrief-status-bar').getByRole('button', { name: 'Live tail', exact: true }).click();
  await expect(page.locator('debrief-status-bar').getByRole('button', { name: 'Live tail', exact: true })).toHaveAttribute('aria-pressed', 'true');
}

const follow = (page: Page) => page.getByTestId('follow-toggle');
const metrics = (page: Page) => viewport(page).evaluate((el) => ({ gap: el.scrollHeight - el.clientHeight - el.scrollTop, top: el.scrollTop, max: el.scrollHeight - el.clientHeight }));

/** Moves the pinned clock on in 20 ms steps until `done()` holds, at most `maxMs` of page time (virtual time, not wall time). */
async function runUntil(page: Page, done: () => Promise<boolean>, maxMs: number): Promise<boolean> {
  for (let spent = 0; spent <= maxMs; spent += 20) {
    if (await done()) return true;
    await page.clock.runFor(20);
  }
  return done();
}

/** On the newest end: the last row oldest first, the first row newest first. */
async function onNewest(page: Page, order: Order): Promise<boolean> {
  const m = await metrics(page);
  return order === 'oldest first' ? m.gap <= 2 : m.top <= 2;
}

/** Goes to the newest end with the keyboard (a user move: it pins the view there). */
async function goNewest(page: Page, order: Order): Promise<void> {
  await viewport(page).focus();
  await page.keyboard.press(order === 'oldest first' ? 'End' : 'Home');
  expect(await runUntil(page, () => onNewest(page, order), 2_000), 'the view reaches the newest end').toBe(true);
}

async function holdWithEvents(page: Page): Promise<void> {
  await follow(page).click();
  await expect(follow(page)).toHaveAttribute('aria-pressed', 'false');
  await page.clock.runFor(1_000);
  await expect(page.getByTestId('new-events')).toBeVisible();
}

for (const order of ORDERS) {
  test.describe(`live tail grid, ${order}`, () => {
    test('reveal, with events pushed between its pages, ends on the newest event', async ({ page }) => {
      await open(page, order);
      await holdWithEvents(page);
      await page.getByTestId('new-events').click();
      expect(await runUntil(page, () => onNewest(page, order), 3_000), 'the view reaches the newest end').toBe(true);
      await page.clock.runFor(1_000);
      expect(await runUntil(page, () => onNewest(page, order), 1_000), 'and stays there').toBe(true);
    });

    test('following over 20 pushes: pinned stays on the newest; not pinned keeps its position', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      await page.clock.runFor(1_000);
      expect(await runUntil(page, () => onNewest(page, order), 1_000), 'pinned: stays on the newest end').toBe(true);
      // Away from the newest end (a user key move): the position is kept while events arrive.
      await viewport(page).focus();
      await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown');
      await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown');
      await page.clock.runFor(200);
      const before = await metrics(page);
      expect(await onNewest(page, order)).toBe(false);
      await page.clock.runFor(1_000);
      await page.clock.runFor(200);
      const after = await metrics(page);
      // Oldest first the rows stay where they are; newest first they are inserted above, so the same events stay in view (the top moves down).
      if (order === 'oldest first') expect(Math.abs(after.top - before.top)).toBeLessThanOrEqual(24);
      else expect(after.top).toBeGreaterThanOrEqual(before.top - 24);
    });

    test('a wheel, the scrollbar or a key during a reveal leave the view where the user put it', async ({ page }) => {
      for (const how of ['wheel', 'key', 'scrollbar'] as const) {
        await open(page, order);
        await holdWithEvents(page);
        await page.getByTestId('new-events').click();
        // The first page of the reveal has been answered, a later one not yet.
        await page.clock.runFor(40);
        const box = await viewport(page).boundingBox();
        if (how === 'wheel') {
          await page.mouse.move((box?.x ?? 0) + 200, (box?.y ?? 0) + 150);
          await page.mouse.wheel(0, order === 'oldest first' ? -900 : 900);
        } else if (how === 'key') {
          await viewport(page).focus();
          await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown');
          await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown');
        } else {
          await viewport(page).evaluate((el, newestFirst) => {
            el.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
            el.scrollTop = newestFirst ? el.scrollTop + 900 : Math.max(0, el.scrollTop - 900);
          }, order === 'newest first');
        }
        await page.clock.runFor(100);
        const left = await metrics(page);
        expect(await onNewest(page, order), `${how}: the view left the newest end`).toBe(false);
        await page.clock.runFor(1_500);
        const settled = await metrics(page);
        expect(await onNewest(page, order), `${how}: and was not pulled back`).toBe(false);
        expect(Math.abs(settled.top - left.top), `${how}: and stays where it was put`).toBeLessThanOrEqual(order === 'oldest first' ? 24 : 24 * 80);
        await page.reload();
      }
    });

    test('a wheel in the scaled scrollbar mode leaves the view where the user put it', async ({ page }) => {
      await open(page, order, SCALED);
      await holdWithEvents(page);
      await page.getByTestId('new-events').click();
      await page.clock.runFor(40);
      const box = await viewport(page).boundingBox();
      await page.mouse.move((box?.x ?? 0) + 200, (box?.y ?? 0) + 150);
      await page.mouse.wheel(0, order === 'oldest first' ? -1_200 : 1_200);
      await page.clock.runFor(100);
      expect(await onNewest(page, order)).toBe(false);
      await page.clock.runFor(1_500);
      expect(await onNewest(page, order), 'not pulled back to the newest end').toBe(false);
    });

    test('resize smaller and larger while pinned: the view stays on the newest end and keeps following', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      for (const height of [620, 900, 720]) {
        await page.setViewportSize({ width: 1440, height });
        await page.clock.runFor(200);
        expect(await runUntil(page, () => onNewest(page, order), 1_000), `${height} px: still on the newest end`).toBe(true);
      }
      await page.clock.runFor(1_000);
      expect(await runUntil(page, () => onNewest(page, order), 1_000), 'and still following').toBe(true);
    });

    test('resize while not pinned keeps the top row', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      await viewport(page).focus();
      for (let i = 0; i < 4; i++) await page.keyboard.press(order === 'oldest first' ? 'PageUp' : 'PageDown');
      await page.clock.runFor(200);
      const before = await metrics(page);
      for (const height of [620, 900]) {
        await page.setViewportSize({ width: 1440, height });
        await page.clock.runFor(300);
        const after = await metrics(page);
        expect(Math.abs(after.top - before.top), `${height} px: the top row is kept`).toBeLessThanOrEqual(order === 'oldest first' ? 24 : 24 * 80);
      }
    });

    test('Follow off counts the new events; Follow on shows the newest', async ({ page }) => {
      await open(page, order);
      await goNewest(page, order);
      await holdWithEvents(page);
      await expect(page.getByTestId('new-events')).toHaveText(/\d[\d,]* new events? — show/);
      await follow(page).click();
      await expect(follow(page)).toHaveAttribute('aria-pressed', 'true');
      expect(await runUntil(page, () => onNewest(page, order), 3_000), 'following again: the newest end').toBe(true);
      await expect(page.getByTestId('new-events')).toHaveCount(0);
    });
  });
}
