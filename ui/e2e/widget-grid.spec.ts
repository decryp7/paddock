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
/** Waits for every finite animation (the FLIP glide) so a capture never shows a widget half way. */
const animationsDone = (page: Page) => page.evaluate(() => Promise.all(document.getAnimations().filter((a) => Number.isFinite(a.effect?.getComputedTiming().endTime as number)).map((a) => a.finished.catch(() => undefined))));
const startEditing = async (page: Page) => {
  await board(page).getByRole('button', { name: 'Edit layout' }).click();
  await expect(board(page).getByText('Editing layout')).toBeVisible();
};
/** One column plus the gap and one row plus the gap, from the grid's own box (12 columns, 16 px gap, 24 px rows). */
async function pitch(page: Page): Promise<{ col: number; row: number }> {
  const g = (await board(page).locator('.pd-wg-grid').first().boundingBox())!;
  return { col: (g.width + 16) / 12, row: 40 };
}
/** Waits until the placeholder is where the dragged frame is (within a cell): the last preview has been drawn. */
async function previewCaughtUp(page: Page, id: string): Promise<void> {
  await expect
    .poll(async () => {
      const ph = await board(page).locator('.pd-wg-ph').boundingBox();
      const fr = await cell(page, id).boundingBox();
      const p = await pitch(page);
      return !!ph && !!fr && Math.abs(ph.x - fr.x) <= p.col * 0.75 && Math.abs(ph.y - fr.y) <= p.row * 4;
    })
    .toBe(true);
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
          const shot = async (loc: Locator, name: string) => {
            await animationsDone(page);
            await loc.screenshot({ path: join(SHOTS as string, `${name}-${theme}-${scale}x.png`) });
          };
          await open(page, theme);
          await shot(board(page), 'board');
          await startEditing(page);
          // A refused key, on a fresh board: Alpha is at the left edge.
          await cell(page, 'a').focus();
          await page.keyboard.press('ArrowLeft');
          await expect(board(page).locator('.pd-wg-feedback--bad')).toHaveText("Alpha is already at the left edge.");
          await shot(board(page), 'board-blocked-focus-ring');
          await page.keyboard.press('ArrowRight');
          await expect(board(page).locator('.pd-wg-feedback')).toContainText('Alpha moved right');
          await cell(page, 'b').focus();
          await shot(board(page), 'board-editing-focus');
          const drag = async (name: string) => {
            const p = await pitch(page);
            const a = (await cell(page, 'e').boundingBox())!;
            await page.mouse.move(a.x + 20, a.y + 20);
            await page.mouse.down();
            await page.mouse.move(a.x + 20 + p.col * 3, a.y + 20 + p.row, { steps: 6 });
            await expect(board(page).locator('.pd-wg-ph')).toBeVisible();
            await previewCaughtUp(page, 'e');
            await animationsDone(page);
            await shot(board(page), name);
            await page.mouse.up();
            await expect(board(page).locator('.pd-wg-ph')).toHaveCount(0);
          };
          await drag('board-dragging');
          const corner = (await cell(page, 'f').locator('.pd-wg-h--se').boundingBox())!;
          const p = await pitch(page);
          await page.mouse.move(corner.x + 12, corner.y + 12);
          await page.mouse.down();
          await page.mouse.move(corner.x + 12 + p.col * 2, corner.y + 12 + p.row, { steps: 6 });
          await expect(board(page).locator('.pd-wg-chip')).toBeVisible();
          await animationsDone(page);
          await shot(board(page), 'board-resizing');
          await page.mouse.up();
          await page.keyboard.press('Escape');
          await expect(board(page).getByRole('button', { name: 'Edit layout' })).toBeVisible();
          await expect(board(page).locator('.pd-wg-feedback')).toHaveCount(0);
          await shot(page.locator('.wgg-states'), 'states');
          // The menu, the undo toast and the add sheet, as the person sees them (the viewport).
          await startEditing(page);
          await cell(page, 'a').getByRole('button', { name: 'Actions for Alpha' }).click();
          await expect(page.getByRole('menuitem', { name: 'Options…' })).toBeVisible();
          await animationsDone(page);
          await page.screenshot({ path: join(SHOTS as string, `options-menu-${theme}-${scale}x.png`) });
          await page.keyboard.press('Escape');
          await cell(page, 'c').focus();
          await page.keyboard.press('Delete');
          await expect(page.locator('.pd-undo-snack')).toBeVisible();
          await animationsDone(page);
          await page.screenshot({ path: join(SHOTS as string, `undo-snack-${theme}-${scale}x.png`) });
          await page.locator('.pd-undo-snack button').click();
          await board(page).getByRole('button', { name: 'Add widget' }).click();
          await expect(page.getByTestId('wgg-sheet')).toBeVisible();
          await animationsDone(page);
          await page.screenshot({ path: join(SHOTS as string, `add-sheet-${theme}-${scale}x.png`) });
          await page.getByTestId('wgg-sheet').getByRole('button', { name: 'Close' }).click();
          await board(page).getByRole('button', { name: 'Done' }).click();
          for (let i = 0; i < 12; i++) await shot(page.getByTestId(`wgg-case-${i}`), `golden-case-${String(i + 1).padStart(2, '0')}`);
          await page.emulateMedia({ reducedMotion: 'reduce' });
          await page.evaluate(() => document.documentElement.setAttribute('data-motion', 'reduce'));
          await startEditing(page);
          await drag('board-dragging-reduced-motion');
          await page.evaluate(() => document.documentElement.setAttribute('data-contrast', 'more'));
          await shot(board(page), 'board-contrast-editing');
          await drag('board-contrast-dragging');
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
  await expect(board(page).locator('.pd-wg-feedback--bad')).toHaveText('Alpha is already at the left edge.');
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
  // Frame cost: the time from a frame's rAF callback to the first task after rendering (style, layout, paint included), per frame.
  await page.evaluate(() => {
    const w = window as unknown as { __frames: number[]; __stop: boolean };
    w.__frames = [];
    w.__stop = false;
    const tick = () => {
      if (w.__stop) return;
      const t0 = performance.now();
      const ch = new MessageChannel();
      ch.port1.onmessage = () => w.__frames.push(performance.now() - t0);
      ch.port2.postMessage(0);
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.mouse.move(a.x + 20, a.y + 20);
  await page.mouse.down();
  await page.mouse.move(a.x + 20 + p.col * 6, a.y + 20 + p.row * 6, { steps: 30 });
  await page.mouse.up();
  await page.evaluate(() => ((window as unknown as { __stop: boolean }).__stop = true));
  const frames = await page.evaluate(() => (window as unknown as { __frames: number[] }).__frames);
  // The budget is 16.7 ms a frame (ADR 0044 §7). CI noise makes the single worst frame a poor gate, so the p95 over the drag is asserted.
  const sorted = [...frames].sort((x, y) => x - y);
  const p95 = sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))] as number;
  test.info().annotations.push({ type: 'frame-cost', description: `p95 ${p95.toFixed(1)} ms, max ${sorted.at(-1)!.toFixed(1)} ms over ${frames.length} frames` });
  expect(frames.length).toBeGreaterThan(30);
  expect(p95).toBeLessThan(16.7);
});

test('Widget grid: the add sheet lists the kinds and adds one; Options is in the menu of a kind that has it', async ({ page }) => {
  await open(page, 'dark');
  await startEditing(page);
  await board(page).getByRole('button', { name: 'Add widget' }).click();
  const sheet = page.getByTestId('wgg-sheet');
  await expect(sheet.getByText('Shown in the')).toHaveCount(0);
  await expect(board(page).locator('[data-pd-cell]')).toHaveCount(6);
  await sheet.getByRole('button', { name: /^Add Golf|^Add .*/ }).first().click();
  await expect(board(page).locator('[data-pd-cell]')).toHaveCount(7);
  await cell(page, 'a').getByRole('button', { name: 'Actions for Alpha' }).click();
  await expect(page.getByRole('menuitem', { name: 'Options…' })).toBeVisible();
});

test('Widget grid: ARIA snapshots in normal and edit mode, every widget is named, moves reach the live region', async ({ page }) => {
  await open(page, 'dark');
  const normal = board(page).locator('[data-pd-cell="a"]');
  await expect(normal).toMatchAriaSnapshot(`
    - region "Alpha"
  `);
  await expect(board(page).getByRole('button', { name: 'Edit layout' })).toBeVisible();
  await startEditing(page);
  await expect(cell(page, 'a')).toMatchAriaSnapshot(`
    - group "Alpha":
      - button "Actions for Alpha"
  `);
  const names = await board(page).locator('[data-pd-cell]').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
  expect(names.length).toBe(6);
  for (const n of names) expect(n && n.length).toBeTruthy();
  expect(await cell(page, 'a').getAttribute('aria-roledescription')).toBe('widget');
  // The CDK live region says what a key did.
  await cell(page, 'a').focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('.cdk-live-announcer-element')).toContainText('Alpha moved right');
});

test('Widget grid: no raw engine text reaches the page, and a refused golden case is in words', async ({ page }) => {
  await open(page, 'dark');
  const text = await page.getByTestId('widget-grid').innerText();
  expect(text).not.toMatch(/Edge\(|Largest\b\(|Smallest\b\(|Block|Debug|\[object/);
  await expect(page.getByTestId('wgg-case-7').getByRole('status')).toHaveText('Refused: Alpha is already at the left edge.');
});
