import { expect, test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';

// ER-21: nothing covers an interactive element on the main tables and pages (the shared check of @decryp7/paddock-e2e, ADR 0027).
const APP = '/?simulate=0#';

for (const [path, heading] of [
  ['/', 'Dashboard'],
  ['/runners', 'Runners'],
  ['/updates', 'Updates'],
  ['/logs', 'Logs'],
] as const) {
  test(`${path}: nothing covers an interactive element`, async ({ page }) => {
    await page.goto(`${APP}${path}`);
    await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
    await expectNothingCovers(page.locator('body'));
  });
}
