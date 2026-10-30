import { expect, test, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { dialogContracts, dialogWindowUrl, type DialogContractEntry } from '../../../../../ui/projects/paddock-e2e/src/index.ts';
import { FIXTURES } from '../dialog-fixtures';

// Pitwall's windows, for the independent review (rule 22): the tray popover window, every section of the Settings window
// (and a refused save), and every dialog window (Pitwall's, About, Updates, What's New, the confirmation) at its fitted
// size in the macOS and the Windows button order, light and dark, @2x. Only on request:
// `PW_SHOT_PORT=<port> WP25_SHOTS=1 npx playwright test -c e2e/screenshots/playwright.config.ts e2e/screenshots/wp25.shot.ts`
// (`WP25_OUT` names another folder). The sheet is index.html next to the pictures (.angular/ is ignored by git).
const OUT = process.env['WP25_OUT'] ? resolve(process.env['WP25_OUT']) : resolve(__dirname, '../../.angular/wp25-shots');
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts(...['src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`)].map((p) => resolve(__dirname, '../..', p)));
const SECTIONS = ['general', 'server', 'memory', 'cache', 'network', 'model-updates', 'privacy', 'service'];
const PLATFORMS = {
  mac: { platform: 'MacIntel', ua: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/130 Safari/537.36' },
  win: { platform: 'Win32', ua: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/130 Safari/537.36' },
} as const;
const Q = '?mock-fast&mock-static&mock-updates';
/** The window titles Rust gives the dialogs (as their openers pass them). */
const TITLES: Record<string, string> = {
  'pw.quit': 'Quit Pitwall',
  'pw.create-app': 'New app',
  'pw.key-secret': 'New key',
  'pw.service-consent': 'Run in the background',
  'pd.about': 'About Pitwall',
  'pd.updates': 'Updates',
  'pd.whats-new': "What's new",
  'pd.confirm': 'Pitwall',
};

const shots: { file: string; group: string; label: string }[] = [];
const shot = async (page: Page, file: string, group: string, label: string, clip?: { x: number; y: number; width: number; height: number }) => {
  mkdirSync(OUT, { recursive: true });
  await page.screenshot({ path: `${OUT}/${file}`, ...(clip ? { clip } : {}) });
  shots.push({ file, group, label });
};

test.describe('WP-2.5 screenshots', () => {
  test.skip(!process.env['WP25_SHOTS'], 'set WP25_SHOTS=1');
  test.setTimeout(120_000);

  test.afterAll(() => {
    const groups = [...new Set(shots.map((s) => s.group))];
    const body = groups
      .map((g) => `<h2>${g}</h2><div class="grid">${shots.filter((s) => s.group === g).map((s) => `<figure><a href="${s.file}"><img src="${s.file}" alt="${s.label}"></a><figcaption>${s.label}</figcaption></figure>`).join('')}</div>`)
      .join('\n');
    writeFileSync(
      `${OUT}/index.html`,
      `<!doctype html><meta charset="utf-8"><title>Pitwall windows</title><style>body{font:13px system-ui;margin:24px;background:#f4f4f6;color:#1d1d1f}h1{font-size:18px}h2{font-size:15px;margin-top:32px}.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(420px,1fr));gap:16px}figure{margin:0;background:#fff;border:1px solid #ddd;border-radius:8px;padding:8px}img{width:100%;height:auto;display:block}figcaption{margin-top:6px;color:#555}</style><h1>Pitwall 1.1 WP-2: the tray, Settings and the dialog windows</h1><p>${shots.length} pictures, browser build with the mock backend, deviceScaleFactor 2.</p>${body}`,
    );
  });

  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });
      test('tray popover window', async ({ page }) => {
        // The popover window as the tray opens it (360x520), with a pairing and model updates waiting.
        await page.setViewportSize({ width: 360, height: 520 });
        await page.goto(`/${Q}#/tray`);
        await expect(page.getByRole('heading', { name: 'Pitwall' })).toBeVisible();
        await expect(page.locator('pw-hot-models .more')).toBeVisible();
        await page.waitForTimeout(800);
        await shot(page, `tray-${scheme}.png`, 'Tray popover window (360x520)', `tray popover, ${scheme}`);
      });
      for (const name of [...SECTIONS, 'appearance', 'save-refused']) {
        test(`settings window, ${name}`, async ({ page }) => {
          await page.setViewportSize({ width: 880, height: 700 });
          const query = name === 'save-refused' ? `${Q}&mock-fail=saveSettings` : Q;
          await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query }));
          await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          if (name === 'appearance') {
            await page.getByText('Appearance', { exact: true }).first().click();
          } else {
            await expect(page.locator('#set-general')).toBeVisible();
          }
          if (name === 'save-refused') {
            await page.getByRole('switch', { name: 'Offline mode' }).click();
            await expect(page.getByTestId('save-error')).toBeVisible();
            // The section starts under the sticky error, not behind it.
            await page.locator('#set-network').evaluate((e) => {
              e.scrollIntoView({ block: 'start' });
              const bar = document.querySelector('[data-testid="save-error"]')!.getBoundingClientRect();
              let scroller: HTMLElement | null = e.parentElement;
              while (scroller && scroller.scrollHeight <= scroller.clientHeight) scroller = scroller.parentElement;
              scroller?.scrollBy(0, -(bar.height + 16));
            });
          } else if (name !== 'appearance') {
            await page.locator(`#set-${name}`).evaluate((e) => e.scrollIntoView({ block: 'start' }));
          }
          await page.mouse.move(2, 2);
          await page.waitForTimeout(900);
          await shot(page, `settings-${name}-${scheme}.png`, 'Settings window (880x700)', `Settings, ${name}, ${scheme}`);
        });
      }
      for (const [id, contract] of Object.entries(CONTRACTS) as [string, DialogContractEntry][]) {
        if (id === 'pd.settings') continue;
        for (const [os, p] of Object.entries(PLATFORMS)) {
          if (os === 'win' && (id === 'pd.about' || id === 'pd.whats-new')) continue;
          test(`dialog ${id} ${os}`, async ({ browser, baseURL }) => {
            const capH = Math.floor(1080 * 0.75);
            const context = await browser.newContext({ baseURL, colorScheme: scheme, deviceScaleFactor: 2, userAgent: p.ua, viewport: { width: contract.width, height: capH } });
            await context.addInitScript((platform) => Object.defineProperty(navigator, 'platform', { get: () => platform }), p.platform);
            try {
              const page = await context.newPage();
              // The service question is worded for the OS it runs on (launch agent, Windows service, systemd user unit).
              const input = id === 'pw.service-consent' ? { platform: os === 'win' ? 'windows' : 'macos' } : (FIXTURES[id]?.[0] ?? {});
              await page.goto(dialogWindowUrl(id, input, { title: TITLES[id] ?? id, query: Q }));
              await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
              await page.waitForTimeout(900);
              const asked = await page.evaluate(() => ({ h: Number(document.documentElement.dataset['fittedHeight'] ?? '0'), min: document.documentElement.dataset['minContent'] ?? '0x0' }));
              const minH = Number(asked.min.split('x')[1] ?? '0');
              const height = contract.kind === 'tool' ? Math.min(contract.height ?? capH, capH) : Math.max(Math.min(asked.h, capH), minH);
              await page.setViewportSize({ width: contract.width, height });
              await expect(page.locator('[data-testid=dialog-footer]')).toBeVisible();
              await page.waitForTimeout(300);
              await shot(page, `dialog-${id}-${os}-${scheme}.png`, `Dialog windows (${scheme})`, `${id}, ${os === 'mac' ? 'macOS' : 'Windows'} order, ${contract.width}x${height}`);
            } finally {
              await context.close();
            }
          });
        }
      }
    });
  }
});
