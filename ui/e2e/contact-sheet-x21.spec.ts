import { expect, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// The contact sheet of the data parts (plan X21: table, chart, forms) for the independent review (rule 22): both themes, the
// table at rest, with a hovered row and its menu, grouped and virtual, and at 320 px; the five chart kinds; the form with its
// errors. `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x21` rewrites docs/review/x21-data/. The reviewer
// looks at the sheet; this test only proves it was made from the real components.

const GALLERY = `http://localhost:${process.env['PW_PORT'] ?? 4300}`;
const OUT = join(__dirname, '..', 'docs', 'review', 'x21-data');
const THEMES = ['dark', 'light'] as const;

test.describe('contact sheet: data parts', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review/x21-data');
  test.use({ viewport: { width: 1100, height: 900 }, deviceScaleFactor: 2 });

  const shots: { theme: string; title: string; file: string }[] = [];

  async function open(page: Page, theme: string, width = 1100): Promise<void> {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`${GALLERY}/?theme=${theme}&accent=debrief`);
    await page.locator('section[data-testid="chart"] canvas').first().waitFor();
    await page.waitForTimeout(700);
  }
  async function shoot(page: Page, theme: string, title: string, name: string, target: ReturnType<Page['locator']>): Promise<void> {
    mkdirSync(OUT, { recursive: true });
    const file = join(OUT, `${theme}-${name}.png`);
    await target.screenshot({ path: file });
    shots.push({ theme, title, file });
  }

  for (const theme of THEMES) {
    test(`table ${theme}`, async ({ page }) => {
      await open(page, theme);
      const small = page.locator('[data-testid="table-small"]');
      await shoot(page, theme, 'Table at rest (sentence-case headers, tabular numbers, middle-ellipsis paths)', 'table-rest', small);
      await small.getByRole('button', { name: 'Sort by Size (KiB)' }).click();
      await small.locator('[role=row][data-index]').nth(1).hover();
      await shoot(page, theme, 'Sorted by size; the hovered row is highlighted and shows its icon actions and the "..." button', 'table-hover', small);
      await small.locator('[role=row][data-index]').nth(1).getByRole('button', { name: 'More actions' }).click();
      const menu = page.getByRole('menu');
      await menu.waitFor();
      const box = (await small.boundingBox())!;
      const menuBox = (await menu.boundingBox())!;
      const bottom = Math.max(box.y + box.height, menuBox.y + menuBox.height) + 8;
      mkdirSync(OUT, { recursive: true });
      const file = join(OUT, `${theme}-table-menu.png`);
      await page.screenshot({ path: file, clip: { x: box.x, y: box.y, width: box.width, height: bottom - box.y } });
      shots.push({ theme, title: 'Row menu: the same list as right-click, the destructive item last in danger text', file });
      await page.keyboard.press('Escape');
      await shoot(page, theme, 'Grouped rows in a 5,000-row virtual viewport', 'table-virtual', page.locator('[data-testid="table-virtual"]'));
    });

    test(`table states ${theme}`, async ({ page }) => {
      await open(page, theme);
      const small = page.locator('[data-testid="table-small"]');
      // Keyboard focus: the row ring and a focused sortable header.
      await small.locator('[role=row][data-index]').first().focus();
      await page.keyboard.press('ArrowDown');
      await shoot(page, theme, 'Focused row (2 px ring) and a hovered header showing the sort chevron', 'table-focus-row', small.locator('.pd-grid'));
      await small.locator('.pd-sort').nth(1).focus();
      await page.keyboard.press('Tab');
      await page.keyboard.press('Shift+Tab');
      await shoot(page, theme, 'Focused sortable header (focus ring, quiet chevron)', 'table-focus-header', small.locator('.pd-header'));
      // A menu opened from the keyboard, the second item active.
      await small.locator('[role=row][data-index]').nth(1).focus();
      await page.keyboard.press('Shift+F10');
      await page.getByRole('menuitem').first().waitFor();
      await page.keyboard.press('ArrowDown');
      const box = (await small.boundingBox())!;
      const menuBox = (await page.getByRole('menu').boundingBox())!;
      mkdirSync(OUT, { recursive: true });
      const file = join(OUT, `${theme}-menu-keyboard.png`);
      await page.screenshot({ path: file, clip: { x: Math.max(0, menuBox.x - 40), y: menuBox.y - 60, width: menuBox.width + 80, height: menuBox.height + 100 } });
      shots.push({ theme, title: 'Row menu opened with Shift+F10, the second item keyboard-active (2 px ring)', file });
      void box;
      await page.keyboard.press('Escape');
      await shoot(page, theme, 'Selected row (accent tint, 2 px bar), descending sort', 'table-selected', page.locator('[data-testid="box-selected"]'));
      await page.locator('[data-testid="table-virtual"] .pd-group').first().click();
      await shoot(page, theme, 'Collapsed group in the grouped table', 'table-collapsed', page.locator('[data-testid="table-virtual"]'));
      await shoot(page, theme, 'Empty table with its next step', 'table-empty', page.locator('[data-testid="box-empty"]'));
      await shoot(page, theme, 'Loading: skeleton rows', 'table-loading', page.locator('[data-testid="box-loading"]'));
      await shoot(page, theme, 'Error with Retry', 'table-error', page.locator('[data-testid="box-error"]'));
    });

    test(`table at 320 px ${theme}`, async ({ page }) => {
      await open(page, theme, 320);
      await shoot(page, theme, 'Table at 320 px: the name keeps its room, other columns drop out, compact toolbar, nothing scrolls sideways', 'table-320', page.locator('[data-testid="table-small"]'));
      await page.locator('section[data-testid="chart"] canvas').first().waitFor();
      await shoot(page, theme, 'Charts at 320 px (titles, unit, donut with labels)', 'charts-320', page.locator('section[data-testid="chart"]'));
    });

    test(`chart ${theme}`, async ({ page }) => {
      await open(page, theme);
      await shoot(page, theme, 'Chart wrapper: titles with units, line, stacked bar (only the top segment rounded), area, donut with total and labelled slices, loading and error', 'charts', page.locator('section[data-testid="chart"]'));
    });

    test(`forms ${theme}`, async ({ page }) => {
      await open(page, theme);
      const forms = page.locator('section[data-testid="forms"]');
      await forms.getByLabel('Port').fill('70000');
      await forms.getByRole('button', { name: 'Save' }).click();
      await expect(forms.getByTestId('field-error').first()).toBeVisible();
      await shoot(page, theme, 'Forms: error replaces the hint right under the control, alert icon at 14 px, short port field, disabled field', 'forms', forms);
      await forms.getByLabel('Name').focus();
      await shoot(page, theme, 'Forms: a focused field', 'forms-focus', forms.locator('pd-field').first());
    });
  }

  test('compose', async ({ page }) => {
    const rows = THEMES.map((theme) => shots.filter((s) => s.theme === theme));
    const cell = (s: { title: string; file: string }) =>
      `<figure><figcaption>${s.title}</figcaption><img alt="${s.title}" src="data:image/png;base64,${readFileSync(s.file).toString('base64')}"></figure>`;
    const html = `<!doctype html><meta charset="utf-8"><style>
      body{margin:0;font:14px system-ui;background:#888;color:#111}
      .row{display:grid;grid-template-columns:1fr 1fr;gap:16px;padding:16px}
      figure{margin:0;background:#fff;padding:8px;border-radius:6px}
      figcaption{font-weight:600;margin-bottom:6px}
      img{width:100%;display:block}
      h1{margin:16px;font-size:18px;color:#fff}
    </style>
    ${THEMES.map((theme, i) => `<h1>${theme}</h1><div class="row">${(rows[i] ?? []).map(cell).join('')}</div>`).join('')}`;
    await page.setViewportSize({ width: 1800, height: 1200 });
    await page.setContent(html);
    await page.waitForLoadState('load');
    const file = join(OUT, 'contact-sheet.png');
    await page.screenshot({ path: file, fullPage: true });
    writeFileSync(
      join(OUT, 'README.md'),
      'Contact sheet of the data parts of paddock-ui (plan X21): table, chart wrapper and forms helpers, light and dark, for the independent review (rules 22 and UI patterns 2, 5, 6, 8, 9, 15). Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x21`. NOT self-approved: awaiting the independent reviewer.\n',
    );
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
