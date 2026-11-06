import { expect, test, type Page } from '@playwright/test';
import { pressMod } from '@decryp7/paddock-e2e';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Documentation screenshots of every screen in both themes (`SCREENSHOTS=1 npx playwright test --project=screenshots`).
 * Deterministic: the clock is frozen, the mock is a macOS laptop, the timezone and locale are fixed and motion is reduced.
 */
// Playwright runs from the app folder; the images live in the repository's docs.
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

/** The tallest a page picture gets (CSS px of content); a taller page ends on the last card that fits. */
const MAX_CONTENT = 1300;

/**
 * Makes the window as tall as the page's content, so no card or row is cut in half: a short page ends where it ends; a long page ends
 * on the bottom edge of the last card that fits in MAX_CONTENT (the content is scrolled inside the window, so the window is resized).
 */
async function fitToContent(page: Page, maxContent = MAX_CONTENT): Promise<void> {
  const main = page.locator('#pd-main');
  const plan = await main.evaluate((el, max) => {
    const chrome = window.innerHeight - el.clientHeight;
    const top = el.getBoundingClientRect().top - el.scrollTop;
    const cards = Array.from(el.querySelectorAll<HTMLElement>('.card, dy-tile, dy-widget, section, [class~="panel"]'))
      .filter((c) => c.offsetParent !== null && !c.closest('mat-sidenav, [role="complementary"], .mat-mdc-select-panel'))
      .map((c) => Math.round(c.getBoundingClientRect().bottom - top))
      .sort((a, b) => a - b);
    const full = el.scrollHeight;
    const content = full <= max ? full : (cards.filter((b) => b <= max).pop() ?? max);
    return { chrome, content, scrollable: full > el.clientHeight };
  }, maxContent);
  if (!plan.scrollable) return;
  await page.setViewportSize({ width: 1440, height: Math.round(plan.chrome + plan.content + 16) });
  await page.clock.runFor(300);
  await main.evaluate((el) => el.scrollTo(0, 0));
}

/** The macOS window buttons, over the top-left of the window as the native title bar draws them (the shell leaves room for them). */
async function macWindowButtons(page: Page): Promise<void> {
  await page.evaluate(() => {
    const bar = document.createElement('div');
    bar.setAttribute('aria-hidden', 'true');
    bar.style.cssText = 'position:fixed;left:14px;top:14px;z-index:2147483647;display:flex;gap:8px;pointer-events:none';
    for (const colour of ['#ff5f57', '#febc2e', '#28c840']) {
      const dot = document.createElement('span');
      dot.style.cssText = `width:12px;height:12px;border-radius:50%;background:${colour};box-shadow:inset 0 0 0 0.5px rgba(0,0,0,.25)`;
      bar.append(dot);
    }
    document.body.append(bar);
  });
}

test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', colorScheme: 'light' });

async function prepare(page: Page, theme: 'dark' | 'light', mock = 'macos'): Promise<void> {
  // A fake clock starting at a fixed time: the mock's live values are a function of the clock, so every run is identical.
  await page.clock.install({ time: FROZEN });
  await page.emulateMedia({ colorScheme: theme });
  void mock;
}

/**
 * Lands the page's clock on one fixed time and holds it there: the mock's live values are a function of the clock, and a running clock
 * moves them by the real time the page took to load, so the light and the dark run would show different numbers.
 */
const frozen = new WeakSet<Page>();
async function freezeClock(page: Page): Promise<void> {
  if (frozen.has(page)) return;
  frozen.add(page);
  const target = FROZEN + 300_000;
  const now = await page.evaluate(() => Date.now());
  if (now < target) await page.clock.runFor(target - now);
  await page.clock.pauseAt(target + 1_000);
  await page.clock.runFor(200);
}

const SCREENS: { name: string; path: string; mock?: string; query?: string; latency?: number; maxContent?: number; ready: (page: Page) => Promise<void>; size?: { width: number; height: number } }[] = [
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
      // The selected row and the pane's "Now" are two reads of the same process: with the clock held they are the same second's sample, so select
      // again after the hold (another row, then this one) to read the pane at the held time.
      await freezeClock(page);
      await page.locator('[role=row][data-key^="p"]').nth(1).click();
      await page.clock.runFor(100);
      await page.locator('[role=row][data-key^="p"]').first().click();
      await expect(page.getByRole('complementary', { name: 'Process details' }).getByText('Command line')).toBeVisible();
      await page.clock.runFor(100);
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
    name: 'inventory',
    path: '/#/inventory',
    ready: async (page) => void (await expect(page.getByRole('img', { name: /Unified memory/ })).toBeVisible()),
  },
  {
    name: 'widgets-edit',
    path: '/#/',
    // The dashboard in edit mode: the edit bar and the widgets with their menus and grips.
    maxContent: 3800,
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit layout' }).click();
      await expect(page.getByText('Editing layout')).toBeVisible();
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
  {
    name: 'extras-changes',
    path: '/#/extras?tab=changes',
    ready: async (page) => void (await expect(page.getByRole('treegrid', { name: 'What changed' })).toBeVisible()),
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
    name: 'extras-files',
    path: '/#/extras?tab=files',
    size: { width: 1440, height: 1320 },
    ready: async (page) => {
      await page.getByRole('button', { name: 'Choose a folder' }).click();
      await expect(page.getByRole('treegrid', { name: 'Large and duplicate files' })).toBeVisible();
    },
  },
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
  {
    name: 'alarms-history',
    path: '/#/alarms',
    ready: async (page) => {
      await page.getByRole('tab', { name: 'History' }).click();
      await expect(page.getByRole('tab', { name: 'History', selected: true })).toBeVisible();
      await page.mouse.move(2, 2);
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      await page.clock.runFor(1_000);
    },
  },
  { name: 'recorder', path: '/#/recorder', size: { width: 1440, height: 1100 }, ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Flight recorder' })).toBeVisible()) },
  { name: 'diagnostics', path: '/#/diagnostics', ready: async (page) => void (await expect(page.getByRole('button', { name: /Report a problem/ })).toBeVisible()) },
  { name: 'why-slow', path: '/#/why-slow', ready: async (page) => void (await expect(page.getByRole('heading', { name: 'Why is my computer slow?' })).toBeVisible()) },
  {
    name: 'throttling',
    path: '/#/throttling',
    ready: async (page) => {
      await expect(page.getByRole('heading', { name: 'Running at full speed' })).toBeVisible();
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
    name: 'issues',
    path: '/#/issues',
    // The whole table, so no group of rows is cut in half.
    maxContent: 3600,
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
      await pressMod(page, 'Mod+p');
      await expect(page.getByRole('combobox', { name: /Type a command/ })).toBeFocused();
      await page.clock.runFor(500);
      await expect(page.getByRole('option').first()).toBeVisible();
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
      await page.goto(`${screen.path.replace('/#/', `/?latency=${screen.latency ?? 0}&chrome=macos&menu=native${screen.mock ? `&mock=${screen.mock}` : ''}${screen.query ?? ''}#/`)}`);
      await expect(page.locator('pd-shell-frame, dy-tray').first()).toBeVisible();
      await page.clock.runFor(45_000);
      await screen.ready(page);
      await freezeClock(page);
      if (screen.name !== 'tray') {
        await fitToContent(page, screen.maxContent);
        await macWindowButtons(page);
      }
      if (screen.name === 'tray') {
        // A desktop-like backdrop (blurred wallpaper gradient and a menu bar) so the rounded, translucent popover is visible. The
        // tray window's root is `html.pd-tray`; the shot fails if the backdrop did not apply (a renamed root would leave it plain).
        const wall = theme === 'dark' ? 'linear-gradient(160deg, #262a33 0%, #3b414d 100%)' : 'linear-gradient(160deg, #c3c7cf 0%, #e1e3e8 100%)';
        await page.addStyleTag({
          content: `html.pd-tray { background: ${wall} !important; }
            html.pd-tray body { padding: 34px 40px 0 !important; }
            #mac-menubar { position: fixed; inset: 0 0 auto 0; height: 24px; display: flex; align-items: center; justify-content: space-between; padding: 0 12px; font: 13px -apple-system, 'Helvetica Neue', Arial, sans-serif; color: ${theme === 'dark' ? '#f2f2f5' : '#1d1d1f'}; background: rgb(${theme === 'dark' ? '20 20 24 / 55%' : '255 255 255 / 55%'}); backdrop-filter: blur(20px); z-index: 0; }
            #mac-menubar span { margin-right: 14px; } #mac-menubar b { margin-right: 14px; } #mac-menubar .right { display: flex; align-items: center; } #mac-menubar .right span { margin: 0 0 0 14px; }`,
        });
        await page.evaluate(() => {
          const bar = document.createElement('div');
          bar.id = 'mac-menubar';
          bar.innerHTML = '<div><b>Dyno</b><span>File</span><span>View</span><span>Window</span><span>Help</span></div>'
            + '<div class="right"><span title="Dyno" style="display:inline-flex;align-items:center;gap:4px"><svg width="26" height="12" viewBox="0 0 26 12"><polyline points="0,9 4,6 8,7 12,3 16,5 20,2 25,4" fill="none" stroke="currentColor" stroke-width="1.4"/></svg>76</span><span>Thu 22 Oct  14:30</span></div>';
          document.body.append(bar);
        });
        await expect(page.locator('body')).toHaveCSS('padding-top', '34px');
        await expect(page.locator('html')).toHaveCSS('background-image', /linear-gradient/);
        // The sparklines have their history before the picture is taken.
        await page.clock.runFor(45_000);
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
        // The whole page, for the README and for review (the 1440x900 image only shows the top): the window is made as tall as the
        // page (the page scrolls inside it), scrolled to the top, and the shot waits until nothing is left to scroll.
        const main = page.locator('#pd-main');
        const total = await main.evaluate((el) => window.innerHeight - el.clientHeight + el.scrollHeight);
        await page.setViewportSize({ width: 1440, height: total });
        await expect(async () => expect(await main.evaluate((el) => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(0)).toPass({ timeout: 10_000 });
        await main.evaluate((el) => el.scrollTo(0, 0));
        await page.clock.runFor(500);
        await page.screenshot({ path: join(OUT, `dashboard-full-${theme}.png`), fullPage: true });
      }
    });
  }
}
