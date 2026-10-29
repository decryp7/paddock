import { expect, Page, test } from '@playwright/test';
import { openApp } from './app';
import { expectNoSeriousViolations } from './axe';
import { MESSAGE_COLUMN_MIN } from '../src/app/grid/message-column';

// Narrow windows: the filter bar folds range, time zone and Clear all into a ⋯ menu (bar < 1000 px), the Explorer
// closes by itself when the layout plan has no room for it beside the events grid (and comes back with a margin), low-priority
// status items go below 1000 px, tool windows auto-hide the same way, and the page never scrolls sideways.

const sidebar = (page: Page) => page.locator('debrief-sidebar');
const more = (page: Page) => page.getByTestId('filter-more');
const noHorizontalScroll = (page: Page) => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);

async function resize(page: Page, width: number): Promise<void> {
  await page.setViewportSize({ width, height: 800 });
}

test.describe('narrow window', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => localStorage.clear());
    await openApp(page);
  });

  test('wide: the Explorer is open, nothing is folded, the status bar shows everything', async ({ page }) => {
    await expect(sidebar(page)).toBeVisible();
    await expect(more(page)).toHaveCount(0);
    await expect(page.locator('debrief-status-bar .low').first()).toBeVisible();
  });

  test('1024 px: the Explorer shrinks to its minimum beside the minimum message column (the plan decides, not a width), the page does not scroll sideways', async ({ page }, testInfo) => {
    await resize(page, 1024);
    await expect(sidebar(page)).toBeVisible();
    expect(await noHorizontalScroll(page)).toBe(true);
    await expect(page.getByRole('grid', { name: 'Events' })).toBeVisible();
    await expectNoSeriousViolations(page, testInfo, 'narrow-1024');
  });

  test('900 px: the filter bar folds into the ⋯ menu, which has range, time zone and Clear all', async ({ page }, testInfo) => {
    await resize(page, 900);
    await expect(sidebar(page)).toHaveCount(0);
    await expect(more(page)).toBeVisible();
    await expect(page.locator('debrief-filter-bar .tz.fold')).toBeHidden();
    expect(await noHorizontalScroll(page)).toBe(true);
    // The search and the level chips always stay.
    await expect(page.getByTestId('query-field')).toBeVisible();
    await expect(page.getByRole('toolbar', { name: 'Levels' })).toBeVisible();
    await expect(page.locator('debrief-status-bar .low').first()).toBeHidden();

    await page.getByRole('toolbar', { name: 'Levels' }).getByRole('button', { name: /^ERR/ }).click();
    await more(page).click();
    const menu = page.getByRole('menu', { name: 'More filter controls' });
    await expect(menu.getByRole('menuitem', { name: /Time zone/ })).toBeVisible();
    await expect(menu.getByRole('menuitem', { name: 'Clear all' })).toBeVisible();
    await expectNoSeriousViolations(page, testInfo, 'narrow-900-more', 'debrief-filter-bar');
    await menu.getByRole('menuitem', { name: /Time zone/ }).click();
    await expect(menu).toHaveCount(0);
    await more(page).click();
    await page.getByRole('menu', { name: 'More filter controls' }).getByRole('menuitem', { name: 'Clear all' }).click();
    await expect(page.getByRole('toolbar', { name: 'Levels' }).getByRole('button', { name: /^ERR/ })).toHaveAttribute('aria-pressed', 'false');
  });

  // The Explorer gives way first (it shrinks to its minimum, then closes by itself); the message column keeps its minimum.
  for (const width of [900, 1024, 1280]) {
    test(`${width} px: the message column keeps its minimum and nothing overflows`, async ({ page }) => {
      await resize(page, width);
      const msg = page.locator('debrief-events-grid [role=row].row').first().locator('.msg');
      await expect(msg).toBeVisible();
      // The grid re-lays out after the window's resize: poll, never read once.
      await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
      const eventsGroup = page.locator('.dv-groupview').filter({ has: page.getByRole('tab', { name: 'Events', exact: true }) });
      await expect.poll(async () => { const g = (await eventsGroup.boundingBox())!; return g.x + g.width; }).toBeLessThanOrEqual(width + 1);
      await expect.poll(() => noHorizontalScroll(page)).toBe(true);
      const grid = page.locator('debrief-events-grid');
      await expect.poll(() => grid.locator('.header').evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      await expect.poll(() => grid.locator('[role=row].row').first().evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      // The Explorer is closed where it cannot fit beside the minimum message column; where the window has room for both it stays open.
      if (width <= 900) await expect(sidebar(page)).toHaveCount(0);
      else await expect(sidebar(page)).toBeVisible();
    });
  }

  test('1440 px: the Explorer shrinks (not below its minimum) so Registry keeps its own column, and the message column keeps its minimum', async ({ page }) => {
    await resize(page, 1440);
    const registry = page.locator('.dv-groupview').filter({ has: page.getByRole('tab', { name: 'Registry', exact: true }) });
    await expect(registry).toBeVisible();
    await expect(registry).not.toContainText('Events');
    await expect(sidebar(page)).toBeVisible();
    await expect.poll(async () => (await sidebar(page).boundingBox())!.width).toBeGreaterThanOrEqual(240);
    const msg = page.locator('debrief-events-grid [role=row].row').first().locator('.msg');
    await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
    await expect(registry).not.toContainText('Events');
  });

  test('the Explorer keeps the width the user chose: it shrinks and closes when the window narrows and comes back at that width', async ({ page }) => {
    const width = async () => Math.round((await sidebar(page).boundingBox())!.width);
    await resize(page, 1600);
    await expect(sidebar(page)).toBeVisible();
    // Choose 360 px with the keyboard on the sash (Home/End move it to its limits, arrows step it).
    const sash = page.getByRole('separator', { name: 'Resize the Explorer' });
    await sash.focus();
    const start = await width();
    for (let i = 0; i < 20 && (await width()) < 360; i++) await page.keyboard.press('ArrowRight');
    const chosen = await width();
    expect(chosen).toBeGreaterThan(start);

    // 1100 px holds the Explorer at its minimum and the message column at its minimum, but not the width chosen.
    await resize(page, 1100);
    const msg = page.locator('debrief-events-grid [role=row].row').first().locator('.msg');
    await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
    await expect(sidebar(page)).toBeVisible();
    await expect.poll(width).toBeLessThan(chosen);
    await resize(page, 900);
    await expect(sidebar(page)).toHaveCount(0);
    await resize(page, 1600);
    await expect(sidebar(page)).toBeVisible();
    await expect.poll(width).toBe(chosen);
  });

  test('the sash stops where the message column would drop under its minimum', async ({ page }) => {
    await resize(page, 1100);
    const sash = page.getByRole('separator', { name: 'Resize the Explorer' });
    await sash.focus();
    await page.keyboard.press('End');
    const msg = page.locator('debrief-events-grid [role=row].row').first().locator('.msg');
    await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
  });

  test('600 px: still usable, no sideways scroll, accessible', async ({ page }, testInfo) => {
    await resize(page, 600);
    await expect(sidebar(page)).toHaveCount(0);
    await expect(more(page)).toBeVisible();
    await expect(page.getByTestId('query-field')).toBeVisible();
    await expect(page.getByRole('grid', { name: 'Events' })).toBeVisible();
    expect(await noHorizontalScroll(page)).toBe(true);
    await expect(page.locator('debrief-status-bar .low').first()).toBeHidden();
    await expectNoSeriousViolations(page, testInfo, 'narrow-600');
  });

  // The filter bar wraps (or folds) so its right-hand buttons never leave the window.
  for (const width of [1024, 900, 600]) {
    test(`the filter bar's buttons stay inside a ${width} px window`, async ({ page }) => {
      await resize(page, width);
      // The bar folds after the window's resize event: wait for it to settle.
      const options = page.getByRole('button', { name: 'View options' });
      await expect.poll(() => options.evaluate((el) => el.getBoundingClientRect().right)).toBeLessThanOrEqual(width);
    });
  }

  test('widening the window brings the Explorer back; Ctrl+B cannot open it where the message column would be squeezed (there is no overlay)', async ({ page }) => {
    await resize(page, 900);
    await expect(sidebar(page)).toHaveCount(0);
    await resize(page, 1440);
    await expect(sidebar(page)).toBeVisible();
    await expect(more(page)).toHaveCount(0);
    await resize(page, 900);
    await expect(sidebar(page)).toHaveCount(0);
    await page.keyboard.press('ControlOrMeta+b');
    // Opened by hand with no room, it closes again; the message column is never squeezed under its minimum.
    await expect(sidebar(page)).toHaveCount(0);
    const msg = page.locator('debrief-events-grid [role=row].row').first().locator('.msg');
    await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
    await resize(page, 1440);
    await expect(sidebar(page)).toBeVisible();
  });

  test('side docks auto-hide when the plan has no room for them and are pinned back, with a margin, when the window widens', async ({ page }) => {
    // Pin the inspector so it is a docked tool window.
    const bar = page.getByRole('navigation', { name: 'Auto-hidden tool windows (right)' });
    await bar.getByRole('button', { name: 'Inspector' }).click();
    await page.getByRole('region', { name: 'Inspector' }).getByRole('button', { name: 'Pin Inspector' }).click();
    const tab = page.getByRole('tab', { name: 'Inspector' });
    await expect(tab).toBeVisible();
    await expect(bar).toHaveCount(0);

    await resize(page, 960);
    await expect(tab).toHaveCount(0);
    await expect(bar.getByRole('button', { name: 'Inspector' })).toBeVisible();
    expect(await noHorizontalScroll(page)).toBe(true);

    await resize(page, 1440);
    await expect(tab).toBeVisible();
    await expect(bar).toHaveCount(0);
  });

  test('an Explorer the user closed stays closed when the window widens', async ({ page }) => {
    await page.keyboard.press('ControlOrMeta+b');
    await expect(sidebar(page)).toHaveCount(0);
    await resize(page, 900);
    await resize(page, 1440);
    await expect(sidebar(page)).toHaveCount(0);
  });
});
