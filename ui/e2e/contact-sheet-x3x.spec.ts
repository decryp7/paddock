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
const GLYPHS = ['file', 'file-text', 'file-code', 'archive', 'reveal', 'folder'] as const;

test.describe('contact sheet: files and storage', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review/x31-x32-files-storage');
  test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });

  const shots: { theme: string; title: string; file: string }[] = [];

  /** The pointer is moved away first, so no row is tinted by a hover that is not the subject. */
  async function shoot(page: Page, theme: string, name: string, title: string, keepPointer = false): Promise<void> {
    mkdirSync(OUT, { recursive: true });
    if (!keepPointer) await page.mouse.move(1, 1);
    const file = join(OUT, `${theme}-${name}.png`);
    await page.screenshot({ path: file, animations: 'disabled', caret: 'hide' });
    shots.push({ theme, title, file });
  }

  async function openStorage(page: Page, app: import('@decryp7/paddock-e2e').ShellApp): Promise<void> {
    await page.goto(`${DEMO}/`);
    await page.getByRole('button', { name: 'Settings…', exact: true }).click();
    await app.settings.nav.getByRole('button', { name: 'Storage', exact: true }).click();
    await expect(page.getByTestId('storage-table')).toBeVisible();
  }

  for (const theme of THEMES) {
    test(`files and storage ${theme}`, async ({ page, app }) => {
      await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
      // The glyphs, drawn from the real path data at the sizes they are used at (14, 16 and 24 px) and enlarged (48 px).
      const [bg, fg] = theme === 'dark' ? ['#1b1d22', '#e6e8ec'] : ['#ffffff', '#1d2129'];
      const rows = GLYPHS.map((n) => `<div class="r"><code>${n}</code>${[14, 16, 24, 48].map((px) => `<svg width="${px}" height="${px}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="${PD_ICON_PATHS[n]}"/></svg>`).join('')}</div>`).join('');
      await page.setContent(`<!doctype html><meta charset="utf-8"><style>body{margin:0;padding:24px;background:${bg};color:${fg};font:14px system-ui}.r{display:flex;align-items:center;gap:28px;margin-bottom:18px}code{width:110px;font:13px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}</style>${rows}`);
      await shoot(page, theme, 'glyphs', 'The file-type glyphs and reveal of the pd: set at 14, 16, 24 and 48 px (folder for comparison)');

      await page.goto(`${DEMO}/#/files`);
      await expect(app.frame.heading).toHaveText('Files');
      await shoot(page, theme, 'files', 'Files page: the file name leads, its folder is the muted second line, a glyph shows the kind (16 px)');
      await page.getByTestId('recent-entry').first().hover();
      await shoot(page, theme, 'files-hover', 'Files page: a hovered entry shows its Reveal and Remove icon buttons (28 px targets)', true);

      await openStorage(page, app);
      await shoot(page, theme, 'storage', 'Settings > Storage: one shared table, one line per row, grouped; the group row carries the size');
      const cache = page.locator('.pd-row[data-key="logs"]');
      await cache.hover();
      await shoot(page, theme, 'storage-hover', 'Settings > Storage: a hovered row shows What is inside and Show in the file manager', true);
      await cache.getByRole('button', { name: 'More actions' }).click();
      await shoot(page, theme, 'storage-menu', 'Settings > Storage: the row menu, Clear in danger text last', true);
      await page.keyboard.press('Escape');

      await page.locator('.pd-row[data-key="cache"]').hover();
      await page.locator('.pd-row[data-key="cache"]').getByRole('button', { name: 'What is inside' }).click();
      await expect(page.getByTestId('storage-tree')).toBeVisible();
      await page.getByTestId('storage-tree').scrollIntoViewIfNeeded();
      await shoot(page, theme, 'storage-tree', 'Settings > Storage: the size tree, the folder size on its group row, glyphs on the items');

      await page.getByTestId('storage-clear-cache').click();
      await expect(page.getByRole('alertdialog', { name: 'Clear cache?' })).toBeVisible();
      await shoot(page, theme, 'storage-confirm-cache', 'The clear confirmation for the cache: Keep is the default, Clear is the danger action');
      await page.keyboard.press('Escape');
      const logs = page.locator('.pd-row[data-key="logs"]');
      await logs.hover();
      await logs.getByRole('button', { name: 'More actions' }).click();
      await page.getByRole('menuitem', { name: 'Clear logs' }).click();
      await expect(page.getByRole('alertdialog', { name: 'Delete logs?' })).toBeVisible();
      await shoot(page, theme, 'storage-confirm-data', 'The clear confirmation for the user\'s own data: it says it cannot be rebuilt');
      await page.keyboard.press('Escape');

      // 320 px: a fresh page each time, so the Settings window of the shots above is not still open over the Files page.
      await page.setViewportSize({ width: 320, height: 640 });
      await page.goto('about:blank');
      await page.goto(`${DEMO}/#/files`);
      await expect(app.frame.heading).toHaveText('Files');
      await expect(page.getByTestId('storage-table')).toHaveCount(0);
      await shoot(page, theme, 'files-320', 'Files page at 320 px: nothing scrolls sideways, the name leads and the folder keeps its separator');
      await page.goto('about:blank');
      await openStorage(page, app);
      await page.getByTestId('storage-table').scrollIntoViewIfNeeded();
      await shoot(page, theme, 'storage-320', 'Settings > Storage at 320 px: the table, still one line per row, actions behind the menu');
      const narrow = page.locator('.pd-row[data-key="logs"]');
      await narrow.hover();
      await narrow.getByRole('button', { name: 'More actions' }).click();
      await shoot(page, theme, 'storage-320-menu', 'Settings > Storage at 320 px: the row menu, Clear in danger text last', true);
    });
  }

  test('compose', async ({ page }) => {
    const cell = (s: { title: string; file: string }) => `<figure><figcaption>${s.title}</figcaption><img alt="${s.title}" src="${s.file.split('/').pop()}"></figure>`;
    const html = `<!doctype html><meta charset="utf-8"><style>
      body{margin:0;font:14px system-ui;background:#888;color:#111}
      .row{display:grid;grid-template-columns:1fr 1fr 1fr 1fr;gap:16px;padding:16px;align-items:start}
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
      'Contact sheet of the files and storage parts (plan X31/X32): the shell demo\'s Files page with file-type glyphs on the recent entries, and Settings > Storage with its size tree, in both themes, for the independent review (rule 22) of the new `pd:` glyphs (`file-text`, `file-code`, `archive`, `reveal`) and the redrawn `file` and the two pages. Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x3x`. NOT self-approved: awaiting the independent reviewer.\n',
    );
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
