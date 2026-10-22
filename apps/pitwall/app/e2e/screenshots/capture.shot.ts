import { expect, test, type Page } from '@playwright/test';

import { resolve } from 'node:path';

const OUT = resolve(__dirname, '../../../docs/screenshots');
// A fixed clock so countdowns, "5 min ago" and chart axes are the same on every run.
const NOW = new Date('2026-10-22T14:30:00').getTime();

const SCREENS: { name: string; path: string; ready: string; prepare?: (page: Page) => Promise<void> }[] = [
  { name: 'dashboard', path: '/', ready: 'pw-echart' },
  { name: 'models', path: '/models', ready: 'h1' },
  { name: 'engines', path: '/engines', ready: 'h1' },
  { name: 'apps', path: '/apps', ready: 'h1' },
  { name: 'requests', path: '/requests', ready: 'h1' },
  { name: 'playground', path: '/playground', ready: 'h1' },
  { name: 'benchmarks', path: '/benchmarks', ready: 'h1' },
  { name: 'plugins', path: '/plugins', ready: 'h1' },
  { name: 'settings', path: '/settings', ready: 'h1' },
];

const ONLY = process.env['SHOT_ONLY']?.split(',');

async function open(page: Page, path: string, theme: 'dark' | 'light') {
  await page.clock.install({ time: NOW });
  await page.addInitScript((t) => localStorage.setItem('pitwall.theme', t), theme);
  await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
  await page.goto(`/?mock-static&mock-fast#${path}`);
}

for (const theme of ['dark', 'light'] as const) {
  for (const s of SCREENS.filter((x) => !ONLY || ONLY.includes(x.name))) {
    test(`${s.name} ${theme}`, async ({ page }) => {
      await open(page, s.path, theme);
      await expect(page.locator(s.ready).first()).toBeVisible();
      await s.prepare?.(page);
      await page.clock.runFor(1500);
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${OUT}/${s.name}-${theme}.png` });
    });
  }

  if (!ONLY || ONLY.includes('tray')) {
    test(`tray ${theme}`, async ({ page }) => {
      await page.setViewportSize({ width: 360, height: 520 });
      await open(page, '/tray', theme);
      await expect(page.locator('h1, .tray').first()).toBeVisible();
      await page.clock.runFor(1500);
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${OUT}/tray-${theme}.png` });
    });
  }
}
