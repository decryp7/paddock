import { expect, test, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Documentation screenshots of every screen in both themes (`SCREENSHOTS=1 npx playwright test --project=screenshots`).
 * Deterministic: the clock is frozen, the mock is a macOS laptop, the timezone and locale are fixed and motion is reduced.
 */
// Playwright runs from the app folder; the images live in the repository's docs.
const OUT = join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', colorScheme: 'light' });

async function prepare(page: Page, theme: 'dark' | 'light', mock = 'macos'): Promise<void> {
  // A fake clock starting at a fixed time: the mock's live values are a function of the clock, so every run is identical.
  await page.clock.install({ time: FROZEN });
  await page.emulateMedia({ colorScheme: theme });
  void mock;
}

const SCREENS: { name: string; path: string; mock?: string; ready: (page: Page) => Promise<void>; size?: { width: number; height: number } }[] = [
  {
    name: 'dashboard',
    path: '/#/',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Check now' }).click();
      await expect(page.getByText('A Rust build is using most of your CPU')).toBeVisible();
      await expect(page.locator('dy-chart canvas').first()).toBeVisible();
      await page.clock.runFor(1_000);
    },
  },
  {
    name: 'processes',
    path: '/#/processes',
    ready: async (page) => {
      await expect(page.getByRole('treegrid')).toBeVisible();
      await page.getByRole('button', { name: 'Expand Rust build (cargo)' }).click();
      await page.locator('[role=row][data-key^="p"]').first().click();
      await expect(page.getByRole('complementary', { name: 'Process details' }).getByText('Command line')).toBeVisible();
      await page.clock.runFor(1_000);
    },
  },
  {
    name: 'cpu-detail',
    path: '/#/cpu',
    ready: async (page) => {
      await expect(page.getByRole('region', { name: 'Per-core history' }).locator('canvas').first()).toBeVisible();
      await page.clock.runFor(1_000);
    },
  },
  {
    name: 'inventory',
    path: '/#/inventory',
    mock: 'windows',
    ready: async (page) => void (await expect(page.getByRole('img', { name: /memory slots filled/ })).toBeVisible()),
  },
  {
    name: 'inventory-apple',
    path: '/#/inventory',
    ready: async (page) => void (await expect(page.getByRole('img', { name: /Unified memory/ })).toBeVisible()),
  },
  { name: 'why-slow', path: '/#/why-slow', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Why slow' })).toBeVisible()) },
  { name: 'rules', path: '/#/rules', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'CPU taming' })).toBeVisible()) },
  { name: 'cleanup', path: '/#/cleanup', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Cleanup' })).toBeVisible()) },
  { name: 'health', path: '/#/health', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Health & tuning' })).toBeVisible()) },
  { name: 'reports', path: '/#/reports', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Reports & alerts' })).toBeVisible()) },
  { name: 'settings', path: '/#/settings', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible()) },
  {
    name: 'tray',
    path: '/#/tray',
    size: { width: 440, height: 660 },
    ready: async (page) => void (await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible()),
  },
];

mkdirSync(OUT, { recursive: true });

for (const theme of ['dark', 'light'] as const) {
  for (const screen of SCREENS) {
    test(`${screen.name} (${theme})`, async ({ page }) => {
      if (screen.size) await page.setViewportSize(screen.size);
      await prepare(page, theme);
      await page.goto(`${screen.path.replace('/#/', `/?latency=0${screen.mock ? `&mock=${screen.mock}` : ''}#/`)}`);
      await expect(page.locator('dy-shell, dy-tray').first()).toBeVisible();
      await page.clock.runFor(45_000);
      await screen.ready(page);
      if (screen.name === 'tray') {
        // A desktop-like backdrop (blurred wallpaper gradient and a menu bar) so the rounded, translucent popover is visible.
        const wall = theme === 'dark' ? 'linear-gradient(135deg, #1d2b64 0%, #7b3f98 55%, #d46a6a 100%)' : 'linear-gradient(135deg, #8ec5fc 0%, #e0c3fc 55%, #fbc2a6 100%)';
        await page.addStyleTag({
          content: `html[data-surface='popover'] { background: ${wall} !important; }
            html[data-surface='popover'] body { padding: 34px 40px 0 !important; }
            body::before { content: ''; position: fixed; inset: 0 0 auto 0; height: 24px; background: rgb(${theme === 'dark' ? '20 20 24 / 55%' : '255 255 255 / 55%'}); backdrop-filter: blur(20px); z-index: 0; }`,
        });
        await page.clock.runFor(500);
      }
      await page.screenshot({ path: join(OUT, `${screen.name}-${theme}.png`) });
      if (screen.name === 'dashboard') {
        await page.getByRole('group', { name: 'CPU', exact: true }).getByRole('button', { name: 'Per core' }).click();
        await page.clock.runFor(1_000);
        await page.getByRole('group', { name: 'CPU', exact: true }).screenshot({ path: join(OUT, `cpu-per-core-tile-${theme}.png`) });
        await page.getByRole('group', { name: 'CPU', exact: true }).getByRole('button', { name: 'Overall' }).click();
      }
      if (screen.name === 'dashboard') {
        // The whole page, for the README and for review (the 1440x900 image only shows the top).
        await page.setViewportSize({ width: 1440, height: 2300 });
        await page.waitForTimeout(500);
        await page.screenshot({ path: join(OUT, `dashboard-full-${theme}.png`) });
      }
    });
  }
}
