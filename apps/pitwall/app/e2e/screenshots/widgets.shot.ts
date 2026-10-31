import { expect, test, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { dialogWindowUrl } from '../../../../../ui/projects/paddock-e2e/src/index.ts';

/**
 * The tray popup's widgets and their chooser, light and dark at 2x, for the independent review (rule 22) and the docs: the
 * default popup in its calm, empty and worst states, the chooser narrow, and the chooser wide with the live preview. Only on
 * request: `SHOTS_OUT=<folder> npx playwright test -c e2e/screenshots/playwright.config.ts widgets`.
 */
const OUT = process.env['SHOTS_OUT'] ?? '/Volumes/d7sst7/dev-targets/review/pitwall-widgets-r1';
const NOW = new Date('2026-10-22T14:30:00').getTime();

async function prepare(page: Page, theme: 'dark' | 'light') {
  mkdirSync(OUT, { recursive: true });
  await page.clock.install({ time: NOW });
  await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
}

for (const theme of ['light', 'dark'] as const) {
  test.describe(`${theme} widgets`, () => {
    /** The popup, drawn at the height the shell gives it (its content up to the family cap) and clipped to its own box. */
    for (const [name, scenario] of [
      ['tray-popup', 'demo'],
      ['tray-popup-empty', 'empty'],
      ['tray-popup-worst', 'worst'],
      ['tray-popup-down', 'down'],
    ] as const) {
      test(name, async ({ page }) => {
        await prepare(page, theme);
        await page.setViewportSize({ width: 372, height: 1000 });
        await page.goto(`/?mock-static&mock-fast&mock-popup=${scenario}#/tray`);
        const popup = page.locator('pd-tray-popover');
        await expect(popup).toBeVisible();
        // Every widget has its values (none is still loading), so the popup is shot in the state a person sees.
        await expect(popup.locator('section.pd-w').first()).toBeVisible();
        await expect(popup.locator('[aria-busy="true"]')).toHaveCount(0);
        await page.evaluate(async () => document.fonts.ready);
        await page.mouse.move(2, 2);
        await page.waitForTimeout(400);
        await popup.screenshot({ path: `${OUT}/${name}-${theme}.png` });
      });
    }

    test('tray-popup-menu', async ({ page }) => {
      await prepare(page, theme);
      await page.setViewportSize({ width: 372, height: 700 });
      await page.goto('/?mock-static&mock-fast#/tray');
      const popup = page.locator('pd-tray-popover');
      await expect(popup.locator('section.pd-w').first()).toBeVisible();
      await popup.getByRole('button', { name: 'More actions' }).click();
      await expect(page.getByRole('menuitem', { name: 'Quit Pitwall' })).toBeVisible();
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${OUT}/tray-popup-menu-${theme}.png` });
    });

    test('chooser in Settings', async ({ page }) => {
      await prepare(page, theme);
      await page.setViewportSize({ width: 880, height: 700 });
      await page.goto(dialogWindowUrl('pd.settings', { page: 'pw.tray' }, { title: 'Settings', query: '?mock-static&mock-fast' }));
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      await expect(page.locator('pd-widget-chooser')).toBeVisible();
      await expect(page.locator('pd-skeleton')).toHaveCount(0);
      await page.evaluate(async () => document.fonts.ready);
      await page.mouse.move(2, 2);
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${OUT}/tray-chooser-${theme}.png` });
    });

    // Wide enough for the live preview beside the list (the chooser draws it from an 880 px container, so about 1300 px of window).
    test('chooser with the live preview', async ({ page }) => {
      await prepare(page, theme);
      await page.setViewportSize({ width: 1700, height: 860 });
      await page.goto(dialogWindowUrl('pd.settings', { page: 'pw.tray' }, { title: 'Settings', query: '?mock-static&mock-fast' }));
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      await expect(page.locator('pd-widget-chooser')).toBeVisible();
      const preview = page.locator('pd-tray-popover[aria-label="Preview of the popup"]');
      await expect(preview.locator('section.pd-w').first()).toBeVisible();
      // The preview is beside the list, not under it (the chooser's wide layout).
      const [list, side] = await Promise.all([page.getByText('Add a widget').boundingBox(), preview.boundingBox()]);
      expect(side!.x, 'the preview is to the right of the lists').toBeGreaterThan(list!.x + 300);
      await page.evaluate(async () => document.fonts.ready);
      await page.mouse.move(2, 2);
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${OUT}/tray-chooser-preview-${theme}.png` });
    });
  });
}
