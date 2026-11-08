import { mkdirSync } from 'node:fs';
import { dialogWindowUrl, expect, test } from '@decryp7/paddock-e2e';

// Every Dyno page and shell surface, light and dark, for the independent review (rule 22). The dialogs are in
// screenshots-dialogs.spec.ts (contract widths, both button orders). Only on request:
// `SCREENSHOTS=1 SHELL_SHOTS_DIR=<dir> PW_PORT=<port> npx playwright test --project=screenshots e2e/screenshots-shell.spec.ts`.
const OUT = process.env['SHELL_SHOTS_DIR'] ?? '';
const PAGES = ['dashboard', 'processes', 'why-slow', 'throttling', 'issues', 'inventory', 'network', 'counters', 'benchmarks', 'alarms', 'reports', 'recorder', 'health', 'extras', 'lifespan', 'diagnostics', 'cpu'];
test.describe('Dyno shell screenshots', () => {
  test.skip(!OUT, 'set SHELL_SHOTS_DIR');
  for (const scheme of ['light', 'dark'] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });
      for (const p of PAGES) {
        test(`page ${p}`, async ({ page }) => {
          mkdirSync(OUT, { recursive: true });
          await page.goto(`/?latency=0#/${p}`);
          await page.getByRole('heading', { level: 1 }).first().waitFor({ timeout: 30_000 });
          await page.waitForTimeout(1500);
          await page.screenshot({ path: `${OUT}/page-${p}-${scheme}.png` });
        });
      }
      test('tray popover', async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        await page.setViewportSize({ width: 380, height: 700 });
        await page.goto('/?latency=0#/tray');
        await page.getByRole('button', { name: /Open Dyno/ }).waitFor();
        await page.waitForTimeout(1200);
        // The popover window is as tall as its content (the shell fits it): the picture is that content, not a tall empty page.
        const box = await page.locator('pd-tray-popover').boundingBox();
        await page.screenshot({ path: `${OUT}/tray-popover-${scheme}.png`, clip: { x: 0, y: 0, width: 380, height: Math.ceil((box?.y ?? 0) + (box?.height ?? 560)) } });
      });
      // The Settings window as Rust opens it (its own window at the contract size), each page whole.
      for (const [name, page] of [['appearance', 'Appearance'], ['dyno', 'Dyno']] as const) {
        test(`settings window, ${name} page`, async ({ page: p }) => {
          mkdirSync(OUT, { recursive: true });
          await p.setViewportSize({ width: 880, height: 700 });
          await p.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
          await p.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          await p.getByText(page, { exact: true }).first().click();
          await p.waitForTimeout(1200);
          await p.screenshot({ path: `${OUT}/settings-${name}-${scheme}.png` });
        });
      }
      test('notification centre and jobs', async ({ page }) => {
        mkdirSync(OUT, { recursive: true });
        // A real job, held at its first step by the mock: the Jobs list shows its phase and Cancel (the documentation picture is
        // screenshots.spec.ts's `jobs`, with the window buttons).
        await page.goto('/?latency=0&chrome=macos&menu=native&jobsteps=40&jobstepms=600000#/extras?tab=files');
        await page.getByRole('heading', { level: 1 }).first().waitFor({ timeout: 30_000 });
        await page.getByRole('button', { name: 'Choose a folder' }).click();
        await expect(page.locator('pd-jobs-status button.pd-pill')).toContainText('1 running');
        await page.getByRole('link', { name: 'Power', exact: true }).click();
        await expect(page.getByRole('heading', { name: 'Power', level: 1 })).toBeVisible();
        const bell = page.getByRole('button', { name: /notifications/i }).first();
        await bell.click();
        await expect(bell).toHaveAttribute('aria-expanded', 'true');
        await page.screenshot({ path: `${OUT}/notification-centre-${scheme}.png` });
        await page.keyboard.press('Escape');
        await expect(bell).toHaveAttribute('aria-expanded', 'false');
        await page.locator('pd-jobs-status button.pd-pill').click();
        const popover = page.getByRole('region', { name: 'Jobs' });
        await expect(popover.getByRole('button', { name: 'Cancel' })).toBeVisible();
        await page.screenshot({ path: `${OUT}/jobs-${scheme}.png` });
      });
    });
  }
});
