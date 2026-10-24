import { expect, test } from '@decryp7/paddock-e2e';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// The contact sheet for the independent drawing review (rule 22): the pd: icon set and the changed components, light and
// dark, in one PNG under docs/review/. `CONTACT_SHEET=1 npx playwright test --project=review` rewrites it. The reviewer
// looks at the sheet; this test only proves it was made from the real components (it fails if a section is missing).

const GALLERY = `http://localhost:${process.env['PW_PORT'] ?? 4300}`;
const DEMO = `http://localhost:${process.env['SHELL_DEMO_PORT'] ?? 4301}`;
const OUT = join(__dirname, '..', 'docs', 'review');
const THEMES = ['dark', 'light'] as const;

test.describe('contact sheet', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review');
  test.use({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 2 });

  const shots: { theme: string; title: string; file: string }[] = [];

  for (const theme of THEMES) {
    test(`parts ${theme}`, async ({ page }) => {
      mkdirSync(join(OUT, 'parts'), { recursive: true });
      await page.goto(`${GALLERY}/?theme=${theme}&accent=debrief`);
      await page.locator('[data-testid="card"] pd-card').first().waitFor();
      for (const [id, title] of [
        ['icons', 'Icons (pd:)'],
        ['controls', 'Controls'],
        ['path-text', 'Path text'],
        ['empty-state', 'Empty state with an icon'],
      ] as const) {
        const file = join(OUT, 'parts', `${theme}-${id}.png`);
        await page.locator(`section[data-testid="${id}"]`).screenshot({ path: file });
        shots.push({ theme, title, file });
      }
      // The themed select, open.
      await page.getByRole('combobox', { name: 'Themed select' }).click();
      await expect(page.getByRole('option', { name: 'Comfortable' })).toBeVisible();
      const file = join(OUT, 'parts', `${theme}-select-open.png`);
      await page.locator(`section[data-testid="controls"]`).screenshot({ path: file });
      shots.push({ theme, title: 'Themed select (open)', file });
    });

    test(`settings window ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.setViewportSize({ width: 880, height: 620 });
      await page.goto(`${DEMO}/#/dialog?id=pd.settings&host=main&label=d-main-1&title=Settings&dialogInput=${encodeURIComponent(JSON.stringify({ page: 'pd.appearance' }))}`);
      await page.getByRole('navigation', { name: 'Settings pages' }).waitFor();
      await expect(page.locator('pd-setting-field[data-key="theme"]')).toBeVisible();
      await page.locator('pd-setting-field[data-key="theme"]').getByRole('combobox').click();
      await expect(page.getByRole('option').first()).toBeVisible();
      const file = join(OUT, 'parts', `${theme}-settings-select.png`);
      await page.screenshot({ path: file });
      shots.push({ theme, title: 'Settings: themed select in Appearance', file });
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
    writeFileSync(join(OUT, 'README.md'), 'Contact sheet of the pd: icon set and the changed components (light and dark), for the independent drawing review. Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review`. Not self-approved: see the report.\n');
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
