import { expect, test, type Page } from '@playwright/test';
import { dialogContracts, dialogWindowUrl } from '../../../../ui/projects/paddock-e2e/src/index.ts';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { openApp } from './app';
import { CONTRACT_FILES, INPUTS } from './dialog-fixtures';

// Layout at the supported window widths (rule 18): for every route of the main window, no element wider than its container, no
// horizontal scroll (of the page or of a panel inside it) and no text clipped without an ellipsis. The measurement is the
// independent reviewer's (Pitwall 1.1 review); visually hidden text (`sr-only`) is clipped on purpose and is not reported.
//
// layout.baseline.json holds the exact lines measured on the tree this spec was added to, per project and state: a line
// that is not in the baseline fails the test (a new problem), and a baseline line that is gone fails it too (a fix: delete the
// entry). WP-2/WP-3 own the fixes. Re-record with `PW_RECORD_BASELINE=1 npx playwright test e2e/layout.e2e.ts
// --project=chromium` (then review the diff). Do not record a new screen's problems: fix the layout.
//
// Each dialog window (the Settings window, Pitwall's four dialogs, the shared confirmation, About, Updates and What's new) is
// measured as Rust opens it: by its URL, at its contract's width (a dialog window has one width, not the main window's three).

const WIDTHS = [900, 1024, 1280] as const;

interface State {
  readonly id: string;
  readonly hash: string;
  readonly query?: string;
  readonly width?: number;
  readonly height?: number;
  readonly act?: (page: Page) => Promise<void>;
}

const openTab = (name: RegExp) => async (page: Page) => {
  await page.getByRole('tab', { name }).first().click();
};

const STATES: readonly State[] = [
  { id: 'dashboard', hash: '/' },
  { id: 'dashboard-down', hash: '/', query: 'mock-down' },
  { id: 'dashboard-update', hash: '/', query: 'mock-update&mock-updates&mock-startfailed' },
  { id: 'models-library', hash: '/models' },
  { id: 'models-discover', hash: '/models', act: openTab(/Discover/) },
  {
    id: 'models-discover-search',
    hash: '/models',
    act: async (page) => {
      await openTab(/Discover/)(page);
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      await page.waitForLoadState('networkidle');
    },
  },
  { id: 'models-presets', hash: '/models', act: openTab(/Presets/) },
  {
    id: 'models-preset-form',
    hash: '/models',
    act: async (page) => {
      await openTab(/Presets/)(page);
      await page.getByRole('button', { name: 'New preset' }).first().click();
    },
  },
  {
    id: 'models-slots',
    hash: '/models',
    act: async (page) => {
      await page.getByRole('row', { name: /Qwen3.5 9B/ }).click();
      await page.getByRole('button', { name: /^Parallel slots for Qwen3.5 9B/ }).click();
    },
  },
  { id: 'engines', hash: '/engines' },
  { id: 'engines-installing', hash: '/engines', query: 'mock-install' },
  { id: 'apps', hash: '/apps' },
  {
    id: 'apps-detail',
    hash: '/apps',
    act: async (page) => {
      await page.getByRole('button', { name: /Spotter/ }).first().click();
    },
  },
  { id: 'requests', hash: '/requests' },
  {
    id: 'requests-detail',
    hash: '/requests',
    act: async (page) => {
      await page.getByRole('row').nth(1).click();
    },
  },
  { id: 'playground', hash: '/playground' },
  { id: 'benchmarks', hash: '/benchmarks' },
  // The tray popover is its own small window, not a resizable one: measured once at its own size.
  { id: 'tray', hash: '/tray', width: 360, height: 560 },
];

interface Report {
  readonly spill: readonly string[];
  readonly scroll: readonly string[];
  readonly clipped: readonly string[];
}

/** Runs in the page: the three kinds of layout problem, each as a short readable line. */
function measure(rootSelector: string | null): { spill: string[]; scroll: string[]; clipped: string[] } {
  const out = { spill: [] as string[], scroll: [] as string[], clipped: [] as string[] };
  const name = (e: Element): string => {
    const parts: string[] = [];
    for (let n: Element | null = e; n && n !== document.body && parts.length < 4; n = n.parentElement) {
      let s = n.tagName.toLowerCase();
      if (n.id) s += `#${n.id}`;
      else if (n.classList.length) s += `.${[...n.classList].slice(0, 2).join('.')}`;
      parts.unshift(s);
    }
    return parts.join(' > ');
  };
  const ownText = (e: Element): string =>
    [...e.childNodes]
      .filter((c) => c.nodeType === 3)
      .map((c) => (c.textContent ?? '').trim())
      .join(' ')
      .slice(0, 50);
  const root = rootSelector ? document.querySelector(rootSelector) : document.body;
  if (!root) return { spill: [], scroll: [], clipped: ['no element matches ' + rootSelector] };
  const pageScroll = document.documentElement.scrollWidth - innerWidth;
  if (!rootSelector && pageScroll > 0) out.scroll.push(`page scrolls sideways by ${pageScroll}px`);
  for (const e of Array.from(root.querySelectorAll('*'))) {
    const cs = getComputedStyle(e);
    if (cs.display === 'none' || cs.visibility === 'hidden') continue;
    const r = e.getBoundingClientRect();
    if (r.width === 0 || r.height === 0 || e.closest('svg, canvas')) continue;
    const parent = e.parentElement;
    if (parent && cs.position !== 'absolute' && cs.position !== 'fixed') {
      const pr = parent.getBoundingClientRect();
      if (r.right > pr.right + 2 && r.left >= pr.left - 2 && getComputedStyle(parent).overflowX === 'visible' && pr.width > 0) {
        out.spill.push(`${name(e)} is ${Math.round(r.width)}px in a ${Math.round(pr.width)}px container (by ${Math.round(r.right - pr.right)}px) ${ownText(e)}`.trim());
      }
    }
    if ((cs.overflowX === 'auto' || cs.overflowX === 'scroll') && e.scrollWidth > e.clientWidth + 1 && e.tagName !== 'TEXTAREA') {
      out.scroll.push(`${name(e)} scrolls sideways (${e.scrollWidth}px in ${e.clientWidth}px)`);
    }
    const text = ownText(e);
    // Visually hidden text (a 1px clip box) is clipped by design.
    const hidden = cs.position === 'absolute' && e.clientWidth <= 1;
    if (text && !hidden && e.scrollWidth > e.clientWidth + 1 && e.clientWidth > 0) {
      const clips = cs.overflowX !== 'visible' && cs.overflowX !== 'auto' && cs.overflowX !== 'scroll' && cs.textOverflow !== 'ellipsis';
      const overflowsBox = cs.overflowX === 'visible' && cs.whiteSpace.includes('nowrap');
      if (clips || overflowsBox) out.clipped.push(`${name(e)} "${text}" needs ${e.scrollWidth}px, has ${e.clientWidth}px`);
    }
  }
  return out;
}

async function problems(page: Page, rootSelector: string | null): Promise<Report> {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))));
  return page.evaluate(measure, rootSelector);
}

const BASELINE_FILE = join(__dirname, 'layout.baseline.json');
const RECORD = process.env['PW_RECORD_BASELINE'] === '1';
const baseline: Record<string, string[]> = existsSync(BASELINE_FILE) ? JSON.parse(readFileSync(BASELINE_FILE, 'utf8')) : {};
const recorded: Record<string, string[]> = {};

test.afterAll(() => {
  if (!RECORD) return;
  // Workers each hold their own results: merge into the file.
  const current: Record<string, string[]> = existsSync(BASELINE_FILE) ? JSON.parse(readFileSync(BASELINE_FILE, 'utf8')) : {};
  const merged = { ...current, ...recorded };
  const sorted = Object.fromEntries(Object.entries(merged).filter(([, v]) => v.length).sort(([x], [y]) => x.localeCompare(y)));
  writeFileSync(BASELINE_FILE, `${JSON.stringify(sorted, null, 2)}\n`);
});

test.describe('layout at the supported window widths', () => {
  for (const state of STATES) {
    for (const width of WIDTHS) {
      // The tray popover has one size; measure it once.
      if (state.width && width !== WIDTHS[0]) continue;
      const key = `${state.id}@${state.width ?? width}`;
      test(`${key}: nothing new spills, scrolls sideways or is clipped`, async ({ page }, info) => {
        await page.setViewportSize({ width: state.width ?? width, height: state.height ?? 780 });
        await openApp(page, state.hash, `mock-fast&mock-static${state.query ? `&${state.query}` : ''}`);
        await page.waitForLoadState('networkidle');
        // A step that breaks fails the test: nothing here is caught.
        await state.act?.(page);
        await page.waitForLoadState('networkidle');
        const found = await problems(page, null);
        const lines = [...found.spill, ...found.scroll, ...found.clipped].sort();
        const id = `${info.project.name}/${key}`;
        if (RECORD) {
          recorded[id] = lines;
          return;
        }
        const known = baseline[id] ?? [];
        expect({ new: lines.filter((l) => !known.includes(l)), fixed: known.filter((l) => !lines.includes(l)) }, id).toEqual({ new: [], fixed: [] });
      });
    }
  }
});

test.describe('layout of every dialog window at its contract width', () => {
  const contracts = dialogContracts(...CONTRACT_FILES);
  for (const [id, contract] of Object.entries(contracts)) {
    const key = `window ${id}@${contract.width}`;
    test(`${key}: nothing new spills, scrolls sideways or is clipped`, async ({ page }, info) => {
      // A tool (the Settings window) opens at its preferred size; the others at a height that fits a long body.
      await page.setViewportSize({ width: contract.width, height: contract.height ?? 640 });
      await page.goto(dialogWindowUrl(id, INPUTS[id] ?? {}, { title: id }));
      await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
      await page.waitForLoadState('networkidle');
      await expect(page.locator('[data-testid=dialog-footer]')).toBeVisible();
      const found = await problems(page, null);
      const lines = [...found.spill, ...found.scroll, ...found.clipped].sort();
      const run = `${info.project.name}/${key}`;
      if (RECORD) {
        recorded[run] = lines;
        return;
      }
      const known = baseline[run] ?? [];
      expect({ new: lines.filter((l) => !known.includes(l)), fixed: known.filter((l) => !lines.includes(l)) }, run).toEqual({ new: [], fixed: [] });
    });
  }
});
