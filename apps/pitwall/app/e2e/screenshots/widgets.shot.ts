import { expect, test, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { dialogWindowUrl, waitForStableFrame } from '../../../../../ui/projects/paddock-e2e/src/index.ts';

/**
 * The tray popup's widgets and their chooser, light and dark at 2x, for the independent review (rule 22) and the docs: the
 * default popup in its calm, empty and worst states, the chooser narrow, and the chooser wide with the live preview. Only on
 * request: `SHOTS_OUT=<folder> npx playwright test -c e2e/screenshots/playwright.config.ts widgets`.
 */
const OUT = process.env['SHOTS_OUT'] ?? resolve(__dirname, '../../.angular/widgets-shots');
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
        await waitForStableFrame(page);
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
      await waitForStableFrame(page);
      // Only the popup and its menu, not the empty page around them.
      const [a, b] = await Promise.all([popup.boundingBox(), page.locator('.mat-mdc-menu-panel').boundingBox()]);
      const x = Math.max(0, Math.min(a!.x, b!.x) - 8);
      const y = Math.max(0, Math.min(a!.y, b!.y) - 8);
      const right = Math.max(a!.x + a!.width, b!.x + b!.width) + 8;
      const bottom = Math.max(a!.y + a!.height, b!.y + b!.height) + 8;
      await page.screenshot({ path: `${OUT}/tray-popup-menu-${theme}.png`, clip: { x, y, width: right - x, height: bottom - y } });
    });

    test('chooser in Settings', async ({ page }) => {
      await prepare(page, theme);
      await page.setViewportSize({ width: 880, height: 1800 });
      await page.goto(dialogWindowUrl('pd.settings', { page: 'pw.tray' }, { title: 'Settings', query: '?mock-static&mock-fast' }));
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      await expect(page.locator('pd-widget-chooser')).toBeVisible();
      await expect(page.locator('pd-skeleton')).toHaveCount(0);
      await page.evaluate(async () => document.fonts.ready);
      await page.mouse.move(2, 2);
      await waitForStableFrame(page);
      // The page's own box (title, the chooser and its preview), not the empty window below it.
      await page.locator('.page').first().screenshot({ path: `${OUT}/tray-chooser-${theme}.png` });
    });

    // Wide enough for the live preview beside the list (the chooser draws it from an 880 px container, so about 1300 px of window).
    test('chooser with the live preview', async ({ page }) => {
      await prepare(page, theme);
      await page.setViewportSize({ width: 1700, height: 1250 });
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
      await waitForStableFrame(page);
      await page.screenshot({ path: `${OUT}/tray-chooser-preview-${theme}.png` });
    });
  });
}
