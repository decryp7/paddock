import { expect } from './test';
import type { Page } from '@playwright/test';
import { captureDocScreens, pressMod, type DocScreen, type Theme } from '@decryp7/paddock-e2e';
import { MOCK_ESTIMATES, mockRun, mockTrace } from '../src/app/core/mock/bench';
import type { BenchOverview } from '../src/app/core/api.types';
import { join } from 'node:path';

/**
 * Documentation screenshots of every screen in both themes (`SCREENSHOTS=1 npx playwright test --project=screenshots`), through the shared runner
 * (`captureDocScreens` in @decryp7/paddock-e2e): the pinned clock, the end of a minute on it, live updates stopped, no animation and no caret before
 * every picture. The mock is a macOS laptop, the timezone and locale are fixed and motion is reduced; `?stable=60000` makes the mock round what its
 * polled replies read of the clock down to a minute (a 2 s poll's phase against the clock otherwise showed in the network and recorder pictures).
 * Measured 2026-11-16 (ER-40, ER-46): the old code reproduces its own tracked pictures byte for byte on this machine (54 of 62), so fonts and
 * rendering are stable here; the tracked ones had drifted because the UI changed (62 of 72 differed after 386 Dyno commits).
 */
// Playwright runs from the app folder; the images live in the repository's docs.
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);

/** The tallest a page picture gets (CSS px of content); a taller page ends on the last card that fits. */
const MAX_CONTENT = 1300;

/** The window never gets shorter than the sidebar needs (CSS px), so the nav list is whole beside a short page. */
const MIN_WINDOW = 900;

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
  await page.setViewportSize({ width: 1440, height: Math.max(MIN_WINDOW, Math.round(plan.chrome + plan.content + 16)) });
  await page.clock.runFor(300);
  await main.evaluate((el) => el.scrollTo(0, 0));
}

/**
 * Ends the window just below the last row of a table that scrolls inside the window (its height follows the window's): a shorter window cut the last
 * row in two and a taller one left an empty card. Call it with the window made too tall for the rows, then it shrinks to them.
 */
async function trimToLastRow(page: Page, gridName: string): Promise<void> {
  const grid = page.getByRole('treegrid', { name: gridName, exact: true });
  const slack = await grid.evaluate((g) => {
    const rows = Array.from(g.querySelectorAll<HTMLElement>('[role=row]')).filter((r) => r.getBoundingClientRect().height > 0);
    const last = rows.at(-1)?.getBoundingClientRect().bottom ?? 0;
    return Math.max(0, Math.floor(g.getBoundingClientRect().bottom - last));
  });
  const height = await page.evaluate(() => window.innerHeight);
  await page.setViewportSize({ width: 1440, height: height - slack + 16 });
  await page.clock.runFor(300);
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

type Screen = { name: string; path: string; mock?: string; query?: string; latency?: number; maxContent?: number; trimTo?: string; before?: (page: Page) => Promise<void>; ready: (page: Page) => Promise<void>; size?: { width: number; height: number } };

const SCREENS: Screen[] = [
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
    // The window is tall enough for the whole table: its rows scroll inside it, so a shorter window cut the last row.
    size: { width: 1440, height: 1500 },
    trimTo: 'Connections',
    ready: async (page) => {
      await expect(page.getByRole('treegrid', { name: 'Connections' })).toBeVisible();
      await page.getByRole('row', { name: /142.250.74.46/ }).click();
      await expect(page.getByRole('complementary', { name: 'Details' })).toBeVisible();
    },
  },
  {
    name: 'benchmarks',
    path: '/#/benchmarks',
    // The Latest result cards and the History table, not only the first card.
    maxContent: 2600,
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
    maxContent: 900,
    ready: async (page) => {
      await page.getByRole('button', { name: 'Edit layout' }).click();
      await expect(page.getByText('Editing layout')).toBeVisible();
      // The widget gallery: the side sheet with every kind and its Add button.
      await page.getByRole('button', { name: 'Add widget', exact: true }).click();
      await expect(page.getByRole('dialog', { name: 'Add a widget' })).toBeVisible();
      await page.clock.runFor(2_000);
    },
  },
  {
    // A sustained processor run in progress (the mock plays the scenario written to localStorage, as screenshots-bench.spec.ts does).
    name: 'benchmarks-running',
    path: '/#/benchmarks',
    maxContent: 2600,
    before: async (page) => {
      const done = mockRun('r1', FROZEN - 3_600_000, 'Baseline', 1);
      const overview: BenchOverview = {
        running: {
          runId: 'r2',
          startedAt: FROZEN,
          kinds: ['cpu'],
          current: 'cpu',
          done: [],
          phase: { phase: 'cpu-sustained', label: 'Sustained run, all cores', rep: 16, of: 60, count: 80, total: 300 },
          liveness: 'running',
          limited: null,
          trace: mockTrace(80, 180, 0.8, 40),
        },
        // Two kinds, so the History row's "Measured" text fits on one line.
        runs: [{ ...done, results: done.results.filter((r) => r.kind === 'cpu' || r.kind === 'memory') }],
        diskTestEnabled: true,
        estimates: MOCK_ESTIMATES,
      };
      await page.addInitScript((o) => localStorage.setItem('dyno.mock.bench', o), JSON.stringify(overview));
    },
    ready: async (page) => {
      await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
      await expect(page.getByText(/Sustained run, all cores/).first()).toBeVisible();
      await page.clock.runFor(500);
    },
  },
  {
    // A real job (the Large and duplicate files search, held at its first step by the mock) in the Jobs popover, with its phase and Cancel.
    // The job is started on Extras and the page then changes to Power: the popover is the status bar's, so it follows the job.
    name: 'jobs',
    path: '/#/extras?tab=files',
    query: '&jobsteps=40&jobstepms=600000',
    ready: async (page) => {
      // Hold the clock first: the job's elapsed time and its "Still working" mark follow the page clock.
      await freezeClock(page);
      await page.getByRole('button', { name: 'Choose a folder' }).click();
      await page.clock.runFor(300);
      const pill = page.locator('pd-jobs-status button.pd-pill');
      await expect(pill).toContainText('1 running');
      await page.getByRole('link', { name: 'Power', exact: true }).click();
      await page.clock.runFor(1_000);
      await expect(page.getByRole('heading', { name: 'Power', level: 1 })).toBeVisible();
      await pill.click();
      await page.clock.runFor(300);
      const popover = page.getByRole('region', { name: 'Jobs' });
      await expect(popover.getByRole('button', { name: 'Cancel' })).toBeVisible();
      await expect(popover.locator('pd-job')).toHaveCount(1);
      await page.mouse.move(2, 2);
      await page.clock.runFor(500);
    },
  },
  {
    // The Power page: keep awake held for an hour, with its tabs for Sleep and shut down, Schedule and Wake devices.
    name: 'power',
    path: '/#/power',
    ready: async (page) => {
      await expect(page.getByRole('heading', { name: 'Power', level: 1 })).toBeVisible();
      await page.getByRole('button', { name: '1 hour' }).click();
      await expect(page.getByRole('button', { name: 'Stop keeping awake' })).toBeVisible();
      await page.mouse.move(2, 2);
      await page.clock.runFor(500);
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
    size: { width: 1440, height: 2900 },
    trimTo: 'Issues',
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

const screenUrl = (screen: Screen) => screen.path.replace('/#/', `/?latency=${screen.latency ?? 0}&stable=60000&chrome=macos&menu=native${screen.mock ? `&mock=${screen.mock}` : ''}${screen.query ?? ''}#/`);

/** The tray popover on a desktop-like backdrop (blurred wallpaper gradient and a menu bar), so the rounded, translucent popover is visible. */
async function trayBackdrop(page: Page, theme: Theme): Promise<void> {
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

/** The whole dashboard, for the README and for review (the 1440x900 picture only shows the top): the window is made as tall as the page (it scrolls inside it). */
async function wholeDashboard(page: Page): Promise<void> {
  const main = page.locator('#pd-main');
  const total = await main.evaluate((el) => window.innerHeight - el.clientHeight + el.scrollHeight);
  await page.setViewportSize({ width: 1440, height: total });
  await expect(async () => expect(await main.evaluate((el) => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(0)).toPass({ timeout: 10_000 });
  await main.evaluate((el) => el.scrollTo(0, 0));
  await page.clock.runFor(500);
}

const toDocScreen = (screen: Screen): DocScreen => ({
  name: screen.name,
  url: screenUrl(screen),
  ...(screen.size ? { size: screen.size } : {}),
  ...(screen.before ? { before: (page: Page) => screen.before!(page) } : {}),
  ready: async (page, theme) => {
    await screen.ready(page);
    if (screen.name === 'tray') {
      await trayBackdrop(page, theme);
      return;
    }
    await freezeClock(page);
    await fitToContent(page, screen.maxContent);
    if (screen.trimTo) await trimToLastRow(page, screen.trimTo);
    await macWindowButtons(page);
  },
  ...(screen.name === 'tray'
    ? {
        // Only the popover plus a margin, not the empty wallpaper below it.
        clip: async (page: Page) => {
          const box = await page.getByRole('button', { name: /Open Dyno/ }).boundingBox();
          const bottom = Math.ceil((box?.y ?? 560) + (box?.height ?? 40));
          return { x: 0, y: 0, width: 440, height: Math.max(480, bottom + 28) };
        },
      }
    : {}),
});

// The shared documentation screenshot runner (paddock-e2e): the pinned clock, the end of a minute on it, live updates stopped, no animation and
// no caret before every picture.
captureDocScreens({
  screens: [
    ...SCREENS.map(toDocScreen),
    {
      ...toDocScreen(SCREENS.find((s) => s.name === 'dashboard')!),
      name: 'dashboard-full',
      ready: async (page, theme) => {
        await SCREENS.find((s) => s.name === 'dashboard')!.ready(page);
        await freezeClock(page);
        await fitToContent(page);
        await macWindowButtons(page);
        await wholeDashboard(page);
        void theme;
      },
    },
  ],
  outDir: OUT,
  layout: 'flat',
  frozenAt: FROZEN,
  timezoneId: 'UTC',
  locale: 'en-GB',
  // 30 s of the clock before a screen's own steps, so a slow start still leaves them inside the first minute (the mock rounds its polled replies to a
  // minute: a scan clicked after the minute turned read a different time, 'scanned 4 min ago' against '5 min ago' in disk-map, measured 2026-11-16).
  warmupMs: 30_000,
  appSelector: 'pd-shell-frame, dy-tray',
  alignToBucketMs: 60_000,
});
