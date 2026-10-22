import { type Page, test } from '@playwright/test';
import { join } from 'node:path';

/**
 * Documentation screenshots of every screen in both themes, against the mock backend:
 * `SCREENSHOTS=1 npx playwright test --project=screenshots` → ../docs/screenshots/<screen>-<theme>.png (1280×800).
 */
const OUT = join(__dirname, '..', '..', 'docs', 'screenshots');
/** A fixed clock so elapsed times and "x min ago" read the same on every run. */
const NOW = new Date('2026-10-22T14:40:00');

interface Shot {
  name: string;
  path: string;
  query?: string;
  /** Prepares the view after load (open a dialog, scroll). */
  prepare?: (page: Page) => Promise<void>;
}

const scrollMain = (y: number) => async (page: Page) => {
  await page.evaluate((top) => document.querySelector('main')?.scrollTo({ top }), y);
};

const SHOTS: Shot[] = [
  { name: 'dashboard', path: '/' },
  { name: 'dashboard-history', path: '/', prepare: scrollMain(640) },
  {
    name: 'dashboard-load',
    path: '/',
    prepare: async (page) => {
      await page.getByRole('heading', { name: 'CPU', exact: true }).scrollIntoViewIfNeeded();
      await page.evaluate(() => document.querySelector('main')?.scrollBy({ top: 120 }));
    },
  },
  { name: 'runners', path: '/runners' },
  { name: 'runner-detail', path: '/runners/build-win' },
  { name: 'runner-updates', path: '/runners/ui-tests?tab=updates' },
  { name: 'runner-disk', path: '/runners/docker-lab?tab=disk' },
  { name: 'activity', path: '/activity' },
  { name: 'logs', path: '/logs?runner=build-win' },
  { name: 'updates', path: '/updates' },
  { name: 'settings', path: '/settings' },
  { name: 'settings-defaults', path: '/settings?section=defaults' },
  { name: 'settings-tokens', path: '/settings?section=tokens' },
  {
    name: 'register-wizard',
    path: '/runners',
    prepare: async (page) => {
      await page.getByRole('button', { name: 'Register runner' }).first().click();
      const dialog = page.getByRole('dialog');
      await dialog.getByLabel('Server URL').fill('https://git.example.com');
      await dialog.getByLabel('Registration token').fill('demo-registration-token');
      await dialog.getByRole('button', { name: 'Continue' }).click();
      await dialog.getByLabel('Name in Pitcrew').fill('nightly');
      await dialog.getByRole('button', { name: 'Continue' }).click();
      await dialog.getByText('Run as the service account').click();
    },
  },
  { name: 'service-unavailable', path: '/', query: 'mock=notInstalled' },
  { name: 'empty', path: '/', query: 'mock=empty' },
];

for (const theme of ['dark', 'light'] as const) {
  test.describe(`${theme} theme`, () => {
    test.use({ colorScheme: theme });

    for (const shot of SHOTS) {
      test(shot.name, async ({ page }) => {
        await page.clock.setFixedTime(NOW);
        await page.addInitScript((t) => localStorage.setItem('pitcrew.theme', t), theme);
        await page.emulateMedia({ reducedMotion: 'reduce' });
        await page.goto(`/?simulate=0${shot.query ? '&' + shot.query : ''}#${shot.path}`);
        await page.waitForLoadState('networkidle');
        await page.waitForTimeout(1200);
        await shot.prepare?.(page);
        await page.waitForTimeout(700);
        await page.screenshot({ path: join(OUT, `${shot.name}-${theme}.png`) });
      });
    }
  });
}
