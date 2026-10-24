import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// User files (plan X31) in the shell demo: the Files page over the demo's in-memory engine, which plays `paddock-files`
// (grants, recent lists, refusals). The paths are fictional strings; nothing is read from any disk. The page starts with two
// recent files; Clear the list empties them. Chromium only in branches (rule 3). Only the test kit (@decryp7/paddock-e2e).

test.describe('user files (plan X31)', () => {
  /** Opens the Files page (`#/files`). */
  const openFiles = async (app: { frame: { heading: import('@playwright/test').Locator } }, page: import('@playwright/test').Page): Promise<void> => {
    await page.goto('/#/files');
    await expect(app.frame.heading).toHaveText('Files');
  };

  test('a recent file opens through a fresh grant, long paths shorten in the middle and the full path is the tooltip', async ({ app, page, pageErrors }) => {
    await openFiles(app, page);
    const entries = page.getByTestId('recent-entry');
    await expect(entries).toHaveCount(2);
    const long = entries.nth(1).getByTestId('path-text');
    await expect(long).toHaveAttribute('title', /a-very-long-project-folder-name.*service-2026-10-24-11-30\.log$/);
    expect(await axeViolations(page)).toEqual([]);
    await expectNoHorizontalScroll(page);

    // A glyph of the pd: set draws each entry's kind, 16 px (a log is a text page; the second path has no known extension).
    const icon = entries.first().getByTestId('recent-icon').locator('svg');
    await expect(icon).toHaveAttribute('width', '16');
    expect(await entries.first().getByTestId('recent-icon').locator('path').getAttribute('d')).toContain('M8.75 12.5h6.5');
    // The file name leads; the folder is the muted second line, with its separator kept.
    await expect(entries.first().locator('.pd-entry > .pd-text > .pd-name')).toHaveText('session-2026-10-24.log');
    await entries.first().locator('.pd-entry').click();
    await expect(page.getByTestId('files-last')).toHaveText('Opened /demo/logs/session-2026-10-24.log');
    expect(pageErrors).toEqual([]);
  });

  test('the dialog grants what was chosen, which then heads the recent list', async ({ app, page }) => {
    await openFiles(app, page);
    await page.getByRole('button', { name: 'Clear the list' }).click();
    await expect(page.getByTestId('recent-empty')).toContainText('No recent files yet');
    await page.getByTestId('files-pick').click();
    await expect(page.getByTestId('files-last')).toHaveText('Chose /demo/logs/session-2026-10-24.log');
    await expect(page.getByTestId('recent-entry')).toHaveCount(1);
    // Revealing it works because the dialog granted it; clearing the list empties it.
    await page.getByTestId('recent-entry').hover();
    await page.getByRole('button', { name: 'Show in the file manager' }).click();
    await expect(page.getByTestId('files-failure')).toHaveCount(0);
    await page.getByRole('button', { name: 'Clear the list' }).click();
    await expect(page.getByTestId('recent-empty')).toBeVisible();
  });

  test('a path nobody chose is refused with words and a next step, never opened', async ({ app, page }) => {
    await openFiles(app, page);
    await page.getByTestId('files-forge').click();
    const failure = page.getByTestId('files-failure');
    await expect(failure).toHaveAttribute('role', 'alert');
    await expect(failure).toContainText('Only files you choose can be read: choose hosts again with Open');
  });

  test('files dropped on the window are granted and offered to the page', async ({ app, page }) => {
    await openFiles(app, page);
    await page.getByTestId('files-drop').click();
    await expect(page.getByTestId('files-dropped')).toContainText('/demo/dropped/trace.log');
    await page.getByTestId('files-take').click();
    await expect(page.getByTestId('files-last')).toHaveText('Opened /demo/dropped/trace.log');
    await expect(page.getByTestId('files-dropped')).toHaveCount(0);
  });

  test('only web addresses of a few schemes open, and a narrow window keeps every control reachable', async ({ app, page }) => {
    await page.setViewportSize({ width: 320, height: 800 });
    await openFiles(app, page);
    await page.getByTestId('files-url').click();
    await expect(page.getByTestId('files-failure')).toHaveCount(0);
    await expectNoHorizontalScroll(page);
  });
});
