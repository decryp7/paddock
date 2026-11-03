import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/** Set WIDGET_GRID_SHOTS to a folder to also write the review screenshots there (light and dark, 1x and 2x). */
const SHOTS = process.env['WIDGET_GRID_SHOTS'];
const THEMES = ['dark', 'light'] as const;

async function open(page: Page, theme: string, contrast: 'off' | 'more' = 'off'): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 1800 });
  await page.goto(`/?theme=${theme}&accent=dyno`);
  await page.getByTestId('widget-grid').waitFor();
  await page.getByTestId('widget-grid').scrollIntoViewIfNeeded();
  await page.evaluate((c) => document.documentElement.setAttribute('data-contrast', c), contrast);
}

const board = (page: Page) => page.getByTestId('wgg-board');
const cell = (page: Page, id: string) => board(page).locator(`[data-pd-cell="${id}"]`);
const lines = (c: Locator) => c.evaluate((e) => ({ column: (e as HTMLElement).style.gridColumn, row: (e as HTMLElement).style.gridRow }));
const startEditing = async (page: Page) => {
  await board(page).getByRole('button', { name: 'Edit layout' }).click();
  await expect(board(page).getByText('Editing layout')).toBeVisible();
};
/** The width of one column plus the gap, read from two widgets side by side (a spans 4 columns). */
async function pitch(page: Page): Promise<{ col: number; row: number }> {
  const a = (await cell(page, 'a').boundingBox())!;
  const b = (await cell(page, 'b').boundingBox())!;
  const d = (await cell(page, 'd').boundingBox())!;
  return { col: (b.x - a.x) / 4, row: (d.y - a.y) / 4 };
}

for (const theme of THEMES) {
  for (const contrast of ['off', 'more'] as const) {
    test(`Widget grid: axe finds no violations while editing (${theme}, contrast ${contrast})`, async ({ page }) => {
      await open(page, theme, contrast);
      await startEditing(page);
      const results = await new AxeBuilder({ page }).include('[data-testid="widget-grid"]').withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
      expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    });
  }

  test.describe(`review screenshots (${theme})`, () => {
    for (const scale of [1, 2]) {
      test.describe(`${scale}x`, () => {
        test.use({ deviceScaleFactor: scale });
        test(`Widget grid: review screenshots (${theme}, ${scale}x)`, async ({ page }) => {
          test.skip(!SHOTS, 'set WIDGET_GRID_SHOTS to a folder');
          mkdirSync(SHOTS as string, { recursive: true });
          const shot = (loc: Locator, name: string) => loc.screenshot({ path: join(SHOTS as string, `${name}-${theme}-${scale}x.png`) });
          await open(page, theme);
          await shot(board(page), 'board');
          await startEditing(page);
          await cell(page, 'b').focus();
          await shot(board(page), 'board-editing-focus');
          const p = await pitch(page);
          const a = (await cell(page, 'a').boundingBox())!;
          await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
          await page.mouse.down();
          await page.mouse.move(a.x + a.width / 2 + p.col * 2, a.y + a.height / 2 + p.row * 2, { steps: 6 });
          await expect(board(page).locator('.pd-wg-ph')).toBeVisible();
          await shot(board(page), 'board-dragging');
          await page.mouse.up();
          const corner = await cell(page, 'e').locator('.pd-wg-h--se').boundingBox();
          await page.mouse.move(corner!.x + 12, corner!.y + 12);
          await page.mouse.down();
          await page.mouse.move(corner!.x + 12 + p.col * 2, corner!.y + 12 + p.row, { steps: 6 });
          await expect(board(page).locator('.pd-wg-chip')).toBeVisible();
          await shot(board(page), 'board-resizing');
          await page.mouse.up();
          await cell(page, 'd').focus();
          await page.keyboard.press('ArrowLeft');
          await expect(board(page).locator('.pd-wg-feedback--bad')).toBeVisible();
          await shot(board(page), 'board-blocked');
          await page.keyboard.press('Escape');
          await shot(page.getByTestId('widget-grid').locator('.wgg-states'), 'states');
          await shot(page.getByTestId('wgg-cases'), 'golden-cases');
          await page.emulateMedia({ reducedMotion: 'reduce' });
          await page.evaluate(() => document.documentElement.setAttribute('data-contrast', 'more'));
          await shot(board(page), 'board-contrast');
        });
      });
    }
  });
}

test('Widget grid: a drag moves the widget to the cell under the pointer, with a placeholder and a drop that is undoable', async ({ page }) => {
  await open(page, 'dark');
  await startEditing(page);
  const p = await pitch(page);
  const a = (await cell(page, 'a').boundingBox())!;
  const before = await lines(cell(page, 'a'));
  await page.mouse.move(a.x + 20, a.y + 20);
  await page.mouse.down();
  await page.mouse.move(a.x + 20 + p.col * 8, a.y + 20 + p.row * 8, { steps: 8 });
  await expect(board(page).locator('.pd-wg-ph')).toBeVisible();
  await expect(board(page).locator('.pd-wg-guide')).toHaveCount(2);
  await page.mouse.up();
  await expect(board(page).locator('.pd-wg-ph')).toHaveCount(0);
  const after = await lines(cell(page, 'a'));
  expect(after).not.toEqual(before);
  await expect(board(page).locator('.pd-wg-feedback')).toContainText('CPU'.replace('CPU', 'Alpha'));
  await board(page).getByRole('button', { name: 'Undo' }).click();
  await expect.poll(() => lines(cell(page, 'a'))).toEqual(before);
});

test('Widget grid: a drag cancelled with Escape changes nothing', async ({ page }) => {
  await open(page, 'dark');
  await startEditing(page);
  const p = await pitch(page);
  const a = (await cell(page, 'a').boundingBox())!;
  const before = await lines(cell(page, 'a'));
  await page.mouse.move(a.x + 20, a.y + 20);
  await page.mouse.down();
  await page.mouse.move(a.x + 20 + p.col * 8, a.y + 20 + p.row * 8, { steps: 8 });
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await expect.poll(() => lines(cell(page, 'a'))).toEqual(before);
  await expect(board(page).locator('.pd-wg-ph')).toHaveCount(0);
  await expect(board(page).getByText('Editing layout')).toBeVisible();
});

test('Widget grid: the corner resizes in cells and the size chip says how big', async ({ page }) => {
  await open(page, 'dark');
  await startEditing(page);
  const p = await pitch(page);
  const corner = (await cell(page, 'a').locator('.pd-wg-h--se').boundingBox())!;
  await page.mouse.move(corner.x + 12, corner.y + 12);
  await page.mouse.down();
  await page.mouse.move(corner.x + 12 + p.col * 2, corner.y + 12 + p.row * 2, { steps: 6 });
  await expect(board(page).locator('.pd-wg-chip')).toHaveText('6 × 6');
  await page.mouse.up();
  expect((await lines(cell(page, 'a'))).column).toBe('1 / span 6');
  expect((await lines(cell(page, 'a'))).row).toBe('1 / span 6');
});

test('Widget grid: the keys move, resize and undo, and a refused key says why', async ({ page }) => {
  await open(page, 'dark');
  await startEditing(page);
  await cell(page, 'b').focus();
  await page.keyboard.press('ArrowLeft');
  await expect.poll(() => lines(cell(page, 'b'))).toEqual(expect.objectContaining({ column: '4 / span 4' }));
  await expect(cell(page, 'b')).toBeFocused();
  await page.keyboard.press('Shift+ArrowRight');
  await expect.poll(() => lines(cell(page, 'b'))).toEqual(expect.objectContaining({ column: '4 / span 5' }));
  await page.keyboard.press('Control+z');
  await expect.poll(() => lines(cell(page, 'b'))).toEqual(expect.objectContaining({ column: '4 / span 4' }));
  await cell(page, 'a').focus();
  await page.keyboard.press('ArrowLeft');
  await expect(board(page).locator('.pd-wg-feedback--bad')).toContainText("Alpha can't move left.");
  await page.keyboard.press('Escape');
  await expect(board(page).getByRole('button', { name: 'Edit layout' })).toBeVisible();
});

test('Widget grid: the actions menu does what the keys do', async ({ page }) => {
  await open(page, 'dark');
  await startEditing(page);
  await cell(page, 'a').getByRole('button', { name: 'Actions for Alpha' }).click();
  await expect(page.getByRole('menuitem', { name: 'Move left' })).toBeDisabled();
  await page.getByRole('menuitem', { name: 'Wider' }).click();
  await expect.poll(() => lines(cell(page, 'a'))).toEqual(expect.objectContaining({ column: '1 / span 5' }));
});

test('Widget grid: a width below the first breakpoint turns Edit layout off and says why', async ({ page }) => {
  await open(page, 'dark');
  const narrow = page.getByTestId('wgg-narrow');
  await expect(narrow.getByRole('button', { name: 'Edit layout' })).toBeDisabled();
  await expect(narrow.getByText('Arranging needs a wider dashboard.')).toBeVisible();
});

test('Widget grid: the states and the twelve golden scenarios are drawn', async ({ page }) => {
  await open(page, 'dark');
  await expect(page.getByTestId('wgg-loading').locator('[aria-busy=true]')).toBeVisible();
  await expect(page.getByTestId('wgg-error').getByRole('button', { name: 'Retry' })).toBeVisible();
  await expect(page.getByTestId('wgg-empty')).toContainText('Nothing on the dashboard yet');
  await expect(page.getByTestId('wgg-cases').locator('article')).toHaveCount(12);
  await expect(page.getByTestId('wgg-case-0').locator('pd-widget-grid')).toHaveCount(2);
  await expect(page.getByTestId('wgg-case-7').getByRole('status')).toContainText('Refused');
});

test('Widget grid: a drag across a 32-widget board stays under the frame budget', async ({ page }) => {
  await open(page, 'dark');
  const big = page.getByTestId('wgg-big');
  await big.getByRole('button', { name: 'Edit layout' }).click();
  const first = big.locator('[data-pd-cell]').first();
  await expect(big.locator('[data-pd-cell]')).toHaveCount(32);
  await first.scrollIntoViewIfNeeded();
  const a = (await first.boundingBox())!;
  const p = { col: a.width / 3, row: a.height / 3 };
  await page.evaluate(() => {
    const w = window as unknown as { __frames: number[] };
    w.__frames = [];
    let last = performance.now();
    const tick = (t: number) => {
      w.__frames.push(t - last);
      last = t;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.mouse.move(a.x + 20, a.y + 20);
  await page.mouse.down();
  await page.mouse.move(a.x + 20 + p.col * 6, a.y + 20 + p.row * 6, { steps: 30 });
  await page.mouse.up();
  const frames = await page.evaluate(() => (window as unknown as { __frames: number[] }).__frames);
  test.info().annotations.push({ type: 'frame-times', description: `max ${Math.max(...frames).toFixed(1)} ms over ${frames.length} frames` });
  expect(Math.max(...frames)).toBeLessThan(100);
});
