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
    await expect(page).toHaveTitle('Overview – Shell demo');
    await expect(sidebar(page).getByRole('link', { name: 'Overview' })).toHaveAttribute('aria-current', 'page');
    await expect(sidebar(page).getByRole('link')).toHaveText(['Overview', 'Dialogs', 'Keyboard', 'Activity', 'About']);

    await sidebar(page).getByRole('link', { name: 'Keyboard' }).click();
    await expect(h1(page)).toHaveText('Keyboard');
    await expect(page).toHaveURL(/#\/keys$/);
    await expect(page).toHaveTitle('Keyboard – Shell demo');
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
    await sidebar(page).getByRole('link', { name: 'Activity' }).click();
    await expect(page.getByTestId('activity-visit')).toHaveText(/^Visit \d+ of this page$/);
    const first = Number((await page.getByTestId('activity-visit').textContent())?.match(/\d+/)?.[0]);
    // The page's component and its providers were fetched only now.
    expect(scripts.length).toBeGreaterThan(0);
    await expect(page.getByRole('heading', { name: 'No jobs' })).toBeVisible();
    await sidebar(page).getByRole('link', { name: 'Overview' }).click();
    await sidebar(page).getByRole('link', { name: 'Activity' }).click();
    await expect(page.getByTestId('activity-visit')).toHaveText(`Visit ${first + 1} of this page`);
  });

  test('a page with a condition comes and goes with it; badges follow the context', async ({ page }) => {
    await open(page);
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    const lab = sidebar(page).getByRole('link', { name: 'Lab' });
    await expect(lab).toHaveCount(0);
    await expect(sidebar(page).getByTestId('page-badge')).toHaveCount(0);
    // Not offered: its address goes to the first page.
    await page.goto('/#/lab');
    await expect(h1(page)).toHaveText('Overview');

    await page.getByRole('button', { name: 'Say hello' }).click();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(sidebar(page).getByRole('link', { name: /^Activity/ }).getByTestId('page-badge')).toHaveText('2');
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
    await expect(bar.getByTestId('greetings-status')).toHaveText('Greetings: 0');
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
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(bar.getByTestId('greetings-status')).toHaveText('Greetings: 1');
  });

  test('a narrow status bar hides the lowest priority first and shows all on request', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await open(page);
    await page.getByRole('button', { name: 'Break the demo engine' }).click();
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    const bar = statusBar(page);
    await expect(bar.getByTestId('service-health-chip')).toBeVisible();
    await expect(bar.getByTestId('greetings-status')).toBeHidden();
    const more = bar.getByTestId('status-more');
    await expect(more).toHaveAttribute('aria-label', /^Show all status items \(\d+ hidden\)$/);
    await more.click();
    await expect(more).toHaveAttribute('aria-expanded', 'true');
    await expect(bar.getByTestId('greetings-status')).toBeVisible();
    expect(await overflow(page)).toBeLessThanOrEqual(0);
    await more.click();
    await expect(bar.getByTestId('greetings-status')).toBeHidden();
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
      for (const hash of ['#/home', '#/dialogs', '#/keys', '#/activity', '#/about']) {
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
    await expect(sidebar(page).getByRole('link', { name: 'Dialogs' })).toBeFocused();
    await page.keyboard.press('End');
    await expect(sidebar(page).getByRole('link', { name: 'About' })).toBeFocused();
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

  for (const colorScheme of ['dark', 'light'] as const) {
    for (const [name, width, height] of [
      ['wide', 1440, 900],
      ['rail', 640, 800],
      ['narrow', 320, 640],
    ] as const) {
      test(`${name} ${colorScheme}`, async ({ page }) => {
        mkdirSync(root, { recursive: true });
        await page.clock.install();
        await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' });
        await page.setViewportSize({ width, height });
        await open(page);
        await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
        await page.getByRole('button', { name: 'Say hello' }).click();
        await page.getByRole('button', { name: 'Say hello' }).click();
        await page.getByRole('button', { name: 'Start quiet job' }).click();
        await page.getByRole('button', { name: 'Break the search index' }).click();
        // The greetings' toasts time out; the quiet job keeps running.
        await page.clock.runFor(10_000);
        await page.evaluate(() => (location.hash = '#/activity'));
        await expect(page.getByTestId('activity-visit')).toBeVisible();
        if (name === 'narrow') await expect.poll(() => frameMode(page)).toBe('overlay');
        await page.mouse.move(0, height - 1);
        await page.screenshot({ path: join(root, `pages-${name}-${colorScheme}.png`) });
        if (name !== 'wide') {
          await toggle(page).click();
          await expect(page.getByTestId('sidebar-scrim')).toBeVisible();
          await page.screenshot({ path: join(root, `pages-${name}-open-${colorScheme}.png`) });
        }
      });
    }
  }
});
