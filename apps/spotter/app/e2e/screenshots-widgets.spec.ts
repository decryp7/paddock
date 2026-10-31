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

    /** The popup, drawn at the height the shell gives it (its content up to the family cap) and clipped to its own box. */
    for (const [name, url] of [
      ['tray-popup', '/#/tray'],
      ['tray-popup-empty', '/?mock=empty#/tray'],
      ['tray-popup-worst', '/?mock=worst#/tray'],
    ] as const) {
      test(name, async ({ page }) => {
        await page.setViewportSize({ width: 372, height: 1000 });
        await page.goto(url);
        const popup = page.locator('pd-tray-popover');
        await expect(popup).toBeVisible();
        // Every widget has its values (none is still loading), so the popup is shot in the state a person sees.
        await expect(popup.locator('section.pd-w').first()).toBeVisible();
        await expect(popup.locator('[aria-busy="true"]')).toHaveCount(0);
        await page.evaluate(async () => document.fonts.ready);
        // The default popup, also in its worst state (failures, a check running, paused notifications), is whole within the cap.
        const box = await popup.boundingBox();
        expect(Math.floor(box?.height ?? 0), `${name} is at most 520 px tall`).toBeLessThanOrEqual(520);
        const scroller = popup.locator('.pd-content');
        if (process.env['ALLOW_SCROLL']) test.info().annotations.push({ type: 'scroll', description: 'allowed for a look' });
        else expect(await scroller.evaluate((e) => e.scrollHeight - e.clientHeight), `${name} needs no scrolling`).toBeLessThanOrEqual(0);
        await popup.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
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

    // Wide enough for the live preview beside the list (the chooser draws it from a 880 px container, so about 1300 px of window).
    test('chooser with the live preview', async ({ page, app }) => {
      await page.setViewportSize({ width: 1700, height: 860 });
      await app.settings.open({ page: 'sp.tray-popup' });
      await expect(page.locator('pd-widget-chooser')).toBeVisible();
      const preview = page.locator('pd-tray-popover[aria-label="Preview of the popup"]');
      await expect(preview.locator('section.pd-w').first()).toBeVisible();
      // The preview is beside the list, not under it (the chooser's wide layout).
      const [list, side] = await Promise.all([page.getByText('Add a widget').boundingBox(), preview.boundingBox()]);
      expect(side!.x, 'the preview is to the right of the lists').toBeGreaterThan(list!.x + 300);
      await page.evaluate(async () => document.fonts.ready);
      await page.screenshot({ path: `${OUT}/tray-chooser-preview-${scheme}.png` });
    });
  });
}
