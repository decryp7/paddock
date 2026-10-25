import { expect, test, type Locator, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';

// The contact sheet of <pd-view> (plan X51, ADR 0002) for the independent review (rule 22), review 5 (after review 4): every node kind of the sample
// document in both themes at desktop width, the form with its errors, the review list asking for a reason, the stale, invalid,
// loading and busy states, keyboard focus and hover, and the whole view at 320 px. Tall subjects are shot in slices of at most 1,800
// CSS px (a single bitmap taller than the GPU texture limit paints blank), and the images are composed into sheets by section, each
// no taller than about 3,000 CSS px, so every sheet opens at 100%. `CONTACT_SHEET=1 npx playwright test --project=review
// contact-sheet-x51` rewrites docs/review/x51-view/review-5/. The reviewer looks at the sheets; this test only proves they were
// made from the real components.

const GALLERY = `http://localhost:${process.env['PW_PORT'] ?? 4300}`;
// The commit every shot of this run was taken from (the sources are committed before the sheet is made, so it names the code on screen).
const commit = ((): string => {
  const git = (...args: string[]): string => execFileSync('git', args, { cwd: __dirname, encoding: 'utf8' }).trim();
  const dirty = git('status', '--porcelain', '--', '../projects', '../e2e/view.spec.ts').length > 0;
  return `${git('rev-parse', '--short=10', 'HEAD')}${dirty ? ' + uncommitted changes' : ''}`;
})();
const OUT = join(__dirname, '..', 'docs', 'review', 'x51-view', 'review-5');
const THEMES = ['dark', 'light'] as const;
const SLICE = 1800;
const SHEET_MAX = 3000;
const COLUMN = 760;

interface Shot {
  readonly theme: string;
  readonly section: string;
  readonly title: string;
  readonly name: string;
  readonly files: readonly string[];
}

test.describe('contact sheet: pd-view', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review/x51-view/review-5');
  test.describe.configure({ timeout: 300_000 });
  test.use({ viewport: { width: 1100, height: 900 }, deviceScaleFactor: 2 });

  const shots: Shot[] = [];

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

  /** Shoots `target` (padded by `pad` CSS px, for a focus ring or a shadow) in slices of at most SLICE px. */
  async function shoot(page: Page, theme: string, section: string, title: string, name: string, target: Locator, pad = 0, limit = Infinity, hover?: Locator): Promise<void> {
    mkdirSync(OUT, { recursive: true });
    // The gallery scrolls inside one of its own containers, so a clip below the fold would paint blank: the window is made tall enough
    // for a whole slice, the container is scrolled until the slice starts at the top, and the clip is taken in window coordinates.
    const viewport = page.viewportSize()!;
    await page.setViewportSize({ width: viewport.width, height: SLICE + 120 });
    const files: string[] = [];
    const total = Math.min(limit, await target.evaluate((e, p) => e.getBoundingClientRect().height + 2 * p, pad));
    for (let top = 0, k = 1; top < total; top += SLICE, k++) {
      const clip = await target.evaluate(
        (e, [pad, offset]) => {
          let box: HTMLElement | null = e.parentElement;
          while (box && !(box.scrollHeight > box.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(box).overflowY))) box = box.parentElement;
          const scroller = box ?? document.scrollingElement!;
          // Room below the last section, so any subject can be scrolled to the top of the window.
          if (!scroller.querySelector(':scope > #shot-spacer')) {
            const spacer = document.createElement('div');
            spacer.id = 'shot-spacer';
            spacer.style.height = '2400px';
            scroller.appendChild(spacer);
          }
          (scroller as HTMLElement).style.scrollBehavior = 'auto';
          scroller.scrollTop += e.getBoundingClientRect().top - pad + offset - 40;
          const r = e.getBoundingClientRect();
          return { x: Math.max(0, r.left - pad), y: Math.max(0, r.top - pad + offset), width: r.width + 2 * pad };
        },
        [pad, top] as [number, number],
      );
      // The pointer must be on the target when the picture is taken: scrolling moved the page under it, so it is put back, and the
      // focus is taken off whatever the earlier steps focused, so a hover shows the hover and not a ring.
      if (hover) {
        await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
        await hover.hover();
        await expect(hover).toBeVisible();
        const moved = await target.evaluate((e, [pad, offset]) => {
          const r = e.getBoundingClientRect();
          return { x: Math.max(0, r.left - pad), y: Math.max(0, r.top - pad + offset), width: r.width + 2 * pad };
        }, [pad, 0] as [number, number]);
        clip.x = moved.x;
        clip.y = moved.y;
      }
      // A shot that is not about hover is taken with the pointer off the view, so no row keeps the hover of an earlier shot.
      if (!hover) await page.mouse.move(0, 0);
      const height = Math.min(SLICE, total - top, SLICE + 120 - clip.y);
      const file = join(OUT, `${theme}-${name}${total > SLICE ? `-${k}` : ''}.png`);
      await page.screenshot({ path: file, animations: 'disabled', caret: 'hide', clip: { x: clip.x, y: clip.y, width: Math.min(clip.width, viewport.width - clip.x), height } });
      files.push(file);
    }
    await page.setViewportSize(viewport);
    shots.push({ theme, section, title, name, files });
  }

  for (const theme of THEMES) {
    test(`kinds ${theme}`, async ({ page }) => {
      const box = await open(page, theme);
      const node = (id: string) => box.locator(`[data-node-id="${id}"]`);
      const s = (section: string, title: string, name: string, target: Locator, pad = 0) => shoot(page, theme, section, title, name, target, pad);
      await s('text', 'Body text', 'text', node('lead'));
      await s('text', 'A heading is a Subtitle under the view title', 'heading', node('h'));
      await s('text', 'Code style: inset surface, wraps at spaces', 'code', node('code'));
      await s('text', 'The five tones as badges, packed at the gap (shape and word, not colour alone)', 'badges', node('badges'));
      await s('text', 'Progress bar; the track reads in dark', 'progress', node('prog'));
      await s('data', 'Table: sorted header, numbers right-aligned with the unit in the header, status nowrap, "Open" row action', 'table', node('t1'));
      await s('data', 'Paged table with its range and Previous / Next; Lines right-aligned', 'table-paged', node('t2'));
      await s('data', 'Lazy tree, one node opened later', 'tree', node('tree'));
      await node('tree').getByRole('button', { name: 'Expand: crates' }).click();
      await s('data', 'Tree after opening: only the mark of a status is coloured, the label stays text colour', 'tree-open', node('tree'));
      await s('data', 'Matrix fits its content; cell actions in text colour on a status tint', 'matrix', node('matrix'));
      await s('data', 'Chart: line with its unit', 'charts', node('c1'));
      await s('data', 'Chart: bar', 'chart-bar', node('c2'));
      await s('data', 'Chart: pie as a donut', 'chart-pie', node('c4'));
      await s('data', 'Chart: scatter in the accent, like the others', 'chart-scatter', node('c5'));
      await s('drawings', 'Layered graph: status glyph in each box, back edge in an orthogonal lane outside the boxes, labels beside the line', 'graph', node('g1'));
      await node('g1').locator('details summary').click();
      await s('drawings', 'The same graph with "Show as a list" open: status words included', 'graph-list', node('g1'));
      await node('g1').locator('details summary').click();
      await s('drawings', 'Radial graph: hub in the middle, spokes on a ring, canvas fitted', 'graph-radial', node('g2'));
      await s('drawings', 'Second radial graph: edges from spoke to spoke bow clear of the boxes and the canvas grows to hold them', 'graph-radial-bows', node('g3'));
      await s('drawings', 'Class diagram: stereotype, rule under the name, members from the left', 'diagram-class', node('d1'));
      await s('drawings', 'Sequence diagram: lifelines and messages in order', 'diagram-sequence', node('d2'));
      await s('form', 'Form: every field kind, help under the control, "Changes data" on Publish, the one filled button of the view', 'form', node('form'));
      await s('form', 'Review list: segmented filter, labelled states, actions', 'review', node('review'));
      await s('states', 'Empty state: 32 px muted icon, the action is secondary because the form holds the primary', 'empty', node('empty'));
      await s('states', "Needs local AI: the family card with the plugin's reason and the app's Open AI settings (the plugin's own action is not drawn)", 'needs-ai', node('ai'));
      await s('states', 'Unknown kind: only that node says "Update the app to view this", the kind in a muted suffix', 'unknown', node('future'));
    });

    test(`states ${theme}`, async ({ page }) => {
      const box = await open(page, theme);
      const s = (section: string, title: string, name: string, target: Locator, pad = 0, limit = Infinity) => shoot(page, theme, section, title, name, target, pad, limit);
      const form = box.locator('[data-node-id="form"]');
      await form.getByLabel('Release name').fill('');
      await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).click();
      await s('form', 'Form with a required field empty: the error replaces the help, nothing was sent; the placeholder differs from the old default', 'form-error', form);
      await form.getByLabel('Release name').fill('v2');
      await form.getByRole('button', { name: 'Output folder: Choose a folder…' }).click();
      await form.getByRole('button', { name: 'Publish token: Choose a secret…' }).click();
      await s('form', 'Chosen folder and secret are shown by name (the path cut in the middle, the full path as its tooltip)', 'form-chosen', form);
      await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).click();
      await s('form', "The host asks the plugin's question before a write action is sent: Cancel left of the action, the same \"Changes data\" badge", 'confirm', page.locator('[data-testid="view-confirm"]'));
      await page.getByTestId('view-confirm-no').click();
      const review = box.locator('[data-node-id="review"]');
      await review.getByRole('button', { name: /^Suppress: / }).click();
      await review.getByTestId('view-reason-send').click();
      await s('form', 'Review: an action that needs a reason asks for it in the row; empty is refused; the same badge on the send button', 'review-reason', review);
      await page.getByTestId('view-stale').click();
      await expect(box.getByTestId('view-error')).toHaveAttribute('data-kind', 'stale');
      await s('states', 'Stale patch: a warning (not a failure) with Reload; the last good view stays on screen, in full', 'stale', box.locator('pd-view'));
      await page.getByTestId('view-bad').click();
      await expect(box.getByTestId('view-error')).toHaveAttribute('data-kind', 'failed');
      await expect(box.locator('article')).toHaveCount(0);
      await s('states', 'Invalid view: error icon in danger, the plugin named, Retry and Open logs, nothing of the view is drawn', 'invalid', box.locator('pd-view'));
      await page.getByTestId('view-reload').click();
      await box.locator('article').waitFor();
      await page.getByTestId('view-busy-demo').click();
      await expect(box.getByTestId('view-busy')).toBeVisible();
      await s('states', 'Busy: the controls are off and a quiet "Working…" status with a thin bar shows (reduced motion: a static bar)', 'busy', box.locator('pd-view'), 4, 360);
      await page.getByTestId('view-busy-demo').click();
      await page.getByTestId('view-load-demo').click();
      await expect(box.getByTestId('view-loading')).toBeVisible();
      await s('states', 'Loading: a skeleton in the geometry of a view', 'loading', box.locator('pd-view'));
      await page.getByTestId('view-reload').click();
    });

    test(`focus and hover ${theme}`, async ({ page }) => {
      const box = await open(page, theme);
      const s = (title: string, name: string, target: Locator, pad = 6, hover?: Locator) => shoot(page, theme, 'interaction', title, name, target, pad, Infinity, hover);
      const form = box.locator('[data-node-id="form"]');
      await form.getByLabel('Release name').focus();
      await s('Keyboard focus in a text field', 'focus-field', form.getByLabel('Release name'));
      await form.getByRole('button', { name: /^Publish( Changes data)?$/ }).focus();
      await s('Keyboard focus on the one filled button (ring at least 3:1 against the surface)', 'focus-primary', form.getByRole('button', { name: /^Publish( Changes data)?$/ }));
      const tree = box.locator('[data-node-id="tree"]');
      await tree.getByRole('treeitem').first().focus();
      await s('Keyboard focus on a tree row (ring inside the row, over the hover fill)', 'focus-tree', tree);
      await tree.getByRole('treeitem').nth(1).hover();
      await s('Hover on a tree row (pointer only, no focus ring)', 'hover-tree', tree, 6, tree.getByRole('treeitem').nth(1));
      const graph = box.locator('[data-node-id="g1"]');
      await graph.locator('summary').focus();
      await s('Keyboard focus on the "Show as a list" disclosure', 'focus-list', graph.locator('details'));
      const act = graph.locator('g[role=button]').first();
      await act.focus();
      await s('Keyboard focus on a drawing box that has an action', 'focus-node', graph);
      await s('Hover on a drawing box that has an action (pointer only, no focus ring)', 'hover-node', graph, 6, act);
      const table = box.locator('[data-node-id="t1"]');
      await s('Hover on a table row that has a row action (pointer only, no focus ring)', 'hover-row', table, 6, table.locator('tbody tr').nth(1));
      await table.getByRole('button', { name: 'Sort by State, ascending' }).focus();
      await s('Keyboard focus on a sortable header', 'focus-sort', table);
      const review = box.locator('[data-node-id="review"]');
      await review.locator('.pd-view-review__filter button').nth(1).focus();
      await s('Keyboard focus on a filter segment', 'focus-filter', review.locator('.pd-view-review__filter'));
    });

    test(`320 px ${theme}`, async ({ page }) => {
      const box = await open(page, theme, 320);
      await page.setViewportSize({ width: 320, height: 1800 });
      await shoot(page, theme, 'narrow', 'The whole view at 320 px: the page never scrolls sideways; wide tables, matrices and drawings scroll inside their own region; the review list fits its column', 'narrow', box);
    });
  }

  test('compose', async ({ page }) => {
    const size = (file: string): { w: number; h: number } => {
      const b = readFileSync(file);
      return { w: b.readUInt32BE(16) / 2, h: b.readUInt32BE(20) / 2 };
    };
    const sections: Record<string, string> = {
      text: 'Text, badges and progress',
      data: 'Tables, trees, matrices and charts',
      drawings: 'Graphs and diagrams',
      form: 'Form and review list',
      states: 'Empty, needs-AI, unknown, stale, invalid, busy and loading',
      interaction: 'Keyboard focus and hover',
      narrow: 'The whole view at 320 px',
    };
    // A row is one subject (or one slice of a tall subject) in both themes.
    const rowsOf = (section: string) => {
      const out: { title: string; dark: string; light: string }[] = [];
      for (const n of new Map(shots.filter((x) => x.section === section).map((x) => [x.name, x])).values()) {
        const dark = shots.find((x) => x.theme === 'dark' && x.section === section && x.name === n.name)?.files ?? [];
        const light = shots.find((x) => x.theme === 'light' && x.section === section && x.name === n.name)?.files ?? [];
        const parts = Math.max(dark.length, light.length);
        for (let k = 0; k < parts; k++) out.push({ title: parts > 1 ? `${n.title} (part ${k + 1} of ${parts})` : n.title, dark: dark[k] ?? '', light: light[k] ?? '' });
      }
      return out;
    };
    const sheets: { id: string; label: string; rows: { title: string; dark: string; light: string }[] }[] = [];
    for (const section of Object.keys(sections)) {
      let current: (typeof sheets)[number] | null = null;
      let used = 0;
      let part = 0;
      const col = section === 'narrow' ? 330 : COLUMN;
      const heightOf = (f: string) => (f ? size(f).h * Math.min(1, col / size(f).w) : 0);
      for (const row of rowsOf(section)) {
        const h = Math.max(heightOf(row.dark), heightOf(row.light)) + 70;
        if (!current || used + h > SHEET_MAX) {
          part++;
          current = { id: `${section}${part > 1 ? `-${part}` : ''}`, label: `${sections[section]}${part > 1 ? ` (${part})` : ''}`, rows: [] };
          sheets.push(current);
          used = 0;
        }
        current.rows.push(row);
        used += h;
      }
    }
    const img = (f: string, t: string) => (f ? `<img alt="${t.replace(/"/g, '&quot;')}" src="data:image/png;base64,${readFileSync(f).toString('base64')}">` : '');
    const esc = (t: string) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;');
    for (const sheet of sheets) {
      const narrow = sheet.id.startsWith('narrow');
      const col = narrow ? 330 : COLUMN;
      const html = `<!doctype html><meta charset="utf-8"><style>
        body{margin:0;font:13px system-ui;background:#7a7f87;color:#111}
        h1{margin:12px 16px;font-size:16px;color:#fff}
        .row{display:grid;grid-template-columns:${col}px ${col}px;gap:16px;padding:0 16px 12px;align-items:start}
        .title{grid-column:1/3;color:#fff;font-weight:600;margin-top:8px}
        figure{margin:0;background:#fff;padding:6px;border-radius:6px}
        figure img{display:block;max-width:100%;height:auto}
        figcaption{font-size:11px;color:#444;margin-bottom:4px}
      </style><h1>${esc(sheet.label)}: review 5, not self-approved (left dark, right light)</h1>
      ${sheet.rows
        .map(
          (r) =>
            `<div class="row"><div class="title">${esc(r.title)}</div><figure><figcaption>dark</figcaption>${img(r.dark, r.title)}</figure><figure><figcaption>light</figcaption>${img(r.light, r.title)}</figure></div>`,
        )
        .join('')}`;
      await page.setViewportSize({ width: col * 2 + 80, height: 200 });
      await page.setContent(html);
      await page.waitForLoadState('load');
      await page.screenshot({ path: join(OUT, `sheet-${sheet.id}.png`), fullPage: true });
    }
    rmSync(join(OUT, 'contact-sheet.png'), { force: true });
    writeFileSync(
      join(OUT, 'README.md'),
      [
        '# Contact sheet of `<pd-view>`: review 5 (NOT self-approved)',
        '',
        `Plan X51, ADR 0002. Made from the real components after the fixes for review 4 (\`../REVIEW.md\`, \`../FIXES.md\`). Every section (kinds, states, interaction, 320 px) was captured in one run from one commit: \`${commit}\`. Awaiting the independent reviewer (rule 22).`,
        '',
        'Open the `sheet-*.png` files: each is one section, dark on the left and light on the right, no taller than about 3,000 px (6,000 px of bitmap), readable at 100%.',
        'The single images (`dark-*.png`, `light-*.png`) are the same shots at 2x; a subject taller than 1,800 px is cut into numbered slices (`-1`, `-2`).',
        '',
        ...sheets.map((s) => `- \`sheet-${s.id}.png\`: ${s.label}`),
        '',
        'Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x51` (rules 22 and 18, UI patterns 2, 5, 6, 8, 9, 15).',
        '',
      ].join('\n'),
    );
    expect(sheets.length).toBeGreaterThan(5);
    for (const s of sheets) expect(readFileSync(join(OUT, `sheet-${s.id}.png`)).length).toBeGreaterThan(5_000);
  });
});
