import { expect, test, WorkbenchApp, type Locator, type Page } from '@decryp7/paddock-e2e';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// The contact sheet of the workbench (plan X50; ADR 0013) for the independent review (rule 22), review 2: the workbench demo
// at @2x in both themes with real app accents (dark: Pacenotes, light: Debrief), covering every state review 1 asked for:
// the first screen; documents, Problems and the Explorer; the palette (commands with groups, a provider, did you mean, no
// match); the Explorer menu at the pointer; an activity fly-out; the assistant (needs-AI, streaming with Stop, a tool card
// with a citation, the write approval, a failed turn); a running job; a pop-out window; keyboard focus on each surface;
// a narrow window; 320 px with the Explorer and with Problems.
// `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x50` rewrites docs/review/x50-workbench/.

const DEMO = `http://localhost:${process.env['WORKBENCH_DEMO_PORT'] ?? 4302}`;
const OUT = join(__dirname, '..', 'docs', 'review', 'x50-workbench');
const SETS = [
  { theme: 'dark', accent: 'pacenotes' },
  { theme: 'light', accent: 'debrief' },
] as const;
type Set = (typeof SETS)[number];

test.describe('contact sheet: workbench', () => {
  test.skip(!process.env['CONTACT_SHEET'], 'set CONTACT_SHEET=1 to rewrite docs/review/x50-workbench');
  test.describe.configure({ timeout: 240_000 });
  test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });

  const shots: { theme: string; title: string; file: string }[] = [];

  async function open(page: Page, set: Set, options: { width?: number; height?: number; query?: string; hash?: string } = {}): Promise<WorkbenchApp> {
    await page.emulateMedia({ colorScheme: set.theme, reducedMotion: 'reduce' });
    await page.setViewportSize({ width: options.width ?? 1440, height: options.height ?? 900 });
    await page.goto(`${DEMO}/?accent=${set.accent}${options.query ? `&${options.query}` : ''}${options.hash ?? ''}`);
    const wb = new WorkbenchApp(page);
    if (!options.hash) {
      await expect(page.locator('pd-activity-bar')).toBeVisible();
      // The real first paint: the dock has restored its layout (review 2 P2-K).
      await expect(page.locator('pd-dock [role=tab]').first()).toBeVisible();
    }
    return wb;
  }
  async function shoot(page: Page, set: Set, name: string, title: string, clip?: Locator, keepPointer = false): Promise<void> {
    mkdirSync(OUT, { recursive: true });
    // The pointer rests on the title bar's empty end, so no hover tint reads as a state (review 2 P2-K).
    if (!keepPointer) await page.mouse.move((page.viewportSize()?.width ?? 1440) - 4, 12);
    const file = join(OUT, `${set.theme}-${name}.png`);
    if (clip) await clip.screenshot({ path: file, animations: 'disabled', caret: 'hide' });
    else await page.screenshot({ path: file, animations: 'disabled', caret: 'hide' });
    shots.push({ theme: set.theme, title, file });
  }
  async function openFiles(wb: WorkbenchApp): Promise<void> {
    await wb.explorer.expand('wd.files', 'src');
    await wb.explorer.expand('wd.files', 'src/server');
    await wb.explorer.node('wd.files', 'src/server/mod.rs').dblclick();
    await wb.explorer.node('wd.files', 'src/main.rs').dblclick();
    await expect(wb.page.getByTestId('code-lines')).toBeVisible();
  }
  async function ask(page: Page, text: string): Promise<void> {
    // The previous turn has ended (no Stop): a question sent while one runs would be refused.
    await expect(page.getByTestId('assistant-stop')).toHaveCount(0);
    const input = page.getByTestId('assistant-input');
    await input.fill(text);
    await input.press('Enter');
  }

  for (const set of SETS) {
    test(`workbench ${set.theme}`, async ({ page }) => {
      let wb = await open(page, set);
      await expect(wb.explorer.node('wd.files', 'src')).toBeVisible();
      await shoot(page, set, '01-first', `First screen (${set.accent} accent): one primary on Welcome, sentence-case Explorer, a status item at the start`);

      await openFiles(wb);
      await wb.activity.item('wd.act.problems').click();
      await expect(page.locator('wd-problems-view [role=row]').nth(3)).toBeVisible();
      await shoot(page, set, '02-files', 'Code instances named by file; Problems by severity below (all rows in view); the Explorer follows the active file');

      await wb.palette.open();
      await shoot(page, set, '03-palette', 'Palette, empty: "Frequently used", then "Other commands"; sans key hints');
      await wb.palette.search('>show');
      await shoot(page, set, '03b-palette-marks', 'Palette, ">show": the matched words underlined in the accent');
      await wb.palette.search('@handle');
      await shoot(page, set, '04-palette-symbols', 'Palette, "@": the names search provider; matches underlined in the accent');
      await wb.palette.search('>indx repositry');
      await shoot(page, set, '05-palette-typo', 'Palette: no match, "Did you mean" quotes the words without the prefix');
      await wb.palette.search('>zzzzzz');
      await shoot(page, set, '06-palette-none', 'Palette: no match and no suggestion');
      await page.keyboard.press('Escape');

      await expect(wb.palette.dialog).toHaveCount(0);
      const readme = wb.explorer.node('wd.files', 'README.md');
      await expect(readme).toBeVisible();
      const box = (await readme.boundingBox())!;
      await page.mouse.click(box.x + 60, box.y + box.height / 2, { button: 'right' });
      await expect.poll(async () => {
        const m = await wb.explorer.menu.boundingBox();
        return m ? Math.abs(m.x - (box.x + 60)) : 999;
      }, { message: 'the menu at the pointer' }).toBeLessThanOrEqual(8);
      await shoot(page, set, '07-explorer-menu', 'Explorer: the menu at the pointer, below its row (the label stays readable), the row marked');
      await page.keyboard.press('Escape');
      await readme.click();
      await page.keyboard.press('Shift+F10');
      await expect(wb.explorer.menu).toBeVisible();
      await expect(wb.explorer.menu.getByRole('menuitem').first()).toBeFocused();
      await page.keyboard.press('ArrowDown');
      await expect(wb.explorer.menu.getByRole('menuitem').nth(1)).toBeFocused();
      await shoot(page, set, '07b-explorer-menu-keys', 'Explorer menu by Shift+F10 with keyboard focus on an item (2 px ring)');
      await page.keyboard.press('Escape');

      await wb.activity.item('wd.act.layouts').click();
      await expect(wb.activity.menu('wd.act.layouts')).toBeVisible();
      await page.keyboard.press('ArrowDown');
      await shoot(page, set, '08-activity-flyout', 'Activity fly-out with a focused item; one selected item, open dots for views in the dock');
      await page.keyboard.press('Escape');

      await wb.activity.item('pd.wb.assistant.activity').click();
      await expect(page.locator('pd-assistant')).toContainText('not running');
      await shoot(page, set, '09-assistant-needs-ai', 'Assistant, Pitwall not running: one primary (Open Pitwall), the composer inert with its reason');

      wb = await open(page, set, { query: 'pitwall=ready' });
      await wb.activity.item('pd.wb.assistant.activity').click();
      await ask(page, '/slow Explain the request pipeline');
      await expect(page.getByTestId('assistant-stop')).toBeVisible();
      await shoot(page, set, '10-assistant-streaming', 'Assistant streaming: Stop in the place of Send; the conversation title as the heading');
      await page.getByTestId('assistant-stop').click();
      await expect(page.getByTestId('assistant-send')).toBeVisible();
      await ask(page, '/tool request handler');
      await expect(page.getByTestId('assistant-cite').first()).toBeVisible();
      await shoot(page, set, '11-assistant-tool', 'Assistant: a tool card and an answer with a citation button');
      // Review 3 P2-1: a code block and inline code on their own surface, in both themes.
      const withCode = page.getByTestId('assistant-answer').filter({ has: page.locator('pre') }).last();
      await withCode.scrollIntoViewIfNeeded();
      await shoot(page, set, '11b-assistant-code', 'Assistant: a code block and inline code on the code surface (differs from the pane in both themes)', withCode);
      await page.getByTestId('assistant-allow-actions').check();
      await ask(page, '/write');
      await expect(page.getByTestId('assistant-approval')).toBeVisible();
      // Review 3 P3-1: open Details by its summary (the <details> centre is empty space), then give the focus back to Decline
      // by keyboard, so the shot shows both the tool with its arguments and the focused safe default.
      await page.getByTestId('assistant-approval-details').locator('summary').click();
      await expect(page.getByTestId('assistant-approval-tool')).toBeVisible();
      await page.getByTestId('assistant-decline').focus();
      await page.keyboard.press('Shift+Tab');
      await page.keyboard.press('Tab');
      await expect(page.getByTestId('assistant-decline')).toBeFocused();
      await shoot(page, set, '12-assistant-approval', 'Assistant: a destructive write waits: danger card, tool and arguments open, Decline focused as the default');
      await page.getByTestId('assistant-decline').click();
      await expect(page.getByTestId('assistant-send')).toBeVisible();
      await shoot(page, set, '12b-assistant-declined', 'Assistant: a declined tool is a neutral card, not an error');
      await ask(page, '/fail');
      await expect(page.getByTestId('assistant-error')).toBeVisible();
      await expect(page.getByTestId('assistant-stop')).toHaveCount(0);
      await shoot(page, set, '13-assistant-failed', 'Assistant: a failed turn as an error state: icon, what failed, likely cause, Retry and Show details');
      await page.getByTestId('assistant-new').click();
      await ask(page, '/tool-slow request handler');
      await expect(page.getByTestId('assistant-stop')).toBeVisible();
      await expect(page.getByTestId('assistant-tool-card').last()).toBeVisible();
      await shoot(page, set, '13b-assistant-tool-running', 'Assistant: a tool card while it runs; Stop in the place of Send');
      await page.getByTestId('assistant-stop').click();
      await expect(page.getByTestId('assistant-stop')).toHaveCount(0);
      await ask(page, '/tool-fail request handler');
      await expect(page.getByTestId('assistant-stop')).toHaveCount(0);
      await shoot(page, set, '13c-assistant-tool-failed', 'Assistant: a failed tool card with its reason (a declined one is neutral, shot 12b)');
      await page.getByTestId('assistant-list-toggle').click();
      await page.getByTestId('assistant-delete').first().focus();
      await page.keyboard.press('Shift');
      await shoot(page, set, '13d-assistant-list', 'Assistant: the conversation list, delete shown on focus in danger text');
      await page.keyboard.press('Enter');
      await expect(page.getByTestId('assistant-undo-button')).toBeFocused();
      await shoot(page, set, '13e-assistant-undo', 'Assistant: deleted, with Undo focused for 8 s');

      await page.getByTestId('assistant-input').blur();
      await wb.palette.open();
      await wb.palette.search('>index the repository');
      await wb.palette.run('Index the repository');
      await wb.palette.open();
      await wb.palette.search('>show jobs');
      await wb.palette.run('Show jobs');
      await expect(page.locator('pd-jobs-list')).toBeVisible();
      await shoot(page, set, '14-job-running', 'A running job: its phase in the Jobs pane and in the status bar');

      await open(page, set, { hash: '#/popout?panel=wd.code%23src%2Fmain.rs&host=main&label=p-main-1', width: 900, height: 560 });
      await expect(page.getByTestId('code-lines')).toBeVisible({ timeout: 15_000 });
      await shoot(page, set, '15-popout', 'A pop-out window: a slim title bar with Dock back (dock-in) and Keep on top (layers), the one view');

      wb = await open(page, set);
      await wb.explorer.node('wd.files', 'src').click();
      await page.keyboard.press('ArrowDown');
      await shoot(page, set, '16-focus-tree', 'Focus: a tree row', page.getByTestId('workbench-sidebar'));
      await wb.activity.item('wd.act.problems').focus();
      await shoot(page, set, '17-focus-activity', 'Focus: an activity item', page.locator('pd-activity-bar'));
      await page.locator('pd-dock [role=tab]').first().focus();
      await page.keyboard.press('Shift');
      await expect(page.locator('pd-dock [role=tab]:focus-visible')).toHaveCount(1);
      await shoot(page, set, '18-focus-tab', 'Focus: a dock tab (the role=tab element, by keyboard)', page.locator('pd-dock'));
      await wb.palette.open();
      await page.keyboard.press('ArrowDown');
      await shoot(page, set, '19-focus-palette', 'Focus: the palette input with the highlighted row', page.getByTestId('palette'));
      await page.keyboard.press('Escape');

      await open(page, set, { width: 700, height: 560 });
      await shoot(page, set, '20-narrow', 'A 700 px window: the layout plan closed the Explorer for the documents');

      wb = await open(page, set, { width: 320, height: 640 });
      await wb.activity.item('wd.act.explorer').click();
      await expect(wb.explorer.node('wd.files', 'src')).toBeVisible();
      await shoot(page, set, '21-320-explorer', '320 px with the Explorer opened by hand');
      await wb.activity.item('wd.act.explorer').click();
      await wb.activity.item('wd.act.problems').click();
      await expect(page.locator('wd-problems-view .where').first()).toBeVisible();
      await shoot(page, set, '22-320-problems', '320 px: Problems folds file:line under each problem; nothing scrolls sideways');
    });
  }

  test('compose', async ({ page }) => {
    // The demo app's icon, for the review (rule 22): copied beside the shots.
    const icon = join(OUT, 'workbench-demo-icon.png');
    writeFileSync(icon, readFileSync(join(__dirname, '..', '..', 'apps', 'workbench-demo', 'src-tauri', 'icons', '128x128@2x.png')));
    shots.push({ theme: 'icon', title: 'The workbench demo app icon (128 px @2x)', file: icon });
    const rows = [...SETS.map((s) => shots.filter((x) => x.theme === s.theme)), shots.filter((x) => x.theme === 'icon')];
    const cell = (s: { title: string; file: string }) => `<figure><figcaption>${s.title}</figcaption><img alt="${s.title}" src="${s.file.split('/').pop()}"></figure>`;
    const html = `<!doctype html><meta charset="utf-8"><style>
      body{margin:0;font:14px system-ui;background:#888;color:#111}
      .row{display:grid;grid-template-columns:1fr 1fr 1fr;gap:16px;padding:16px;align-items:start}
      figure{margin:0;background:#fff;padding:8px;border-radius:6px}
      figcaption{font-weight:600;margin-bottom:6px}
      img{width:100%;display:block}
      h1{margin:16px;font-size:18px;color:#fff}
    </style>
    ${SETS.map((s, i) => `<h1>${s.theme} · ${s.accent}</h1><div class="row">${(rows[i] ?? []).map(cell).join('')}</div>`).join('')}
    <h1>App icon</h1><div class="row">${(rows[SETS.length] ?? []).map(cell).join('')}</div>`;
    const sheet = join(OUT, 'contact-sheet.html');
    writeFileSync(sheet, html);
    await page.setViewportSize({ width: 1800, height: 1200 });
    await page.goto(`file://${sheet}`);
    await page.waitForLoadState('load');
    const file = join(OUT, 'contact-sheet.png');
    await page.screenshot({ path: file, fullPage: true, type: 'png' });
    writeFileSync(
      join(OUT, 'README.md'),
      'Contact sheet of the Paddock workbench (plan X50, ADR 0013), review 5: the workbench demo at @2x, dark with the Pacenotes accent and light with the Debrief accent, with every state review 1 asked for (see REVIEW.md to REVIEW-4.md and RESPONSE-1.md to RESPONSE-4.md), the demo app icon, and demo-icons-compare.png (the workbench demo beside the shell demo at 16, 32 and 1024 px). Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x50`. NOT self-approved: awaiting the independent reviewer.\n',
    );
    expect(readFileSync(file).length).toBeGreaterThan(10_000);
  });
});
