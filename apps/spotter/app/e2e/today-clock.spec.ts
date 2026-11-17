import { pinnedInstant } from '../../../../ui/projects/paddock-e2e/src/pinned-clock.ts';
import { expect, test } from './test';

// Regression for CI run 8154: the first-load flow failed when the real time was just after local midnight.
test('the page runs on the pinned e2e clock, so "Today" has items at any wall-clock time', async ({ page }) => {
  await page.goto('/');
  const pageNow = await page.evaluate(() => Date.now());
  expect(Math.abs(pageNow - pinnedInstant({}).getTime())).toBeLessThan(60_000);
  await expect(page.getByRole('heading', { name: 'Today' })).toBeVisible();
});

// The product at 00:19 local (a second install wins over the fixture's): the feed groups the mock's items under the days they
// fall on, so "Yesterday" has a release row and the window is not blank. "Today" is empty then, which is correct.
test('just after midnight the feed still shows content, under Yesterday', async ({ page }) => {
  await page.clock.install({ time: new Date('2026-11-16T00:19:00Z') });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: "What's new" })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Yesterday' })).toBeVisible();
  await expect(page.locator('sp-whats-new li').first()).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Today' })).toHaveCount(0);
});
