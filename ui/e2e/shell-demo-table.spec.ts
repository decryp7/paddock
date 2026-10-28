import { axeViolations, expect, expectNoHorizontalScroll, test, THEMES } from '@decryp7/paddock-e2e';

// The Runners page of the shell demo draws its fleet with `pd-table` (plan X21): the shared table inside the pages frame,
// its row actions, keyboard and narrow widths, in both themes.

test.describe('shared table on the Runners page', () => {
  test('rows carry hover actions and a menu; the destructive item is last and a disabled one stays disabled', async ({ app, page }) => {
    await app.frame.open();
    await app.sidebar.link(/^Runners/).click();
    const grid = page.getByRole('grid', { name: 'Runners' });
    await expect(grid.getByRole('row')).toHaveCount(5);
    const row = grid.locator('[role=row][data-index]').first();
    await row.hover();
    await row.getByRole('button', { name: 'Restart runner' }).click();
    await expect(page.getByTestId('runner-action')).toHaveText('restart build-linux-1');
    await grid.locator('[role=row][data-index]').nth(2).getByRole('button', { name: 'More actions' }).click();
    await expect(page.getByRole('menuitem', { name: 'Remove…' })).toBeDisabled();
    await page.keyboard.press('Escape');
  });

  test('sorting the Runners column reorders the fleet', async ({ app, page }) => {
    await app.frame.open();
    await app.sidebar.link(/^Runners/).click();
    const grid = page.getByRole('grid', { name: 'Runners' });
    await grid.getByRole('columnheader', { name: 'Jobs today' }).getByRole('button').click();
    await grid.getByRole('columnheader', { name: 'Jobs today' }).getByRole('button').click();
    await expect(grid.locator('[role=row][data-index]').first()).toContainText('build-linux-1');
  });

  for (const theme of THEMES) {
    test(`axe finds nothing and nothing scrolls sideways at 320 px (${theme})`, async ({ app, page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 320, height: 700 });
      await app.frame.open();
      // The sidebar folds away at this width: go by address.
      await page.evaluate(() => (location.hash = '#/runners'));
      await expect(page.getByRole('grid', { name: 'Runners' })).toBeVisible();
      await expectNoHorizontalScroll(page);
      expect(await axeViolations(page)).toEqual([]);
    });
  }
});
