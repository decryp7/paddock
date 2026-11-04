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
  await page.getByRole('navigation', { name: 'Settings pages' }).getByRole('button', { name: 'Dyno', exact: true }).click();
  await page.locator('#menu-bar-items').scrollIntoViewIfNeeded();
  await expect(page.getByText('What the tray / menu-bar icon draws live.'), 'the limit sentence appears once').toHaveCount(1);
  const trigger = page.locator('mat-select[aria-label^="Format of"]').first();
  await expect(trigger).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  const closed = await trigger.locator('.mat-mdc-select-value-text').evaluate((el) => ({ cut: el.scrollWidth - el.clientWidth, text: el.textContent?.trim() }));
  expect(closed.text).toBe('Label over value');
  expect(closed.cut, 'the closed select cuts its text').toBeLessThanOrEqual(0);
  const desc = await page.locator('.pd-wc-list').first().locator('.pd-wc-txt span').first().evaluate((el) => el.getBoundingClientRect().width);
  expect(desc, 'the description column is narrow').toBeGreaterThanOrEqual(120);
  // The row stays one line: the description is not wrapped and the row's "more" button sits beside the format select.
  const row = await page.locator('.pd-wc-list').first().locator('li.pd-wc-item').first().evaluate((li) => {
    const top = (sel: string) => li.querySelector(sel)!.getBoundingClientRect();
    const d = li.querySelector('.pd-wc-txt span')!;
    return { moreDy: Math.abs(top('.pd-wc-more').top + top('.pd-wc-more').height / 2 - (top('mat-select').top + top('mat-select').height / 2)), descLines: d.getBoundingClientRect().height / parseFloat(getComputedStyle(d).lineHeight) };
  });
  expect(row.descLines, 'the description wraps').toBeLessThan(1.5);
  expect(row.moreDy, 'the row wraps its more button onto a second line').toBeLessThan(24);
  await trigger.click();
  const options = page.getByRole('option');
  await expect(options.first()).toBeVisible();
  const lines = await options.evaluateAll((os) => os.map((o) => {
    const t = o.querySelector('.mdc-list-item__primary-text') ?? o;
    const range = document.createRange();
    range.selectNodeContents(t);
    // One client rect per line box of the text.
    const tops = new Set([...range.getClientRects()].map((r) => Math.round(r.top)));
    return { text: o.textContent?.trim(), lines: tops.size };
  }));
  expect(lines.length).toBe(2);
  for (const o of lines) expect(o.lines, `${o.text} wraps`).toBe(1);
  await ctx.close();
});
