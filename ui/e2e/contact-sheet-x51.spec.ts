import { expect, test, type Locator, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// The contact sheet of <pd-view> (plan X51, ADR 0002) for the independent review (rule 22): every node kind of the sample document in
// both themes at desktop width, the form with its errors, the review list asking for a reason, the stale and invalid states, and the
// whole view at 320 px. `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x51` rewrites docs/review/x51-view/. The
// reviewer looks at the sheet; this test only proves it was made from the real components.

const GALLERY = `http://localhost:${process.env['PW_PORT'] ?? 4300}`;
const OUT = join(__dirname, '..', 'docs', 'review', 'x51-view');
const THEMES = ['dark', 'light'] as const;

test.describe('contact sheet: pd-view', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review/x51-view');
  test.describe.configure({ timeout: 240_000 });
  test.use({ viewport: { width: 1100, height: 900 }, deviceScaleFactor: 2 });

  const shots: { theme: string; title: string; file: string }[] = [];

  async function open(page: Page, theme: string, width = 1100): Promise<Locator> {
    await page.emulateMedia({ colorScheme: theme as 'dark' | 'light', reducedMotion: 'reduce' });
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`${GALLERY}/?theme=${theme}&accent=pacenotes`);
    const box = page.locator('[data-testid="view-box"]');
    await box.locator('pd-view article').waitFor();
    await expect(box.locator('pd-chart canvas')).toHaveCount(5);
    await page.mouse.move(0, 0);
    return box;
  }
  async function shoot(theme: string, title: string, name: string, target: Locator): Promise<void> {
    mkdirSync(OUT, { recursive: true });
    const file = join(OUT, `${theme}-${name}.png`);
    await target.scrollIntoViewIfNeeded();
    await target.screenshot({ path: file, animations: 'disabled', caret: 'hide' });
    shots.push({ theme, title, file });
  }

  for (const theme of THEMES) {
    test(`kinds ${theme}`, async ({ page }) => {
      const box = await open(page, theme);
      const node = (id: string) => box.locator(`[data-node-id="${id}"]`);
      await shoot(theme, 'Heading, body text, code and caption styles', 'text', node('lead'));
      await shoot(theme, 'Code style', 'code', node('code'));
      await shoot(theme, 'The five tones as badges (shape and word, not colour alone)', 'badges', node('badges'));
      await shoot(theme, 'Progress bar with its label and value', 'progress', node('prog'));
      await shoot(theme, 'Table: sorted header with arrow, tones with icons, cell links, "Open" row action', 'table', node('t1'));
      await shoot(theme, 'Paged table with its range and Previous / Next', 'table-paged', node('t2'));
      await shoot(theme, 'Lazy tree, one node opened', 'tree', node('tree'));
      await node('tree').getByRole('button', { name: 'Expand: crates' }).click();
      await shoot(theme, 'Tree after opening a node with its children in the list', 'tree-open', node('tree'));
      await shoot(theme, 'Layered graph (cycle broken), tones as outlines, boxes with an action are buttons', 'graph', node('g1'));
      await shoot(theme, 'Radial graph', 'graph-radial', node('g2'));
      await shoot(theme, 'Class diagram: group above the name, detail under it', 'diagram-class', node('d1'));
      await shoot(theme, 'Sequence diagram: lifelines and messages in order', 'diagram-sequence', node('d2'));
      await shoot(theme, 'Form: every field kind, help under the control, the "Changes data" badge on Publish', 'form', node('form'));
      await shoot(theme, 'Review list with state filter, labelled states and actions', 'review', node('review'));
      await shoot(theme, 'Matrix with sparse cells and cell actions', 'matrix', node('matrix'));
      await shoot(theme, 'Chart: line (the bar, area, pie and scatter below use the same wrapper)', 'charts', node('c1'));
      await shoot(theme, 'Chart: bar', 'chart-bar', node('c2'));
      await shoot(theme, 'Chart: pie as a donut', 'chart-pie', node('c4'));
      await shoot(theme, 'Chart: scatter', 'chart-scatter', node('c5'));
      await shoot(theme, 'Empty state with its action', 'empty', node('empty'));
      await shoot(theme, 'Needs local AI', 'needs-ai', node('ai'));
      await shoot(theme, 'Unknown kind: only that node says "Update the app to view this"', 'unknown', node('future'));
    });

    test(`states ${theme}`, async ({ page }) => {
      const box = await open(page, theme);
      const form = box.locator('[data-node-id="form"]');
      await form.getByLabel('Release name').fill('');
      await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).click();
      await shoot(theme, 'Form with a required field empty: the error replaces the help, alert icon at 14 px, nothing was sent', 'form-error', form);
      await form.getByLabel('Release name').fill('v2');
      await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).click();
      await shoot(theme, 'The host asks the plugin\'s question before a write action is sent (gallery stand-in for the OS window)', 'confirm', page.locator('[data-testid="view-confirm"]'));
      await page.getByTestId('view-confirm-no').click();
      const review = box.locator('[data-node-id="review"]');
      await review.getByRole('button', { name: /^Suppress: / }).click();
      await review.getByTestId('view-reason-send').click();
      await shoot(theme, 'Review: an action that needs a reason asks for it in the row; empty is refused', 'review-reason', review);
      await page.getByTestId('view-stale').click();
      await shoot(theme, 'Stale patch: inline error with Retry, the last good view stays on screen', 'stale', box.locator('pd-view'));
      await page.getByTestId('view-bad').click();
      await shoot(theme, 'Invalid view: inline error with Retry, nothing of it is drawn', 'invalid', box);
    });

    test(`320 px ${theme}`, async ({ page }) => {
      const box = await open(page, theme, 320);
      await page.setViewportSize({ width: 320, height: 9000 });
      await shoot(theme, 'The whole view at 320 px: the page never scrolls sideways; wide tables, matrices and drawings scroll inside their own region', 'narrow', box);
    });
  }

  test('compose', async ({ page }) => {
    const rows = THEMES.map((theme) => shots.filter((s) => s.theme === theme));
    const cell = (s: { title: string; file: string }) =>
      `<figure><figcaption>${s.title}</figcaption><img alt="${s.title}" src="data:image/png;base64,${readFileSync(s.file).toString('base64')}"></figure>`;
    const html = `<!doctype html><meta charset="utf-8"><style>
      body{margin:0;font:14px system-ui;background:#888;color:#111}
      .row{display:grid;grid-template-columns:1fr 1fr;gap:16px;padding:16px;align-items:start}
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
      'Contact sheet of `<pd-view>` (plan X51, ADR 0002): every node kind of the sample document, the form and review states, the stale and invalid errors and the whole view at 320 px, light and dark, for the independent review (rules 22 and 18, UI patterns 2, 5, 6, 8, 9, 15). Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x51`. NOT self-approved: awaiting the independent reviewer.\n',
    );
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
