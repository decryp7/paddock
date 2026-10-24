import { captureDocScreens, e2ePort, expect, installTauriFake, ShellApp, waitForStableFrame, WINDOW_SIZES, type DocScreen, type Locator, type Page } from '@decryp7/paddock-e2e';

// The documentation screenshots of this repository (UI rule 20, rule 22), all taken by the kit's runner (plan X39):
// 1440 x 900 at twice the pixel density (the `screenshots` project), a fake clock at one fixed time (paused for the gallery; the shell demo's runs until its app context has loaded, then `freeze` stops it), UTC and en-GB,
// reduced motion, each screen in both themes, written to ui/docs/screenshots with a hashes.json that conformance check C24
// compares. `SCREENSHOTS=1 npx playwright test --project=screenshots` rewrites them (both dev servers are started).
const GALLERY = `http://localhost:${e2ePort('PW_PORT', 4300)}`;
const DEMO = `http://localhost:${e2ePort('SHELL_DEMO_PORT', 4301)}`;
const OUT = `${process.cwd()}/docs/screenshots`;
const common = { outDir: OUT, clock: 'paused' as const };

// ---- the component gallery: each section of each accent, the two full pages, the density comparison --------------------

const ACCENTS = ['debrief', 'pitcrew', 'pitwall', 'dyno', 'spotter', 'pacenotes', 'sighter'] as const;
const SECTIONS = ['card', 'status-pill', 'metric', 'sparkline', 'empty-state', 'error-state', 'skeleton', 'job', 'segmented-control', 'icons', 'controls', 'path-text', 'tray-popover', 'chart-theme'] as const;

/** Opens the gallery in the run's theme and lets the charts draw on the paused clock (their animation is 300 ms). */
async function gallery(page: Page, theme: string, query: string): Promise<void> {
  await page.goto(`${GALLERY}/?theme=${theme}&${query}`);
  await page.locator('[data-testid="card"] pd-card').first().waitFor();
  await page.clock.runFor(1_000);
}

captureDocScreens({
  ...common,
  appSelector: 'pd-card',
  screens: ACCENTS.flatMap((accent): DocScreen[] =>
    SECTIONS.map((section) => ({
      name: `${accent}/${section}`,
      url: `${GALLERY}/?accent=${accent}`,
      ready: (page, theme) => gallery(page, theme, `accent=${accent}&density=standard`),
      element: (page) => page.locator(`section[data-testid="${section}"]`),
    })),
  ),
});

captureDocScreens({
  ...common,
  layout: 'flat',
  appSelector: 'pd-card',
  screens: [
    ...(['debrief', 'dyno'] as const).map((accent): DocScreen => ({
      name: `gallery-${accent}`,
      url: `${GALLERY}/?accent=${accent}`,
      ready: (page, theme) => gallery(page, theme, `accent=${accent}&density=standard`),
    })),
    ...(['comfortable', 'compact'] as const).map((density): DocScreen => ({
      name: `density/card-${density}`,
      url: `${GALLERY}/?accent=debrief`,
      themes: ['dark'],
      ready: (page, theme) => gallery(page, theme, `accent=debrief&density=${density}`),
      element: (page) => page.locator('[data-testid="card"]'),
    })),
  ],
});

// ---- the shell demo: the pages frame and the look ----------------------------------------------------------------------

/** Waits for the demo's app context (loaded when the window is idle, which needs the clock running). */
const loaded = (page: Page) => expect(page.getByTestId('app-context')).toContainText('loaded');

/**
 * Stops the clock once the screen shows what it documents, so what is photographed no longer depends on how long the run
 * took (job times, toasts) and the image is the same on every run.
 */
async function freeze(page: Page): Promise<void> {
  await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 50);
}

const STATUS = 'pd-status-bar [data-status-item]';
const settle = async (page: Page): Promise<void> => {
  // Fades (the sidebar's scrim) are played to their end first: the image shows the resting state.
  await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))));
  await waitForStableFrame(page, { selector: STATUS });
};

/** The Runners page with a running job and the search index down; the toasts have timed out. Developer pages off. */
async function scene(page: Page, size: { readonly width: number; readonly height: number }): Promise<ShellApp> {
  const app = new ShellApp(page);
  await page.setViewportSize(size);
  await loaded(page);
  await page.getByRole('button', { name: 'Start quiet job' }).click();
  await page.getByRole('button', { name: 'Break the search index' }).click();
  await page.getByRole('button', { name: 'Notify three times' }).click();
  await page.evaluate(() => (location.hash = '#/runners'));
  await expect(page.locator('sd-runners-page')).toBeVisible();
  await expect(app.sidebar.link('Dialogs')).toHaveCount(0);
  await page.clock.runFor(14_000);
  await page.mouse.move(size.width / 2, size.height - 60);
  await expect(app.statusBar.item('pd.jobs')).not.toHaveClass(/hidden/);
  await settle(page);
  return app;
}

const pages = (name: string, size: keyof typeof WINDOW_SIZES, steps?: (app: ShellApp, page: Page) => Promise<void>): DocScreen => ({
  name: `shell-demo/pages-${name}`,
  url: `${DEMO}/#/home`,
  size: WINDOW_SIZES[size],
  ready: async (page) => {
    const app = await scene(page, WINDOW_SIZES[size]);
    await steps?.(app, page);
    await settle(page);
    await freeze(page);
  },
});

const openSidebar = async (app: ShellApp): Promise<void> => {
  await app.frame.toggle.click();
  await expect(app.frame.scrim).toBeVisible();
};

const developerOff = async (page: Page): Promise<void> => {
  await page.addInitScript(() => localStorage.setItem('sd.developer', 'off'));
};

captureDocScreens({
  ...common,
  clock: 'ticking', // the demo loads its app context when the window is idle: timers run until `freeze`
  layout: 'flat',
  appSelector: 'pd-shell-frame',
  prepare: developerOff,
  screens: [
    pages('wide', 'wide'),
    pages('rail', 'rail'),
    pages('narrow', 'narrow'),
    pages('rail-open', 'rail', (app) => openSidebar(app)),
    pages('narrow-open', 'narrow', (app) => openSidebar(app)),
    pages('health', 'wide', async (app) => {
      await app.statusBar.chip.click();
      await expect(app.notifications.services).toBeVisible();
    }),
    pages('focus-nav-selected', 'wide', async (app) => {
      await app.frame.focusBy('F6', '[data-page-link]');
      await expect(app.sidebar.link(/^Runners/)).toBeFocused();
    }),
    pages('focus-nav', 'wide', async (app, page) => {
      await app.frame.focusBy('F6', '[data-page-link]');
      await page.keyboard.press('ArrowUp');
      await expect(app.sidebar.link('Overview')).toBeFocused();
    }),
    pages('focus-toggle', 'wide', async (app, page) => {
      await page.setViewportSize(WINDOW_SIZES.narrow);
      await expect.poll(() => app.frame.sidebarMode()).toBe('overlay');
      await app.frame.focusBy('F6', '[data-testid="sidebar-toggle"]');
    }),
    pages('focus-more', 'wide', async (app, page) => {
      await page.setViewportSize(WINDOW_SIZES.narrow);
      await expect.poll(() => app.frame.sidebarMode()).toBe('overlay');
      await app.frame.focusBy('F6', '[data-testid="sidebar-toggle"]');
      await app.frame.focusBy('Shift+Tab', '[data-testid="status-more"]');
    }),
  ],
});

// The macOS desktop app, played in the browser: a Mac platform and a fake Tauri IPC that answers every call.
captureDocScreens({
  ...common,
  clock: 'ticking', // the demo loads its app context when the window is idle: timers run until `freeze`
  layout: 'flat',
  appSelector: 'pd-shell-frame',
  prepare: async (page) => {
    await developerOff(page);
    await installTauriFake(page, { platform: 'mac', otherwise: 'null' });
  },
  screens: [pages('macos', 'wide')],
});

/** Clicks on the stopped clock and lets the page's timers run a moment (the page reacts to a click on a timer). */
async function tap(page: Page, target: Locator): Promise<void> {
  await target.click();
  await page.clock.runFor(300);
}

/** Chooses an item of View > Appearance (the demo's theme, contrast and language commands). */
async function appearance(page: Page, item: string): Promise<void> {
  await tap(page, page.getByRole('menubar').getByRole('menuitem', { name: 'View', exact: true }));
  await tap(page, page.getByRole('menu', { name: 'View' }).getByRole('menuitem', { name: 'Appearance' }));
  await tap(page, page.getByRole('menu', { name: 'Appearance' }).getByRole('menuitemcheckbox', { name: item }));
  await expect(page.getByRole('menu')).toHaveCount(0);
}

/** The demo with a running job, the engine down and its details open: the whole shell in one look. */
async function theLook(page: Page): Promise<void> {
  await tap(page, page.locator('button.cmd[data-command="sd.job.start"]'));
  await tap(page, page.locator('button.cmd[data-command="sd.engine.break"]'));
  await tap(page, page.getByTestId('service-health-chip'));
  await page.clock.runFor(1_000);
  await settle(page);
}

captureDocScreens({
  ...common,
  clock: 'ticking', // the demo loads its app context when the window is idle: timers run until `freeze`
  layout: 'flat',
  appSelector: 'pd-shell-frame',
  screens: [
    {
      name: 'shell-demo/theme',
      url: `${DEMO}/`,
      ready: async (page) => {
        await loaded(page);
        await freeze(page);
        await theLook(page);
      },
    },
    {
      name: 'shell-demo/theme-high-contrast',
      url: `${DEMO}/`,
      ready: async (page) => {
        await loaded(page);
        await freeze(page);
        await appearance(page, 'High contrast');
        await expect(page.locator('html')).toHaveAttribute('data-contrast', 'more');
        await theLook(page);
      },
    },
  ],
});

captureDocScreens({
  ...common,
  clock: 'ticking', // the demo loads its app context when the window is idle: timers run until `freeze`
  layout: 'flat',
  appSelector: 'pd-shell-frame',
  prepare: async (page) => {
    await page.addInitScript(() => localStorage.setItem('paddock.language', JSON.stringify({ version: 1, language: 'pseudo' })));
  },
  screens: [
    {
      name: 'shell-demo/theme-pseudo-locale',
      url: `${DEMO}/`,
      themes: ['dark'],
      ready: async (page) => {
        await loaded(page);
        await freeze(page);
        await tap(page, page.locator('button.cmd[data-command="sd.job.start"]'));
        // The View menu (after File).
        await tap(page, page.getByRole('menubar').getByRole('menuitem').nth(1));
        await tap(page, page.getByRole('menu').first().locator('[aria-haspopup]').first());
        await settle(page);
      },
    },
  ],
});
