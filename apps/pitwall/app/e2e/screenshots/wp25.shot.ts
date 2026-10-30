import { expect, test, type Page } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { dialogContracts, dialogWindowUrl, type DialogContractEntry } from '../../../../../ui/projects/paddock-e2e/src/index.ts';
import { FIXTURES } from '../dialog-fixtures';

// Pitwall on the shared shell (1.1 WP-2.5/2.6), for the independent review (rule 22): every page of the main window, the
// tray popover, the Settings window, and every dialog window at its fitted size in the macOS and the Windows button order,
// light and dark, 1440x900 @2x for the windows. Only on request:
// `PW_SHOT_PORT=<port> WP25_SHOTS=1 npx playwright test -c e2e/screenshots/playwright.config.ts e2e/screenshots/wp25.shot.ts`.
// The sheet is index.html next to the pictures (.angular/ is ignored by git).
const OUT = resolve(__dirname, '../../.angular/wp25-shots');
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts(...['src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`)].map((p) => resolve(__dirname, '../..', p)));
const PAGES = ['dashboard', 'models', 'engines', 'apps', 'requests', 'playground', 'benchmarks'];
const PLATFORMS = {
  mac: { platform: 'MacIntel', ua: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/130 Safari/537.36' },
  win: { platform: 'Win32', ua: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/130 Safari/537.36' },
} as const;
const Q = '?mock-fast&mock-static&mock-updates';

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
      `<!doctype html><meta charset="utf-8"><title>Pitwall on the shell</title><style>body{font:13px system-ui;margin:24px;background:#f4f4f6;color:#1d1d1f}h1{font-size:18px}h2{font-size:15px;margin-top:32px}.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(420px,1fr));gap:16px}figure{margin:0;background:#fff;border:1px solid #ddd;border-radius:8px;padding:8px}img{width:100%;height:auto;display:block}figcaption{margin-top:6px;color:#555}</style><h1>Pitwall 1.1 WP-2.5/2.6: the UI on the shared shell, the dialogs as windows</h1><p>${shots.length} pictures, browser build with the mock backend, deviceScaleFactor 2.</p>${body}`,
    );
  });

  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });
      for (const p of PAGES) {
        test(`page ${p}`, async ({ page }) => {
          await page.goto(`/${Q}#/${p}`);
          await expect(page.locator('h1').first()).toBeVisible({ timeout: 30_000 });
          await page.waitForLoadState('networkidle');
          await page.waitForTimeout(1200);
          await shot(page, `page-${p}-${scheme}.png`, `Pages (${scheme})`, `${p}, ${scheme}`);
        });
      }
      test('settings in the main window (in page, browser build)', async ({ page }) => {
        await page.goto(`/${Q}#/dashboard`);
        await expect(page.locator('h1').first()).toBeVisible({ timeout: 30_000 });
        await page.keyboard.press('ControlOrMeta+Comma');
        await expect(page.locator('#set-general')).toBeVisible();
        await page.waitForTimeout(800);
        await shot(page, `main-settings-${scheme}.png`, `Pages (${scheme})`, `Settings opened with Mod+, over the main window, ${scheme}`);
      });
      test('tray popover', async ({ page }) => {
        await page.setViewportSize({ width: 360, height: 640 });
        await page.goto(`/${Q}#/tray`);
        await expect(page.getByRole('heading', { name: 'Pitwall' })).toBeVisible();
        await page.waitForTimeout(800);
        const box = await page.locator('pw-tray-popover').boundingBox();
        await shot(page, `tray-${scheme}.png`, `Tray popover`, `tray popover, ${scheme}`, { x: 0, y: 0, width: 360, height: Math.min(640, Math.ceil((box?.y ?? 0) + (box?.height ?? 600))) });
      });
      for (const [name, nav] of [['pitwall', 'Pitwall'], ['pitwall-service', 'Pitwall'], ['appearance', 'Appearance']] as const) {
        test(`settings window, ${name}`, async ({ page }) => {
          await page.setViewportSize({ width: 880, height: 700 });
          await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: Q }));
          await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          await page.getByText(nav, { exact: true }).first().click();
          if (name !== 'appearance') await expect(page.locator('#set-general')).toBeVisible();
          if (name === 'pitwall-service') await page.locator('#set-service').scrollIntoViewIfNeeded();
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
              await page.goto(dialogWindowUrl(id, FIXTURES[id]?.[0] ?? {}, { title: id, query: Q }));
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
