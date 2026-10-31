import { expect, test } from '@decryp7/paddock-e2e';

/**
 * The default tray popup fits the family's 520 px cap whole, with nothing to scroll, in its calm, empty and worst states
 * (failures, a check running, paused notifications). A normal browser e2e run (CI's chromium project), not a screenshot: a layout that
 * grows past the cap fails here. The mock serves the engine's recorded output for each scenario.
 */
for (const [name, url] of [
  ['demo', '/#/tray'],
  ['empty', '/?mock=empty#/tray'],
  ['worst', '/?mock=worst#/tray'],
] as const) {
  test(`the default popup fits 520 px without scrolling: ${name}`, async ({ page }) => {
    await page.setViewportSize({ width: 372, height: 1000 });
    await page.goto(url);
    const popup = page.locator('pd-tray-popover');
    await expect(popup).toBeVisible();
    await expect(popup.locator('section.pd-w').first()).toBeVisible();
    await expect(popup.locator('[aria-busy="true"]')).toHaveCount(0);
    await page.evaluate(async () => document.fonts.ready);
    const box = await popup.boundingBox();
    expect(Math.floor(box?.height ?? 0), `${name} is at most 520 px tall`).toBeLessThanOrEqual(520);
    const overflow = await popup.locator('.pd-content').evaluate((e) => e.scrollHeight - e.clientHeight);
    expect(overflow, `${name} needs no scrolling`).toBeLessThanOrEqual(0);
  });
}
