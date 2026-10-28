import { mkdirSync } from 'node:fs';
import { dialogWindowUrl, test } from '@decryp7/paddock-e2e';
import { FIXTURES } from './dialog-fixtures';

// Every Dyno page and shell surface, light and dark, for the independent review (rule 22). Only on request:
// `SCREENSHOTS=1 SHELL_SHOTS_DIR=<dir> PW_PORT=<port> npx playwright test --project=screenshots e2e/screenshots-shell.spec.ts`.
const OUT = process.env['SHELL_SHOTS_DIR'] ?? '';
const PAGES = ['dashboard', 'processes', 'why-slow', 'throttling', 'issues', 'inventory', 'network', 'counters', 'benchmarks', 'alarms', 'reports', 'recorder', 'health', 'backup', 'extras', 'lifespan', 'diagnostics', 'cpu'];
const DIALOGS: [string, string][] = [
  ['pd.settings', 'settings-window'],
  ['pd.about', 'about'],
  ['pd.updates', 'updates'],
  ['pd.whats-new', 'whats-new'],
  ['pd.confirm', 'confirm'],
  ['dy.alert-rule', 'dialog-alert-rule'],
  ['dy.lockers', 'dialog-lockers'],
  ['dy.web-search', 'dialog-web-search'],
  ['dy.file-hash', 'dialog-file-hash'],
];

test.describe('Dyno shell screenshots', () => {
  test.skip(!OUT, 'set SHELL_SHOTS_DIR');
  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });
      for (const p of PAGES) {
        test(`page ${p}`, async ({ page }) => {
          mkdirSync(OUT, { recursive: true });
          await page.goto(`/?latency=0#/${p}`);
          await page.waitForTimeout(1800);
          await page.screenshot({ path: `${OUT}/page-${p}-${scheme}.png` });
        });
      }
      test('tray popover', async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.setViewportSize({ width: 380, height: 560 });
        await page.goto('/?latency=0#/tray');
        await page.waitForTimeout(1800);
        await page.screenshot({ path: `${OUT}/tray-popover-${scheme}.png` });
      });
      test('settings window, Dyno page', async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.goto('/?latency=0#/dashboard');
        await page.waitForTimeout(1200);
        await page.keyboard.press('Meta+,');
        await page.getByRole('dialog').getByText('Dyno', { exact: true }).click();
        await page.waitForTimeout(1200);
        await page.screenshot({ path: `${OUT}/settings-dyno-page-${scheme}.png` });
      });
      test('notification centre and jobs', async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.goto('/?latency=0#/dashboard');
        await page.waitForTimeout(1500);
        await page.getByRole('button', { name: /notifications/i }).first().click();
        await page.waitForTimeout(600);
        await page.screenshot({ path: `${OUT}/notification-centre-${scheme}.png` });
        await page.keyboard.press('Escape');
        await page.getByRole('button', { name: /jobs/i }).first().click();
        await page.waitForTimeout(600);
        await page.screenshot({ path: `${OUT}/jobs-${scheme}.png` });
      });
      for (const [id, name] of DIALOGS) {
        test(`dialog ${id}`, async ({ page }) => {
          mkdirSync(OUT, { recursive: true });
          await page.setViewportSize({ width: id === 'pd.settings' ? 900 : 760, height: id === 'pd.settings' ? 680 : 600 });
          await page.goto(dialogWindowUrl(id, FIXTURES[id]?.[0] ?? {}, { title: id }));
          await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          await page.waitForTimeout(1200);
          await page.screenshot({ path: `${OUT}/${name}-${scheme}.png` });
        });
      }
    });
  }
});
