import { mkdirSync } from 'node:fs';
import type { Frame, FrameLocator, Page } from '@playwright/test';
import { join } from 'node:path';
import { FIXTURES } from './dialog-fixtures';
import { dialogContracts, dialogWindowUrl, expect, test, type DialogContractEntry } from '@decryp7/paddock-e2e';

/**
 * The dialogs of the documentation, as the app shows them: each is its own window (family rule), so the picture is the window's
 * page at its contract width and fitted height, not an overlay on a page (`SCREENSHOTS=1 npx playwright test --project=screenshots`).
 * Settings is the shell's window at the size Rust opens it.
 */
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts('src/app/generated/dialog-contracts.ts', ...['about', 'confirm', 'diagnostics'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`));
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', deviceScaleFactor: 2 });

/** The fitted-content dialogs: what shows that the content (not the skeleton) is there, then the size the window fits to. */
const FITTED: { name: string; id: string; title: string; loaded: string; payload?: unknown }[] = [
  // A text that still reads well after the redaction of the user name and the address.
  { name: 'web-search', id: 'dy.web-search', title: 'Search the web: Slack', loaded: 'input[name=query]', payload: { text: 'Slack crashed EXC_BAD_ACCESS macOS 27 alex', subject: 'Slack' } },
  // Report a problem: the diagnostics bundle's preview, with the list of every file and its size.
  { name: 'diagnostics-bundle', id: 'pd.report-problem', title: 'Report a problem', loaded: '[data-testid=diag-files]', payload: {} },
  { name: 'hash-search', id: 'dy.file-hash', title: 'Search the file hash on VirusTotal', loaded: 'code[aria-label="SHA-256 hash"]' },
];

const TITLE_H = 28;
/** Room around the window for its shadow. */
const MARGIN = 28;

/**
 * Shows a dialog window's page in a frame inside a macOS window (traffic lights, a title, rounded corners and the window shadow;
 * the OS draws the real ones, so the page alone has none): the picture is the window, at `width` x `height` under its title bar, on a
 * transparent background. Returns the frame to act in and to wait on.
 */
async function openWindow(page: Page, route: string, title: string, width: number, height: number): Promise<{ in: FrameLocator; frame: Frame }> {
  await page.setViewportSize({ width: width + 2 * MARGIN, height: height + TITLE_H + 2 * MARGIN });
  // The frame is on the app's own origin (a page of the dev server), so the dialog window's route loads as it does in its window.
  const html = `<!doctype html><meta name="color-scheme" content="light dark"><style>
    html, body { margin: 0; background: transparent; overflow: hidden; }
    .win { position: absolute; left: ${MARGIN}px; top: ${MARGIN}px; width: ${width}px; border-radius: 10px; overflow: hidden; box-shadow: 0 0 0 0.5px rgba(0,0,0,.35), 0 12px 32px rgba(0,0,0,.35), 0 2px 6px rgba(0,0,0,.25); }
    .bar { position: relative; box-sizing: border-box; height: ${TITLE_H}px; display: flex; align-items: center; justify-content: center; font: 13px -apple-system, 'Helvetica Neue', Arial, sans-serif; font-weight: 600; color: #3c3c43; background: #ececf0; border-bottom: 1px solid #cfcfd4; }
    .lights { position: absolute; left: 12px; top: 8px; display: flex; gap: 8px; }
    .lights i { width: 12px; height: 12px; border-radius: 50%; box-shadow: inset 0 0 0 0.5px rgba(0,0,0,.25); }
    iframe { display: block; border: 0; width: ${width}px; height: ${height}px; }
    @media (prefers-color-scheme: dark) { .bar { color: #e6e6ea; background: #2c2c31; border-bottom-color: #131315; } }
  </style><div class="win"><div class="bar"><span class="lights"><i style="background:#ff5f57"></i><i style="background:#febc2e"></i><i style="background:#28c840"></i></span>${title}</div><iframe title="${title}"></iframe></div>`;
  await page.route('**/__window.html', (r) => r.fulfill({ contentType: 'text/html', body: html }));
  await page.goto('/__window.html');
  await page.locator('iframe').evaluate((el, src) => ((el as HTMLIFrameElement).src = src), route);
  const handle = await page.locator('iframe').elementHandle();
  const frame = (await handle!.contentFrame())!;
  await frame.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
  return { in: page.frameLocator('iframe'), frame };
}

for (const scheme of ['dark', 'light'] as const) {
  test.describe(`documentation dialogs (${scheme})`, () => {
    test.use({ colorScheme: scheme });

    for (const d of FITTED) {
      test(`${d.name} (${scheme})`, async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        const contract = (CONTRACTS as Record<string, DialogContractEntry>)[d.id]!;
        await page.clock.install({ time: FROZEN });
        const route = dialogWindowUrl(d.id, d.payload ?? FIXTURES[d.id]![0] ?? {}, { title: d.title });
        const win = await openWindow(page, route, d.title, contract.width, 800);
        await expect(win.in.locator(d.loaded).first()).toBeVisible({ timeout: 15_000 });
        await page.clock.runFor(1_000);
        let last = -1;
        await expect(async () => {
          const h = await win.frame.evaluate(() => Number(document.documentElement.dataset['fittedHeight'] ?? '0'));
          const previous = last;
          last = h;
          expect(h).toBe(previous);
        }).toPass({ timeout: 10_000, intervals: [200, 200, 300] });
        const fitted = await openWindow(page, route, d.title, contract.width, last);
        await expect(fitted.in.locator(d.loaded).first()).toBeVisible({ timeout: 15_000 });
        await expect(fitted.in.locator('[data-testid=dialog-footer]')).toBeVisible();
        await page.clock.runFor(1_000);
        await page.screenshot({ path: join(OUT, `${d.name}-${scheme}.png`), omitBackground: true });
      });
    }

    // The Settings window, Dyno's page of it, as tall as ends between two cards (the page scrolls inside the window); the second picture has the menu bar items edited.
    for (const [name, height] of [['settings', 874], ['settings-menu-bar', 1480]] as const) {
      test(`${name} (${scheme})`, async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.clock.install({ time: FROZEN });
        const win = await openWindow(page, dialogWindowUrl('pd.settings', {}, { title: 'Settings' }), 'Settings', 880, height);
        await win.in.getByText('Dyno', { exact: true }).first().click();
        await expect(win.in.getByRole('region', { name: 'Menu bar' })).toBeVisible();
        // The sample history (the menu bar preview draws it) fills as it does on a page: a minute of fake time.
        await page.clock.runFor(45_000);
        if (name === 'settings-menu-bar') {
          const choose = async (label: string, option: string) => {
            await win.in.getByRole('combobox', { name: label }).click();
            await win.in.getByRole('option', { name: option, exact: true }).click();
          };
          const section = win.in.locator('dy-menu-bar-items');
          // Counts are relative to what the menu bar starts with, so a change of the default does not break the picture.
          const items = section.locator('ul.pd-wc-list').first().getByRole('listitem');
          const start = await items.count();
          await section.getByRole('button', { name: /^Add (another )?Line graph/ }).click();
          await expect(items).toHaveCount(start + 1);
          await section.getByRole('button', { name: /^Add (another )?Number/ }).click();
          await expect(items).toHaveCount(start + 2);
          const last = `position ${start + 2} of ${start + 2}`;
          await choose(`Metric of Number, ${last}`, 'Disk activity');
          await expect(section.getByRole('combobox', { name: new RegExp(`Metric of Number, ${last}`, 'i') })).toContainText('Disk activity');
          // A disk's activity is a percentage: the unit a person would type for it.
          const unit = section.getByRole('textbox', { name: new RegExp(`Unit of Number, ${last}`) });
          await unit.fill('%');
          await unit.press('Enter');
          // From the top of the section: its title, its description, the items and then the popup chooser with its live preview.
          await win.in.getByRole('region', { name: 'Menu bar' }).evaluate((el) => el.scrollIntoView({ block: 'start' }));
        }
        await win.frame.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
        await page.mouse.move(2, 2);
        await page.clock.runFor(2_000);
        await page.screenshot({ path: join(OUT, `${name}-${scheme}.png`), omitBackground: true });
      });
    }
  });
}
