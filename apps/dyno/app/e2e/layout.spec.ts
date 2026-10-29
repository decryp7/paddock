import { expect, test, type Page } from '@playwright/test';

/**
 * No table row of any Dyno page clips its own content at the window sizes the app supports (900 px is the window minimum in
 * tauri.conf.json): a row whose columns need more room than it has (`scrollWidth > clientWidth`) hides what overflows, such as the
 * row actions, under the table's `overflow: clip`. Every page, every row of every table and list, three widths.
 */
const PAGES = ['', 'processes', 'why-slow', 'throttling', 'issues', 'inventory', 'network', 'counters', 'benchmarks', 'alarms', 'reports', 'recorder', 'health', 'backup', 'extras', 'lifespan', 'diagnostics', 'cpu'];
const WIDTHS = [900, 1024, 1280];
const ROWS = 'tr, [role="row"], .row, .item, li.win';

async function clipped(page: Page): Promise<string[]> {
  return page.evaluate((selector) => {
    const out: string[] = [];
    for (const el of Array.from(document.querySelectorAll<HTMLElement>(selector))) {
      if (el.clientWidth === 0) continue; // not drawn (a collapsed or hidden row)
      if (el.scrollWidth > el.clientWidth + 1) {
        const name = `${el.tagName.toLowerCase()}.${String(el.className).trim().split(/\s+/).join('.')}`;
        out.push(`${name}: needs ${el.scrollWidth}px, has ${el.clientWidth}px`);
      }
    }
    return out;
  }, ROWS);
}

/** A state of the app to measure: a page, or a page with its detail panel open, a second view or a tab. */
type State = { name: string; path: string; setup?: (page: Page) => Promise<void> };
const EXTRAS = ['changes', 'sleep', 'boot', 'apps', 'gpu', 'files', 'inventory', 'maintenance'];

const STATES: State[] = [
  ...PAGES.map((p): State => ({ name: p || 'dashboard', path: p })),
  ...EXTRAS.map((t): State => ({ name: `extras ${t}`, path: `extras?tab=${t}` })),
  { name: 'alarms rules', path: 'alarms', setup: (page) => page.getByRole('tab', { name: 'Rules' }).click() },
  { name: 'alarms detail open', path: 'alarms', setup: async (page) => {
      await page.getByRole('grid', { name: 'Open alarms' }).getByRole('row').nth(1).click();
      await expect(page.locator('dy-alarm-detail')).toBeVisible();
    },
  },
  { name: 'processes detail open', path: 'processes', setup: async (page) => {
      await page.getByRole('button', { name: 'Expand Rust build (cargo)' }).click();
      await page.locator('[role=row][data-key^="p"]').first().click();
      await expect(page.getByRole('complementary', { name: 'Process details' })).toBeVisible();
    },
  },
  { name: 'issues detail open', path: 'issues', setup: async (page) => {
      await page.getByRole('treegrid', { name: 'Issues', exact: true }).getByRole('row', { name: /Safari/ }).first().click();
      await expect(page.getByRole('complementary', { name: 'Issue details' })).toBeVisible();
    },
  },
  { name: 'issues new and unusual', path: 'issues', setup: (page) => page.getByRole('radio', { name: /New and unusual/ }).click() },
  {
    name: 'counters charted table',
    path: 'counters',
    setup: async (page) => {
      await page.getByRole('searchbox', { name: 'Search counters' }).fill('bytes');
      const results = page.getByRole('list', { name: 'Search results' });
      for (const n of [/^Chart Bytes Received \(en0\)/, /^Chart Bytes Sent \(en0\)/]) await results.getByRole('button', { name: n }).click();
      await expect(page.getByRole('grid', { name: 'Charted counters' })).toBeVisible();
    },
  },
  { name: 'network listening', path: 'network', setup: (page) => page.getByRole('radio', { name: 'Listening' }).click() },
];

test.describe('rows never clip their content', () => {
  test.setTimeout(300_000);
  for (const width of WIDTHS) {
    test(`at a ${width} px window`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      const problems: Record<string, string[]> = {};
      for (const state of STATES) {
        await page.goto(`/?latency=0#/${state.path}`);
        await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
        await page.locator('.mat-mdc-progress-bar, [aria-busy="true"]').first().waitFor({ state: 'detached', timeout: 5000 }).catch(() => undefined);
        if (state.setup) {
          await state.setup(page);
          await page.mouse.move(0, 0);
        }
        const found = [...new Set(await clipped(page))];
        if (found.length) problems[state.name] = found;
      }
      expect(problems).toEqual({});
    });
  }
});
