import { mkdirSync } from 'node:fs';
import { expect, test } from '@playwright/test';

// The Power page in every designed state, light and dark, for the independent review (rule 22). Only on request:
// `SCREENSHOTS=1 POWER_SHOTS_DIR=<dir> npx playwright test --project=screenshots e2e/screenshots-power.spec.ts`.
const OUT = process.env['POWER_SHOTS_DIR'] ?? '';

const STATES: { name: string; query: string; act?: (page: import('@playwright/test').Page) => Promise<void>; tab?: 'Energy' | 'Sleep and shut down' }[] = [
  { name: 'keep-awake-off', query: '?mock=macos' },
  {
    name: 'keep-awake-holding',
    query: '?mock=macos',
    act: async (page) => {
      await page.getByRole('button', { name: '1 hour' }).click();
      await expect(page.getByRole('button', { name: 'Stop keeping awake' })).toBeVisible();
    },
  },
  { name: 'keep-awake-waiting', query: '?mock=macos&power=waiting' },
  { name: 'keep-awake-busy', query: '?mock=macos&power=busy' },
  { name: 'keep-awake-quiet', query: '?mock=macos&power=quiet' },
  { name: 'keep-awake-ended', query: '?mock=macos&power=ended' },
  { name: 'keep-awake-unavailable', query: '?mock=linux&power=unavailable' },
  { name: 'sleep-off', query: '?mock=macos', tab: 'Sleep and shut down' },
  { name: 'sleep-watching', query: '?mock=macos&idle=watching', tab: 'Sleep and shut down' },
  { name: 'sleep-blocked', query: '?mock=macos&idle=blocked', tab: 'Sleep and shut down' },
  { name: 'sleep-countdown', query: '?mock=macos&idle=countdown', tab: 'Sleep and shut down' },
  { name: 'sleep-acting', query: '?mock=macos&idle=acting', tab: 'Sleep and shut down' },
  { name: 'sleep-stopped', query: '?mock=macos&idle=stopped', tab: 'Sleep and shut down' },
  { name: 'energy-not-set', query: '?mock=macos', tab: 'Energy' },
  { name: 'energy-priced-7-days', query: '?mock=macos&energy=priced', tab: 'Energy' },
  {
    name: 'energy-priced-30-days',
    query: '?mock=macos&energy=priced',
    tab: 'Energy',
    act: async (page) => {
      await page.locator('mat-button-toggle button', { hasText: '30 days' }).click();
      await expect(page.locator('tbody tr')).toHaveCount(30);
    },
  },
];

test.describe('Power page screenshots', () => {
  test.skip(!OUT, 'set POWER_SHOTS_DIR');
  for (const scheme of ['light', 'dark'] as const) {
    for (const s of STATES) {
      test(`${s.name} ${scheme}`, async ({ browser, baseURL }) => {
        mkdirSync(OUT, { recursive: true });
        const context = await browser.newContext({ baseURL, colorScheme: scheme, deviceScaleFactor: 2, viewport: { width: 1100, height: 760 } });
        try {
          const page = await context.newPage();
          await page.goto(`/${s.query}#/power`);
          await expect(page.getByRole('heading', { name: 'Power', level: 1 })).toBeVisible();
          if (s.tab) await page.getByRole('tab', { name: s.tab }).click();
          if (s.act) await s.act(page);
          await expect(page.locator('dy-async [aria-busy="true"]')).toHaveCount(0);
          await page.screenshot({ path: `${OUT}/${s.name}-${scheme}.png` });
        } finally {
          await context.close();
        }
      });
    }
  }
});
