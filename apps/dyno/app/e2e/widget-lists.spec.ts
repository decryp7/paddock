import { expect, stablePressPoint, test } from '@decryp7/paddock-e2e';
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

const LIST_SELECTOR = 'ul, ol, [role="list"], cdk-virtual-scroll-viewport';

/**
 * Puts the pointer over the list, near its right edge, once the list has stopped moving (the page may still be settling). Not the
 * middle: a row's text there opens a tooltip (dyFullText) that sits under the pointer and takes the wheel, found in WebKit by
 * listing the elements under the pointer while the wheel did nothing.
 */
async function pointAt(page: Page, list: Locator): Promise<{ x: number; y: number }> {
  const { y } = await stablePressPoint(page, list, LIST_SELECTOR);
  const box = (await list.boundingBox())!;
  const x = box.x + box.width - 14;
  await page.mouse.move(x, y);
  await assertListUnder(page, { x, y });
  return { x, y };
}

/** Fails, naming what is there, when something other than the list is under the pointer (an overlay would swallow the wheel). */
async function assertListUnder(page: Page, at: { x: number; y: number }): Promise<void> {
  const under = await page.evaluate(({ x, y, sel }) => {
    const el = document.elementFromPoint(x, y);
    return el?.closest(sel) != null ? '' : `${el?.tagName}.${el?.className}`;
  }, { ...at, sel: LIST_SELECTOR });
  expect(under, 'the element under the pointer is not the list: it would take the wheel').toBe('');
}

/** The list's and the page's scroll positions, read in one step so they belong to the same moment (main is -1 where there is none). */
interface Snap {
  list: number;
  main: number;
  end: boolean;
}
const snap = (list: Locator): Promise<Snap> =>
  list.evaluate((e) => ({ list: e.scrollTop, main: e.closest('main')?.scrollTop ?? -1, end: e.scrollTop + e.clientHeight >= e.scrollHeight - 1 }));

/**
 * One wheel, then waits on the condition: the list or the page has moved from `from`. A wheel the browser absorbed changes
 * nothing, so the wait ends (bounded) and the caller sends it again; the next wheel never goes out before this one took effect.
 */
async function wheelAndSettle(page: Page, list: Locator, from: Snap, dy: number): Promise<void> {
  await page.mouse.wheel(0, dy);
  const moved = async (): Promise<boolean> => {
    const now = await snap(list);
    return now.list !== from.list || now.main !== from.main;
  };
  await expect.poll(moved, { timeout: 3_000, intervals: [25] }).toBe(true).catch(() => undefined);
}

/**
 * Wheels over the list until `done(snap)` holds. The wheel goes inside the poll, so a wheel the browser absorbed (WebKit under
 * load) is sent again, and each state is read in one step after the wheel took effect (never a fixed frame).
 */
async function wheelUntil(page: Page, list: Locator, done: (s: Snap) => boolean, what: string, onState?: (s: Snap) => void): Promise<void> {
  const at = await pointAt(page, list);
  await expect
    .poll(
      async () => {
        const s = await snap(list);
        if (done(s)) return true;
        // Checked only while the list still has to move: once it has reached its end, a wheel that carried on to the page (WebKit
        // passes the rest of a step on) moves the list from under the still pointer, and that is not a swallowed wheel.
        await assertListUnder(page, at);
        onState?.(s);
        await wheelAndSettle(page, list, s, 400);
        return false;
      },
      { message: what, timeout: 30_000, intervals: [50] },
    )
    .toBe(true);
}

/** Wheels over the list until it is at its end; `whileMoving` is checked on every state in which the list is not yet at its end. */
const wheelToEnd = (page: Page, list: Locator, whileMoving?: (s: Snap) => void): Promise<void> => wheelUntil(page, list, (s) => s.end, 'the list reaches its end', whileMoving);

/** Whether the rows reach the bottom of the list's viewport: a viewport the CDK did not measure again leaves blank space below its last row. */
async function drawnToBottom(list: Locator): Promise<boolean> {
  return list.evaluate((e) => {
    const view = e.getBoundingClientRect();
    const rows = [...e.querySelectorAll('[role="listitem"]')].map((r) => r.getBoundingClientRect());
    return rows.length > 0 && Math.max(...rows.map((r) => r.bottom)) >= view.bottom - 1;
  });
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
    // Virtual rows say where they are in the whole list.
    const total = Number(await list.locator('[role="listitem"]').first().getAttribute('aria-setsize'));
    expect(total).toBeGreaterThanOrEqual(50);
    expect(await list.locator('[role="listitem"]').first().getAttribute('aria-posinset')).toBe('1');
    await wheelToEnd(page, list);
    await expect(list.getByText('Dyno started')).toBeVisible();
    expect(await list.locator('[role="listitem"]').last().getAttribute('aria-posinset')).toBe(String(total));
  });

  test('the wheel scrolls the list under the pointer and, at its end, passes on to the dashboard', async ({ page }) => {
    await openDashboard(page);
    const list = page.locator('dy-events-feed cdk-virtual-scroll-viewport');
    await centre(page, list);
    const main = page.locator('main');
    await pointAt(page, list);
    const before = await scrollTop(main);
    // While the list is still moving, the dashboard stays where it is: the list takes the wheel.
    await wheelToEnd(page, list, (s) => expect(s.main, 'the dashboard moved while the list could still scroll').toBe(before));
    expect(await scrollTop(list)).toBeGreaterThan(0);
    // At its end the list lets the wheel through (it does not trap it): the dashboard scrolls. A wheel that was absorbed is sent again.
    await expect
      .poll(async () => {
        if ((await scrollTop(main)) > before) return true;
        await page.mouse.wheel(0, 300);
        return false;
      }, { message: 'the dashboard scrolls once the list is at its end', timeout: 30_000, intervals: [50] })
      .toBe(true);
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
    // Sideways, over the middle of the list: the list must not take the pointer, and the page must not scroll under the drag.
    const { x, y } = await stablePressPoint(page, list, '[data-pd-cell]');
    const before = await cell.boundingBox();
    if (!before) throw new Error('no box');
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

test.describe('the viewport follows its size', () => {
  test('a widget resized by its grip draws rows to the new bottom', async ({ page }) => {
    // Tall, so the widget can grow by more than the rows the CDK draws beyond the old edge.
    await openDashboard(page, 1280, 1500);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const list = page.locator('dy-events-feed cdk-virtual-scroll-viewport');
    await centre(page, list);
    expect(await drawnToBottom(list)).toBe(true);
    const before = (await list.boundingBox())!.height;
    const grip = page.locator('[data-pd-cell]', { has: page.locator('dy-events-feed') }).locator('.pd-wg-h--se');
    const { x, y } = await stablePressPoint(page, grip, '.pd-wg-h--se');
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x, y + 40, { steps: 4 });
    await page.mouse.move(x, y + 320, { steps: 16 });
    await page.mouse.up();
    await expect.poll(async () => (await list.boundingBox())!.height, { timeout: 15_000 }).toBeGreaterThan(before + 150);
    await expect.poll(() => drawnToBottom(list), { timeout: 15_000 }).toBe(true);
  });

  test('events that arrive after the first draw fill the viewport', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/?latency=500&lists=long#/');
    const list = page.locator('dy-events-feed cdk-virtual-scroll-viewport');
    await expect(list).toBeAttached({ timeout: 45_000 });
    await expect.poll(() => list.evaluate((e) => e.querySelectorAll('[role="listitem"]').length), { timeout: 30_000 }).toBeGreaterThan(3);
    await centre(page, list);
    await expect.poll(() => drawnToBottom(list), { timeout: 15_000 }).toBe(true);
  });

  test('the popup alarm list fills its viewport when the real rows replace the skeleton', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto('/?latency=500&lists=long#/tray');
    const list = page.getByRole('list', { name: 'Active alarms' });
    await expect.poll(async () => (await list.boundingBox())?.height ?? 0, { timeout: 30_000 }).toBeGreaterThan(200);
    await expect.poll(() => drawnToBottom(list), { timeout: 15_000 }).toBe(true);
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
    await expect(page.getByText('Showing the 200 most severe of 205.')).toBeVisible();
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
