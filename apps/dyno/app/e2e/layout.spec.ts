import { expect, test, type Page } from '@playwright/test';
import { dialogContracts, dialogWindowUrl, expectNoButtonLabelWraps, pressMod, textClippedWithoutEllipsis, textCutOffWithoutTooltip } from '@decryp7/paddock-e2e';
import { FIXTURES } from './dialog-fixtures';
import { NAV } from '../src/app/shell/nav';
import { EXTRAS_TABS } from '../src/app/features/extras/extras-model';

/**
 * No table row of any Dyno page clips its own content at the window sizes the app supports (900 px is the window minimum in
 * tauri.conf.json): a row whose columns need more room than it has (`scrollWidth > clientWidth`) hides what overflows, such as the
 * row actions, under the table's `overflow: clip`. Every page, every row of every table and list, three widths.
 */
// Every screen of the sidebar (so a new one is measured without editing this list), and the cpu screen the sidebar does not list.
const PAGES = [...new Set([...NAV.map((n) => n.path.replace(/^\//, '')), 'cpu'])];
const WIDTHS = [900, 1024, 1280];
const ROWS = 'tr, [role="row"], .row, .item, li.win, th, [role="columnheader"]';

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
const EXTRAS = EXTRAS_TABS.map((t) => t.id);

const STATES: State[] = [
  ...PAGES.map((p): State => ({ name: p || 'dashboard', path: p })),
  ...EXTRAS.map((t): State => ({ name: `extras ${t}`, path: `extras?tab=${t}` })),
  { name: 'disk map table', path: 'disk-map', setup: async (page) => {
      await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
      await page.getByRole('radio', { name: 'Table' }).click();
      await expect(page.getByRole('table', { name: /Contents of Macintosh HD/ })).toBeVisible();
    },
  },
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
  {
    name: 'network listening, ungrouped',
    path: 'network',
    setup: async (page) => {
      await page.getByRole('radio', { name: 'Listening' }).click();
      await page.getByRole('radio', { name: 'None' }).click();
      await expect(page.getByRole('columnheader', { name: 'Program' })).toBeVisible();
    },
  },
];

/** Opens a state and waits until it is drawn: its heading is there and no skeleton or indeterminate progress bar is left. */
async function openState(page: Page, state: State): Promise<void> {
  await page.goto(`/?latency=0#/${state.path}`);
  await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
  await expect(page.locator('[aria-busy="true"], .mdc-linear-progress--indeterminate')).toHaveCount(0);
  if (state.setup) {
    await state.setup(page);
    await page.mouse.move(0, 0);
  }
  await page.evaluate(() => document.fonts.ready);
  // A pd-table draws its columns for a default width (1024) until its ResizeObserver has told it the real one, and WebKit can be slow to
  // deliver that (a header measured meanwhile "needs 912px, has 634px"): wait until every drawn table has fitted its columns to its width.
  await page.waitForFunction(() =>
    Array.from(document.querySelectorAll<HTMLElement>('pd-table')).every((t) => !t.clientWidth || t.getAttribute('data-fit-width') === String(Math.round(t.getBoundingClientRect().width))),
  );
}

test.describe('rows never clip their content, and text that is cut says so with an ellipsis', () => {
  test.setTimeout(300_000);
  for (const width of WIDTHS) {
    test(`at a ${width} px window`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 });
      const problems: Record<string, string[]> = {};
      for (const state of STATES) {
        await openState(page, state);
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
  const wide = await page.evaluate(() => {
    const out: string[] = [];
    const vw = document.documentElement.clientWidth;
    if (document.documentElement.scrollWidth > vw + 1) out.push(`document scrolls sideways: ${document.documentElement.scrollWidth} > ${vw}`);
    for (const el of Array.from(document.querySelectorAll<HTMLElement>('.pd-page'))) {
      if (el.scrollWidth > el.clientWidth + 1) out.push(`${el.tagName.toLowerCase()}.pd-page content ${el.scrollWidth}px wider than the page ${el.clientWidth}px`);
    }
    return out;
  });
  // The shared rule (paddock-e2e): text beyond its box or the window, or cut by its own box, unless an accepted ellipsis.
  return [...wide, ...(await textCutOffWithoutTooltip(page)), ...(await textClippedWithoutEllipsis(page))];
}

test.describe('nothing is cut off', () => {
  test.setTimeout(300_000);
  for (const { width, height } of SIZES) {
    test(`pages and their other states at ${width}x${height}`, async ({ page }) => {
      await page.setViewportSize({ width, height });
      const problems: Record<string, string[]> = {};
      const plain: State[] = PAGES.map((p) => ({ name: p || 'dashboard', path: p }));
      for (const state of [...plain, ...STATES.filter((s) => s.setup || s.path.includes('?'))]) {
        await openState(page, state);
        const found = await cutOff(page);
        if (found.length) problems[state.name] = found;
      }
      expect(problems).toEqual({});
    });

    for (const advanced of [false, true]) {
      test(`settings pages at ${width}x${height}${advanced ? ' with the advanced fields' : ''}`, async ({ browser, baseURL }) => {
        const ctx = await browser.newContext({ baseURL, viewport: { width: Math.max(width, CONTRACTS['pd.settings'].width), height } });
        const page = await ctx.newPage();
        await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings' }));
        await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
        if (advanced) {
          const toggle = page.getByRole('switch', { name: 'Show advanced' });
          await toggle.click();
          await expect(toggle).toHaveAttribute('aria-checked', 'true');
        }
        const items = page.locator('nav.pd-nav a, nav.pd-nav button');
        const labels = (await items.allInnerTexts()).map((l) => l.trim());
        expect(labels.length, 'the settings pages are listed').toBeGreaterThan(0);
        const problems: Record<string, string[]> = {};
        for (let i = 0; i < labels.length; i++) {
          await items.nth(i).click();
          await expect(items.nth(i)).toHaveAttribute('aria-current', 'page');
          await expect(page.getByRole('heading', { name: labels[i], exact: true }).first()).toBeVisible();
          await page.evaluate(() => document.fonts.ready);
          const found = await cutOff(page);
          if (found.length) problems[labels[i]] = found;
        }
        await ctx.close();
        expect(problems).toEqual({});
      });
    }

    test(`dialog windows at their contract width, window ${width}x${height}`, async ({ browser, baseURL }) => {
      const problems: Record<string, string[]> = {};
      for (const [id, c] of Object.entries(CONTRACTS)) {
        for (const [n, input] of (FIXTURES[id] ?? [{}]).entries()) {
          const ctx = await browser.newContext({ baseURL, viewport: { width: c.width, height: (c as { height?: number }).height ?? height } });
          const page = await ctx.newPage();
          await page.goto(dialogWindowUrl(id, input, { title: id }));
          await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
          const found = await cutOff(page);
          if (found.length) problems[`${id} #${n + 1} (${c.width}px)`] = found;
          await ctx.close();
        }
      }
      expect(problems).toEqual({});
    });
  }
});

/** Rule: no button label wraps. The disk-map settings put "Forget saved maps" beside a long text; the button keeps its label on one line. */
for (const width of WIDTHS) {
  test(`no button label wraps on the settings page at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/?latency=0#/');
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible({ timeout: 45_000 });
    await pressMod(page, 'Mod+Comma');
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('button', { name: 'Dyno', exact: true }).dispatchEvent('click');
    await expect(dialog.getByRole('button', { name: 'Forget saved maps' })).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    await expectNoButtonLabelWraps(page, `settings at ${width}`);
  });
}
