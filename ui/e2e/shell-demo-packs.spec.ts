import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// Settings > Knowledge packs (plan X15/X23) in the shell demo: the page over the demo's in-memory engine, which plays
// `paddock-packs` (the same views, health and refusals). `?packs=problems|notes|clean` picks where it starts. Chromium only in
// branches (rule 3). Only the test kit.

test.describe('knowledge packs (plan X15/X23)', () => {
  test('a refused pack is named with what to fix, Reload is the primary button, and it clears when fixed', async ({ app, page, pageErrors }) => {
    await app.settings.open({ page: 'pd.packs' }, '?packs=problems');
    await expect(page.getByTestId('packs-state-notes')).toContainText('One pack left out');
    const problem = page.getByTestId('packs-problems-notes').getByRole('alert');
    await expect(problem).toContainText('broken.toml');
    await expect(problem).toContainText('line 2, column 3');
    const reload = page.getByRole('button', { name: 'Reload' });
    await expect(reload).toHaveClass(/mat-mdc-unelevated-button/);
    // The icons are 16 px (shared glyphs, never smaller than 14).
    const size = await page.getByTestId('packs-kind-notes').locator('svg').first().boundingBox();
    expect(size?.width).toBeGreaterThanOrEqual(14);
    expect(await axeViolations(page)).toEqual([]);
    await expectNoHorizontalScroll(page);

    await reload.click();
    await expect(page.getByTestId('packs-problems-notes')).toHaveCount(0);
    await expect(page.getByTestId('packs-state-notes')).toContainText('All loaded');
    await expect(reload).not.toHaveClass(/mat-mdc-unelevated-button/);
    expect(pageErrors).toEqual([]);
  });

  test('a clean start lists built-in and the user\'s packs, with a long file name shortened in the middle', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.packs' }, '?packs=clean');
    await expect(page.getByTestId('packs-state-notes')).toContainText('All loaded');
    const rows = page.getByTestId('packs-kind-notes').locator('.pd-packs__row');
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toContainText('Built in');
    await expect(rows.nth(1)).toContainText('Yours');
    // The full name is the tooltip; the shown text is shortened without a horizontal scroll.
    await expect(rows.nth(1).getByTestId('path-text')).toHaveAttribute('title', /my-very-long-pack-file-name.*\.toml/);
    await expectNoHorizontalScroll(page);
  });

  test('keys that were ignored are listed as notes, not as problems', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.packs' }, '?packs=notes');
    await expect(page.getByTestId('packs-state-notes')).toContainText('some keys ignored');
    await expect(page.getByTestId('packs-warnings-notes')).toContainText('unknown key "extra" is ignored');
    await expect(page.getByTestId('packs-problems-notes')).toHaveCount(0);
  });
});
