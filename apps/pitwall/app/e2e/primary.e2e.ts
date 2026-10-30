import { expect, test, type Page } from '@playwright/test';
import { openApp, openSettingsWindow } from './app';

// One primary action per region (UI review 2 rule 1, design guide: Buttons): on every route and tab, at most one filled
// Material button (`matButton="filled"`, the accent fill) is visible in each page region (the page header, a card or panel,
// a detail panel, a dialog, a settings section, a form), and none at all in a table row or list row. A filled danger button
// (`pd-button-danger`, confirmations only) counts as that region's primary.

interface Screen {
  readonly name: string;
  readonly hash: string;
  readonly query?: string;
  readonly act?: (page: Page) => Promise<void>;
}

const tab = (name: RegExp) => async (page: Page) => {
  await page.getByRole('tab', { name }).first().click();
};

const SCREENS: readonly Screen[] = [
  { name: 'dashboard', hash: '/' },
  { name: 'dashboard, server down', hash: '/', query: 'mock-down' },
  { name: 'dashboard, updates', hash: '/', query: 'mock-update&mock-updates' },
  { name: 'dashboard, no data', hash: '/', query: 'mock-empty' },
  { name: 'models, library', hash: '/models', query: 'mock-updates' },
  {
    name: 'models, detail',
    hash: '/models',
    act: async (page) => {
      await page.getByRole('row', { name: /Qwen3.5 9B/ }).click();
      await expect(page.getByRole('complementary', { name: 'Qwen3.5 9B' })).toBeVisible();
    },
  },
  { name: 'models, discover', hash: '/models', act: tab(/Discover/) },
  { name: 'models, presets', hash: '/models', act: tab(/Presets/) },
  {
    name: 'models, preset form',
    hash: '/models',
    act: async (page) => {
      await tab(/Presets/)(page);
      await page.getByRole('button', { name: 'New preset' }).first().click();
    },
  },
  { name: 'models, downloads', hash: '/models', act: tab(/Downloads/) },
  { name: 'models, empty', hash: '/models', query: 'mock-empty' },
  { name: 'engines', hash: '/engines' },
  { name: 'engines, installing', hash: '/engines', query: 'mock-install' },
  { name: 'apps', hash: '/apps' },
  {
    name: 'apps, detail',
    hash: '/apps',
    act: async (page) => {
      await page.getByRole('button', { name: /Spotter\s+3\.5k requests/ }).click();
      await expect(page.getByRole('heading', { name: 'Spotter', level: 2 })).toBeVisible();
    },
  },
  { name: 'apps, empty', hash: '/apps', query: 'mock-empty' },
  { name: 'requests', hash: '/requests' },
  {
    name: 'requests, detail',
    hash: '/requests',
    act: async (page) => {
      await page.getByRole('row').filter({ hasText: 'debrief-explain' }).first().click();
      await expect(page.locator('pw-request-detail')).toBeVisible();
    },
  },
  { name: 'playground', hash: '/playground' },
  { name: 'benchmarks', hash: '/benchmarks' },
  { name: 'tray', hash: '/tray' },
];

/** The region of each visible filled button: its nearest region ancestor, named for the message; a row is its own region. */
async function filledByRegion(page: Page): Promise<{ inRows: string[]; regions: Record<string, string[]> }> {
  return page.evaluate(() => {
    const REGION = 'tr, [role="row"], li, header, section, form, aside, pw-panel, pw-detail-panel, [role="dialog"], [role="alertdialog"], .card, .tray, main';
    const name = (el: Element) =>
      `${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''}${el.classList.length ? '.' + Array.from(el.classList).slice(0, 2).join('.') : ''}${el.getAttribute('aria-label') ? `[${el.getAttribute('aria-label')}]` : ''}`;
    const regions = new Map<Element, string[]>();
    const inRows: string[] = [];
    for (const b of Array.from(document.querySelectorAll<HTMLElement>('.mat-mdc-unelevated-button'))) {
      const box = b.getBoundingClientRect();
      if (box.width === 0 || box.height === 0 || getComputedStyle(b).visibility === 'hidden') continue;
      const region = b.closest(REGION);
      const label = (b.textContent ?? '').trim().replace(/\s+/g, ' ');
      // A list item is a row when its list has siblings for it: a filled button on each of several items is the bug.
      const listRow = region?.matches('li') && (region.parentElement?.children.length ?? 0) > 1;
      if (region && (region.matches('tr, [role="row"]') || listRow)) {
        inRows.push(`${label} in ${name(region)}`);
        continue;
      }
      const key = region ?? document.body;
      regions.set(key, [...(regions.get(key) ?? []), label]);
    }
    return { inRows, regions: Object.fromEntries(Array.from(regions, ([el, labels]) => [name(el), labels])) };
  });
}

test.describe('one primary action per region (UI review 2 rule 1)', () => {
  // Settings is the shell's Settings window (its own OS window, rule 17), loaded by its URL as Rust loads it.
  test('the Settings window: at most one filled button per region, none in a row', async ({ page }) => {
    await openSettingsWindow(page);
    await page.waitForLoadState('networkidle');
    const { inRows, regions } = await filledByRegion(page);
    expect(inRows, 'a filled button in a table or list row').toEqual([]);
    expect(Object.entries(regions).filter(([, labels]) => labels.length > 1), 'regions with more than one filled button').toEqual([]);
  });

  for (const s of SCREENS) {
    test(`${s.name}: at most one filled button per region, none in a row`, async ({ page }) => {
      await openApp(page, s.hash, `mock-fast&mock-static${s.query ? '&' + s.query : ''}`);
      await s.act?.(page);
      await page.waitForLoadState('networkidle');
      const { inRows, regions } = await filledByRegion(page);
      expect(inRows, 'a filled button in a table or list row').toEqual([]);
      const crowded = Object.entries(regions).filter(([, labels]) => labels.length > 1);
      expect(crowded, 'regions with more than one filled button').toEqual([]);
    });
  }
});
