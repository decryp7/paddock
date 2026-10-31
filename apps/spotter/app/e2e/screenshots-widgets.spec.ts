import { mkdirSync } from 'node:fs';
import { expect, test } from '@decryp7/paddock-e2e';

/**
 * The tray popup widgets and their chooser, light and dark at 2x, for the independent review (rule 22). Only on request:
 * `SCREENSHOTS=1 SHOTS_OUT=<folder> npx playwright test --project=screenshots e2e/screenshots-widgets.spec.ts`.
 */
const OUT = process.env['SHOTS_OUT'] ?? '../docs/screenshots';
const NOW = new Date('2026-10-22T12:00:00Z');

for (const scheme of ['light', 'dark'] as const) {
  test.describe(`${scheme} widgets`, () => {
    test.use({ colorScheme: scheme, timezoneId: 'UTC', locale: 'en-US' });
    test.beforeEach(async ({ page }) => {
      await page.clock.setFixedTime(NOW);
      mkdirSync(OUT, { recursive: true });
    });

    /** The popup, sized to its content like the shell sizes the window. */
    for (const [name, url] of [
      ['tray-popup', '/#/tray'],
      ['tray-popup-empty', '/?mock=empty#/tray'],
    ] as const) {
      test(name, async ({ page }) => {
        await page.setViewportSize({ width: 372, height: 1000 });
        await page.goto(url);
        await expect(page.locator('pd-tray-popover')).toBeVisible();
        // Every widget has its values (none is still loading), so the popup is shot in the state a person sees.
        await expect(page.locator('pd-tray-popover section.pd-w').first()).toBeVisible();
        await expect(page.locator('pd-tray-popover [aria-busy="true"]')).toHaveCount(0);
        await page.evaluate(async () => document.fonts.ready);
        const height = await page.locator('pd-tray-popover').evaluate((e) => Math.ceil(e.getBoundingClientRect().height));
        await page.setViewportSize({ width: 372, height: height + 12 });
        await page.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
      });
    }

    test('chooser in Settings', async ({ page, app }) => {
      await page.setViewportSize({ width: 880, height: 640 });
      await app.settings.open({ page: 'sp.tray-popup' });
      await expect(page.locator('pd-widget-chooser')).toBeVisible();
      await expect(page.locator('pd-skeleton')).toHaveCount(0);
      await page.evaluate(async () => document.fonts.ready);
      await page.screenshot({ path: `${OUT}/tray-chooser-${scheme}.png` });
    });
  });
}
