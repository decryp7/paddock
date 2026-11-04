import { expect, test } from '@playwright/test';
import { dialogWindowUrl } from '@decryp7/paddock-e2e';

/**
 * Settings > Menu bar items: a number row shows its format whole ("Label over value" is neither cut nor wrapped, in the closed
 * select or the open list), and the kind's description keeps a readable width. Waits are on conditions, never sleeps.
 */
test('a number row shows its format and description whole', async ({ browser, baseURL }) => {
  const ctx = await browser.newContext({ baseURL, viewport: { width: 980, height: 900 } });
  const page = await ctx.newPage();
  await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
  await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
  await page.locator('nav.pd-nav').getByText('Menu bar', { exact: true }).first().click();
  const trigger = page.locator('mat-select[aria-label^="Format of"]').first();
  await expect(trigger).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  const closed = await trigger.locator('.mat-mdc-select-value-text').evaluate((el) => ({ cut: el.scrollWidth - el.clientWidth, text: el.textContent?.trim() }));
  expect(closed.text).toBe('Label over value');
  expect(closed.cut, 'the closed select cuts its text').toBeLessThanOrEqual(0);
  const desc = await page.locator('.pd-wc-list').first().locator('.pd-wc-txt span').first().evaluate((el) => el.getBoundingClientRect().width);
  expect(desc, 'the description column is narrow').toBeGreaterThanOrEqual(160);
  await trigger.click();
  const options = page.getByRole('option');
  await expect(options.first()).toBeVisible();
  const lines = await options.evaluateAll((os) => os.map((o) => {
    const t = o.querySelector('.mdc-list-item__primary-text') ?? o;
    const lh = parseFloat(getComputedStyle(t).lineHeight) || 20;
    return { text: o.textContent?.trim(), lines: Math.round(t.getBoundingClientRect().height / lh) };
  }));
  expect(lines.length).toBe(2);
  for (const o of lines) expect(o.lines, `${o.text} wraps`).toBe(1);
  await ctx.close();
});
