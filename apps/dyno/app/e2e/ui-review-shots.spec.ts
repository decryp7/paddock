import { expect, pressMod, test } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Review screenshots at 150% for the independent review (rule 22): the disk-map settings button and the dashboard's Health widget, dark and
 * light. Set UI_REVIEW_SHOTS to a folder to write them; without it the tests skip.
 */
const SHOTS = process.env['UI_REVIEW_SHOTS'];
const LABEL = process.env['UI_REVIEW_LABEL'] ?? 'after';

test.use({ deviceScaleFactor: 1.5 });

for (const theme of ['dark', 'light'] as const) {
  test(`review shots: button label and Health widget (${theme})`, async ({ page }) => {
    test.skip(!SHOTS, 'set UI_REVIEW_SHOTS to a folder');
    mkdirSync(SHOTS!, { recursive: true });
    await page.emulateMedia({ colorScheme: theme });
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto('/?latency=0#/');
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible({ timeout: 45_000 });
    await pressMod(page, 'Mod+Comma');
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('button', { name: 'Dyno', exact: true }).dispatchEvent('click');
    const row = dialog.getByRole('button', { name: 'Forget saved maps' }).locator('xpath=ancestor::div[contains(@class,"row")][1]');
    await expect(row).toBeVisible({ timeout: 45_000 });
    await page.evaluate(() => document.fonts.ready);
    await row.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(SHOTS!, `${LABEL}-button-${theme}.png`), clip: (await row.boundingBox())!, animations: 'disabled' });
    await page.keyboard.press('Escape');

    await page.goto('/?latency=0#/');
    const health = page.locator('[data-pd-cell]').filter({ hasText: 'What holds it back' }).first();
    await expect(health).toBeVisible({ timeout: 45_000 });
    await expect(page.locator('pd-widget-frame [aria-busy="true"], pd-widget-frame[aria-busy="true"]')).toHaveCount(0, { timeout: 30_000 });
    await health.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(SHOTS!, `${LABEL}-health-${theme}.png`), clip: (await health.boundingBox())!, animations: 'disabled' });

    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto('/?latency=0#/tray');
    await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible({ timeout: 45_000 });
    await page.screenshot({ path: join(SHOTS!, `${LABEL}-popup-${theme}.png`), animations: 'disabled' });
  });
}
