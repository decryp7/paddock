import { expect, test, WorkbenchApp, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// The contact sheet of the workbench (plan X50; ADR 0013) for the independent review (rule 22): the workbench demo in both
// themes: the first screen, files open in the dock with the Explorer, the palette (commands, a provider by prefix, did you
// mean), the Explorer's context menu, an activity fly-out, the assistant's needs-AI state, the Jobs pane, and the narrow
// window (Explorer closed by the plan, 320 px). `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x50`
// rewrites docs/review/x50-workbench/. The reviewer looks at the sheet; this test only proves it was made from the real parts.

const DEMO = `http://localhost:${process.env['WORKBENCH_DEMO_PORT'] ?? 4302}`;
const OUT = join(__dirname, '..', 'docs', 'review', 'x50-workbench');
const THEMES = ['dark', 'light'] as const;

test.describe('contact sheet: workbench', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review/x50-workbench');
  test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });

  const shots: { theme: string; title: string; file: string }[] = [];

  async function open(page: Page, theme: string, width = 1440, height = 900): Promise<WorkbenchApp> {
    await page.emulateMedia({ colorScheme: theme as 'dark' | 'light', reducedMotion: 'reduce' });
    await page.setViewportSize({ width, height });
    await page.goto(DEMO);
    const wb = new WorkbenchApp(page);
    if (width >= 1024) await expect(wb.explorer.node('wd.files', 'src')).toBeVisible();
    await expect(page.locator('[data-testid="pd-dock-tab"]').first()).toBeVisible();
    return wb;
  }
  async function shoot(page: Page, theme: string, name: string, title: string): Promise<void> {
    mkdirSync(OUT, { recursive: true });
    const file = join(OUT, `${theme}-${name}.png`);
    await page.screenshot({ path: file, animations: 'disabled', caret: 'hide' });
    shots.push({ theme, title, file });
  }
  async function openFiles(wb: WorkbenchApp): Promise<void> {
    await wb.explorer.expand('wd.files', 'src');
    await wb.explorer.expand('wd.files', 'src/server');
    await wb.explorer.node('wd.files', 'src/server/mod.rs').dblclick();
    await wb.explorer.node('wd.files', 'src/main.rs').dblclick();
    await expect(wb.page.getByTestId('code-lines')).toBeVisible();
  }

  for (const theme of THEMES) {
    test(`workbench ${theme}`, async ({ page }) => {
      let wb = await open(page, theme);
      await shoot(page, theme, 'first', 'First screen: activity bar, Explorer, Welcome document and Outline in the dock, status bar');
      await openFiles(wb);
      await wb.activity.item('wd.act.problems').click();
      await expect(page.locator('[data-testid="pd-dock-tab"][data-panel-id="wd.problems"]')).toBeVisible();
      await expect(page.locator('pd-table [role=row]').nth(1)).toBeVisible();
      await expect.poll(() => page.locator('wd-problems-view').evaluate((e) => e.getBoundingClientRect().height)).toBeGreaterThanOrEqual(160);
      await shoot(page, theme, 'files', 'Two files as instances of Code, Problems (pd-table) below, Outline at the side; every group at or above its minimum');
      await wb.palette.open();
      await shoot(page, theme, 'palette', 'Palette: commands and "Open <view>", recent and common first, keys and categories, prefixes listed');
      await wb.palette.search('@handle');
      await shoot(page, theme, 'palette-symbols', 'Palette, "@": the names search provider (engine matching) with why it matched');
      await wb.palette.search('>indx repositry');
      await shoot(page, theme, 'palette-typo', 'Palette: nothing matches, "Did you mean" offers the close command');
      await page.keyboard.press('Escape');
      await wb.explorer.node('wd.files', 'README.md').click({ button: 'right' });
      await expect(wb.explorer.menu).toBeVisible();
      await shoot(page, theme, 'explorer-menu', 'Explorer: context menu of a file (also Shift+F10); folders paged with "Show more"');
      await page.keyboard.press('Escape');
      await wb.activity.item('wd.act.layouts').click();
      await expect(wb.activity.menu('wd.act.layouts')).toBeVisible();
      await shoot(page, theme, 'activity-flyout', 'Activity bar: a fly-out item (perspectives); the Problems badge is part of its name');
      await page.keyboard.press('Escape');
      await wb.activity.item('pd.wb.assistant.activity').click();
      await expect(page.locator('pd-assistant')).toContainText('not running');
      await shoot(page, theme, 'assistant', 'Assistant pane on Pitwall: the calm "not running" state with its fix (pd-needs-ai)');
      await wb.palette.open();
      await wb.palette.search('>show jobs');
      await wb.palette.run('Show jobs');
      await shoot(page, theme, 'jobs', 'Jobs: the shell jobs list as a status pane at the bottom edge');
      wb = await open(page, theme, 600, 800);
      await expect.poll(() => wb.frame.explorer()).toBe('closed');
      await shoot(page, theme, 'narrow', 'A 600 px window: the layout plan closed the Explorer for the document (Mod+B brings it back)');
      wb = await open(page, theme, 320, 640);
      await shoot(page, theme, '320', '320 px: nothing scrolls sideways (rule 18)');
    });
  }

  test('compose', async ({ page }) => {
    const rows = THEMES.map((theme) => shots.filter((s) => s.theme === theme));
    const cell = (s: { title: string; file: string }) =>
      `<figure><figcaption>${s.title}</figcaption><img alt="${s.title}" src="${s.file.split('/').pop()}"></figure>`;
    const html = `<!doctype html><meta charset="utf-8"><style>
      body{margin:0;font:14px system-ui;background:#888;color:#111}
      .row{display:grid;grid-template-columns:1fr 1fr 1fr;gap:16px;padding:16px;align-items:start}
      figure{margin:0;background:#fff;padding:8px;border-radius:6px}
      figcaption{font-weight:600;margin-bottom:6px}
      img{width:100%;display:block}
      h1{margin:16px;font-size:18px;color:#fff}
    </style>
    ${THEMES.map((theme, i) => `<h1>${theme}</h1><div class="row">${(rows[i] ?? []).map(cell).join('')}</div>`).join('')}`;
    // A page beside the shots (file URLs, not data URLs: twenty full windows inline exhaust the renderer).
    const sheet = join(OUT, 'contact-sheet.html');
    writeFileSync(sheet, html);
    await page.setViewportSize({ width: 1800, height: 1200 });
    await page.goto(`file://${sheet}`);
    await page.waitForLoadState('load');
    const file = join(OUT, 'contact-sheet.png');
    await page.screenshot({ path: file, fullPage: true, type: 'png' });
    writeFileSync(
      join(OUT, 'README.md'),
      'Contact sheet of the Paddock workbench (plan X50, ADR 0013): the workbench demo in both themes (first screen, documents and panes in the dock, palette with commands, a provider and "did you mean", Explorer context menu, activity fly-out, the assistant\'s needs-AI state, Jobs pane, a narrow window and 320 px), for the independent review (rule 22). Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x50`. NOT self-approved: awaiting the independent reviewer.\n',
    );
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
