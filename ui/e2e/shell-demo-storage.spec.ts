import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// Settings > Storage (plan X32) in the shell demo: the page over the demo's in-memory engine, which plays `paddock-storage` (the
// same rules: a clear needs a scan, the user's data needs a confirmation, one job at a time, Cancel between items) with
// fictional sizes. The confirmation is a window of its own (in the page in the browser build). Chromium only in branches
// (rule 3). Only the test kit (@decryp7/paddock-e2e).

test.describe('storage (plan X32)', () => {
  /** The Settings window in the page, on the Storage page, with the first measurement done. */
  async function openStorage(app: import('@decryp7/paddock-e2e').ShellApp, page: import('@playwright/test').Page): Promise<void> {
    await page.goto('/');
    await page.getByRole('button', { name: 'Settings…', exact: true }).click();
    await app.settings.nav.getByRole('button', { name: 'Storage', exact: true }).click();
    await expect(page.getByTestId('storage-table')).toBeVisible();
  }

  test('lists what the app keeps, asks before clearing the cache and says what it freed', async ({ app, page, pageErrors }) => {
    await openStorage(app, page);
    await expect(page.locator('tbody tr')).toHaveCount(3);
    await expect(page.getByTestId('storage-total')).toContainText('≥');
    await expect(page.getByTestId('storage-row-models')).toContainText('≥');
    expect(await axeViolations(page)).toEqual([]);

    await page.getByTestId('storage-clear-cache').click();
    const confirm = page.getByRole('dialog', { name: 'Clear Cache?' });
    await expect(confirm).toContainText('The app rebuilds it when it needs it.');
    await expect(confirm.getByRole('button', { name: 'Keep' })).toBeFocused();
    expect(await axeViolations(page)).toEqual([]);
    // Escape keeps everything.
    await page.keyboard.press('Escape');
    await expect(confirm).toHaveCount(0);
    await expect(page.getByTestId('storage-working')).toHaveCount(0);

    await page.getByTestId('storage-clear-cache').click();
    await page.getByRole('dialog', { name: 'Clear Cache?' }).getByRole('button', { name: 'Clear', exact: true }).click();
    await expect(page.getByTestId('storage-working')).toContainText('Removing');
    await expect(page.getByTestId('storage-still-working')).toContainText('Still working');
    await expect(page.getByTestId('storage-result')).toContainText('freed');
    // The table is measured again by itself, so the cache row is empty and nothing is left to clear there.
    await expect(page.getByTestId('storage-working')).toHaveCount(0);
    await expect(page.getByTestId('storage-clear-cache')).toBeDisabled();
    expect(pageErrors).toEqual([]);
  });

  test("the user's own data names itself in the confirmation and is cleared one category at a time", async ({ app, page }) => {
    await openStorage(app, page);
    await page.getByTestId('storage-clear-logs').click();
    const confirm = page.getByRole('dialog', { name: 'Clear Logs?' });
    await expect(confirm.getByTestId('storage-confirm-data')).toContainText('your own data');
    await confirm.getByRole('button', { name: 'Clear', exact: true }).click();
    await expect(page.getByTestId('storage-result')).toContainText('freed');
    await expect(page.getByTestId('storage-row-logs')).toBeVisible();
  });

  test('Cancel stops a clear between items and says what was done', async ({ app, page }) => {
    await openStorage(app, page);
    await page.getByTestId('storage-clear-logs').click();
    await page.getByRole('dialog', { name: 'Clear Logs?' }).getByRole('button', { name: 'Clear', exact: true }).click();
    await page.getByTestId('storage-cancel').click();
    await expect(page.getByTestId('storage-result')).toContainText('Stopped');
  });

  test('at 320 px nothing scrolls sideways and every action keeps a name', async ({ app, page }) => {
    await page.setViewportSize({ width: 320, height: 700 });
    await openStorage(app, page);
    await expectNoHorizontalScroll(page);
    await expect(page.getByRole('button', { name: 'Show Cache in the file manager' })).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
  });

  test('the Folders button shows what is in a category as a table grouped by folder, which folds away', async ({ app, page, pageErrors }) => {
    await openStorage(app, page);
    await page.getByTestId('storage-tree-cache').click();
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
    // Asking again hides it.
    await page.getByTestId('storage-tree-cache').click();
    await expect(tree).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });
});
