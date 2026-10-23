import { expect, test } from '@playwright/test';

test.describe('row menus, keyboard rows, sticky headers and charts', () => {
  test('a right click on a feed row opens the row actions and Esc returns to the row', async ({ page }) => {
    await page.goto('/');
    const row = page.locator('sp-whats-new li.item').first();
    await row.click({ button: 'right' });
    const menu = page.getByRole('menu');
    await expect(menu).toBeVisible();
    await expect(menu.getByRole('menuitem', { name: 'Open release' })).toBeVisible();
    await expect(menu.getByRole('menuitem', { name: 'Download' })).toBeVisible();
    await expect(menu.getByRole('menuitem', { name: 'Copy link' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(row).toBeFocused();
  });

  test('the feed is walked with Up, Down, Home and End and a row opens with Enter', async ({ page }) => {
    await page.goto('/');
    const rows = page.locator('sp-whats-new li.item');
    await rows.first().focus();
    await expect(rows.first()).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(rows.nth(1)).toBeFocused();
    await page.keyboard.press('End');
    await expect(rows.last()).toBeFocused();
    await page.keyboard.press('Home');
    await expect(rows.first()).toBeFocused();
    // The focused row is visibly outlined.
    const outline = await rows.first().evaluate((e) => getComputedStyle(e).outlineStyle);
    expect(outline).toBe('solid');
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/#\/watches\/w-\d+$/);
  });

  test('only one feed row is in the tab order', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('sp-whats-new li.item').first()).toBeVisible();
    const stops = await page.locator('sp-whats-new li.item').evaluateAll((els) => els.filter((e) => e.getAttribute('tabindex') === '0').length);
    expect(stops).toBe(1);
  });

  test('the watch table has the row actions on right click, Delete asks by name and cancelling keeps the watch', async ({ page }) => {
    await page.goto('/#/watches');
    const row = page.locator('tbody tr', { hasText: 'Lumen CLI' });
    await row.click({ button: 'right' });
    const menu = page.getByRole('menu');
    await expect(menu.getByRole('menuitem')).toHaveText([/Open details/, /Check now/, /Open latest release/, /Mark as seen|Pause/, /Pause|Copy address/, /Copy address|Edit/, /Edit|Delete watch/, /Delete watch/].slice(0, await menu.getByRole('menuitem').count()));
    await menu.getByRole('menuitem', { name: 'Delete watch' }).click();
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toContainText('Delete "Lumen CLI"?');
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(row).toBeVisible();
  });

  test('Shift+F10 opens the menu of the focused watch row, Down and Enter walk and open', async ({ page }) => {
    await page.goto('/#/watches');
    const rows = page.locator('tbody tr');
    await rows.first().focus();
    await page.keyboard.press('ArrowDown');
    await expect(rows.nth(1)).toBeFocused();
    await page.keyboard.press('Shift+F10');
    await expect(page.getByRole('menu')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('menu')).toHaveCount(0);
    await rows.nth(1).focus();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/#\/watches\/w-\d+$/);
  });

  test('the watch table shows when the schedule runs next to the next check', async ({ page }) => {
    await page.goto('/#/watches');
    const cell = page.locator('tbody tr', { hasText: 'Acme Installer' }).locator('.next');
    await expect(cell.locator('.sched')).not.toBeEmpty();
    await expect(page.getByRole('columnheader', { name: 'Next' })).toBeVisible();
  });

  test('the release table header stays put while the rows scroll', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto('/#/watches/w-1');
    const wrap = page.locator('section[aria-labelledby=hist-h] .table-wrap');
    await expect(wrap).toBeVisible();
    const scrollable = await wrap.evaluate((e) => e.scrollHeight > e.clientHeight);
    const th = wrap.locator('thead th').first();
    const before = await th.boundingBox();
    if (scrollable) {
      await wrap.evaluate((e) => (e.scrollTop = e.scrollHeight));
      const after = await th.boundingBox();
      expect(Math.abs((after?.y ?? 0) - (before?.y ?? 0))).toBeLessThan(2);
    }
    expect(await th.evaluate((e) => getComputedStyle(e).position)).toBe('sticky');
  });

  test('the dashboard draws the reliability and downloads charts with their legends', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Check reliability' })).toBeVisible();
    await expect(page.getByRole('img', { name: /^Checks per day: \d+ in total, \d+ failed\.$/ })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Downloads', exact: true })).toBeVisible();
    await expect(page.getByRole('img', { name: /^Downloads: \d+ in total/ })).toBeVisible();
    await expect(page.locator('[data-testid=downloads-chart] canvas')).toBeVisible();
  });

  test('the tray page renders its list and keeps working without a window to resize', async ({ page }) => {
    await page.goto('/#/tray');
    await expect(page.getByText('Spotter', { exact: true }).first()).toBeVisible();
    await expect(page.locator('pd-tray-popover li').first()).toBeVisible();
  });
});
