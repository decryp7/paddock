import { axeInThemes, expect, test } from '@decryp7/paddock-e2e';
import type { Locator, Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * The dashboard on the shared widget grid (family ADR 0044, WP-C2) against the mock backend, which answers the engine's gestures from the
 * golden cases: the frames and their content, edit mode (enter, keyboard move, keyboard resize, add through the side sheet, options in the
 * widget's menu, Undo and Redo), the narrow window that cannot edit, and axe in both themes. Waits are on conditions, never sleeps.
 */
const OPEN = '/?latency=0#/';
const cells = (page: Page): Locator => page.locator('[data-pd-cell]');

async function open(page: Page, width = 1280, height = 900): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(OPEN);
  await expect(cells(page).first()).toBeVisible({ timeout: 45_000 });
  // Settled: no frame is loading.
  await expect(page.locator('pd-widget-frame [aria-busy="true"], pd-widget-frame[aria-busy="true"]')).toHaveCount(0, { timeout: 30_000 });
}

const box = async (cell: Locator): Promise<{ x: number; y: number; width: number; height: number }> => {
  const b = await cell.boundingBox();
  if (!b) throw new Error('the widget has no box');
  return b;
};

test.describe('dashboard grid', () => {
  test('draws the frames with their content only: one card per widget, no card inside a frame', async ({ page }) => {
    await open(page);
    expect(await cells(page).count()).toBeGreaterThan(8);
    await expect(page.locator('pd-widget-frame dy-widget')).not.toHaveCount(0);
    // A frame is the card: nothing inside its body draws another one, and the old chooser bar and span toggles are gone.
    await expect(page.locator('pd-widget-frame .card, pd-widget-frame dy-tile, pd-widget-frame .card-head')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Customize dashboard' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Edit layout' })).toBeEnabled();
  });

  test('edit mode: enter, move and resize by keyboard, undo and redo', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const second = cells(page).nth(1);
    await second.focus();
    const before = await box(second);
    await page.keyboard.press('ArrowLeft');
    await expect.poll(async () => (await box(cells(page).nth(1))).x, { timeout: 15_000 }).not.toBe(before.x);
    const undo = page.getByRole('button', { name: 'Undo' });
    await expect(undo).toBeEnabled();
    // Resize: Shift+Right makes the focused widget wider by one step of its kind's rule.
    const focused = page.locator('[data-pd-cell]:focus');
    await expect(focused).toHaveCount(1);
    // Undo and Redo take the focus, so the widget is found by its id from here on.
    const id = await focused.getAttribute('data-pd-cell');
    const target = page.locator(`[data-pd-cell="${id}"]`);
    const w0 = (await box(target)).width;
    await page.keyboard.press('Shift+ArrowRight');
    await expect.poll(async () => (await box(target)).width, { timeout: 15_000 }).toBeGreaterThan(w0);
    await undo.click();
    await expect.poll(async () => (await box(target)).width, { timeout: 15_000 }).toBeCloseTo(w0, 0);
    await expect(page.getByRole('button', { name: 'Redo' })).toBeEnabled();
    await page.getByRole('button', { name: 'Redo' }).click();
    await expect.poll(async () => (await box(target)).width, { timeout: 15_000 }).toBeGreaterThan(w0);
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(page.getByRole('button', { name: 'Edit layout' })).toBeVisible();
  });

  test('edit mode: add a widget through the side sheet', async ({ page }) => {
    await open(page);
    const count = await cells(page).count();
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await page.getByRole('button', { name: 'Add widget' }).click();
    const sheet = page.getByRole('dialog', { name: 'Add a widget' });
    await expect(sheet).toBeVisible();
    await sheet.getByRole('button', { name: /^Add/ }).first().click();
    await expect(cells(page)).toHaveCount(count + 1);
    await page.keyboard.press('Escape');
    await expect(sheet).toHaveCount(0);
  });

  test('edit mode: the metric is an option in the widget menu, not a control on the card', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    await expect(page.getByRole('button', { name: /^Actions for/ })).not.toHaveCount(0);
    // Each widget's menu button sits inside its own card (a button that flows out of the card is covered by the next one).
    const outside = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('[data-pd-cell]')].filter((c) => {
        const b = c.querySelector<HTMLElement>('.pd-wg-more')?.getBoundingClientRect();
        const r = c.getBoundingClientRect();
        return !b || b.left < r.left - 1 || b.right > r.right + 1 || b.top < r.top - 1 || b.bottom > r.bottom + 1;
      }).length,
    );
    expect(outside).toBe(0);
    const graph = cells(page).filter({ has: page.locator('dy-widget[data-kind="graph"]') }).first();
    await graph.getByRole('button', { name: /^Actions for/ }).click();
    await page.getByRole('menuitem', { name: /^Options for/ }).click();
    const sheet = page.getByRole('dialog', { name: 'Widget options' });
    await expect(sheet).toBeVisible();
    await expect(sheet.getByRole('combobox', { name: /^Metric of/ })).toBeVisible();
  });

  test('dragging a widget near the bottom and top edge of the page scrolls the shell main element', async ({ page }) => {
    await open(page, 1280, 600);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const main = page.locator('main');
    const scrollTop = (): Promise<number> => main.evaluate((m) => m.scrollTop);
    expect(await scrollTop()).toBe(0);
    const first = await box(cells(page).first());
    await page.mouse.move(first.x + first.width / 2, first.y + first.height / 2);
    await page.mouse.down();
    const area = await main.boundingBox();
    if (!area) throw new Error('the page has no box');
    await page.mouse.move(first.x + first.width / 2, first.y + first.height / 2 + 40, { steps: 5 });
    await page.mouse.move(first.x + first.width / 2, area.y + area.height - 6, { steps: 8 });
    await expect.poll(scrollTop, { timeout: 15_000 }).toBeGreaterThan(0);
    // Back up: near the top edge it scrolls the other way.
    const down = await scrollTop();
    await page.mouse.move(first.x + first.width / 2, area.y + 6, { steps: 8 });
    await expect.poll(scrollTop, { timeout: 15_000 }).toBeLessThan(down);
    await page.keyboard.press('Escape');
    await page.mouse.up();
  });

  test('a window too narrow to arrange disables Edit layout and says why', async ({ page }) => {
    await open(page, 700);
    const edit = page.getByRole('button', { name: 'Edit layout' });
    await expect(edit).toBeDisabled();
    await expect(page.getByText(/needs a wider dashboard/)).toBeVisible();
  });

  test('has no accessibility violations in either theme, in normal and in edit mode', async ({ page }) => {
    await open(page);
    expect(await axeInThemes(page, { include: ['dy-widget-board'] })).toEqual([]);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    expect(await axeInThemes(page, { include: ['dy-widget-board'] })).toEqual([]);
  });
});

// Screenshots for the independent review (ER-23): SHOTS_DIR=<folder> npx playwright test dashboard-grid --project=chromium -g screenshots
test.describe('dashboard grid screenshots', () => {
  test.skip(!process.env['SHOTS_DIR'], 'set SHOTS_DIR to take them');
  for (const theme of ['light', 'dark'] as const) {
    test(`screenshots ${theme}`, async ({ page }) => {
      const dir = process.env['SHOTS_DIR'] ?? '';
      mkdirSync(dir, { recursive: true });
      await page.emulateMedia({ colorScheme: theme });
      // Tall, so the whole dashboard is in the picture (the page scrolls inside the shell, not as a document).
      await page.setViewportSize({ width: 1280, height: 2400 });
      await open(page, 1280, 2400);
      await page.screenshot({ path: join(dir, `dashboard-normal-${theme}.png`), fullPage: true });
      await page.getByRole('button', { name: 'Edit layout' }).click();
      await expect(page.getByText('Editing layout')).toBeVisible();
      await page.screenshot({ path: join(dir, `dashboard-edit-${theme}.png`), fullPage: true });
      const suffix = theme === 'light' ? '' : '-dark';
      await open(page, 900, 2800);
      await page.screenshot({ path: join(dir, `dashboard-900-six-columns${suffix}.png`), fullPage: true });
      await open(page, 500, 5200);
      await page.screenshot({ path: join(dir, `dashboard-500-one-column${suffix}.png`), fullPage: true });
    });
  }
});
