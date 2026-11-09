import { expect, test } from '@playwright/test';
import { dialogWindowUrl } from '@decryp7/paddock-e2e';

/**
 * Settings > Menu bar items: a number row shows its style whole ("Number" is neither cut nor wrapped, in the
 * closed select or the open list), and the kind's description keeps a readable width. Waits are on conditions, never sleeps.
 */
test('a number row shows its style and description whole', async ({ browser, baseURL }) => {
  const ctx = await browser.newContext({ baseURL, viewport: { width: 980, height: 900 } });
  const page = await ctx.newPage();
  await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
  await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
  await page.getByRole('navigation', { name: 'Settings pages' }).getByRole('button', { name: 'Dyno', exact: true }).click();
  await page.locator('#menu-bar-items').scrollIntoViewIfNeeded();
  await expect(page.getByText('What the tray / menu-bar icon draws live.'), 'the limit sentence appears once').toHaveCount(1);
  const trigger = page.locator('mat-select[aria-label^="Style of"]').first();
  await expect(trigger).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  const closed = await trigger.locator('.mat-mdc-select-value-text').evaluate((el) => ({ cut: el.scrollWidth - el.clientWidth, text: el.textContent?.trim() }));
  expect(closed.text).toBe('Number');
  expect(closed.cut, 'the closed select cuts its text').toBeLessThanOrEqual(0);
  const desc = await page.locator('.pd-wc-list').first().locator('.pd-wc-txt span').first().evaluate((el) => el.getBoundingClientRect().width);
  expect(desc, 'the description column is narrow').toBeGreaterThanOrEqual(96);
  // The row stays one line: the description is not wrapped and the row's "more" button sits beside the style select.
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
  // Every style of the catalogue, the number as two (its formats).
  expect(lines.length).toBe(7);
  for (const o of lines) expect(o.lines, `${o.text} wraps`).toBe(1);
  // The longest style fits the closed select too (the change goes to the engine and back).
  await page.getByRole('option', { name: 'Graph and number' }).click();
  const longest = page.locator('mat-select[aria-label^="Style of"]').first().locator('.mat-mdc-select-value-text');
  await expect(longest).toHaveText('Graph and number');
  expect(await longest.evaluate((el) => el.scrollWidth - el.clientWidth), 'the style select cuts "Graph and number"').toBeLessThanOrEqual(0);
  await ctx.close();
});

/**
 * The whole matrix in one place (platform x window width x kind): every row of the menu bar list, with the kinds with the
 * fewest and the most controls and the longest description. Each row keeps its description whole, wraps its controls under the
 * description's column (never left of it), keeps "more" on the last line of controls, and is one line wide, two at most narrow.
 */
const PLATFORMS = [
  { name: 'mac', query: '', note: null },
  { name: 'win16', query: '?mock=windows&tray=16', note: '16 px, too small for a label' },
  { name: 'win24', query: '?mock=windows&tray=24', note: 'one number layout' },
] as const;
for (const platform of PLATFORMS) {
  for (const width of [980, 760]) {
    test(`menu bar rows: ${platform.name} at ${width} px (number, line graph, meter, graph and number)`, async ({ browser, baseURL }) => {
      const ctx = await browser.newContext({ baseURL, viewport: { width, height: 900 } });
      const page = await ctx.newPage();
      await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: platform.query }));
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      await page.getByRole('navigation', { name: 'Settings pages' }).getByRole('button', { name: 'Dyno', exact: true }).click();
      const rows = page.locator('.pd-wc-list').first().locator('li.pd-wc-item');
      await expect(rows).toHaveCount(4);
      for (const [i, kind] of ['Line graph', 'Meter', 'Graph and number'].entries()) {
        await page.getByRole('button', { name: `Add ${kind}`, exact: true }).click();
        await expect(rows).toHaveCount(5 + i);
      }
      // A style select on every row but the network and temperature ones, which say what they draw.
      await expect(page.locator('mat-select[aria-label^="Style of"]')).toHaveCount(5);
      await expect(page.locator('[data-testid="style-fixed"]')).toHaveCount(2);
      await expect(page.locator('mat-select[aria-label^="Style of"]').first().locator('.mat-mdc-select-value-text')).toHaveText('Number');
      if (platform.note) await expect(page.locator('p.info[role="note"]')).toContainText(platform.note);
      // The keyboard hint names this OS's modifier (the shell's data-platform; the mock sets it for its OS).
      await expect(page.locator('dy-menu-bar-items .pd-wc-hint')).toContainText(platform.name === 'mac' ? 'hold Option' : 'hold Alt');
      await expect(rows.first().locator('.fixed'), 'the note says it once; no per-row text').toHaveCount(0);
      const found = await rows.evaluateAll((lis) =>
        lis.map((li) => {
          const body = li.querySelector('.pd-wc-body') ?? li;
          const parts = [...body.children].filter((c) => !c.matches('.pd-wc-lead, .pd-wc-grip, .pd-wc-kicon, .cdk-drag-placeholder')) as HTMLElement[];
          const txt = li.querySelector('.pd-wc-txt')!.getBoundingClientRect();
          const more = li.querySelector('.pd-wc-more')!.getBoundingClientRect();
          const mid = (r: DOMRect) => r.top + r.height / 2;
          const lineMids = parts.map((c) => mid(c.getBoundingClientRect()));
          // Parts on one line share a centre line (to a few px); count the distinct ones.
          const lines = [...lineMids].sort((a, b) => a - b).reduce<number[]>((acc, m) => (acc.length && m - acc[acc.length - 1]! < 12 ? acc : [...acc, m]), []);
          const controls = parts.filter((c) => !c.matches('.pd-wc-txt, .pd-wc-more')).map((c) => c.getBoundingClientRect());
          const desc = li.querySelector('.pd-wc-txt span')!;
          return {
            title: li.querySelector('.pd-wc-txt b')!.textContent,
            lines: lines.length,
            // "more" is level with a line of controls, and that is the last line.
            moreLevel: controls.some((c) => Math.abs(mid(c) - mid(more)) < 12),
            moreLast: lines.length > 0 && Math.abs(lines[lines.length - 1]! - mid(more)) < 12,
            // Nothing left of the text column.
            leftOfText: Math.min(...controls.map((c) => c.left), more.left) - txt.left,
            descLines: desc.getBoundingClientRect().height / parseFloat(getComputedStyle(desc).lineHeight),
          };
        }),
      );
      expect(found).toHaveLength(7);
      if (width === 980) {
        // The metric and style columns line up on every row (a number's label and unit come before them).
        const lefts = await rows.evaluateAll((lis) => lis.map((li) => Math.round(li.querySelector('mat-select[aria-label^="Metric of"]')!.getBoundingClientRect().left)));
        expect(Math.max(...lefts) - Math.min(...lefts), `${platform.name}: metric columns ${lefts.join(', ')}`).toBeLessThanOrEqual(2);
      }
      for (const r of found) {
        const at = `${platform.name} ${width} ${r.title}`;
        expect(r.moreLevel, `${at}: "more" is not level with the controls`).toBe(true);
        expect(r.moreLast, `${at}: "more" is not on the last line`).toBe(true);
        expect(r.leftOfText, `${at}: something sits left of the description column`).toBeGreaterThanOrEqual(-2);
        if (width === 980) {
          expect(r.lines, `${at}: wraps at 980 px`).toBe(1);
          expect(r.descLines, `${at}: the description wraps`).toBeLessThan(1.5);
        } else {
          expect(r.lines, `${at}: more than two lines at 760 px`).toBeLessThanOrEqual(2);
        }
      }
      await ctx.close();
    });
  }
}

/**
 * The chooser sizes its rows by the width of its list, not of the whole chooser: with a preview column beside it (the popup
 * widgets, a 900 px chooser) the list is about 520 px, so the description keeps its 10rem and the controls wrap under it.
 */
test('the popup widgets chooser with its preview column: rows follow the list width', async ({ browser, baseURL }) => {
  const ctx = await browser.newContext({ baseURL, viewport: { width: 1320, height: 1000 } });
  const page = await ctx.newPage();
  await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
  await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
  await page.getByRole('navigation', { name: 'Settings pages' }).getByRole('button', { name: 'Dyno', exact: true }).click();
  // Settings caps its content at 790 px, so the chooser never gets its preview column beside the list here; give it the width an
  // app with a wider page has (900 px: the list is then about 520 px).
  await page.addStyleTag({ content: 'dy-popup-widgets pd-widget-chooser { width: 900px; max-width: none; }' });
  const list = page.locator('dy-popup-widgets .pd-wc-list').first();
  await expect(list.locator('li.pd-wc-row')).not.toHaveCount(0);
  const preview = page.locator('dy-popup-widgets .pd-wc-preview');
  await expect(preview).toBeVisible();
  const [listBox, previewBox] = [await list.boundingBox(), await preview.boundingBox()];
  expect(previewBox!.x, 'the preview column is beside the list').toBeGreaterThan(listBox!.x + listBox!.width);
  const found = await list.evaluate((ul) => {
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
    return {
      width: ul.getBoundingClientRect().width,
      rem,
      rows: [...ul.querySelectorAll('li.pd-wc-row')].map((li) => {
        const txt = li.querySelector('.pd-wc-txt')!;
        const more = li.querySelector('.pd-wc-more')!.getBoundingClientRect();
        const d = txt.getBoundingClientRect();
        return { basis: getComputedStyle(txt).flexBasis, descW: d.width, moreLeftOfText: more.left < d.left };
      }),
    };
  });
  expect(found.width, 'the list is narrower than the chooser').toBeLessThan(560);
  for (const r of found.rows) {
    expect(r.basis, 'a list under 35rem makes the description keep 10rem').toBe(`${10 * found.rem}px`);
    expect(r.moreLeftOfText).toBe(false);
  }
  await ctx.close();
});
