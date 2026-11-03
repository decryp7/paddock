import { mkdirSync } from 'node:fs';
import { expect, test } from '@playwright/test';
import { dialogWindowUrl } from '@decryp7/paddock-e2e';

// The Administrator readings card in every state, light and dark, for the independent review (ER-23). Only on request:
// `SCREENSHOTS=1 HELPER_SHOTS_DIR=<dir> npx playwright test --project=screenshots e2e/screenshots-helper.spec.ts`.
const OUT = process.env['HELPER_SHOTS_DIR'] ?? '';
const STATES = ['off', 'silent', 'foreign', 'outdated', 'ready', 'notIncluded'] as const;

test.describe('Administrator readings card screenshots', () => {
  test.skip(!OUT, 'set HELPER_SHOTS_DIR');
  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });
      for (const helper of STATES) {
        test(`state ${helper}`, async ({ page }) => {
          mkdirSync(OUT, { recursive: true });
          await page.setViewportSize({ width: 880, height: 760 });
          await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: `?latency=0&helper=${helper}` }));
          await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          await page.getByText('Dyno', { exact: true }).first().click();
          const card = page.getByRole('region', { name: 'Administrator readings' });
          await expect(card.locator('.chip')).toBeVisible();
          await card.screenshot({ path: `${OUT}/card-${helper}-${scheme}.png` });
        });
      }
      test('turning on, mid-job (the prompt phase cannot be cancelled)', async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.setViewportSize({ width: 880, height: 760 });
        await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: '?latency=400&helper=off&jobsteps=40' }));
        await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
        await page.getByText('Dyno', { exact: true }).first().click();
        const card = page.getByRole('region', { name: 'Administrator readings' });
        await card.getByRole('button', { name: 'Turn on administrator readings' }).click();
        await expect(card.locator('.running')).toContainText('Turning on administrator readings');
        await page.waitForTimeout(2500);
        await card.screenshot({ path: `${OUT}/card-turning-on-${scheme}.png` });
      });
    });
  }
});
