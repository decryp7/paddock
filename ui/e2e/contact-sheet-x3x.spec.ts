import { expect, test, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { PD_ICON_PATHS } from '../projects/paddock-ui/icons/src/paths';

// The contact sheet of the files and storage parts (plan X31/X32) for the independent review (rule 22): the new file-type glyphs of
// the `pd:` set at 14, 16 and 24 px (the review looks at the drawings themselves), the Files page's recent list with them, and the
// Storage page with its size tree, in both themes. `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x3x`
// rewrites docs/review/x31-x32-files-storage/. The reviewer looks at the sheet; this test only proves it was made from the real
// parts. NOT self-approved.

const DEMO = `http://localhost:${process.env['SHELL_DEMO_PORT'] ?? 4301}`;
const OUT = join(__dirname, '..', 'docs', 'review', 'x31-x32-files-storage');
const THEMES = ['dark', 'light'] as const;
const GLYPHS = ['file', 'file-text', 'file-code', 'archive', 'folder'] as const;

test.describe('contact sheet: files and storage', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review/x31-x32-files-storage');
  test.use({ viewport: { width: 900, height: 640 }, deviceScaleFactor: 2 });

  const shots: { theme: string; title: string; file: string }[] = [];

  async function shoot(page: Page, theme: string, name: string, title: string): Promise<void> {
    mkdirSync(OUT, { recursive: true });
    const file = join(OUT, `${theme}-${name}.png`);
    await page.screenshot({ path: file, animations: 'disabled', caret: 'hide' });
    shots.push({ theme, title, file });
  }

  for (const theme of THEMES) {
    test(`files and storage ${theme}`, async ({ page, app }) => {
      await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
      // The glyphs, drawn from the real path data at the sizes they are used at (14, 16 and 24 px) and enlarged (48 px).
      const [bg, fg] = theme === 'dark' ? ['#1b1d22', '#e6e8ec'] : ['#ffffff', '#1d2129'];
      const rows = GLYPHS.map((n) => `<div class="r"><code>${n}</code>${[14, 16, 24, 48].map((px) => `<svg width="${px}" height="${px}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="${PD_ICON_PATHS[n]}"/></svg>`).join('')}</div>`).join('');
      await page.setContent(`<!doctype html><meta charset="utf-8"><style>body{margin:0;padding:24px;background:${bg};color:${fg};font:14px system-ui}.r{display:flex;align-items:center;gap:28px;margin-bottom:18px}code{width:110px}</style>${rows}`);
      await shoot(page, theme, 'glyphs', 'The new file-type glyphs of the pd: set at 14, 16, 24 and 48 px (folder for comparison)');
      await page.goto(`${DEMO}/#/files`);
      await expect(app.frame.heading).toHaveText('Files');
      await shoot(page, theme, 'files', 'Files page: recent entries drawn with file-type glyphs (16 px), the full path as the tooltip');
      await page.goto(`${DEMO}/`);
      await page.getByRole('button', { name: 'Settings…', exact: true }).click();
      await app.settings.nav.getByRole('button', { name: 'Storage', exact: true }).click();
      await expect(page.getByTestId('storage-table')).toBeVisible();
      await shoot(page, theme, 'storage', 'Settings > Storage: what the app keeps, per folder, with Folders, Show and Clear');
      await page.getByTestId('storage-tree-cache').click();
      await expect(page.getByTestId('storage-tree')).toBeVisible();
      await page.getByTestId('storage-tree').scrollIntoViewIfNeeded();
      await shoot(page, theme, 'storage-tree', 'Settings > Storage: the size tree of the cache as a table grouped by folder');
    });
  }

  test('compose', async ({ page }) => {
    const cell = (s: { title: string; file: string }) => `<figure><figcaption>${s.title}</figcaption><img alt="${s.title}" src="${s.file.split('/').pop()}"></figure>`;
    const html = `<!doctype html><meta charset="utf-8"><style>
      body{margin:0;font:14px system-ui;background:#888;color:#111}
      .row{display:grid;grid-template-columns:1fr 1fr 1fr;gap:16px;padding:16px;align-items:start}
      figure{margin:0;background:#fff;padding:8px;border-radius:6px}
      figcaption{font-weight:600;margin-bottom:6px}
      img{width:100%;display:block}
      h1{margin:16px;font-size:18px;color:#fff}
    </style>
    ${THEMES.map((theme) => `<h1>${theme}</h1><div class="row">${shots.filter((s) => s.theme === theme).map(cell).join('')}</div>`).join('')}`;
    const sheet = join(OUT, 'contact-sheet.html');
    writeFileSync(sheet, html);
    await page.setViewportSize({ width: 1800, height: 1200 });
    await page.goto(`file://${sheet}`);
    await page.waitForLoadState('load');
    const file = join(OUT, 'contact-sheet.png');
    await page.screenshot({ path: file, fullPage: true, type: 'png' });
    writeFileSync(
      join(OUT, 'README.md'),
      'Contact sheet of the files and storage parts (plan X31/X32): the shell demo\'s Files page with file-type glyphs on the recent entries, and Settings > Storage with its size tree, in both themes, for the independent review (rule 22) of the four new `pd:` glyphs (`file`, `file-text`, `file-code`, `archive`) and the two pages. Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x3x`. NOT self-approved: awaiting the independent reviewer.\n',
    );
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
