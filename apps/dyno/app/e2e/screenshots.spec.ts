import { expect, test, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Documentation screenshots of every screen in both themes (`SCREENSHOTS=1 npx playwright test --project=screenshots`).
 * Deterministic: the clock is frozen, the mock is a macOS laptop, the timezone and locale are fixed and motion is reduced.
 */
// Playwright runs from the app folder; the images live in the repository's docs.
/** Picks an option of a Material select (the list opens in an overlay on the page). */
async function choose(page: Page, label: string, option: string | RegExp): Promise<void> {
  await page.getByRole('combobox', { name: label }).click();
  await page.getByRole('option', { name: option, exact: typeof option === 'string' }).click();
}

/** Opens the shell's Settings window (Mod+, on the mock's macOS) and shows Dyno's page of it. */
async function openSettings(page: Page): Promise<void> {
  await page.keyboard.press('ControlOrMeta+Comma');
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByText('Dyno', { exact: true }).click();
}

const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', colorScheme: 'light' });

async function prepare(page: Page, theme: 'dark' | 'light', mock = 'macos'): Promise<void> {
  // A fake clock starting at a fixed time: the mock's live values are a function of the clock, so every run is identical.
  await page.clock.install({ time: FROZEN });
  await page.emulateMedia({ colorScheme: theme });
  void mock;
}

const SCREENS: { name: string; path: string; mock?: string; query?: string; latency?: number; ready: (page: Page) => Promise<void>; size?: { width: number; height: number } }[] = [
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
    name: 'counters',
    path: '/#/counters',
    ready: async (page) => {
      await page.getByRole('button', { name: /Sets/ }).click();
      await page.getByRole('menuitem', { name: /^Disk and network \d/ }).click();
      await page.getByRole('list', { name: 'Objects' }).getByRole('button', { name: /^Network Interface/ }).click();
      await expect(page.getByRole('grid', { name: 'Charted counters' })).toBeVisible();
      await expect(page.locator('dy-chart canvas').first()).toBeVisible();
      await page.clock.runFor(2_500);
    },
  },
  {
    name: 'counters-search',
    path: '/#/counters',
    ready: async (page) => {
      await page.getByRole('searchbox', { name: 'Search counters' }).fill('bytes');
      await page.clock.runFor(300);
      await expect(page.getByRole('list', { name: 'Search results' })).toBeVisible();
      await page.getByRole('button', { name: /Chart Bytes Received \(en0\)/ }).click();
      await page.getByRole('button', { name: /Chart Bytes Sent \(en0\)/ }).click();
      await page.getByRole('button', { name: /Chart Bytes Transferred \(disk0\)/ }).click();
      await expect(page.locator('dy-chart canvas').first()).toBeVisible();
      // No tooltip of the last click hovering over the list in the picture.
      await page.mouse.move(2, 2);
      await page.clock.runFor(2_500);
    },
  },
  {
    name: 'network',
    path: '/#/network',
    ready: async (page) => {
      await page.getByRole('radio', { name: 'Listening' }).click();
      await expect(page.getByRole('treegrid', { name: 'Listening ports' })).toBeVisible();
    },
  },
  {
    name: 'network-connections',
    path: '/#/network',
    ready: async (page) => {
      await expect(page.getByRole('treegrid', { name: 'Connections' })).toBeVisible();
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
      await choose(page, 'Preset', 'Developer');
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'widgets-battery',
    path: '/#/',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit dashboard' }).click();
      await choose(page, 'Preset', 'Battery');
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'widgets-gaming',
    path: '/#/',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit dashboard' }).click();
      await choose(page, 'Preset', 'Gaming');
      await page.clock.runFor(2_000);
    },
  },
  {
    name: 'settings-menu-bar',
    path: '/#/',
    size: { width: 1440, height: 1900 },
    ready: async (page) => {
      await openSettings(page);
      const section = page.getByRole('region', { name: 'Menu bar items' });
      await section.getByRole('button', { name: 'Add item' }).click();
      await expect(section.getByRole('listitem')).toHaveCount(3);
      await section.getByRole('button', { name: 'Add item' }).click();
      await expect(section.getByRole('listitem')).toHaveCount(4);
      await choose(page, 'Style of item 1', 'Graph and number');
      await choose(page, 'Metric of item 2', 'Memory');
      await choose(page, 'Style of item 2', 'Ring');
      await choose(page, 'Metric of item 3', 'Network down');
      await choose(page, 'Style of item 3', 'Line graph');
      await choose(page, 'Metric of item 4', 'Temperature');
      await choose(page, 'Style of item 4', 'Number');
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
      await expect(page.getByRole('region', { name: 'Start-up items' }).getByRole('grid')).toBeVisible();
    },
  },
  { name: 'backup', path: '/#/backup', ready: async (page) => void (await expect(page.getByRole('grid', { name: 'Backup tools' })).toBeVisible()) },
  {
    name: 'extras-changes',
    path: '/#/extras?tab=changes',
    ready: async (page) => void (await expect(page.getByRole('treegrid', { name: 'What changed' })).toBeVisible()),
  },
  {
    name: 'extras-sleep',
    path: '/#/extras?tab=sleep',
    ready: async (page) => void (await expect(page.getByRole('grid', { name: 'Recent wakes' })).toBeVisible()),
  },
  {
    name: 'extras-boot',
    path: '/#/extras?tab=boot',
    ready: async (page) => {
      await expect(page.getByRole('grid', { name: 'What slowed start-up' })).toBeVisible();
      await expect(page.locator('dy-chart canvas').first()).toBeVisible();
      await page.clock.runFor(500);
    },
  },
  {
    name: 'extras-apps',
    path: '/#/extras?tab=apps',
    ready: async (page) => void (await expect(page.getByRole('grid', { name: 'Network and energy per app' })).toBeVisible()),
  },
  {
    name: 'extras-gpu',
    path: '/#/extras?tab=gpu',
    ready: async (page) => void (await expect(page.getByRole('grid', { name: 'GPU use per process' })).toBeVisible()),
  },
  {
    name: 'extras-files',
    path: '/#/extras?tab=files',
    size: { width: 1440, height: 1320 },
    ready: async (page) => {
      await page.getByRole('button', { name: 'Choose a folder' }).click();
      await expect(page.getByRole('treegrid', { name: 'Large and duplicate files' })).toBeVisible();
    },
  },
  {
    name: 'extras-inventory',
    path: '/#/extras?tab=inventory',
    ready: async (page) => void (await expect(page.getByRole('grid', { name: 'Installed software' })).toBeVisible()),
  },
  {
    name: 'extras-maintenance',
    path: '/#/extras?tab=maintenance',
    ready: async (page) => void (await expect(page.getByRole('region', { name: 'Maintenance windows' })).toContainText('Mon to Fri')),
  },
  { name: 'settings', path: '/#/', size: { width: 1440, height: 1500 }, ready: async (page) => openSettings(page) },
  { name: 'alarms', path: '/#/alarms', ready: async (page) => void (await expect(page.getByRole('button', { name: /Acknowledge all/ })).toBeVisible()) },
  {
    name: 'alarms-rules',
    path: '/#/alarms',
    size: { width: 1440, height: 1300 },
    ready: async (page) => {
      await page.getByRole('tab', { name: 'Rules' }).click();
      await expect(page.getByRole('button', { name: 'Add rule' })).toBeVisible();
      // No hover or focus state left on the tab in the picture.
      await page.mouse.move(2, 2);
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    },
  },
  { name: 'recorder', path: '/#/recorder', size: { width: 1440, height: 1100 }, ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Flight recorder' })).toBeVisible()) },
  { name: 'diagnostics', path: '/#/diagnostics', ready: async (page) => void (await expect(page.getByRole('button', { name: /Save bundle/ })).toBeVisible()) },
  {
    name: 'web-search',
    path: '/#/issues',
    ready: async (page) => {
      await page.getByRole('treegrid', { name: 'Issues', exact: true }).getByRole('row', { name: /Safari/ }).first().click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Search the web…' }).click();
      await expect(page.getByRole('dialog', { name: 'Search the web' }).getByText(/Removed from the text/)).toBeVisible();
    },
  },
  {
    name: 'hash-search',
    path: '/#/processes',
    ready: async (page) => {
      await page.getByRole('radio', { name: 'All', exact: true }).click();
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
      await page.getByRole('checkbox', { name: /Hide system entries/ }).click();
      await expect(page.getByText(/hidden by the filter/)).toBeVisible();
    },
  },
  { name: 'why-slow', path: '/#/why-slow', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Why is my PC slow?' })).toBeVisible()) },
  {
    name: 'throttling',
    path: '/#/throttling',
    ready: async (page) => {
      await expect(page.getByRole('heading', { name: 'Running at full speed' })).toBeVisible();
      await expect(page.getByRole('grid').first()).toBeVisible();
    },
  },
  {
    name: 'throttling-hot',
    path: '/#/throttling',
    query: '&hw=hot',
    ready: async (page) => {
      await expect(page.getByRole('heading', { name: 'Slowed down by heat: 44% slower' })).toBeVisible();
      await expect(page.getByRole('grid').first()).toBeVisible();
    },
  },
  {
    name: 'forecasts',
    path: '/#/lifespan',
    ready: async (page) => {
      await expect(page.getByText('Replace in about 23 months (17 to 31 months)')).toBeVisible();
      await expect(page.getByRole('img', { name: /Soonest/ }).first()).toBeVisible();
    },
  },
  {
    // The "Is this normal?" card sits at the top of the dashboard, so the 1440x900 view shows it whole.
    name: 'baselines',
    path: '/#/',
    ready: async (page) => {
      const card = page.getByRole('region', { name: 'Is this normal?' });
      await expect(card.getByText('Higher than usual for Tuesday mornings').first()).toBeVisible();
      await card.scrollIntoViewIfNeeded();
    },
  },
  {
    name: 'issues',
    path: '/#/issues',
    size: { width: 1440, height: 1100 },
    ready: async (page) => {
      await expect(page.getByRole('region', { name: 'Stability' })).toBeVisible();
      await expect(page.locator('dy-chart canvas').first()).toBeVisible();
      await page.getByRole('treegrid', { name: 'Issues', exact: true }).getByRole('row', { name: /Slack/ }).first().click();
      await expect(page.getByRole('complementary', { name: 'Issue details' })).toBeVisible();
    },
  },
  {
    name: 'unusual',
    path: '/#/issues',
    size: { width: 1440, height: 900 },
    ready: async (page) => {
      await page.getByRole('radio', { name: /New and unusual/ }).click();
      await page.getByRole('row', { name: /updater-helper/ }).click();
      await expect(page.getByRole('complementary', { name: 'Program details' })).toBeVisible();
    },
  },
  { name: 'reports', path: '/#/reports', size: { width: 1440, height: 1100 }, ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Reports', exact: true })).toBeVisible()) },
  {
    name: 'disk-map',
    path: '/#/disk-map',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
      await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
      await expect(page.getByRole('tree', { name: 'Disk usage of Macintosh HD' })).toBeVisible();
      await page.mouse.move(2, 2);
      await page.clock.runFor(1_000);
    },
  },
  {
    name: 'disk-map-table',
    path: '/#/disk-map',
    ready: async (page) => {
      await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
      await expect(page.getByRole('button', { name: 'Rescan Macintosh HD' })).toBeVisible();
      await page.getByRole('radio', { name: 'Table' }).click();
      await expect(page.getByRole('table', { name: /Contents of Macintosh HD/ })).toBeVisible();
      await page.mouse.move(2, 2);
      await page.clock.runFor(1_000);
    },
  },
  {
    name: 'command-palette',
    path: '/#/',
    ready: async (page) => {
      await page.keyboard.press('ControlOrMeta+p');
      await expect(page.getByRole('combobox', { name: /Type a command/ })).toBeFocused();
      await page.clock.runFor(500);
      await expect(page.getByRole('option').first()).toBeVisible();
    },
  },
  {
    name: 'jobs',
    path: '/#/issues',
    latency: 4_000,
    ready: async (page) => {
      await page.getByRole('button', { name: 'Read the system logs now' }).click();
      await page.clock.runFor(1_500);
      await page.getByRole('button', { name: /^Jobs/ }).click();
      await expect(page.getByRole('region', { name: 'Jobs' }).getByRole('listitem').first()).toBeVisible();
      await page.mouse.move(2, 2);
      await page.clock.runFor(200);
    },
  },
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
      await page.goto(`${screen.path.replace('/#/', `/?latency=${screen.latency ?? 0}&chrome=macos${screen.mock ? `&mock=${screen.mock}` : ''}${screen.query ?? ''}#/`)}`);
      await expect(page.locator('pd-shell-frame, dy-tray').first()).toBeVisible();
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
        await page.getByRole('group', { name: 'CPU', exact: true }).getByRole('radio', { name: 'Per core' }).click();
        await page.clock.runFor(1_000);
        await page.getByRole('group', { name: 'CPU', exact: true }).screenshot({ path: join(OUT, `cpu-per-core-tile-${theme}.png`) });
        await page.getByRole('group', { name: 'CPU', exact: true }).getByRole('radio', { name: 'Overall' }).click();
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
