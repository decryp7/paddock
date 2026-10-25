import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// Settings > Storage (plan X32) in the shell demo: the page over the demo's in-memory engine, which plays `paddock-storage` (the
// same rules: a clear needs a scan, the user's data needs a confirmation, one job at a time, Cancel between items) with
// fictional sizes. The confirmation is a window of its own (in the page in the browser build). Chromium only in branches
// (rule 3). Only the test kit (@decryp7/paddock-e2e).

test.describe('storage (plan X32)', () => {
  /** Opens a row's "…" menu and chooses an item (destructive items live only there, in danger text, last). */
  async function clearFromMenu(page: import('@playwright/test').Page, id: string, item: string): Promise<void> {
    const row = page.locator(`.pd-row[data-key="${id}"]`);
    await row.hover();
    await row.getByRole('button', { name: 'More actions' }).click();
    const entry = page.getByRole('menuitem', { name: item });
    await expect(entry).toHaveClass(/pd-popmenu__item--danger/);
    await entry.click();
  }

  /** The Settings window in the page, on the Storage page, with the first measurement done. */
  async function openStorage(app: import('@decryp7/paddock-e2e').ShellApp, page: import('@playwright/test').Page): Promise<void> {
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings…', exact: true }).click();
    await app.settings.nav.getByRole('button', { name: 'Storage', exact: true }).click();
    await expect(page.getByTestId('storage-table')).toBeVisible();
  }

  test('lists what the app keeps, asks before clearing the cache and says what it freed', async ({ app, page, pageErrors }) => {
    await openStorage(app, page);
    // One line per row, grouped by what it is; the group row carries the aggregate; a lower bound is words in the sentence.
    await expect(page.locator('.pd-row[data-key]')).toHaveCount(3);
    await expect(page.getByTestId('storage-table').locator('.pd-group')).toHaveCount(2);
    await expect(page.getByTestId('storage-total')).toContainText('at least');
    await expect(page.locator('.pd-row[data-key="models"]')).toContainText('≥');
    const heights = await page.locator('.pd-row[data-key]').evaluateAll((rows) => rows.map((r) => Math.round(r.getBoundingClientRect().height)));
    expect(new Set(heights).size).toBe(1);
    expect(await axeViolations(page)).toEqual([]);

    await expect(page.getByTestId('storage-clear-cache')).not.toHaveClass(/primary/);
    await page.getByTestId('storage-clear-cache').click();
    const confirm = page.getByRole('dialog', { name: 'Clear cache?' });
    await expect(confirm).toContainText('The app rebuilds it when it needs it.');
    await expect(confirm.getByRole('button', { name: 'Keep' })).toBeFocused();
    expect(await axeViolations(page)).toEqual([]);
    // Escape keeps everything.
    await page.keyboard.press('Escape');
    await expect(confirm).toHaveCount(0);
    await expect(page.getByTestId('storage-working')).toHaveCount(0);

    await page.getByTestId('storage-clear-cache').click();
    await page.getByRole('dialog', { name: 'Clear cache?' }).getByRole('button', { name: 'Clear', exact: true }).click();
    // The job shows at once (its phase text changes every few hundred ms in the demo, so it is not asserted: the unit tests hold it).
    await expect(page.getByTestId('storage-working')).toBeVisible();
    await expect(page.getByTestId('storage-result')).toContainText('freed');
    // The table is measured again by itself, so the cache row is empty and nothing is left to clear there.
    await expect(page.getByTestId('storage-working')).toHaveCount(0);
    await expect(page.getByTestId('storage-clear-cache')).toBeDisabled();
    expect(pageErrors).toEqual([]);
  });

  test("the user's own data names itself in the confirmation and is cleared one category at a time", async ({ app, page }) => {
    await openStorage(app, page);
    await clearFromMenu(page, 'logs', 'Clear logs');
    const confirm = page.getByRole('dialog', { name: 'Clear logs?' });
    await expect(confirm.getByTestId('storage-confirm-data')).toContainText('your own data');
    await confirm.getByRole('button', { name: 'Clear', exact: true }).click();
    await expect(page.getByTestId('storage-result')).toContainText('freed');
    await expect(page.locator('.pd-row[data-key="logs"]')).toBeVisible();
  });

  test('Cancel stops a clear between items and says what was done', async ({ app, page }) => {
    await openStorage(app, page);
    await clearFromMenu(page, 'logs', 'Clear logs');
    await page.getByRole('dialog', { name: 'Clear logs?' }).getByRole('button', { name: 'Clear', exact: true }).click();
    await page.getByTestId('storage-cancel').click();
    await expect(page.getByTestId('storage-result')).toContainText('Stopped');
  });

  test('at 320 px nothing scrolls sideways and every action keeps a name', async ({ app, page }) => {
    await page.setViewportSize({ width: 320, height: 700 });
    await openStorage(app, page);
    await expectNoHorizontalScroll(page);
    // No sideways scroll and every row still one line with its actions reachable (hover icons and the "…" menu).
    const row = page.locator('.pd-row[data-key="cache"]');
    await row.hover();
    // The rows fit the body (nothing scrolls sideways inside the table) and the last action stays inside the table, even though the
    // body scrolls vertically.
    const fit = await page.getByTestId('storage-table').evaluate((t) => {
      const body = t.querySelector('.pd-body') as HTMLElement;
      const grid = t.querySelector('.pd-grid')!.getBoundingClientRect();
      const more = t.querySelector('.pd-row[data-key] .pd-more')!.getBoundingClientRect();
      return { scroll: body.scrollWidth, client: body.clientWidth, inside: more.right <= grid.right + 0.5 && more.left >= grid.left };
    });
    expect(fit.scroll).toBeLessThanOrEqual(fit.client);
    expect(fit.inside).toBe(true);
    // A narrow table keeps one line per row: the actions are behind the "…" menu.
    await expect(row.getByRole('button', { name: 'More actions' })).toBeVisible();
    await row.getByRole('button', { name: 'More actions' }).click();
    await expect(page.getByRole('menuitem', { name: 'Show in the file manager' })).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
  });

  test('the Folders button shows what is in a category as a table grouped by folder, which folds away', async ({ app, page, pageErrors }) => {
    await openStorage(app, page);
    await page.locator('.pd-row[data-key="cache"]').hover();
    await page.locator('.pd-row[data-key="cache"]').getByRole('button', { name: 'What is inside' }).click();
    await expect(page.getByTestId('storage-working')).toBeVisible();
    const tree = page.getByTestId('storage-tree');
    await expect(tree).toContainText('What is in Cache');
    await expect(tree.getByRole('grid', { name: 'What is in Cache' })).toBeVisible();
    // The biggest folder first, with its contents below its heading; the smallest items are one line.
    await expect(tree).toContainText('thumbnails');
    await expect(tree).toContainText('7 smaller items');
    await expect(tree.getByRole('button', { name: 'thumbnails' }).first()).toHaveAttribute('aria-expanded', 'true');
    expect(await axeViolations(page)).toEqual([]);
    await tree.getByRole('button', { name: 'thumbnails' }).first().click();
    await expect(tree.getByRole('button', { name: 'thumbnails' }).first()).toHaveAttribute('aria-expanded', 'false');
    // The folder's size is on its heading and the title has a Close.
    await expect(tree.locator('.pd-group-summary.pd-end').first()).toContainText('MB');
    await expect(tree).toContainText('items');
    // A column's header and its values share one right edge, whether or not the body scrolls.
    const edge = async (loc: import('@playwright/test').Locator): Promise<number> => Math.round((await loc.boundingBox())!.x + (await loc.boundingBox())!.width);
    const header = await edge(tree.getByRole('columnheader', { name: /Size/ }).locator('.pd-th-text'));
    const value = await edge(tree.locator('.pd-row[data-key] .pd-cell.pd-end .pd-text').first());
    expect(Math.abs(header - value)).toBeLessThanOrEqual(1);
    await tree.getByTestId('storage-tree-close').click();
    await expect(tree).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });
});
