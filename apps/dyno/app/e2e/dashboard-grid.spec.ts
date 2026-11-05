import { axeInThemes, expect, test } from '@decryp7/paddock-e2e';
import type { Locator, Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * The dashboard on the shared widget grid (family ADR 0044, WP-C2) against the mock backend, which answers the engine's gestures from the
 * golden cases: the frames and their content, edit mode (enter, keyboard move, keyboard resize, add through the side sheet, options in the
 * widget's menu, Undo and Redo), the narrow window that cannot edit, and axe in both themes. Waits are on conditions, never sleeps.
 */
const OPEN = '/?latency=0#/';
const cells = (page: Page): Locator => page.locator('[data-pd-cell]');

async function open(page: Page, width = 1280, height = 900): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto(OPEN);
  await expect(cells(page).first()).toBeVisible({ timeout: 45_000 });
  // Settled: no frame is loading.
  await expect(page.locator('pd-widget-frame [aria-busy="true"], pd-widget-frame[aria-busy="true"]')).toHaveCount(0, { timeout: 30_000 });
}

/** The live graphs have a line to draw (a reviewer's shot of one sample shows empty cards): wait on the samples, not on time. */
async function graphsHaveALine(page: Page): Promise<void> {
  await expect
    .poll(() => page.locator('dy-widget dy-sparkline path').first().getAttribute('d').then((d) => (d?.match(/L/g) ?? []).length), { timeout: 60_000 })
    .toBeGreaterThan(8);
  // The history, consumers and disk charts draw after their data (a dark shot once caught them empty): wait until each has a drawn surface.
  await expect
    .poll(() => page.evaluate(() => { const c = Array.from(document.querySelectorAll('dy-chart')); return c.length > 0 && c.every((e) => { const r = e.querySelector('canvas, svg')?.getBoundingClientRect(); return !!r && r.width > 50 && r.height > 20; }); }), { timeout: 60_000 })
    .toBe(true);
}

const box = async (cell: Locator): Promise<{ x: number; y: number; width: number; height: number }> => {
  const b = await cell.boundingBox();
  if (!b) throw new Error('the widget has no box');
  return b;
};

test.describe('dashboard grid', () => {
  test('draws the frames with their content only: one card per widget, no card inside a frame', async ({ page }) => {
    await open(page);
    expect(await cells(page).count()).toBeGreaterThan(8);
    await expect(page.locator('pd-widget-frame dy-widget')).not.toHaveCount(0);
    // A frame is the card: nothing inside its body draws another one, and the old chooser bar and span toggles are gone.
    await expect(page.locator('pd-widget-frame .card, pd-widget-frame dy-tile, pd-widget-frame .card-head')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Customize dashboard' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Edit layout' })).toBeEnabled();
  });

  test('edit mode: enter, move and resize by keyboard, undo and redo', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const second = cells(page).nth(1);
    await second.focus();
    const before = await box(second);
    await page.keyboard.press('ArrowLeft');
    await expect.poll(async () => (await box(cells(page).nth(1))).x, { timeout: 15_000 }).not.toBe(before.x);
    const undo = page.getByRole('button', { name: 'Undo' });
    await expect(undo).toBeEnabled();
    // Resize: Shift+Right makes the focused widget wider by one step of its kind's rule.
    const focused = page.locator('[data-pd-cell]:focus');
    await expect(focused).toHaveCount(1);
    // Undo and Redo take the focus, so the widget is found by its id from here on.
    const id = await focused.getAttribute('data-pd-cell');
    const target = page.locator(`[data-pd-cell="${id}"]`);
    const w0 = (await box(target)).width;
    await page.keyboard.press('Shift+ArrowRight');
    await expect.poll(async () => (await box(target)).width, { timeout: 15_000 }).toBeGreaterThan(w0);
    await undo.click();
    await expect.poll(async () => (await box(target)).width, { timeout: 15_000 }).toBeCloseTo(w0, 0);
    await expect(page.getByRole('button', { name: 'Redo' })).toBeEnabled();
    await page.getByRole('button', { name: 'Redo' }).click();
    await expect.poll(async () => (await box(target)).width, { timeout: 15_000 }).toBeGreaterThan(w0);
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(page.getByRole('button', { name: 'Edit layout' })).toBeVisible();
  });

  test('edit mode: add a widget through the side sheet', async ({ page }) => {
    await open(page);
    const count = await cells(page).count();
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await page.getByRole('button', { name: 'Add widget' }).click();
    const sheet = page.getByRole('dialog', { name: 'Add a widget' });
    await expect(sheet).toBeVisible();
    await sheet.getByRole('button', { name: /^Add/ }).first().click();
    await expect(cells(page)).toHaveCount(count + 1);
    await page.keyboard.press('Escape');
    await expect(sheet).toHaveCount(0);
  });

  test('edit mode: the metric is an option in the widget menu, not a control on the card', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    await expect(page.getByRole('button', { name: /^Actions for/ })).not.toHaveCount(0);
    // Each widget's menu button sits inside its own card (a button that flows out of the card is covered by the next one).
    const outside = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('[data-pd-cell]')].filter((c) => {
        const b = c.querySelector<HTMLElement>('.pd-wg-more')?.getBoundingClientRect();
        const r = c.getBoundingClientRect();
        return !b || b.left < r.left - 1 || b.right > r.right + 1 || b.top < r.top - 1 || b.bottom > r.bottom + 1;
      }).length,
    );
    expect(outside).toBe(0);
    const graph = cells(page).filter({ has: page.locator('dy-widget[data-kind="graph"]') }).first();
    await graph.getByRole('button', { name: /^Actions for/ }).click();
    await page.getByRole('menuitem', { name: /^Options for/ }).click();
    const sheet = page.getByRole('dialog', { name: 'Widget options' });
    await expect(sheet).toBeVisible();
    await expect(sheet.getByRole('combobox', { name: /^Metric of/ })).toBeVisible();
  });

  test('dragging a widget near the bottom and top edge of the page scrolls the shell main element', async ({ page }) => {
    await open(page, 1280, 600);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const main = page.locator('main');
    const scrollTop = (): Promise<number> => main.evaluate((m) => m.scrollTop);
    expect(await scrollTop()).toBe(0);
    const first = await box(cells(page).first());
    await page.mouse.move(first.x + first.width / 2, first.y + first.height / 2);
    await page.mouse.down();
    const area = await main.boundingBox();
    if (!area) throw new Error('the page has no box');
    await page.mouse.move(first.x + first.width / 2, first.y + first.height / 2 + 40, { steps: 5 });
    await page.mouse.move(first.x + first.width / 2, area.y + area.height - 6, { steps: 8 });
    await expect.poll(scrollTop, { timeout: 15_000 }).toBeGreaterThan(0);
    // Back up: near the top edge it scrolls the other way.
    const down = await scrollTop();
    await page.mouse.move(first.x + first.width / 2, area.y + 6, { steps: 8 });
    await expect.poll(scrollTop, { timeout: 15_000 }).toBeLessThan(down);
    await page.keyboard.press('Escape');
    await page.mouse.up();
  });

  test('edit mode: the segmented controls and checkboxes of the bodies read at reduced emphasis, and at full strength again after Done', async ({ page }) => {
    await open(page);
    const strength = () =>
      page.evaluate(() =>
        [...document.querySelectorAll<HTMLElement>('pd-widget-frame .mat-button-toggle-group, pd-widget-frame .mat-mdc-checkbox')].map((c) => Number(getComputedStyle(c).opacity)),
      );
    const normal = await strength();
    expect(normal.length).toBeGreaterThan(4);
    expect(normal.every((o) => o === 1)).toBe(true);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    expect((await strength()).every((o) => o < 1 && o >= 0.6)).toBe(true);
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(page.getByText('Editing layout')).toHaveCount(0);
    expect((await strength()).every((o) => o === 1)).toBe(true);
  });

  test('a window too narrow to arrange disables Edit layout and says why', async ({ page }) => {
    await open(page, 700);
    const edit = page.getByRole('button', { name: 'Edit layout' });
    await expect(edit).toBeDisabled();
    await expect(page.getByText(/needs a wider dashboard/)).toBeVisible();
  });

  test('edit mode: the page keeps one primary button (Done); a body button reads as inactive', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const filled = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('main .mat-mdc-unelevated-button')]
        .filter((b) => getComputedStyle(b).backgroundColor !== 'rgba(0, 0, 0, 0)')
        .map((b) => (b.textContent ?? '').trim()),
    );
    expect(filled).toEqual(['Done']);
    // Outside edit mode the Why slow widget's own button is the filled one again.
    await page.getByRole('button', { name: 'Done' }).click();
    await expect(page.getByText('Editing layout')).toHaveCount(0);
    await expect(page.locator('dy-diagnosis-card .mat-mdc-unelevated-button')).toHaveCSS('background-color', /rgb\((?!0, 0, 0)/);
  });

  for (const width of [1280, 900, 500]) {
    test(`never cuts a value at ${width} px: a clipped text is a name with its full text in a tooltip`, async ({ page }) => {
      await open(page, width, 4000);
      for (const mode of ['normal', 'edit']) {
        if (mode === 'edit') {
          if (!(await page.getByRole('button', { name: 'Edit layout' }).isEnabled())) continue;
          await page.getByRole('button', { name: 'Edit layout' }).click();
          await expect(page.getByText('Editing layout')).toBeVisible();
        }
        const cut = await page.evaluate(() => {
          const valueLike = /^[\s\-+\u2212~<>]*\d[\d.,\s]*[^\s]{0,8}(\s+[^\s]{1,8})?$/;
          const out: string[] = [];
          for (const el of document.querySelectorAll<HTMLElement>('main *')) {
            if ((el.children.length > 0 && !el.matches('strong, span, dd, b, small, p, li')) || el.closest('.visually-hidden, .cdk-visually-hidden')) continue;
            const cs = getComputedStyle(el);
            const clipsX = cs.overflow === 'hidden' || cs.overflowX === 'hidden';
            const text = (el.textContent ?? '').trim();
            if (!text || !clipsX || el.clientWidth <= 1 || el.scrollWidth <= el.clientWidth + 1) continue;
            const named = el.hasAttribute('data-full-text') || el.closest('[data-full-text]') !== null || el.hasAttribute('title');
            if (valueLike.test(text) || !named) out.push(text.slice(0, 40));
          }
          return out;
        });
        expect(cut, `${mode} at ${width}`).toEqual([]);
      }
    });
  }

  test('Is this normal? at one column wraps each value under its sentence instead of cutting it', async ({ page }) => {
    await open(page, 500, 1200);
    const rows = page.locator('dy-normal-card li');
    await expect(rows.first()).toBeVisible();
    const n = await rows.count();
    for (let i = 0; i < n; i++) {
      const li = rows.nth(i);
      const [row, vals] = await Promise.all([box(li), box(li.locator('.vals'))]);
      expect(vals.x + vals.width, `row ${i} value inside the card`).toBeLessThanOrEqual(row.x + row.width + 0.5);
      expect(await li.locator('.vals').evaluate((v) => v.scrollWidth <= v.clientWidth + 1)).toBe(true);
    }
  });

  test('the corner grip never covers the last line of a widget', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    const overlaps = await page.evaluate(() => {
      const bad: string[] = [];
      for (const cell of document.querySelectorAll<HTMLElement>('[data-pd-cell]')) {
        const grip = cell.querySelector('.pd-wg-h--se')?.getBoundingClientRect();
        if (!grip) continue;
        for (const el of cell.querySelectorAll<HTMLElement>('pd-widget-frame *')) {
          if (el.children.length > 0 || !(el.textContent ?? '').trim() || el.closest('.visually-hidden')) continue;
          // Only what shows: a row of a scrolling list (a virtual list renders a few beyond its edge) that its viewport clips is not under the grip.
          let r = el.getBoundingClientRect();
          for (let up = el.parentElement; up && up !== cell; up = up.parentElement) {
            if (getComputedStyle(up).overflowY === 'visible') continue;
            const c = up.getBoundingClientRect();
            r = new DOMRect(Math.max(r.left, c.left), Math.max(r.top, c.top), Math.max(0, Math.min(r.right, c.right) - Math.max(r.left, c.left)), Math.max(0, Math.min(r.bottom, c.bottom) - Math.max(r.top, c.top)));
          }
          if (r.width === 0 || r.height === 0) continue;
          if (r.left < grip.right && r.right > grip.left && r.top < grip.bottom && r.bottom > grip.top) bad.push(`${cell.getAttribute('data-pd-cell')}: ${(el.textContent ?? '').trim().slice(0, 30)}`);
        }
      }
      return bad;
    });
    expect(overlaps).toEqual([]);
  });

  test('the six-column arrangement leaves no empty half beside a widget', async ({ page }) => {
    await open(page, 900, 3000);
    const board = await box(page.locator('dy-widget-board'));
    const consumers = page.locator('[data-pd-cell]', { has: page.locator('dy-consumers-panel') });
    const c = await box(consumers);
    expect(c.width).toBeGreaterThan(board.width * 0.9);
  });

  for (const theme of ['light', 'dark'] as const) {
    test(`every per-core cell, the lowest step too, is 3:1 on the card in ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await open(page);
      const ratio = await page.evaluate(() => {
        const rgb = (css: string): number[] => {
          const c = document.createElement('canvas').getContext('2d')!;
          c.fillStyle = '#000';
          c.fillStyle = css;
          c.fillRect(0, 0, 1, 1);
          return [...c.getImageData(0, 0, 1, 1).data].slice(0, 3);
        };
        const lum = (v: number[]): number => {
          const [r, g, b] = v.map((x) => { const s = x / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; });
          return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
        };
        // Every per-core cell on the dashboard, the idle ones included, against the card behind it.
        const cells = [...document.querySelectorAll<HTMLElement>('dy-cpu-tile .core, dy-widget .cell')];
        if (cells.length === 0) throw new Error('no per-core cells drawn');
        let lowest = Infinity;
        for (const cell of cells) {
          const [a, b] = [lum(rgb(getComputedStyle(cell).backgroundColor)), lum(rgb(getComputedStyle(cell.closest('.pd-w')!).backgroundColor))];
          lowest = Math.min(lowest, (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05));
        }
        return lowest;
      });
      expect(ratio).toBeGreaterThanOrEqual(3);
    });
  }

  test('has no accessibility violations in either theme, in normal and in edit mode', async ({ page }) => {
    await open(page);
    expect(await axeInThemes(page, { include: ['dy-widget-board'] })).toEqual([]);
    await page.getByRole('button', { name: 'Edit layout' }).click();
    await expect(page.getByText('Editing layout')).toBeVisible();
    expect(await axeInThemes(page, { include: ['dy-widget-board'] })).toEqual([]);
  });
});

// Screenshots for the independent review (ER-23): SHOTS_DIR=<folder> npx playwright test dashboard-grid --project=chromium -g screenshots
test.describe('dashboard grid screenshots', () => {
  test.skip(!process.env['SHOTS_DIR'], 'set SHOTS_DIR to take them');
  for (const theme of ['light', 'dark'] as const) {
    test(`screenshots ${theme}`, async ({ page }) => {
      const dir = process.env['SHOTS_DIR'] ?? '';
      mkdirSync(dir, { recursive: true });
      await page.emulateMedia({ colorScheme: theme });
      // Tall, so the whole dashboard is in the picture (the page scrolls inside the shell, not as a document).
      await page.setViewportSize({ width: 1280, height: 2400 });
      await open(page, 1280, 2400);
      await graphsHaveALine(page);
      await page.screenshot({ path: join(dir, `dashboard-normal-${theme}.png`), fullPage: true });
      await page.getByRole('button', { name: 'Edit layout' }).click();
      await expect(page.getByText('Editing layout')).toBeVisible();
      await page.screenshot({ path: join(dir, `dashboard-edit-${theme}.png`), fullPage: true });
      const suffix = theme === 'light' ? '' : '-dark';
      await open(page, 900, 2800);
      await graphsHaveALine(page);
      await page.screenshot({ path: join(dir, `dashboard-900-six-columns${suffix}.png`), fullPage: true });
      await open(page, 500, 5200);
      await graphsHaveALine(page);
      await page.screenshot({ path: join(dir, `dashboard-500-one-column${suffix}.png`), fullPage: true });
    });
  }
});
