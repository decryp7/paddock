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
          await expect(card.locator('pd-status-pill')).toBeVisible();
          await card.screenshot({ path: `${OUT}/card-${helper}-${scheme}.png` });
        });
      }
      for (const os of ['windows', 'linux'] as const) {
        test(`${os} card, off`, async ({ page }) => {
          mkdirSync(OUT, { recursive: true });
          await page.setViewportSize({ width: 880, height: 760 });
          await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: `?latency=0&mock=${os}&helper=off` }));
          await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          await page.getByText('Dyno', { exact: true }).first().click();
          const card = page.getByRole('region', { name: 'Administrator readings' });
          await expect(card.locator('pd-status-pill')).toBeVisible();
          await card.screenshot({ path: `${OUT}/card-off-${os}-${scheme}.png` });
        });
      }
      test('turning on, mid-job (the prompt phase cannot be cancelled)', async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.emulateMedia({ reducedMotion: 'reduce' });
        await page.setViewportSize({ width: 880, height: 760 });
        await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: '?latency=400&helper=off&jobsteps=40' }));
        await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
        await page.getByText('Dyno', { exact: true }).first().click();
        const card = page.getByRole('region', { name: 'Administrator readings' });
        await card.getByRole('button', { name: 'Turn on administrator readings' }).click();
        await expect(card.locator('pd-status-pill')).toContainText('Turning on');
        await page.waitForTimeout(2500);
        await card.screenshot({ path: `${OUT}/card-turning-on-${scheme}.png` });
      });
    });
  }
});
