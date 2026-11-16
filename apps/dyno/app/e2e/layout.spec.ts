import { expect, test } from './test';
import type { Page } from '@playwright/test';
import { dialogContracts, dialogWindowUrl, expectNoButtonLabelWraps, newPinnedContext, pressMod, settledLayout, textClippedWithoutEllipsis, textCutOffWithoutTooltip } from '@decryp7/paddock-e2e';
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
  {
    name: 'network detail open',
    path: 'network',
    setup: async (page) => {
      await page.getByRole('row', { name: /142.250.74.46/ }).click();
      await expect(page.getByRole('complementary', { name: 'Details' })).toBeVisible();
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

/**
 * The states that draw no row, table or list by design (measured 2026-11-16 by counting the drawn rows, tables and path boxes of every
 * state at 900 px): their settled reading is empty. Every other state must draw a row before it counts as settled.
 */
const NO_ROWS = new Set(['why-slow', 'inventory', 'disk-map', 'map', 'power', 'lifespan', 'diagnostics', 'cpu', 'extras files']);

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
  // deliver that (a header measured meanwhile "needs 912px, has 634px"); a path is cut to the width its box has once the table has fitted
  // (the ResizeObserver behind it runs after the fit). Wait until two reads agree on the drawn rows and their widths and every table and
  // path fits: never on "every() of no boxes", which is true before any row exists (the pinned clock showed 31 of 240 Firewall visits read
  // too early).
  await settledLayout(
    page,
    () =>
      page.evaluate((rows) => {
        const drawn = Array.from(document.querySelectorAll<HTMLElement>(rows)).filter((e) => e.clientWidth > 0);
        const tables = Array.from(document.querySelectorAll<HTMLElement>('pd-table'));
        const paths = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="path-text"]'));
        const tablesFit = tables.every((t) => !t.clientWidth || t.getAttribute('data-fit-width') === String(Math.round(t.getBoundingClientRect().width)));
        const pathsFit = paths.every((b) => {
          const shown = b.querySelector<HTMLElement>('.pd-shown');
          return !b.clientWidth || !shown || shown.scrollWidth <= b.clientWidth + 1;
        });
        return {
          count: drawn.length,
          widths: [...tables, ...paths, ...drawn].map((e) => Math.round(e.getBoundingClientRect().width)),
          fits: tablesFit && pathsFit,
        };
      }, ROWS),
    { what: `the ${state.name} state`, timeout: 45_000, minCount: NO_ROWS.has(state.name) ? 0 : 1 },
  );
}

test.describe('a search field shows its whole placeholder', () => {
  test.setTimeout(300_000);
  // The network search was cut to "For example" in a 96 px field (since 942363c4e, and again before): every search field of every state shows the
  // text it invites you to type, at the narrowest window the app supports and at the width of the documentation pictures (the network details pane
  // narrows the bar there).
  for (const width of [900, 1440]) test(`at a ${width} px window, in every state`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    const cut: string[] = [];
    for (const state of STATES) {
      await openState(page, state);
      const found = await page.evaluate(() => {
        const out: string[] = [];
        const canvas = document.createElement('canvas').getContext('2d')!;
        for (const input of Array.from(document.querySelectorAll<HTMLInputElement>('dy-search-field input'))) {
          if (!input.clientWidth || !input.placeholder) continue;
          const style = getComputedStyle(input);
          canvas.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
          const room = input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
          const need = canvas.measureText(input.placeholder).width;
          if (need > room + 0.5) out.push(`"${input.placeholder}" needs ${Math.ceil(need)}px, the field has ${Math.floor(room)}px`);
        }
        return out;
      });
      if (found.length) cut.push(`${state.name}: ${found.join('; ')}`);
    }
    expect(cut).toEqual([]);
  });
});

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
        const ctx = await newPinnedContext(browser, { baseURL, viewport: { width: Math.max(width, CONTRACTS['pd.settings'].width), height } });
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
          const ctx = await newPinnedContext(browser, { baseURL, viewport: { width: c.width, height: (c as { height?: number }).height ?? height } });
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
    await dialog.getByRole('button', { name: 'Dyno', exact: true }).click();
    await expect(dialog.getByRole('button', { name: 'Forget saved maps' })).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    await expectNoButtonLabelWraps(page, `settings at ${width}`);
  });
}

/** Rule: a select shows its whole chosen text. "Refresh when opened" has the longest choice, "Never (only when I press Rescan)"; it ran past the window edge. */
for (const width of WIDTHS) {
  test(`the Refresh when opened select shows its whole text inside the window at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/?latency=0#/');
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible({ timeout: 45_000 });
    await pressMod(page, 'Mod+Comma');
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('button', { name: 'Dyno', exact: true }).dispatchEvent('click');
    const select = dialog.getByRole('combobox', { name: 'Refresh when opened' });
    await select.scrollIntoViewIfNeeded();
    await expect(select).toContainText('Never (only when I press Rescan)');
    await page.evaluate(() => document.fonts.ready);
    const m = await select.evaluate((el) => {
      const text = el.querySelector<HTMLElement>('.mat-mdc-select-min-line') ?? el;
      const r = text.getBoundingClientRect();
      return { right: r.right, vw: document.documentElement.clientWidth, needs: text.scrollWidth, has: text.clientWidth };
    });
    expect(m.right, 'the select text ends inside the window').toBeLessThanOrEqual(m.vw);
    expect(m.needs, 'the select text is not cut').toBeLessThanOrEqual(m.has + 1);
  });
}
