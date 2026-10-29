import { expect, test, type Page } from '@playwright/test';
import { dialogContracts, dialogWindowUrl, textClippedWithoutEllipsis } from '@decryp7/paddock-e2e';

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

test.describe('rows never clip their content, and text that is cut says so with an ellipsis', () => {
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
        const found = [...new Set([...(await clipped(page)), ...(await textClippedWithoutEllipsis(page))])];
        if (found.length) problems[state.name] = found;
      }
      expect(problems).toEqual({});
    });
  }
});

/**
 * Nothing is cut, silently: the shell's `.pd-page` hides sideways overflow (`overflow-x: hidden`), so a track that is too wide
 * simply loses its right edge. For every page (and every Settings page) at the supported sizes this fails when the page content
 * is wider than the page, when a text sits past the window edge, or when a text is cut by a clipping or scrolling ancestor
 * without `text-overflow: ellipsis`.
 */
const SIZES = [
  { width: 900, height: 600 },
  { width: 1024, height: 800 },
  { width: 1280, height: 800 },
];
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts('src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`));

async function cutOff(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const out: string[] = [];
    const vw = document.documentElement.clientWidth;
    const name = (el: Element) => `${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''}.${String((el as HTMLElement).className?.toString?.() ?? '').trim().split(/\s+/).slice(0, 3).join('.')}`;
    if (document.documentElement.scrollWidth > vw + 1) out.push(`document scrolls sideways: ${document.documentElement.scrollWidth} > ${vw}`);
    for (const el of Array.from(document.querySelectorAll<HTMLElement>('.pd-page'))) {
      if (el.scrollWidth > el.clientWidth + 1) out.push(`${name(el)} content ${el.scrollWidth}px wider than the page ${el.clientWidth}px`);
    }
    const seen = new Set<string>();
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const text = (n.textContent ?? '').trim();
      const parent = n.parentElement;
      if (!text || !parent) continue;
      const ps = getComputedStyle(parent);
      if (ps.visibility === 'hidden' || ps.display === 'none') continue;
      const r = document.createRange();
      r.selectNodeContents(n);
      const tr = r.getBoundingClientRect();
      if (tr.width === 0 || tr.height === 0) continue;
      // Truncation with an ellipsis is a decision; only text that is neither cut short nor wrapped is a problem.
      let ellipsised = false;
      for (let e: Element | null = parent; e && e !== document.body; e = e.parentElement) {
        if (getComputedStyle(e).textOverflow === 'ellipsis') { ellipsised = true; break; }
      }
      if (!ellipsised && tr.right > vw + 1 && tr.left < vw) {
        const k = `edge:${name(parent)}`;
        if (!seen.has(k)) { seen.add(k); out.push(`text past the window edge (${Math.round(tr.right)} > ${vw}): "${text.slice(0, 50)}" in ${name(parent)}`); }
      }
      for (let a: Element | null = parent; a && a !== document.body; a = a.parentElement) {
        const s = getComputedStyle(a);
        if (s.overflowX === 'visible') continue;
        const ar = a.getBoundingClientRect();
        if (ar.width <= 2 || ar.height <= 2) break; // visually hidden text
        if (tr.right > ar.right + 1 || tr.left < ar.left - 1) {
          let ellipsis = false;
          for (let e: Element | null = parent; e; e = e.parentElement) {
            if (getComputedStyle(e).textOverflow === 'ellipsis') { ellipsis = true; break; }
            if (e === a) break;
          }
          if (!ellipsis) {
            const k = `cut:${name(parent)}`;
            if (!seen.has(k)) { seen.add(k); out.push(`cut without ellipsis: "${text.slice(0, 50)}" in ${name(parent)} (by ${name(a)})`); }
          }
        }
        break;
      }
    }
    return out;
  });
}

test.describe('nothing is cut off', () => {
  test.setTimeout(300_000);
  for (const { width, height } of SIZES) {
    test(`pages at ${width}x${height}`, async ({ page }) => {
      await page.setViewportSize({ width, height });
      const problems: Record<string, string[]> = {};
      for (const state of STATES.filter((s) => !s.setup)) {
        await page.goto(`/?latency=0#/${state.path}`);
        await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
        await page.locator('.mat-mdc-progress-bar, [aria-busy="true"]').first().waitFor({ state: 'detached', timeout: 5000 }).catch(() => undefined);
        await page.evaluate(() => document.fonts.ready);
        const found = await cutOff(page);
        if (found.length) problems[state.name] = found;
      }
      expect(problems).toEqual({});
    });

    test(`settings pages at ${width}x${height}`, async ({ browser, baseURL }) => {
      const ctx = await browser.newContext({ baseURL, viewport: { width: Math.max(width, CONTRACTS['pd.settings'].width), height } });
      const page = await ctx.newPage();
      await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      const items = page.locator('nav.pd-nav a, nav.pd-nav button');
      const labels = await items.allInnerTexts();
      expect(labels.length, 'the settings pages are listed').toBeGreaterThan(0);
      const problems: Record<string, string[]> = {};
      for (let i = 0; i < labels.length; i++) {
        await items.nth(i).click();
        await page.evaluate(() => document.fonts.ready);
        const found = await cutOff(page);
        if (found.length) problems[labels[i].trim()] = found;
      }
      await ctx.close();
      expect(problems).toEqual({});
    });
  }
});
