import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

// The pages frame (plan S14) in the shell demo: navigation, lazy pages with their own providers, conditions and badges,
// the status bar's items, the sidebar by the layout plan at every width, F6 regions, axe in both themes.

const sidebar = (page: Page) => page.getByRole('navigation', { name: 'Pages' });
const statusBar = (page: Page) => page.getByRole('contentinfo', { name: 'Status bar' });
const toggle = (page: Page) => page.getByTestId('sidebar-toggle');
const h1 = (page: Page) => page.getByRole('heading', { level: 1 });
const frameMode = (page: Page) => page.locator('pd-shell-frame').getAttribute('data-sidebar');
const overflow = (page: Page) => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);

async function open(page: Page, hash = ''): Promise<void> {
  await page.goto(`/${hash}`);
  await expect(h1(page)).toBeVisible();
}

/** The macOS desktop app, played in the browser: a Mac platform and a fake Tauri IPC that answers every call. */
async function asMacApp(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    Object.defineProperty(navigator, 'platform', { get: () => 'MacIntel' });
    let cb = 1;
    w['__TAURI_INTERNALS__'] = {
      transformCallback: (f: unknown) => {
        const id = cb++;
        w[`_${id}`] = f;
        return id;
      },
      invoke: (cmd: string) => Promise.resolve(cmd === 'window_ready' ? { recovered: null, ui: null } : cmd === 'plugin:event|listen' ? cb++ : null),
    };
  });
}

async function axe(page: Page): Promise<string[]> {
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
  return results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`);
}

test.describe('pages frame (plan S14)', () => {
  test('the sidebar navigates between routed pages, with the current page marked, the URL and the window title', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await open(page);
    await expect(page).toHaveURL(/#\/home$/);
    await expect(h1(page)).toHaveText('Overview');
    await expect(page).toHaveTitle('Overview – Paddock demo');
    await expect(sidebar(page).getByRole('link', { name: 'Overview' })).toHaveAttribute('aria-current', 'page');
    await expect(sidebar(page).getByRole('link')).toHaveText([/^Overview$/, /^Runners/, /^Dialogs$/, /^Keyboard$/, /^About$/]);

    await sidebar(page).getByRole('link', { name: 'Keyboard' }).click();
    await expect(h1(page)).toHaveText('Keyboard');
    await expect(page).toHaveURL(/#\/keys$/);
    await expect(page).toHaveTitle('Keyboard – Paddock demo');
    await expect(sidebar(page).getByRole('link', { name: 'Keyboard' })).toHaveAttribute('aria-current', 'page');
    // Focus moves to the page, so a screen reader starts there.
    await expect(page.locator('#pd-main')).toBeFocused();
    await sidebar(page).getByRole('link', { name: 'About' }).click();
    await expect(h1(page)).toHaveText('About the shell');
    await page.goBack();
    await expect(h1(page)).toHaveText('Keyboard');
    // An unknown address goes to the first page.
    await page.goto('/#/nowhere');
    await expect(h1(page)).toHaveText('Overview');
    expect(errors).toEqual([]);
  });

  test('a page is a lazy chunk with providers of its own, new on each visit', async ({ page }) => {
    await open(page);
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await page.waitForLoadState('networkidle');
    const scripts: string[] = [];
    page.on('request', (r) => r.resourceType() === 'script' && scripts.push(r.url()));
    await sidebar(page).getByRole('link', { name: /^Runners/ }).click();
    const host = page.locator('sd-runners-page');
    await expect(host).toHaveAttribute('data-visit', /^\d+$/);
    const first = Number(await host.getAttribute('data-visit'));
    await expect(page.getByRole('table').getByRole('row')).toHaveCount(5);
    // The page's component and its providers were fetched only now.
    expect(scripts.length).toBeGreaterThan(0);
    await expect(page.getByRole('heading', { name: 'No jobs' })).toBeVisible();
    await sidebar(page).getByRole('link', { name: 'Overview' }).click();
    await sidebar(page).getByRole('link', { name: /^Runners/ }).click();
    await expect(host).toHaveAttribute('data-visit', String(first + 1));
  });

  test('a page with a condition comes and goes with it; badges follow the context', async ({ page }) => {
    await open(page);
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    const lab = sidebar(page).getByRole('link', { name: 'Lab' });
    await expect(lab).toHaveCount(0);
    // The Runners badge: the offline runner needs the user.
    await expect(sidebar(page).getByTestId('page-badge')).toHaveText(['1']);
    // Not offered: its address goes to the first page.
    await page.goto('/#/lab');
    await expect(h1(page)).toHaveText('Overview');

    await page.getByRole('button', { name: 'Say hello' }).click();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await lab.click();
    await expect(h1(page)).toHaveText('Lab');
    // The condition turns false while it is shown: the first page takes over.
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await (await menuItem(page, 'Help', 'Reset greetings')).click();
    await expect(h1(page)).toHaveText('Overview');
    await expect(lab).toHaveCount(0);
  });

  test('the status bar hosts the shell’s items and the app’s, by side and priority', async ({ page }) => {
    await open(page);
    const bar = statusBar(page);
    await expect(bar.locator('.start [data-status-item]')).toHaveCount(1);
    await expect(bar.getByTestId('runners-status')).toHaveText('3 of 4 runners online');
    await expect(bar.locator('.end [data-status-item]')).toHaveCount(3);
    await expect(bar.locator('.end [data-status-item]').evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset['statusItem']))).resolves.toEqual([
      'pd.health',
      'pd.jobs',
      'pd.notifications',
    ]);
    await expect(bar.getByRole('button', { name: 'Jobs' })).toBeVisible();
    await expect(bar.getByTestId('notifications-status')).toBeVisible();
    await page.getByRole('button', { name: 'Break the demo engine' }).click();
    await expect(bar.getByTestId('service-health-chip')).toHaveText('Demo engine: stopped');
  });

  test('narrowing the window never drops the running job from the status bar, whatever has focus (review N1)', async ({ page }) => {
    await open(page);
    await page.getByRole('button', { name: 'Break the search index' }).click();
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await expect(statusBar(page).locator('[data-status-item="pd.jobs"]')).toBeVisible();
    // Watch every frame from before the resize until well after it.
    await page.evaluate(() => {
      const w = window as unknown as { __dropped: number; __watch: boolean };
      w.__dropped = 0;
      w.__watch = true;
      const tick = () => {
        if (document.querySelector('[data-status-item="pd.jobs"]')?.classList.contains('hidden')) w.__dropped++;
        if (w.__watch) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
    await page.setViewportSize({ width: 320, height: 640 });
    await expect.poll(() => frameMode(page)).toBe('overlay');
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press('F6');
      await expect(statusBar(page).locator('[data-status-item="pd.jobs"]')).not.toHaveClass(/hidden/);
    }
    const dropped = await page.evaluate(() => {
      const w = window as unknown as { __dropped: number; __watch: boolean };
      w.__watch = false;
      return w.__dropped;
    });
    expect(dropped).toBe(0);
  });

  test('a narrow status bar shrinks items first, keeps the bell and the chip, then hides the lowest and counts them', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await open(page);
    await page.getByRole('button', { name: 'Break the demo engine' }).click();
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await page.getByRole('button', { name: 'Notify three times' }).click();
    const bar = statusBar(page);
    // Compact forms: the chip says the state, the jobs item the count; the full text is their name and tooltip.
    const chip = bar.getByTestId('service-health-chip');
    await expect(chip).toHaveText('Engine stopped');
    await expect(chip).toHaveAttribute('aria-label', 'Demo engine: stopped');
    await expect(bar.getByRole('button', { name: '1 running: Update runner' })).toHaveText('1');
    // The bell and its count are pinned.
    await expect(bar.getByTestId('notifications-status')).toBeVisible();
    await expect(bar.getByTestId('notifications-status')).toContainText('1');
    await expect(bar.getByTestId('runners-status')).toBeHidden();
    const more = bar.getByTestId('status-more');
    await expect(more).toHaveAttribute('aria-label', 'More status (1)');
    await expect(more).toHaveText('1');
    const box = await more.boundingBox();
    expect(box?.height).toBeGreaterThanOrEqual(24);
    await more.click();
    await expect(more).toHaveAttribute('aria-expanded', 'true');
    await expect(bar.getByTestId('runners-status')).toBeVisible();
    await expect(chip).toHaveText('Demo engine: stopped');
    expect(await overflow(page)).toBeLessThanOrEqual(0);
    await more.click();
    await expect(bar.getByTestId('runners-status')).toBeHidden();
    // Wide again: the full forms come back.
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect(chip).toHaveText('Demo engine: stopped');
    await expect(bar.getByTestId('runners-status')).toBeVisible();
  });

  test('the sidebar is expanded, a rail or over the page by the layout plan; the user’s collapse is kept', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await open(page);
    await expect.poll(() => frameMode(page)).toBe('expanded');
    await expect(toggle(page)).toHaveAttribute('aria-label', 'Collapse sidebar');
    await toggle(page).click();
    await expect.poll(() => frameMode(page)).toBe('rail');
    await expect(sidebar(page).getByRole('link', { name: 'Keyboard' })).toHaveAttribute('title', 'Keyboard');
    await page.reload();
    await expect(h1(page)).toBeVisible();
    await expect.poll(() => frameMode(page)).toBe('rail');
    await toggle(page).click();
    await expect.poll(() => frameMode(page)).toBe('expanded');

    await page.setViewportSize({ width: 640, height: 800 });
    await expect.poll(() => frameMode(page)).toBe('rail');
    await expect(toggle(page)).toHaveAttribute('aria-label', 'Show pages');
    await toggle(page).click();
    await expect(page.getByTestId('sidebar-scrim')).toBeVisible();
    await expect(sidebar(page).getByRole('link', { name: 'Overview' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(page.getByTestId('sidebar-scrim')).toHaveCount(0);
    await expect(toggle(page)).toBeFocused();

    await page.setViewportSize({ width: 320, height: 640 });
    await expect.poll(() => frameMode(page)).toBe('overlay');
    await expect(sidebar(page)).toBeHidden();
    await toggle(page).click();
    await expect(sidebar(page)).toBeVisible();
    await page.getByTestId('sidebar-scrim').click({ position: { x: 300, y: 300 } });
    await expect(sidebar(page)).toBeHidden();
    await toggle(page).click();
    await sidebar(page).getByRole('link', { name: 'Dialogs' }).click();
    await expect(h1(page)).toHaveText('Dialogs');
    await expect(sidebar(page)).toBeHidden();
    // Wide again: the drawer is gone and the sidebar is beside the page.
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect.poll(() => frameMode(page)).toBe('expanded');
    await expect(sidebar(page)).toBeVisible();
  });

  for (const width of [320, 360, 400, 640, 775, 776, 1024]) {
    test(`no horizontal scroll at ${width} px on any page (rule 18)`, async ({ page }) => {
      await page.setViewportSize({ width, height: 700 });
      for (const hash of ['#/home', '#/dialogs', '#/keys', '#/runners', '#/about']) {
        await open(page, hash);
        expect(await overflow(page), `${hash} at ${width}`).toBeLessThanOrEqual(0);
        const box = await page.locator('#pd-main').boundingBox();
        expect(box && box.x + box.width).toBeLessThanOrEqual(width);
      }
    });
  }

  test('F6 and Shift+F6 cycle the title bar, the sidebar, the page and the status bar', async ({ page }) => {
    await open(page);
    const region = () => page.evaluate(() => document.activeElement?.closest('[data-pd-region]')?.getAttribute('data-pd-region') ?? null);
    await page.keyboard.press('F6');
    expect(await region()).toBe('title');
    await page.keyboard.press('F6');
    expect(await region()).toBe('sidebar');
    await expect(sidebar(page).getByRole('link', { name: 'Overview' })).toBeFocused();
    await page.keyboard.press('F6');
    expect(await region()).toBe('main');
    await page.keyboard.press('F6');
    expect(await region()).toBe('status');
    await page.keyboard.press('F6');
    expect(await region()).toBe('title');
    await page.keyboard.press('Shift+F6');
    expect(await region()).toBe('status');
    // The arrow keys move between the pages.
    await sidebar(page).getByRole('link', { name: 'Overview' }).focus();
    await page.keyboard.press('ArrowDown');
    await expect(sidebar(page).getByRole('link', { name: /^Runners/ })).toBeFocused();
    await page.keyboard.press('End');
    await expect(sidebar(page).getByRole('link', { name: 'About' })).toBeFocused();
  });

  test('the macOS app has its menus in the native menu bar only, and keeps clear of the window buttons', async ({ page }) => {
    await asMacApp(page);
    await open(page);
    await expect(page.getByRole('menubar', { name: 'Application menu' })).toHaveCount(0);
    const inset = await page.locator('pd-title-bar').evaluate((el) => getComputedStyle(el).paddingLeft);
    expect(parseFloat(inset)).toBeGreaterThanOrEqual(78);
  });

  test('the browser build has the in-app menu bar', async ({ page }) => {
    await open(page);
    await expect(page.getByRole('menubar', { name: 'Application menu' })).toBeVisible();
  });

  test('the toggle keeps its glyph and the app name stays (truncated, with a tooltip) when narrow', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await open(page);
    await expect(page.getByTestId('app-name')).toBeVisible();
    await expect(page.getByTestId('app-name')).toHaveAttribute('title', 'Paddock demo');
    const glyph = () => toggle(page).locator('pd-icon').evaluate((el) => el.innerHTML);
    const narrow = await glyph();
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect.poll(() => frameMode(page)).toBe('expanded');
    expect(await glyph()).toBe(narrow);
  });

  test('over the page the sidebar is modal: the scrim covers the status bar, which is inert', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await open(page);
    await toggle(page).click();
    const scrim = await page.getByTestId('sidebar-scrim').boundingBox();
    expect(scrim && scrim.y + scrim.height).toBe(640);
    await expect(statusBar(page)).toHaveAttribute('inert', '');
    await expect(page.locator('#pd-main')).toHaveAttribute('inert', '');
    await page.keyboard.press('Escape');
    await expect(statusBar(page)).not.toHaveAttribute('inert', '');
  });

  test('a skip link leads to the page', async ({ page }) => {
    await open(page);
    await page.keyboard.press('Tab');
    const skip = page.getByRole('link', { name: 'Skip to page' });
    await expect(skip).toBeFocused();
    await skip.press('Enter');
    await expect(page.locator('#pd-main')).toBeFocused();
  });

  for (const colorScheme of ['dark', 'light'] as const) {
    test(`axe finds nothing: expanded, rail and over the page (${colorScheme})`, async ({ page }) => {
      // Without motion, so the drawer is checked at rest and not mid fade-in.
      await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' });
      await open(page);
      await page.getByRole('button', { name: 'Say hello' }).click();
      await page.getByRole('button', { name: 'Start quiet job' }).click();
      expect(await axe(page)).toEqual([]);
      await page.setViewportSize({ width: 640, height: 700 });
      await expect.poll(() => frameMode(page)).toBe('rail');
      expect(await axe(page)).toEqual([]);
      await page.setViewportSize({ width: 320, height: 640 });
      await expect.poll(() => frameMode(page)).toBe('overlay');
      await toggle(page).click();
      await expect(page.getByTestId('sidebar-scrim')).toBeVisible();
      expect(await axe(page)).toEqual([]);
    });
  }
});

/** Opens a menu of the menu bar and returns one of its items. */
async function menuItem(page: Page, menu: string, item: string) {
  await page.getByRole('menubar', { name: 'Application menu' }).getByRole('menuitem', { name: menu, exact: true }).click();
  return page.getByRole('menu', { name: menu }).getByRole('menuitem', { name: item });
}

// Screenshots for review (UI rule 20): 1440x900 @2x and narrow widths, both themes. `SCREENSHOTS=1 npx playwright test`.
test.describe('pages frame screenshots', () => {
  test.skip(!process.env['SCREENSHOTS'], 'set SCREENSHOTS=1 to rewrite docs/screenshots/shell-demo');
  test.use({ deviceScaleFactor: 2 });
  const root = join(__dirname, '..', 'docs', 'screenshots', 'shell-demo');

  /** Waits until the status bar has laid out (no item changes visibility or width over two checks). */
  async function stableStatusBar(page: Page): Promise<void> {
    const layout = () =>
      page.locator('pd-status-bar [data-status-item]').evaluateAll((els) => els.map((e) => `${e.className}:${Math.round(e.getBoundingClientRect().width)}`).join('|'));
    let last = '';
    await expect
      .poll(async () => {
        const now = await layout();
        const same = now === last;
        last = now;
        return same;
      })
      .toBe(true);
    await expect(statusBar(page).locator('[data-status-item="pd.jobs"]')).not.toHaveClass(/hidden/);
  }

  /** Presses `key` until focus is on an element matching `selector` (F6 cycles regions from wherever focus is). */
  async function focusBy(page: Page, key: string, selector: string): Promise<void> {
    for (let i = 0; i < 6; i++) {
      if (await page.evaluate((sel) => !!document.activeElement?.matches(sel), selector)) return;
      await page.keyboard.press(key);
    }
    expect(await page.evaluate((sel) => !!document.activeElement?.matches(sel), selector)).toBe(true);
  }

  /** The Runners page with a running job and the search index down; the toasts have timed out. Developer pages off. */
  async function scene(page: Page, colorScheme: 'dark' | 'light', width: number, height: number): Promise<void> {
    mkdirSync(root, { recursive: true });
    await page.addInitScript(() => localStorage.setItem('sd.developer', 'off'));
    await page.clock.install();
    await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' });
    await page.setViewportSize({ width, height });
    await open(page);
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await page.getByRole('button', { name: 'Break the search index' }).click();
    await page.getByRole('button', { name: 'Notify three times' }).click();
    await page.evaluate(() => (location.hash = '#/runners'));
    await expect(page.locator('sd-runners-page')).toBeVisible();
    await expect(sidebar(page).getByRole('link', { name: 'Dialogs' })).toHaveCount(0);
    await page.clock.runFor(14_000);
    await page.mouse.move(width / 2, height - 60);
    await stableStatusBar(page);
  }

  for (const colorScheme of ['dark', 'light'] as const) {
    for (const [name, width, height] of [
      ['wide', 1440, 900],
      ['rail', 640, 800],
      ['narrow', 320, 640],
    ] as const) {
      test(`${name} ${colorScheme}`, async ({ page }) => {
        await scene(page, colorScheme, width, height);
        await page.screenshot({ path: join(root, `pages-${name}-${colorScheme}.png`) });
        if (name !== 'wide') {
          await toggle(page).click();
          await expect(page.getByTestId('sidebar-scrim')).toBeVisible();
          await page.screenshot({ path: join(root, `pages-${name}-open-${colorScheme}.png`) });
        }
      });
    }

    test(`health popover ${colorScheme}`, async ({ page }) => {
      await scene(page, colorScheme, 1440, 900);
      await statusBar(page).getByTestId('service-health-chip').click();
      await expect(page.getByRole('region', { name: 'Services' })).toBeVisible();
      await page.screenshot({ path: join(root, `pages-health-${colorScheme}.png`) });
    });

    test(`keyboard focus ${colorScheme}`, async ({ page }) => {
      await scene(page, colorScheme, 1440, 900);
      // A focused page link that is also the selected one (F6 to the sidebar), then one that is not.
      await focusBy(page, 'F6', '[data-page-link]');
      await expect(sidebar(page).getByRole('link', { name: /^Runners/ })).toBeFocused();
      await page.screenshot({ path: join(root, `pages-focus-nav-selected-${colorScheme}.png`) });
      await page.keyboard.press('ArrowUp');
      await expect(sidebar(page).getByRole('link', { name: 'Overview' })).toBeFocused();
      await page.screenshot({ path: join(root, `pages-focus-nav-${colorScheme}.png`) });
      // Narrow: the title bar's toggle, then "…" in the status bar (the ring inside the bar).
      await page.setViewportSize({ width: 320, height: 640 });
      await expect.poll(() => frameMode(page)).toBe('overlay');
      await stableStatusBar(page);
      await focusBy(page, 'F6', '[data-testid="sidebar-toggle"]');
      await page.screenshot({ path: join(root, `pages-focus-toggle-${colorScheme}.png`) });
      await focusBy(page, 'Shift+Tab', '[data-testid="status-more"]');
      await page.screenshot({ path: join(root, `pages-focus-more-${colorScheme}.png`) });
    });

    test(`macOS app ${colorScheme}`, async ({ page }) => {
      await asMacApp(page);
      await scene(page, colorScheme, 1440, 900);
      await page.screenshot({ path: join(root, `pages-macos-${colorScheme}.png`) });
    });
  }
});
