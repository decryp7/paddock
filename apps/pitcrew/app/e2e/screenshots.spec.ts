import { captureScreen, expect, test } from '@decryp7/paddock-e2e';
import type { Page } from '@playwright/test';
import { join } from 'node:path';

/**
 * Documentation screenshots of every screen in both themes, against the mock backend:
 * `SCREENSHOTS=1 npx playwright test --project=screenshots` → ../docs/screenshots/<theme>/<screen>.png (1440×900 at 2x), or
 * into `SHOTS_OUT` when it is set. Every image is taken with `captureScreen`: it waits for loaded content (no skeleton or
 * `aria-busy`, fonts, images, no animation, a still layout), scrolls only `main`, takes a long page whole (`fullPage: 'main'`) and
 * refuses a cut-off element or an overlay painted over the page. Nothing is cropped or covered.
 */
const OUT = process.env['SHOTS_OUT'] ?? join(__dirname, '..', '..', 'docs', 'screenshots');
/** A fixed clock so elapsed times and "x min ago" read the same on every run. */
const NOW = new Date('2026-10-22T14:40:00');

interface Shot {
  name: string;
  path: string;
  query?: string;
  /** Prepares the view after load (open a card, fill a form). */
  prepare?: (page: Page) => Promise<void>;
}

/** The main screens: each is taken whole (`fullPage: 'main'`), so nothing is cut and nothing is scrolled to. */
const SHOTS: Shot[] = [
  { name: 'dashboard', path: '/' },
  { name: 'runners', path: '/runners' },
  { name: 'runner-detail', path: '/runners/build-win' },
  { name: 'runner-updates', path: '/runners/ui-tests?tab=updates' },
  { name: 'runner-disk', path: '/runners/docker-lab?tab=disk' },
  { name: 'runner-config-form', path: '/runners/docker-lab?tab=config' },
  {
    name: 'runner-config-advanced',
    path: '/runners/ui-tests?tab=config',
    prepare: async (page) => {
      await page.getByRole('radio', { name: 'Advanced (YAML)' }).click();
      const editor = page.getByRole('textbox', { name: 'config.yaml' });
      const text = await editor.inputValue();
      await editor.fill(text.replace('  capacity: 1\n', '  capacity: 1\n\t  retries: 3\n'));
      await page.getByTestId('yaml-status').waitFor();
      await page.getByRole('button', { name: /Error on line/ }).hover();
    },
  },
  { name: 'runner-processes', path: '/runners/build-win?tab=processes' },
  { name: 'activity', path: '/activity' },
  { name: 'insights', path: '/insights' },
  { name: 'storage', path: '/storage' },
  {
    name: 'storage-prune',
    path: '/storage',
    prepare: async (page) => {
      await page.getByTestId('preview').click();
      await page.getByTestId('engine-preview').waitFor();
    },
  },
  { name: 'logs', path: '/logs?runner=build-win' },
  { name: 'updates', path: '/updates' },
  { name: 'service-unavailable', path: '/', query: 'mock=notInstalled' },
  { name: 'empty', path: '/', query: 'mock=empty' },
];

/** Settings pages (a window of the shell at its declared 920 x 680; the long ones in a taller window so the whole page shows). */
const SETTINGS: { name: string; page: string; height?: number }[] = [
  { name: 'settings', page: 'pc.general' },
  { name: 'settings-defaults', page: 'pc.defaults', height: 1700 },
  { name: 'settings-tokens', page: 'pc.tokens' },
  { name: 'settings-channels', page: 'pc.channels' },
];

for (const theme of ['dark', 'light'] as const) {
  test.describe(`${theme} theme`, () => {
    test.use({ colorScheme: theme });
    test.beforeEach(async ({ page }) => {
      await page.clock.setFixedTime(NOW);
      await page.addInitScript((t) => localStorage.setItem('pitcrew.theme', t), theme);
      await page.emulateMedia({ reducedMotion: 'reduce' });
    });

    for (const shot of SHOTS) {
      test(shot.name, async ({ page }) => {
        await page.goto(`/?simulate=0${shot.query ? '&' + shot.query : ''}#${shot.path}`);
        await page.getByRole('navigation', { name: 'Pages' }).waitFor();
        await shot.prepare?.(page);
        // Grown to the page's whole height only once the page has its content (a skeleton is short).
        await expect(page.locator('pd-skeleton, .skeleton, [aria-busy=true]')).toHaveCount(0);
        await captureScreen(page, join(OUT, theme, `${shot.name}.png`), { fullPage: 'main' });
      });
    }

    for (const w of SETTINGS) {
      test(w.name, async ({ page, app }) => {
        await page.setViewportSize({ width: 920, height: w.height ?? 680 });
        await app.dialogs.openWindow('pd.settings', { page: w.page });
        await app.dialogs.loaded();
        await captureScreen(page, join(OUT, theme, `${w.name}.png`));
      });
    }
  });
}
