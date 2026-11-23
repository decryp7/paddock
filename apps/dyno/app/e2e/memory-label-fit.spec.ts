import { expect, test } from './test';
import { dialogWindowUrl, newPinnedContext, textClippedWithoutEllipsis, textCutOffWithoutTooltip } from '@decryp7/paddock-e2e';
import type { Page } from '@playwright/test';

/**
 * The CPU and memory widget's "Memory 24.8 of 48 GB" label is as wide as the live value: it was cut by 3 px in the Settings
 * preview (run 8674, chromium, 1280x800; dyno-release.yml 8446, webkit, 900x600) with no tooltip to give the whole text. The mock
 * pins the widest realistic values (`?memory=88.8:99.9`, "Memory 88.8 of 99.9 GB", and `?memory=888.8:999`, "Memory 889 of 999 GB": the format keeps three digits), so the check never depends on the value of the
 * moment: whatever is cut says so (an ellipsis with its full text), in the popup and in the Settings preview of it.
 */
const WIDEST = [
  { query: 'memory=88.8:99.9', label: 'Memory 88.8 of 99.9 GB' },
  { query: 'memory=888.8:999', label: 'Memory 889 of 999 GB' },
];
const SIZES = [
  { width: 900, height: 600 },
  { width: 1280, height: 800 },
];

async function cut(page: Page): Promise<string[]> {
  await page.evaluate(() => document.fonts.ready);
  return [...(await textCutOffWithoutTooltip(page)), ...(await textClippedWithoutEllipsis(page))];
}

for (const { width, height } of SIZES) for (const { query, label: text } of WIDEST) {
  test(`"${text}" is not cut off in the Settings preview at ${width}x${height}`, async ({ browser, baseURL }) => {
    const ctx = await newPinnedContext(browser, { baseURL, viewport: { width: Math.max(width, 900), height } });
    const page = await ctx.newPage();
    await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: `?latency=0&${query}` }));
    await page.locator('html[data-dialog-ready]').waitFor();
    await page.locator('nav.pd-nav').getByText('Dyno', { exact: true }).click();
    const label = page.locator('dy-popup-widgets .dy-pw-half .dy-pw-muted', { hasText: text });
    await expect(label, 'the preview shows the pinned memory value').toBeVisible();
    expect(await cut(page)).toEqual([]);
    await ctx.close();
  });
}

for (const { query, label: text } of WIDEST) {
  test(`"${text}" is not cut off in the popup`, async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 640 });
    await page.goto(`/?latency=0&${query}#/tray`);
    await expect(page.locator('.dy-pw-muted', { hasText: text })).toBeVisible();
    expect(await cut(page)).toEqual([]);
  });
}
