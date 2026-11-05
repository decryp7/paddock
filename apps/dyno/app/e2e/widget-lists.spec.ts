import { expect, test } from '@decryp7/paddock-e2e';
import type { Locator, Page } from '@playwright/test';

/**
 * Widgets that can scroll list everything and scroll inside themselves (Dyno 1.1.0): the dashboard's Recent events (a virtual viewport)
 * and Issues (plain scrolling), the tray popover's Active alarms (virtual) and Disk space (plain), each with a long mock list
 * (`?lists=long`). Nothing ends in "+N more", the last row can be reached by the wheel and by the keyboard, the wheel passes on to the
 * dashboard at the end of a list, and a drag in edit mode still moves the widget. Waits are on conditions, never sleeps.
 */
const DASHBOARD = '/?latency=0&lists=long#/';
const TRAY = '/?latency=0&lists=long#/tray';

const scrollTop = (l: Locator): Promise<number> => l.evaluate((e) => e.scrollTop);
const atEnd = (l: Locator): Promise<boolean> => l.evaluate((e) => e.scrollTop + e.clientHeight >= e.scrollHeight - 1);

/** Puts the pointer over the middle of the list. */
async function pointAt(page: Page, list: Locator): Promise<void> {
  const b = await list.boundingBox();
  if (!b) throw new Error('the list has no box');
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
}

/** One wheel step over the list, then a frame, so a virtual list has rendered what the step reached. */
async function wheelStep(page: Page, dy: number): Promise<void> {
  await page.mouse.wheel(0, dy);
  await page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => done())));
}

/** Wheels over the list until it is at its end (a bounded number of steps; the caller checks the outcome). */
async function wheelToEnd(page: Page, list: Locator, onStep?: () => Promise<void>): Promise<void> {
  await pointAt(page, list);
  for (let i = 0; i < 120 && !(await atEnd(list)); i++) {
    await wheelStep(page, 400);
    if (onStep && !(await atEnd(list))) await onStep();
  }
  expect(await atEnd(list)).toBe(true);
}

async function openDashboard(page: Page, width = 900, height = 700): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(DASHBOARD);
  await expect(page.locator('[data-pd-cell]').first()).toBeVisible({ timeout: 45_000 });
  await expect(page.locator('pd-widget-frame [aria-busy="true"], pd-widget-frame[aria-busy="true"]')).toHaveCount(0, { timeout: 30_000 });
  await expect(page.locator('dy-events-feed .row').first()).toBeVisible();
}

/** Puts the widget in the middle of the shell's main element, so the page can still scroll both ways. */
async function centre(page: Page, el: Locator): Promise<void> {
  await el.evaluate((e) => e.scrollIntoView({ block: 'center' }));
  await expect(el).toBeInViewport();
}

test.describe('widget lists on the dashboard at 900 px', () => {
  test('Recent events lists every entry in a scrolling viewport, ends at the oldest and never says "+N more"', async ({ page }) => {
    await openDashboard(page);
    await expect(page.locator('main')).not.toContainText(/\+\d+ more/);
    const list = page.locator('dy-events-feed cdk-virtual-scroll-viewport');
    await centre(page, list);
    expect(await list.evaluate((e) => e.scrollHeight - e.clientHeight)).toBeGreaterThan(200);
    // Virtual: far fewer rows in the page than entries.
    expect(await page.locator('dy-events-feed .row').count()).toBeLessThan(40);
    await wheelToEnd(page, list);
    await expect(list.getByText('Dyno started')).toBeVisible();
  });

  test('the wheel scrolls the list under the pointer and, at its end, passes on to the dashboard', async ({ page }) => {
    await openDashboard(page);
    const list = page.locator('dy-events-feed cdk-virtual-scroll-viewport');
    await centre(page, list);
    const main = page.locator('main');
    const before = await scrollTop(main);
    // While the list is still moving, the dashboard stays where it is: the list takes the wheel.
    await wheelToEnd(page, list, async () => expect(await scrollTop(main)).toBe(before));
    expect(await scrollTop(list)).toBeGreaterThan(0);
    // At its end the list lets the wheel through (it does not trap it): the dashboard scrolls.
    await wheelStep(page, 300);
    await expect.poll(() => scrollTop(main), { timeout: 15_000 }).toBeGreaterThan(before);
  });

  test('Issues lists every issue, is reachable by keyboard and reaches the last one', async ({ page }) => {
    await openDashboard(page);
    const region = page.getByRole('region', { name: 'Unusual things found' });
    await centre(page, region);
    await expect(page.locator('main')).not.toContainText(/\+\d+ more/);
    expect(await region.locator('li').count()).toBeGreaterThanOrEqual(50);
    await region.focus();
    await expect(region).toBeFocused();
    await page.keyboard.press('End');
    await expect.poll(() => atEnd(region), { timeout: 15_000 }).toBe(true);
    await expect(region.getByText('Invented app 50 memory keeps growing')).toBeVisible();
  });

  test('the events list is a focusable list with an accessible name and scrolls by keyboard', async ({ page }) => {
    await openDashboard(page);
    const list = page.getByRole('list', { name: 'Activity and alerts' });
    await centre(page, list);
    await list.focus();
    await page.keyboard.press('PageDown');
    await expect.poll(() => scrollTop(list), { timeout: 15_000 }).toBeGreaterThan(0);
  });

  test('in edit mode the list does not steal a drag: the widget moves', async ({ page }) => {
    // Editing needs the wide arrangement (900 px is too narrow to arrange).
    await openDashboard(page, 1280, 900);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const list = page.locator('dy-events-feed cdk-virtual-scroll-viewport');
    await centre(page, list);
    const cell = page.locator('[data-pd-cell]', { has: page.locator('dy-events-feed') });
    const before = await cell.boundingBox();
    const at = await list.boundingBox();
    if (!before || !at) throw new Error('no box');
    // Sideways, over the middle of the list: the list must not take the pointer, and the page must not scroll under the drag.
    const [x, y] = [at.x + at.width / 2, at.y + at.height / 2];
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x + 40, y, { steps: 5 });
    await page.mouse.move(x + 320, y, { steps: 12 });
    await page.mouse.up();
    await expect
      .poll(async () => {
        const after = await cell.boundingBox();
        return after ? Math.abs(after.x - before.x) + Math.abs(after.y - before.y) : 0;
      }, { timeout: 15_000 })
      .toBeGreaterThan(10);
  });
});

test.describe('widget lists in the tray popover', () => {
  test('Active alarms scrolls inside a viewport of fixed height, reaches the last alarm, and says what the engine cut', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto(TRAY);
    const list = page.getByRole('list', { name: 'Active alarms' });
    await expect(list).toBeVisible();
    await expect(page.locator('body')).not.toContainText(/\+\d+ more/);
    const height = await page.locator('pd-tray-popover').evaluate((e) => e.getBoundingClientRect().height);
    expect((await list.boundingBox())!.height).toBeLessThanOrEqual(210);
    expect(await page.locator('.dy-pw-item').count()).toBeLessThan(30);
    await wheelToEnd(page, list);
    await expect(list.getByText('Invented alarm 200')).toBeVisible();
    // The popover did not grow while the list scrolled.
    expect(await page.locator('pd-tray-popover').evaluate((e) => e.getBoundingClientRect().height)).toBe(height);
    await expect(page.getByText('Showing the latest 200 of 205.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open alarms' })).toBeVisible();
  });

  test('Active alarms is reachable and scrollable by keyboard', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto(TRAY);
    const list = page.getByRole('list', { name: 'Active alarms' });
    await list.focus();
    await expect(list).toBeFocused();
    await page.keyboard.press('PageDown');
    await expect.poll(() => scrollTop(list), { timeout: 15_000 }).toBeGreaterThan(0);
  });

  test('Disk space lists every volume in a scrolling list and reaches the last one', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto(TRAY);
    const volumes = page.getByRole('list', { name: 'Volumes' });
    await expect(volumes).toBeVisible();
    expect(await volumes.locator('li').count()).toBe(30);
    expect((await volumes.boundingBox())!.height).toBeLessThanOrEqual(140);
    await wheelToEnd(page, volumes);
    await expect(volumes.getByText('Invented volume 30')).toBeVisible();
  });
});
