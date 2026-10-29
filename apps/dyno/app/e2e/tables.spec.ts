import { expectHeaderOnTop, expect, test } from '@decryp7/paddock-e2e';

/** The grids of Dyno are the shared pd-table: their headers stay above the rows while they scroll (C43), and the keyboard is the table's. */
const url = (path: string) => `/?latency=0#${path}`;

test.use({ viewport: { width: 1280, height: 420 } });

test.describe('table headers stay on top while the rows scroll and one is hovered', () => {
  test('processes', async ({ page }) => {
    await page.goto(url('/processes'));
    await page.getByRole('radio', { name: 'All', exact: true }).click();
    await expect(page.getByRole('treegrid', { name: 'Processes' })).toBeVisible();
    await expectHeaderOnTop(page.getByRole('treegrid', { name: 'Processes' }));
  });

  test('network connections', async ({ page }) => {
    await page.goto(url('/network'));
    await page.getByRole('radio', { name: 'None', exact: true }).click();
    await page.getByRole('checkbox', { name: 'Show loopback' }).click();
    await expectHeaderOnTop(page.getByRole('grid', { name: 'Connections' }));
  });

  test('alarms', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 280 });
    await page.goto(url('/alarms'));
    await expect(page.getByRole('grid', { name: 'Open alarms' })).toBeVisible();
    await expectHeaderOnTop(page.getByRole('grid', { name: 'Open alarms' }));
  });
});

test.describe('keyboard on the shared grids', () => {
  test('processes: one tab stop, Right unfolds an app, Left folds it, Enter opens the details', async ({ page }) => {
    await page.goto(url('/processes'));
    const grid = page.getByRole('treegrid', { name: 'Processes' });
    await expect(grid).toBeVisible();
    const rows = grid.getByRole('row');
    const app = grid.locator('[role=row][aria-expanded]').first();
    await app.focus();
    const open = (await app.getAttribute('aria-expanded')) === 'true';
    await page.keyboard.press(open ? 'ArrowLeft' : 'ArrowRight');
    await expect(app).toHaveAttribute('aria-expanded', open ? 'false' : 'true');
    await expect(app).toBeFocused();
    // Unfolded (now or again), Right enters the app: its first process is focused, one level down.
    if (open) {
      await page.keyboard.press('ArrowRight');
      await expect(app).toHaveAttribute('aria-expanded', 'true');
    }
    await page.keyboard.press('ArrowRight');
    await expect(page.locator(':focus')).toHaveAttribute('aria-level', '2');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('complementary', { name: 'Process details' })).toBeVisible();
    expect(await rows.count()).toBeGreaterThan(2);
  });

  test('alarms: arrows move between rows, Right reaches the row buttons, Enter opens the panel and Escape closes it', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto(url('/alarms'));
    const grid = page.getByRole('grid', { name: 'Open alarms' });
    const rows = grid.locator('[role=row][data-key]');
    await rows.first().focus();
    await page.keyboard.press('ArrowDown');
    await expect(rows.nth(1)).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(rows.nth(1).getByRole('button').first()).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(rows.nth(1)).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('complementary', { name: 'Alarm details' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('complementary', { name: 'Alarm details' })).toHaveCount(0);
    await expect(rows.nth(1)).toBeFocused();
  });

  test('processes: Enter on a row far down opens the details, the row keeps the focus, every drawn row has the header\'s cells, and Escape closes the panel', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 600 });
    await page.clock.install();
    await page.goto(url('/processes'));
    await page.getByRole('radio', { name: 'All', exact: true }).click();
    const grid = page.getByRole('treegrid', { name: 'Processes' });
    await expect(grid).toBeVisible();
    await grid.getByRole('row').nth(3).focus();
    const focused = page.locator('.pd-body [role=row]:focus');
    await grid.locator('.pd-body [role=row][data-key]').nth(6).focus();
    const key = await page.evaluate(() => (document.activeElement as HTMLElement).dataset['key']);
    await page.keyboard.press('Enter');
    await expect(page.getByRole('complementary', { name: 'Process details' })).toBeVisible();
    // Three live updates (the mock's clock is the page's): the row still has the focus afterwards.
    await page.clock.runFor(3000);
    await expect(focused).toHaveAttribute('data-key', key as string);
    const counts = await page.evaluate(() => {
      const head = document.querySelectorAll('.pd-head-table [role=columnheader]').length;
      return { head, rows: [...document.querySelectorAll('.pd-body [role=row][data-key]')].map((r) => r.querySelectorAll('[role=gridcell]').length) };
    });
    expect(counts.rows.length).toBeGreaterThan(3);
    expect(new Set(counts.rows)).toEqual(new Set([counts.head]));
    await page.keyboard.press('Escape');
    await expect(page.getByRole('complementary', { name: 'Process details' })).toHaveCount(0);
  });
});

test.describe('live updates keep the keyboard where it was', () => {
  test('processes sorted by CPU: a focused row keeps the focus while the rows reorder, the keys keep working, Escape closes the details', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.clock.install();
    await page.goto(url('/processes'));
    await page.getByRole('radio', { name: 'All', exact: true }).click();
    const grid = page.getByRole('treegrid', { name: 'Processes' });
    await expect(grid).toBeVisible();
    const rows = grid.locator('.pd-body [role=row][data-key]');
    await rows.nth(4).focus();
    const key = await page.evaluate(() => (document.activeElement as HTMLElement).dataset['key'] as string);
    const order = () => page.evaluate(() => [...document.querySelectorAll('.pd-body [role=row][data-key]')].map((r) => (r as HTMLElement).dataset['key']));
    const before = await order();
    // Drive the live updates by the clock until the rows came in another order (bounded: a refresh every second).
    for (let i = 0; i < 20 && JSON.stringify(await order()) === JSON.stringify(before); i++) await page.clock.runFor(1000);
    expect(await order()).not.toEqual(before);
    await expect(page.locator(`.pd-body [role=row][data-key="${key}"]`)).toBeFocused();
    // The keyboard still works from there.
    const at = (await order()).indexOf(key);
    await page.keyboard.press('ArrowDown');
    await expect(rows.nth(at + 1)).toBeFocused();
    await page.keyboard.press('ArrowUp');
    await expect(page.locator(`.pd-body [role=row][data-key="${key}"]`)).toBeFocused();
    // Enter opens the details; more updates; Escape closes them and the row keeps the focus.
    await page.keyboard.press('Enter');
    await expect(page.getByRole('complementary', { name: 'Process details' })).toBeVisible();
    await page.clock.runFor(3000);
    await page.keyboard.press('Escape');
    await expect(page.getByRole('complementary', { name: 'Process details' })).toHaveCount(0);
    await expect(page.locator(`.pd-body [role=row][data-key="${key}"]`)).toBeFocused();
  });
});

test.describe('the row menu from the keyboard', () => {
  test('processes: after switching Apps to All, Shift+F10 opens the focused row\'s menu and Escape gives the focus back', async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 800 });
    await page.goto(url('/processes'));
    const grid = page.getByRole('treegrid', { name: 'Processes' });
    await expect(grid).toBeVisible();
    await page.getByRole('radio', { name: 'All', exact: true }).click();
    const row = grid.locator('.pd-body [role=row][data-key]:not([aria-expanded])').first();
    await row.focus();
    await page.keyboard.press('Shift+F10');
    const menu = page.getByRole('menu');
    await expect(menu).toBeVisible();
    await expect(menu.getByRole('menuitem', { name: 'Details' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(row).toBeFocused();
  });
});
