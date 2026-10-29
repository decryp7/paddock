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

test.describe('rows never clip their content', () => {
  test.setTimeout(300_000);
  for (const width of WIDTHS) {
    test(`at a ${width} px window`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      const problems: Record<string, string[]> = {};
      for (const path of PAGES) {
        await page.goto(`/?latency=0#/${path}`);
        await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
        await page.locator('.mat-mdc-progress-bar, [aria-busy="true"]').first().waitFor({ state: 'detached', timeout: 5000 }).catch(() => undefined);
        const found = [...new Set(await clipped(page))];
        if (found.length) problems[path || 'dashboard'] = found;
      }
      // The alarms table has a second view.
      await page.goto(`/?latency=0#/alarms`);
      await page.getByRole('tab', { name: 'Rules' }).click();
      const rules = [...new Set(await clipped(page))];
      if (rules.length) problems['alarms rules'] = rules;
      expect(problems).toEqual({});
    });
  }
});
