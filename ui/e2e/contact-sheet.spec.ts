import { expect, test, type Page } from '@decryp7/paddock-e2e';
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

  /** Forces :hover and :focus(-visible) on the elements marked `data-state` (Chromium's DevTools protocol), so one sheet shows them all. */
  async function forceStates(page: Page): Promise<void> {
    const client = await page.context().newCDPSession(page);
    await client.send('DOM.enable');
    await client.send('CSS.enable');
    const { root } = await client.send('DOM.getDocument', { depth: 0 });
    const force = async (selector: string, forced: string[]): Promise<void> => {
      const { nodeIds } = await client.send('DOM.querySelectorAll', { nodeId: root.nodeId, selector });
      for (const nodeId of nodeIds) await client.send('CSS.forcePseudoState', { nodeId, forcedPseudoClasses: forced });
    };
    await force('[data-state="hover"]', ['hover']);
    await force('[data-state="focus"]:not(pd-select), pd-select[data-state="focus"] button', ['focus', 'focus-visible']);
  }

  for (const theme of THEMES) {
    for (const accent of ['debrief', 'pitwall'] as const) {
      test(`parts ${theme} ${accent}`, async ({ page }) => {
        mkdirSync(join(OUT, 'parts'), { recursive: true });
        await page.goto(`${GALLERY}/?theme=${theme}&accent=${accent}`);
        await page.locator('[data-testid="card"] pd-card').first().waitFor();
        const ids = accent === 'debrief'
          ? ([['icons', 'Icons (pd:): glyph and mat-icon literal rows, 16 and 24 px'], ['controls', 'Controls'], ['states', 'Control states (rest, hover, focus, disabled; page and card)'], ['path-text', 'Path text'], ['empty-state', 'Empty state with an icon']] as const)
          : ([['states', 'Control states, Pitwall accent (purple)']] as const);
        await forceStates(page);
        for (const [id, title] of ids) {
          const file = join(OUT, 'parts', `${theme}-${accent}-${id}.png`);
          await page.locator(`section[data-testid="${id}"]`).screenshot({ path: file });
          shots.push({ theme, title, file });
        }
        if (accent !== 'debrief') return;
        // The themed select, open, with the keyboard-active option: the trigger and the whole list in one crop.
        const trigger = page.getByRole('combobox', { name: 'Themed select', exact: true });
        await trigger.scrollIntoViewIfNeeded();
        await trigger.click();
        await expect(page.getByRole('option', { name: 'Comfortable' })).toBeVisible();
        await page.keyboard.press('ArrowDown');
        const a = (await trigger.boundingBox())!;
        const b = (await page.locator('.pd-list').boundingBox())!;
        const x = Math.max(0, Math.min(a.x, b.x) - 24);
        const y = Math.max(0, Math.min(a.y, b.y) - 24);
        const clip = { x, y, width: Math.max(a.x + a.width, b.x + b.width) + 24 - x, height: Math.max(a.y + a.height, b.y + b.height) + 24 - y };
        const file = join(OUT, 'parts', `${theme}-select-open.png`);
        await page.screenshot({ path: file, clip });
        shots.push({ theme, title: 'Themed select (open, one option keyboard-active)', file });
      });
    }

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
    writeFileSync(join(OUT, 'README.md'), 'Contact sheet of the pd: icon set and the changed components (light and dark), for the independent drawing review (second round, after the fixes of the first review). Includes control states (rest, hover, focus, disabled), switches and checkboxes on and off, the pd-glyph and mat-icon-literal rows at 16 and 24 px, and the open select in full. Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review`. Not self-approved: see the report.\n');
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
