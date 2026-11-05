import { expect, test } from '@playwright/test';
import { dialogWindowUrl } from '@decryp7/paddock-e2e';
import type { Locator, Page } from '@playwright/test';

/**
 * Settings > Menu bar items and Popup widgets: while a row is dragged, the ghost under the pointer is the whole row (title,
 * summary, controls and "more" where the row has them, the grip, no kind icon), the width of the row and laid out as the row is
 * (also in the dialog's real size, 880x700, where rows wrap), and the placeholder keeps the row's height. The dashboard's Edit
 * layout drag moves the real widget and draws no clone. Waits are on conditions, never sleeps; a press first checks that the
 * pointer is over the element it means to grab. WIDGET_GHOST_SHOTS=<dir> writes mid-drag screenshots (review evidence).
 */

/** Waits until the element's box is the same on two animation frames, then returns the point to press and checks what is under it. */
async function pressPoint(page: Page, el: Locator, within: string, dy?: number): Promise<{ x: number; y: number }> {
  const box = async (): Promise<string> =>
    el.evaluate(
      (e) =>
        new Promise<string>((done) =>
          requestAnimationFrame(() => {
            const r = e.getBoundingClientRect();
            requestAnimationFrame(() => {
              const s = e.getBoundingClientRect();
              done(r.x === s.x && r.y === s.y && r.width === s.width && r.height === s.height ? `${s.x},${s.y},${s.width},${s.height}` : '');
            });
          }),
        ),
    );
  await expect.poll(box, { message: 'the element to grab keeps still', timeout: 10_000 }).not.toBe('');
  const b = (await el.boundingBox())!;
  const x = b.x + b.width / 2;
  const y = dy === undefined ? b.y + b.height / 2 : b.y + dy;
  const hit = await page.evaluate(({ x, y, within }) => document.elementFromPoint(x, y)?.closest(within) != null, { x, y, within });
  expect(hit, `the pointer at ${x},${y} is not over ${within}: the element moved before the press`).toBe(true);
  return { x, y };
}

/** Where the grip, title, first control and "more" sit from the row's corner; null for a part the row does not have. */
const offsets = (el: Locator): Promise<(number[] | null)[]> =>
  el.evaluate((e) => {
    const o = e.getBoundingClientRect();
    const at = (sel: string): number[] | null => {
      const b = e.querySelector(sel)?.getBoundingClientRect();
      return b ? [b.left - o.left, b.top - o.top] : null;
    };
    return [at('.pd-wc-grip'), at('.pd-wc-txt b'), at('.pd-wc-body > :not(.pd-wc-txt):not(.pd-wc-more)'), at('.pd-wc-more')];
  });

const SIZES = [
  { name: 'wide', width: 980, height: 900 },
  // The Settings dialog's own size (src-tauri dialogs.rs): the list is about 546 px, so rows take the narrow, wrapped layout.
  { name: 'dialog', width: 880, height: 700 },
] as const;

for (const size of SIZES) {
  for (const [scheme, which] of [['light', 'first'], ['dark', 'first'], ['light', 'last'], ['dark', 'last']] as const) {
    test(`the drag ghost is the whole row (${size.name}, ${scheme}, ${which} chooser)`, async ({ browser, baseURL }, info) => {
      const ctx = await browser.newContext({ baseURL, viewport: { width: size.width, height: size.height }, colorScheme: scheme });
      try {
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
        await row.locator('.pd-wc-grip').scrollIntoViewIfNeeded();
        const { x, y } = await pressPoint(page, row.locator('.pd-wc-grip'), '.pd-wc-grip');
        const rowBox = (await row.boundingBox())!;
        const inRow = await offsets(row);
        if (which === 'first') expect(inRow[2], 'the Menu bar items row has a control').not.toBeNull();
        await page.mouse.move(x, y);
        await page.mouse.down();
        await page.mouse.move(x + 6, y + 10, { steps: 4 });
        await page.mouse.move(x + 20, y + 70, { steps: 8 });
        const ghost = page.locator('.cdk-drag-preview');
        await expect(ghost).toHaveCount(1);
        await expect(ghost).toContainText(title);
        const dir = process.env['WIDGET_GHOST_SHOTS'];
        if (dir) await page.screenshot({ path: `${dir}/${info.project.name}-${size.name}-${scheme}-${which}-${process.env['WIDGET_GHOST_TAG'] ?? 'run'}.png` });
        const ghostBox = (await ghost.boundingBox())!;
        const inGhost = await offsets(ghost);
        inGhost.forEach((v, i) => {
          expect(v === null, `part ${i} is in the row: ${JSON.stringify(inRow)}, in the ghost: ${JSON.stringify(inGhost)}`).toBe(inRow[i] === null);
          if (v && inRow[i]) {
            expect(Math.abs(v[0] - inRow[i]![0]), `part ${i} x: ghost ${v}, row ${inRow[i]}`).toBeLessThanOrEqual(3);
            expect(Math.abs(v[1] - inRow[i]![1]), `part ${i} y: ghost ${v}, row ${inRow[i]}`).toBeLessThanOrEqual(3);
          }
        });
        const biggest = await ghost.evaluate((e) => Math.max(0, ...[...e.querySelectorAll('svg')].map((v) => v.getBoundingClientRect().width)));
        expect(biggest, 'a symbol in the ghost is drawn huge').toBeLessThanOrEqual(32);
        expect(await ghost.locator('.pd-wc-kicon').evaluate((e) => getComputedStyle(e).visibility), 'the ghost shows the kind icon').toBe('hidden');
        expect(await ghost.locator('.pd-wc-grip').evaluate((e) => getComputedStyle(e).visibility), 'the grip stays').toBe('visible');
        // The placeholder keeps the row's size, so nothing jumps.
        const ph = (await page.locator('.cdk-drag-placeholder').first().boundingBox())!;
        await page.mouse.up();
        expect(Math.abs(ghostBox.width - rowBox.width), 'the ghost is the row width').toBeLessThanOrEqual(2);
        expect(Math.abs(ghostBox.height - rowBox.height), `the ghost is the row height (${size.name})`).toBeLessThanOrEqual(2);
        expect(Math.abs(ph.height - rowBox.height), `the placeholder keeps the row height (${size.name})`).toBeLessThanOrEqual(2);
      } finally {
        await ctx.close();
      }
    });
  }
}

for (const scheme of ['light', 'dark'] as const) {
  test(`the dashboard's Edit layout drag moves the widget, draws no clone and no stray symbol (${scheme})`, async ({ browser, baseURL }, info) => {
    const ctx = await browser.newContext({ baseURL, viewport: { width: 1280, height: 900 }, colorScheme: scheme });
    try {
      const page = await ctx.newPage();
      await page.goto('/?latency=0&lists=long#/');
      await expect(page.locator('[data-pd-cell]').first()).toBeVisible({ timeout: 45_000 });
      await expect(page.locator('pd-widget-frame [aria-busy="true"], pd-widget-frame[aria-busy="true"]')).toHaveCount(0, { timeout: 30_000 });
      await page.getByRole('button', { name: 'Edit layout' }).click();
      await expect(page.getByText('Editing layout')).toBeVisible();
      // The editing dim (widgets are inert) must not show a checkbox's invisible native control: it drew large native squares.
      const native = page.locator('dy-history-panel .mdc-checkbox__native-control, .legend .mdc-checkbox__native-control');
      await expect(native.first()).toBeAttached();
      expect(await native.first().evaluate((e) => getComputedStyle(e).opacity), 'a checkbox native control is visible while editing').toBe('0');
      const cell = page.locator('[data-pd-cell]', { has: page.locator('dy-events-feed') });
      await cell.evaluate((e) => e.scrollIntoView({ block: 'center' }));
      const { x, y } = await pressPoint(page, cell, '[data-pd-cell]', 20);
      const before = (await cell.boundingBox())!;
      await page.mouse.move(x, y);
      await page.mouse.down();
      await page.mouse.move(x + 40, y, { steps: 5 });
      await page.mouse.move(x + 240, y + 60, { steps: 12 });
      // A real drag: the widget follows the pointer (its box has moved by most of the pointer's distance).
      const moved = async (): Promise<number> => {
        const b = (await cell.boundingBox())!;
        return Math.hypot(b.x - before.x, b.y - before.y);
      };
      await expect.poll(moved, { message: 'the widget did not follow the pointer' }).toBeGreaterThan(150);
      const dir = process.env['WIDGET_GHOST_SHOTS'];
      if (dir) await page.screenshot({ path: `${dir}/${info.project.name}-${scheme}-dashboard-${process.env['WIDGET_GHOST_TAG'] ?? 'run'}.png` });
      const mid = (await cell.boundingBox())!;
      expect(await page.locator('.cdk-drag-preview').count(), 'a drag preview clone exists').toBe(0);
      expect(Math.abs(mid.width - before.width) / before.width, 'the widget changed width while moved').toBeLessThanOrEqual(0.04);
      expect(Math.abs(mid.height - before.height) / before.height, 'the widget changed height while moved').toBeLessThanOrEqual(0.04);
      const stray = await page.evaluate(() => [...document.querySelectorAll('svg')].filter((v) => v.getBoundingClientRect().width > 120 && !v.closest('[data-pd-cell]')).length);
      expect(stray, 'a large symbol outside the widgets').toBe(0);
      await page.mouse.up();
    } finally {
      await ctx.close();
    }
  });
}
