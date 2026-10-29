import { expect, test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';

// ER-21: nothing covers an interactive element on the main pages (the shared check of @decryp7/paddock-e2e, ADR 0027).
const url = (path: string) => `/?latency=0#${path}`;

for (const [path, heading] of [
  ['/', 'Dashboard'],
  ['/processes', 'Processes'],
  ['/issues', 'Issues'],
  ['/health', 'Health'],
  ['/network', 'Network'],
  ['/counters', 'Counters'],
  ['/throttling', 'Throttling'],
  ['/lifespan', 'Disk and battery life'],
  ['/backup', 'Backup'],
  ['/settings', 'Settings'],
] as const) {
  test(`${path}: nothing covers an interactive element`, async ({ page }) => {
    await page.goto(url(path));
    await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
    await expectNothingCovers(page.locator('body'));
  });
}
