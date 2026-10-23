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
    name: 'network',
    path: '/#/network',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Listening' }).click();
      await expect(page.getByRole('grid', { name: 'Listening ports' })).toBeVisible();
    },
  },
  {
    name: 'network-connections',
    path: '/#/network',
    ready: async (page) => {
      await expect(page.getByRole('grid', { name: 'Connections' })).toBeVisible();
      await page.getByRole('row', { name: /142.250.74.46/ }).click();
      await expect(page.getByRole('complementary', { name: 'Details' })).toBeVisible();
    },
  },
  {
    name: 'benchmarks',
    path: '/#/benchmarks',
    ready: async (page) => void (await expect(page.getByRole('region', { name: 'Latest result' })).toBeVisible()),
  },
  {
    name: 'benchmarks-running',
    path: '/#/benchmarks',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Run benchmarks' }).click();
      await expect(page.getByRole('list', { name: 'Progress' })).toBeVisible();
      await page.clock.runFor(1_000);
    },
  },
  {
    name: 'benchmarks-compare',
    path: '/#/benchmarks',
    size: { width: 1440, height: 1500 },
    ready: async (page) => {
      await page.getByRole('checkbox', { name: 'Select Power mode: High performance (before)' }).check();
      await page.getByRole('checkbox', { name: 'Select Power mode: High performance (after)' }).check();
      await page.getByRole('button', { name: 'Compare selected' }).click();
      await expect(page.getByRole('region', { name: 'Comparison' })).toBeVisible();
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
  { name: 'inventory-ddr5', path: '/#/inventory', mock: 'windows&inv=ddr5', ready: async (page) => void (await expect(page.getByRole('img', { name: /memory slots filled/ })).toBeVisible()) },
  { name: 'inventory-laptop', path: '/#/inventory', mock: 'linux', ready: async (page) => void (await expect(page.getByRole('img', { name: /1 of 2 memory slots filled/ })).toBeVisible()) },
  { name: 'inventory-soldered', path: '/#/inventory', mock: 'windows&inv=soldered', ready: async (page) => void (await expect(page.getByRole('img', { name: /Soldered memory/ })).toBeVisible()) },
  { name: 'inventory-generic', path: '/#/inventory', mock: 'windows&inv=generic', ready: async (page) => void (await expect(page.getByRole('img', { name: /memory slots filled/ })).toBeVisible()) },
  {
    name: 'widgets-overview',
    path: '/#/',
    ready: async (page) => {
      await expect(page.locator('dy-widget').first()).toBeVisible();
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'widgets-edit',
    path: '/#/',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit dashboard' }).click();
      await page.getByRole('button', { name: 'Add widget' }).click();
      await expect(page.getByRole('region', { name: 'Widget gallery' })).toBeVisible();
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'widgets-developer',
    path: '/#/',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit dashboard' }).click();
      await page.getByLabel('Preset').selectOption('developer');
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'widgets-battery',
    path: '/#/',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit dashboard' }).click();
      await page.getByLabel('Preset').selectOption('battery');
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'widgets-gaming',
    path: '/#/',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit dashboard' }).click();
      await page.getByLabel('Preset').selectOption('gaming');
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'settings-menu-bar',
    path: '/#/settings',
    ready: async (page) => {
      const section = page.getByRole('region', { name: 'Menu bar items' });
      await section.getByRole('button', { name: 'Add item' }).click();
      await expect(section.getByRole('listitem')).toHaveCount(3);
      await section.getByRole('button', { name: 'Add item' }).click();
      await expect(section.getByRole('listitem')).toHaveCount(4);
      await section.getByLabel('Style of item 1').selectOption('graphAndNumber');
      await section.getByLabel('Metric of item 2').selectOption('memory');
      await section.getByLabel('Style of item 2').selectOption('ring');
      await section.getByLabel('Metric of item 3').selectOption('networkDown');
      await section.getByLabel('Style of item 3').selectOption('lineGraph');
      await section.getByLabel('Metric of item 4').selectOption('temperature');
      await section.getByLabel('Style of item 4').selectOption('number');
      await section.scrollIntoViewIfNeeded();
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'health',
    path: '/#/health',
    size: { width: 1440, height: 1500 },
    ready: async (page) => {
      await expect(page.getByRole('heading', { name: 'Health', exact: true })).toBeVisible();
      await expect(page.getByRole('region', { name: 'Startup manager' }).getByRole('table')).toBeVisible();
    },
  },
  { name: 'settings', path: '/#/settings', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible()) },
  { name: 'rules', path: '/#/rules', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'CPU taming' })).toBeVisible()) },
  { name: 'alarms', path: '/#/alarms', ready: async (page) => void (await expect(page.getByRole('button', { name: /Acknowledge all/ })).toBeVisible()) },
  {
    name: 'alarms-rules',
    path: '/#/alarms',
    size: { width: 1440, height: 1300 },
    ready: async (page) => {
      await page.getByRole('tab', { name: 'Rules' }).click();
      await expect(page.getByRole('button', { name: 'Add rule' })).toBeVisible();
    },
  },
  { name: 'recorder', path: '/#/recorder', size: { width: 1440, height: 1100 }, ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Flight recorder' })).toBeVisible()) },
  { name: 'diagnostics', path: '/#/diagnostics', ready: async (page) => void (await expect(page.getByRole('button', { name: /Save bundle/ })).toBeVisible()) },
  { name: 'tuning', path: '/#/tuning', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Tuning', exact: true })).toBeVisible()) },
  { name: 'cleanup', path: '/#/cleanup', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Cleanup', exact: true })).toBeVisible()) },
  {
    name: 'cleanup-scanned',
    path: '/#/cleanup',
    size: { width: 1440, height: 1100 },
    ready: async (page) => {
      await page.getByRole('button', { name: /^Scan / }).click();
      await page.clock.runFor(3_000);
      await expect(page.getByText(/Scanning…/)).toHaveCount(0);
      await page.getByRole('region', { name: 'Preview' }).scrollIntoViewIfNeeded();
      await page.clock.runFor(500);
    },
  },
  {
    name: 'watched-folders',
    path: '/#/watched',
    ready: async (page) => {
      await expect(page.getByRole('table', { name: 'Watched folders' }).locator('tbody tr')).toHaveCount(3);
      await page.getByRole('row', { name: /Downloads/ }).hover();
    },
  },
  {
    name: 'watched-folders-preview',
    path: '/#/watched',
    ready: async (page) => {
      await page.getByRole('button', { name: 'See what would move in Downloads' }).click({ force: true });
      await expect(page.getByRole('dialog', { name: 'What would move' }).locator('tbody tr').first()).toBeVisible();
    },
  },
  {
    name: 'watched-folders-add',
    path: '/#/watched',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Add folder' }).click();
      await expect(page.getByRole('dialog', { name: 'Watch a folder' })).toBeVisible();
    },
  },
  {
    name: 'web-search',
    path: '/#/issues',
    ready: async (page) => {
      await page.getByRole('table', { name: 'Issues', exact: true }).getByRole('row', { name: /Safari/ }).first().click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Search the web…' }).click();
      await expect(page.getByRole('dialog', { name: 'Search the web' }).getByText(/Removed from the text/)).toBeVisible();
    },
  },
  {
    name: 'hash-search',
    path: '/#/processes',
    ready: async (page) => {
      await page.getByRole('button', { name: 'All', exact: true }).click();
      await page.getByLabel('Filter processes').fill('rustc');
      await page.locator('[role=row][data-key^="p"]').first().click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Search file hash on VirusTotal…' }).click();
      await expect(page.getByRole('dialog', { name: /VirusTotal/ }).getByLabel('SHA-256 hash')).toBeVisible();
    },
  },
  {
    name: 'health-startup-filter',
    path: '/#/health',
    size: { width: 1440, height: 1100 },
    ready: async (page) => {
      await page.getByRole('button', { name: /Hide system entries/ }).click();
      await expect(page.getByText(/hidden by the filter/)).toBeVisible();
    },
  },
  { name: 'why-slow', path: '/#/why-slow', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Why is my PC slow?' })).toBeVisible()) },
  {
    name: 'issues',
    path: '/#/issues',
    size: { width: 1440, height: 1100 },
    ready: async (page) => {
      await expect(page.getByRole('region', { name: 'Stability' })).toBeVisible();
      await expect(page.locator('dy-chart canvas').first()).toBeVisible();
      await page.getByRole('table', { name: 'Issues', exact: true }).getByRole('row', { name: /Slack/ }).first().click();
      await expect(page.getByRole('complementary', { name: 'Issue details' })).toBeVisible();
    },
  },
  {
    name: 'unusual',
    path: '/#/issues',
    size: { width: 1440, height: 900 },
    ready: async (page) => {
      await page.getByRole('button', { name: /New and unusual/ }).click();
      await page.getByRole('row', { name: /updater-helper/ }).click();
      await expect(page.getByRole('complementary', { name: 'Program details' })).toBeVisible();
    },
  },
  { name: 'reports', path: '/#/reports', size: { width: 1440, height: 1100 }, ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Reports', exact: true })).toBeVisible()) },
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
      await page.goto(`${screen.path.replace('/#/', `/?latency=0&chrome=macos${screen.mock ? `&mock=${screen.mock}` : ''}#/`)}`);
      await expect(page.locator('dy-shell, dy-tray').first()).toBeVisible();
      await page.clock.runFor(45_000);
      await screen.ready(page);
      if (screen.name === 'tray') {
        // A desktop-like backdrop (blurred wallpaper gradient and a menu bar) so the rounded, translucent popover is visible.
        const wall = theme === 'dark' ? 'linear-gradient(160deg, #262a33 0%, #3b414d 100%)' : 'linear-gradient(160deg, #c3c7cf 0%, #e1e3e8 100%)';
        await page.addStyleTag({
          content: `html[data-surface='popover'] { background: ${wall} !important; }
            html[data-surface='popover'] body { padding: 34px 40px 0 !important; }
            body::before { content: ''; position: fixed; inset: 0 0 auto 0; height: 24px; background: rgb(${theme === 'dark' ? '20 20 24 / 55%' : '255 255 255 / 55%'}); backdrop-filter: blur(20px); z-index: 0; }`,
        });
        await page.clock.runFor(500);
      }
      if (screen.name === 'tray') {
        // Only the popover plus a margin, not the empty wallpaper below it.
        const box = await page.getByRole('button', { name: /Open Dyno/ }).boundingBox();
        const bottom = Math.ceil((box?.y ?? 560) + (box?.height ?? 40));
        await page.screenshot({ path: join(OUT, `${screen.name}-${theme}.png`), clip: { x: 0, y: 0, width: 440, height: Math.max(480, bottom + 28) } });
      } else {
        await page.screenshot({ path: join(OUT, `${screen.name}-${theme}.png`) });
      }
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
