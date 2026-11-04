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
  expect(desc, 'the description column is narrow').toBeGreaterThanOrEqual(100);
  // The row stays one line: the description is not wrapped and the row's "more" button sits beside the format select.
  const row = await page.locator('.pd-wc-list').first().locator('li.pd-wc-item').first().evaluate((li) => {
    const top = (sel: string) => li.querySelector(sel)!.getBoundingClientRect();
    const d = li.querySelector('.pd-wc-txt span')!;
    return { moreDy: Math.abs(top('.pd-wc-more').top + top('.pd-wc-more').height / 2 - (top('mat-select').top + top('mat-select').height / 2)), descLines: d.getBoundingClientRect().height / parseFloat(getComputedStyle(d).lineHeight) };
  });
  expect(row.descLines, 'the description wraps').toBeLessThan(1.5);
  expect(row.moreDy, 'the row wraps its more button onto a second line').toBeLessThan(24);
  // At least 8 px between the text and the caret.
  const gap = await trigger.evaluate((el) => {
    const range = document.createRange();
    range.selectNodeContents(el.querySelector('.mat-mdc-select-value-text')!);
    return el.querySelector('.mat-mdc-select-arrow-wrapper')!.getBoundingClientRect().left - range.getBoundingClientRect().right;
  });
  expect(gap, 'the text touches the caret').toBeGreaterThanOrEqual(8);
  // The longest metric title fits its select.
  const metric = page.locator('mat-select[aria-label^="Metric of"]').first();
  await metric.click();
  await page.getByRole('option', { name: 'Memory pressure' }).click();
  // The choice goes to the engine and back: wait on the shown value, never on time.
  const metricText = metric.locator('.mat-mdc-select-value-text');
  await expect(metricText).toHaveText('Memory pressure');
  const metricCut = await metricText.evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(metricCut, 'the metric select cuts "Memory pressure"').toBeLessThanOrEqual(0);
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

/**
 * Where the tray draws one layout there is no per-row format text: the info note says it once, and the row is the metric, Label,
 * Unit and "more". At 980 px each row is one line; at 760 px the controls wrap as a group onto a second line under the
 * description's column, with "more" beside them.
 */
for (const [tray, noteText] of [[16, '16 px, too small for a label'], [24, 'one number layout']] as const) {
  test(`a Windows ${tray} px tray: rows say nothing per row, stay on one line, and wrap under the description`, async ({ browser, baseURL }) => {
    for (const width of [980, 760]) {
      const ctx = await browser.newContext({ baseURL, viewport: { width, height: 900 } });
      const page = await ctx.newPage();
      await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: `?mock=windows&tray=${tray}` }));
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      await page.getByRole('navigation', { name: 'Settings pages' }).getByRole('button', { name: 'Dyno', exact: true }).click();
      await expect(page.locator('mat-select[aria-label^="Format of"]')).toHaveCount(0);
      await expect(page.locator('p.info[role="note"]')).toContainText(noteText);
      const rows = page.locator('.pd-wc-list').first().locator('li.pd-wc-item');
      await expect(rows).toHaveCount(4);
      await expect(rows.first().locator('.fixed'), 'the note says it once; no per-row text').toHaveCount(0);
      const found = await rows.evaluateAll((lis) =>
        lis.map((li) => {
          const box = (sel: string) => li.querySelector(sel)!.getBoundingClientRect();
          const desc = li.querySelector('.pd-wc-txt span')!;
          const metric = box('mat-select');
          const more = box('.pd-wc-more');
          const label = box('input[aria-label^="Label of"]');
          const unit = box('input[aria-label^="Unit of"]');
          const d = box('.pd-wc-txt');
          return {
            descLines: desc.getBoundingClientRect().height / parseFloat(getComputedStyle(desc).lineHeight),
            moreDy: Math.abs(more.top + more.height / 2 - (metric.top + metric.height / 2)),
            // Label and Unit stay together on one line.
            pairDy: Math.abs(label.top - unit.top),
            // No control sits left of the description's column.
            leftOfText: Math.min(metric.left, label.left, unit.left) - d.left,
            height: li.getBoundingClientRect().height,
          };
        }),
      );
      for (const r of found) {
        expect(r.descLines, `${width}: the description wraps`).toBeLessThan(1.5);
        expect(r.moreDy, `${width}: "more" is not level with the controls`).toBeLessThan(24);
        expect(r.pairDy, `${width}: Label and Unit are on different lines`).toBeLessThan(4);
        expect(r.leftOfText, `${width}: a control sits left of the description column`).toBeGreaterThanOrEqual(-2);
        if (width === 980) expect(r.height, 'the row wraps at 980 px').toBeLessThan(70);
      }
      await ctx.close();
    }
  });
}
