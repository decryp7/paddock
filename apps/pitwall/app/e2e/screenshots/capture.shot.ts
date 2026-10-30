import { expect, test, type Page } from '@playwright/test';

import { resolve } from 'node:path';
import { dialogContracts, dialogWindowUrl } from '../../../../../ui/projects/paddock-e2e/src/index.ts';
import { CONTRACT_FILES } from '../dialog-fixtures';

const OUT = resolve(__dirname, '../../../docs/screenshots');
const CONTRACTS = dialogContracts(...CONTRACT_FILES.map((p) => resolve(__dirname, '../..', p)));
const WIDTH = 1440;
const HEIGHT = 900;
// A fixed clock so countdowns, "5 min ago" and chart axes are the same on every run.
const NOW = new Date('2026-10-22T14:30:00').getTime();

interface Screen {
  name: string;
  path: string;
  /** Extra mock options in the URL (`mock-updates` offers the two model updates). */
  query?: string;
  /** A virtual list that must end on a whole row: the viewport shrinks by the partial row at its bottom. */
  snapList?: string;
  /** Capture only this element (full height, whatever the window shows) instead of the page. */
  element?: string;
  /** A dialog window (its id) opened by its URL, as Rust loads it in its own OS window, instead of a page of the main window. */
  window?: string;
  /** The window's title (as Rust gives it) and its input. */
  title?: string;
  input?: unknown;
  /** The Settings window: the section (its id) scrolled to the top of the page area, so the shot starts at a section. */
  section?: string;
  /** Waits for the screen and drives it into the state to show. */
  prepare?: (page: Page) => Promise<void>;
}

const openModels = async (page: Page, tab: RegExp) => {
  await page.getByRole('tab', { name: tab }).click();
};

const SCREENS: Screen[] = [
  { name: 'dashboard', path: '/', query: 'mock-updates', prepare: (p) => expect(p.locator('pw-usage-card pw-echart canvas')).toBeVisible() },
  { name: 'models-library', path: '/models', query: 'mock-updates', prepare: (p) => expect(p.locator('pw-library-tab tbody tr').first()).toBeVisible() },
  {
    name: 'models-detail',
    path: '/models',
    prepare: async (p) => {
      await p.getByRole('row', { name: /Qwen3.5 9B/ }).click();
      const panel = p.getByRole('complementary', { name: 'Qwen3.5 9B' });
      await expect(panel).toBeVisible();
      // Pin it to the CUDA build, as a user holding one model on a known-good engine would.
      await panel.getByRole('combobox', { name: /^Engine build for Qwen3.5 9B/ }).click();
      await p.getByRole('option', { name: /^Pin to CUDA 13/ }).click();
      await expect(panel.locator('.pinned-build')).toBeVisible();
    },
  },
  {
    name: 'models-slots',
    path: '/models',
    prepare: async (p) => {
      await p.getByRole('row', { name: /Qwen3.5 9B/ }).click();
      await p.getByRole('button', { name: /^Parallel slots for Qwen3.5 9B/ }).click();
      await expect(p.locator('pw-slots-panel table.choices tbody tr').first()).toBeVisible();
    },
  },
  {
    name: 'models-discover',
    path: '/models',
    prepare: async (p) => {
      await openModels(p, /Discover/);
      await expect(p.getByText('Recommended for this machine')).toBeVisible();
      await p.getByLabel('Search Hugging Face for GGUF models').fill('gemma');
      await p.getByRole('button', { name: 'Search', exact: true }).click();
      await p.locator('pw-discover-tab .hit').first().click();
      await expect(p.getByText('Will it fit?')).toBeVisible();
    },
  },
  {
    name: 'models-presets',
    path: '/models',
    prepare: async (p) => {
      await openModels(p, /Presets/);
      await expect(p.locator('pw-presets-tab tbody tr').first()).toBeVisible();
    },
  },
  {
    name: 'models-downloads',
    path: '/models',
    prepare: async (p) => {
      await openModels(p, /Downloads/);
      await expect(p.locator('pw-downloads-list')).toBeVisible();
    },
  },
  {
    name: 'engines',
    path: '/engines',
    query: 'mock-clean',
    prepare: async (p) => {
      // What the old CPU build lacks, in words, and the Vulkan build pinned for all models.
      await p.getByRole('button', { name: 'What CPU (AVX2) b4300 supports' }).click();
      await p.getByRole('button', { name: 'Pin Vulkan b11669 for all models' }).click();
      await expect(p.getByRole('status').filter({ hasText: 'Pinned to Vulkan b11669 for all models' })).toBeVisible();
    },
  },
  {
    name: 'engines-install',
    path: '/engines',
    query: 'mock-install&mock-clean',
    // First start: llama.cpp installs the build suggested for this machine and models wait for it.
    prepare: (p) => expect(p.getByRole('status', { name: 'Installing llama.cpp' })).toBeVisible(),
  },
  {
    // The quit question in its own window, as Rust opens it: titled "Quit Pitwall", at the contract's width and fitted height.
    name: 'quit-dialog',
    path: '',
    window: 'pw.quit',
    title: 'Quit Pitwall',
    prepare: (p) => expect(p.getByRole('alertdialog', { name: 'Keep serving after you quit?' })).toBeVisible(),
  },
  { name: 'apps', path: '/apps', prepare: (p) => expect(p.locator('h1')).toBeVisible() },
  {
    name: 'apps-detail',
    path: '/apps',
    prepare: async (p) => {
      await p.getByRole('button', { name: /Spotter\s+3\.5k requests/ }).click();
      await expect(p.getByRole('heading', { name: 'Spotter', level: 2 })).toBeVisible();
      await expect(p.locator('pw-app-usage pw-echart canvas').first()).toBeVisible();
    },
  },
  {
    // Debrief opted in to cached answers: the app's Cached answers card with today's hits, misses and hit rate.
    name: 'apps-cache',
    path: '/apps',
    element: 'pw-panel.cache-panel',
    prepare: async (p) => {
      await p.getByRole('button', { name: /Debrief\s+1\.3k requests/ }).click();
      await expect(p.getByRole('heading', { name: 'Debrief', level: 2 })).toBeVisible();
      await expect(p.locator('pw-panel.cache-panel')).toContainText('on for this app');
      await expect(p.locator('pw-panel.cache-panel .cache-stats')).toContainText('Cache hits');
    },
  },
  { name: 'requests', path: '/requests', snapList: 'cdk-virtual-scroll-viewport', prepare: (p) => expect(p.getByRole('row').nth(1)).toBeVisible() },
  {
    name: 'requests-detail',
    path: '/requests',
    snapList: 'cdk-virtual-scroll-viewport',
    prepare: async (p) => {
      await p.getByRole('row').filter({ hasText: 'debrief-explain' }).first().click();
      await expect(p.locator('pw-request-detail')).toContainText('Preset asked for');
    },
  },
  {
    name: 'playground',
    path: '/playground',
    prepare: async (p) => {
      await p.getByRole('button', { name: /Explain what a KV cache is/ }).click();
      await expect(p.getByText(/keeps models hot only while they are useful/).first()).toBeVisible();
    },
  },
  {
    name: 'models-preset-cache',
    path: '/models',
    prepare: async (p) => {
      await openModels(p, /Presets/);
      await p.getByRole('row', { name: /debrief-explain/ }).getByRole('button', { name: 'Edit debrief-explain' }).click();
      await expect(p.getByRole('switch', { name: 'Cache answers' })).toBeChecked();
      await p.getByRole('switch', { name: 'Cache answers' }).scrollIntoViewIfNeeded();
    },
  },
  {
    name: 'requests-cache',
    path: '/requests',
    snapList: 'cdk-virtual-scroll-viewport',
    prepare: async (p) => {
      await p.getByRole('row').filter({ has: p.locator('.cached') }).first().click();
      await expect(p.locator('pw-request-detail')).toContainText('Cache hit');
    },
  },
  { name: 'benchmarks', path: '/benchmarks', prepare: (p) => expect(p.locator('h1')).toBeVisible() },
  {
    name: 'settings-quit',
    path: '',
    window: 'pd.settings',
    section: '#set-general',
    prepare: async (p) => {
      await p.locator('#gen-quit').click();
      await p.getByRole('option', { name: 'Ask each time' }).click();
      await expect(p.locator('#gen-quit')).toContainText('Ask each time');
    },
  },
  {
    name: 'settings-cache',
    path: '',
    window: 'pd.settings',
    section: '#set-cache',
    prepare: (p) => expect(p.locator('#set-cache .cache-stats').first()).toBeVisible(),
  },
  {
    name: 'settings-service',
    path: '',
    window: 'pd.settings',
    section: '#set-service',
    prepare: (p) => expect(p.getByTestId('service-pill')).toContainText('Running'),
  },
  {
    name: 'settings',
    path: '',
    window: 'pd.settings',
    section: '#set-model-updates',
    prepare: async (p) => {
      // Model updates switched on and checked once, as a user who opted in would see it.
      const panel = p.locator('#set-model-updates');
      await expect(panel).toBeVisible();
      await panel.getByRole('switch', { name: 'Check for model updates' }).click();
      await panel.getByRole('button', { name: 'Check now' }).click();
      await expect(panel).toContainText('Last checked');
    },
  },
];

const ONLY = process.env['SHOT_ONLY']?.split(',');

async function open(page: Page, path: string, theme: 'dark' | 'light', query = '') {
  await page.clock.install({ time: NOW });
  // The theme follows the system (the shell's Appearance default), emulated here.
  await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
  await page.goto(`/?mock-static&mock-fast${query ? '&' + query : ''}#${path}`);
}

/** Grows the viewport (never the width) until the page scroller hides nothing, so no card is sliced at the bottom. */
async function fitHeight(page: Page): Promise<number> {
  let height = HEIGHT;
  for (let i = 0; i < 3; i++) {
    const extra = await page.evaluate(() => {
      // The page scrolls in <main>; lists with their own scroller (events, tables) keep it.
      const main = document.querySelector('main');
      return main ? main.scrollHeight - main.clientHeight : 0;
    });
    if (extra <= 1) break;
    height += Math.ceil(extra);
    await page.setViewportSize({ width: WIDTH, height });
    await page.waitForTimeout(250);
  }
  return height;
}

for (const theme of ['dark', 'light'] as const) {
  for (const s of SCREENS.filter((x) => !ONLY || ONLY.includes(x.name))) {
    test(`${s.name} ${theme}`, async ({ page }) => {
      if (s.window) {
        // A window at its own size, as the shell opens it: Settings at the contract's 880x700, a dialog at its contract's
        // width and the height it fits itself to.
        const contract = CONTRACTS[s.window]!;
        const fixed = contract.kind === 'tool';
        await page.clock.install({ time: NOW });
        await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
        await page.setViewportSize({ width: contract.width, height: fixed ? (contract.height ?? 700) : 810 });
        await page.goto(dialogWindowUrl(s.window, s.input ?? {}, { title: s.title ?? 'Settings', query: `?mock-static&mock-fast${s.query ? '&' + s.query : ''}` }));
        await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
        await s.prepare?.(page);
        // Long enough for the window's own confirmations ("Checking every week.") to go: no toast in a documentation shot.
        await page.clock.runFor(12_000);
        for (const close of await page.getByRole('button', { name: /^(Dismiss|Close notification)/ }).all()) await close.click();
        await page.mouse.move(2, 2);
        if (!fixed) {
          const asked = await page.evaluate(() => ({ h: Number(document.documentElement.dataset['fittedHeight'] ?? '0'), min: document.documentElement.dataset['minContent'] ?? '0x0' }));
          await page.setViewportSize({ width: contract.width, height: Math.max(asked.h, Number(asked.min.split('x')[1] ?? '0')) });
        }
        // The shot starts at the top of a section, never mid-card.
        if (s.section) await page.locator(s.section).evaluate((e) => e.scrollIntoView({ block: 'start' }));
        await page.waitForTimeout(600);
        await page.screenshot({ path: `${OUT}/${s.name}-${theme}.png` });
        return;
      }
      await open(page, s.path, theme, s.query);
      await expect(page.locator('h1').first()).toBeVisible();
      await s.prepare?.(page);
      await page.clock.runFor(1500);
      // No toast, tooltip or hover state in a documentation shot.
      for (const close of await page.locator('pd-notification-host [data-testid="notification-dismiss"]').all()) await close.click();
      await page.mouse.move(WIDTH - 2, 2);
      // Focusing a control at a card's edge can scroll a table sideways, which slices its first column in the shot.
      await page.evaluate(() => document.querySelectorAll('*').forEach((e) => e.scrollLeft > 0 && (e.scrollLeft = 0)));
      await page.waitForTimeout(600);
      if (s.element) {
        // The page scrolls inside <main>: make the window tall enough to show the whole element, then shoot it.
        const box = await page.locator(s.element).boundingBox();
        await page.setViewportSize({ width: WIDTH, height: Math.ceil((box?.height ?? HEIGHT) + 160) });
        await page.waitForTimeout(300);
        await page.locator(s.element).scrollIntoViewIfNeeded();
        await page.locator(s.element).screenshot({ path: `${OUT}/${s.name}-${theme}.png` });
        return;
      }
      await fitHeight(page);
      if (s.snapList) {
        const rest = await page.evaluate((sel) => {
          const vp = document.querySelector<HTMLElement>(sel);
          const row = vp?.querySelector<HTMLElement>('[role="row"]');
          return vp && row ? vp.clientHeight % row.offsetHeight : 0;
        }, s.snapList);
        if (rest > 0) await page.setViewportSize({ width: WIDTH, height: (page.viewportSize()?.height ?? HEIGHT) - rest });
        await page.waitForTimeout(250);
      }
      await page.screenshot({ path: `${OUT}/${s.name}-${theme}.png` });
    });
  }

  if (!ONLY || ONLY.includes('tray')) {
    // The tray popover window (`#/tray`, 360x520 logical) as the tray opens it, with both banners (a pairing and model
    // updates) and the loaded models summarised under them.
    test(`tray ${theme}`, async ({ page }) => {
      await page.setViewportSize({ width: 360, height: 520 });
      await open(page, '/tray', theme, 'mock-updates');
      await expect(page.getByText(/model updates are available/)).toBeVisible();
      await expect(page.locator('pw-hot-models .more')).toContainText('more loaded');
      const tray = page.locator('.tray');
      await expect(tray).toBeVisible();
      await page.clock.runFor(1500);
      await page.waitForTimeout(400);
      const box = await tray.boundingBox();
      expect(box?.width).toBe(360);
      expect(box?.height).toBe(520);
      expect(await tray.evaluate((e) => e.scrollHeight <= e.clientHeight)).toBe(true);
      await tray.screenshot({ path: `${OUT}/tray-${theme}.png` });
    });
  }
}
