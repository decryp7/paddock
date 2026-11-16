import { E2E_NOW, expect, test } from './fixtures';

// Regression for CI run 8154: the first-load flow failed when the real time was just after local midnight.
test('the page runs on the fixed e2e clock, so "Today" has items at any wall-clock time', async ({ page }) => {
  await page.goto('/');
  const pageNow = await page.evaluate(() => Date.now());
  expect(Math.abs(pageNow - E2E_NOW.getTime())).toBeLessThan(60_000);
  await expect(page.getByRole('heading', { name: 'Today' })).toBeVisible();
});
