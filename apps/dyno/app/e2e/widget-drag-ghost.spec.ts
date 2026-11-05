import { expect, test } from '@playwright/test';
import { dialogWindowUrl } from '@decryp7/paddock-e2e';
import type { Locator } from '@playwright/test';

/**
 * Settings > Menu bar items: while a row is dragged, the ghost under the pointer is the whole row (its title and summary,
 * about the row's width), not only the grip's symbol, and lines up as in the list. Waits are on conditions, never sleeps.
 * WIDGET_GHOST_SHOTS=<dir> writes mid-drag screenshots (review evidence).
 */
for (const [scheme, which] of [['light', 'first'], ['dark', 'first'], ['light', 'last'], ['dark', 'last']] as const) {
  test(`the drag ghost is the whole row (${scheme}, ${which} chooser)`, async ({ browser, baseURL }, info) => {
    const ctx = await browser.newContext({ baseURL, viewport: { width: 980, height: 900 }, colorScheme: scheme });
    const page = await ctx.newPage();
    await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
    await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
    await page.getByRole('navigation', { name: 'Settings pages' }).getByRole('button', { name: 'Dyno', exact: true }).click();
    await page.locator('#menu-bar-items').scrollIntoViewIfNeeded();
    const lists = page.locator('.pd-wc-list:has(.pd-wc-grip)');
    const list = which === 'first' ? lists.first() : lists.last();
    await list.scrollIntoViewIfNeeded();
    const row = list.locator('li.pd-wc-item').first();
    await expect(row).toBeVisible();
    const title = (await row.locator('.pd-wc-txt b').innerText()).trim();
    const rowBox = (await row.boundingBox())!;
    const grip = row.locator('.pd-wc-grip');
    const g = (await grip.boundingBox())!;
    const x = g.x + g.width / 2;
    const y = g.y + g.height / 2;
    // Lines up as in the list: the title, the grip and the first control sit at the same offsets from the row's corner.
    const offsets = (el: Locator): Promise<number[]> =>
      el.evaluate((e) => {
        const o = e.getBoundingClientRect();
        const at = (sel: string): number[] => {
          const b = e.querySelector(sel)?.getBoundingClientRect();
          return b ? [b.left - o.left, b.top - o.top] : [-1, -1];
        };
        return [...at('.pd-wc-grip'), ...at('.pd-wc-txt b'), ...at('.pd-wc-body > :not(.pd-wc-txt):not(.pd-wc-more)')];
      });
    const inRow = await offsets(row);
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x + 6, y + 10, { steps: 4 });
    await page.mouse.move(x + 20, y + 70, { steps: 8 });
    const ghost = page.locator('.cdk-drag-preview');
    await expect(ghost).toHaveCount(1);
    await expect(ghost).toContainText(title);
    const ghostBox = (await ghost.boundingBox())!;
    const dir = process.env['WIDGET_GHOST_SHOTS'];
    if (dir) await page.screenshot({ path: `${dir}/${info.project.name}-${scheme}-${which}-${process.env['WIDGET_GHOST_TAG'] ?? 'run'}.png` });
    const inGhost = await offsets(ghost);
    inGhost.forEach((v, i) => expect(Math.abs(v - inRow[i]), `offset ${i}: ghost ${inGhost}, row ${inRow}`).toBeLessThanOrEqual(3));
    // No stray symbol: no svg in the ghost is larger than an icon.
    const biggest = await ghost.evaluate((e) => Math.max(0, ...[...e.querySelectorAll('svg')].map((v) => v.getBoundingClientRect().width)));
    expect(biggest, 'a symbol in the ghost is drawn huge').toBeLessThanOrEqual(32);
    if (dir) await page.screenshot({ path: `${dir}/${info.project.name}-${scheme}-${process.env['WIDGET_GHOST_TAG'] ?? 'run'}.png` });
    // The placeholder keeps the row's height, so nothing jumps.
    const ph = (await page.locator('.cdk-drag-placeholder').first().boundingBox())!;
    await page.mouse.up();
    expect(ghostBox.width, 'the ghost is about the row width').toBeGreaterThan(rowBox.width * 0.9);
    expect(ghostBox.width).toBeLessThan(rowBox.width * 1.1);
    expect(ghostBox.height, 'the ghost is about the row height').toBeGreaterThan(rowBox.height * 0.8);
    expect(Math.abs(ph.height - rowBox.height), 'the placeholder keeps the row height').toBeLessThanOrEqual(2);
    await ctx.close();
  });
}
