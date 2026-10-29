import { expect, test, type Page } from '@playwright/test';
import { openApp } from './app';

// Layout at the supported window widths (rule 18): for every route and dialog, no element wider than its container, no
// horizontal scroll (of the page or of a panel inside it) and no text clipped without an ellipsis. The measurement is the
// independent reviewer's (Pitwall 1.1 review); visually hidden text (`sr-only`) is clipped on purpose and is not reported.
//
// KNOWN_FAILURES lists the measured failures of the tree this spec was added to; each is marked `test.fail`, so the spec
// is green now and turns red the moment a fix lands without its entry being removed (WP-2/WP-3 own the layout fixes and
// delete the entries they fix). Do not add an entry for a new screen: fix the layout.

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
  {
    id: 'apps-new-dialog',
    hash: '/apps',
    act: async (page) => {
      await page.getByRole('button', { name: 'New app' }).first().click();
    },
  },
  {
    id: 'apps-key-secret',
    hash: '/apps',
    act: async (page) => {
      await page.getByRole('button', { name: 'New app' }).first().click();
      await page.getByRole('dialog').getByRole('textbox').first().fill('A rather long application name for testing');
      await page.getByRole('button', { name: 'Create app' }).click();
      await page.getByRole('dialog').waitFor();
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
  { id: 'settings', hash: '/settings' },
  { id: 'consent-dialog', hash: '/', query: 'mock-consent' },
  { id: 'quit-dialog', hash: '/', query: 'mock-quit' },
  // The tray popover is its own small window, not a resizable one: measured once at its own size.
  { id: 'tray', hash: '/tray', width: 360, height: 560 },
];

/** `<state>@<width>` -> what was measured on 2026-10-29. WP-2/WP-3 remove entries as they fix them. */
const KNOWN_FAILURES: Readonly<Record<string, string>> = {
  'dashboard@900': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 598px)',
  'dashboard@1280': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 565px) | section.tile > div.trio > div > span.k is 72px in a 62px container (by 10px) Prompt | section.tile > div.trio > div > span.u is 28px in a 62px container (by 10px) tok/s',
  'dashboard-down@900': 'div.grid > pw-panel.c5 > div.error > button.btn.sm is 49px in a 260px container (by 8px) Retry | div.grid > pw-panel.c4 > div.error > button.btn.sm is 49px in a 260px container (by 8px) Retry | div.grid > pw-panel.c4 > div.error > button.btn.sm is 49px in a 260px container (by 8px) Retry | div.grid ',
  'dashboard-update@900': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 598px)',
  'dashboard-update@1280': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 565px) | section.tile > div.trio > div > span.k is 72px in a 62px container (by 10px) Prompt | section.tile > div.trio > div > span.u is 28px in a 62px container (by 10px) tok/s',
  'models-presets@900': 'div#panel-presets > pw-presets-tab > pw-panel.flush > div.scroll scrolls sideways (1160px in 598px)',
  'models-presets@1024': 'div#panel-presets > pw-presets-tab > pw-panel.flush > div.scroll scrolls sideways (1160px in 722px)',
  'models-presets@1280': 'div#panel-presets > pw-presets-tab > pw-panel.flush > div.scroll scrolls sideways (1160px in 978px)',
  'models-preset-form@900': 'div#panel-presets > pw-presets-tab > pw-panel.flush > div.scroll scrolls sideways (1160px in 598px) | form.editor > div.grid.four > div.field > label is 172px in a 133px container (by 40px) Context (tokens) | form.editor > div.grid.four > div.field > input#pw-preset-context is 172px in a 133px conta',
  'models-preset-form@1024': 'div#panel-presets > pw-presets-tab > pw-panel.flush > div.scroll scrolls sideways (1160px in 722px) | form.editor > div.grid.four > div.field > label is 172px in a 164px container (by 9px) Context (tokens) | form.editor > div.grid.four > div.field > input#pw-preset-context is 172px in a 164px contai',
  'models-preset-form@1280': 'div#panel-presets > pw-presets-tab > pw-panel.flush > div.scroll scrolls sideways (1160px in 978px)',
  'models-slots@900': 'pw-library-tab > div.with-panel > pw-panel.flush > div.scroll scrolls sideways (407px in 226px)',
  'models-slots@1024': 'pw-library-tab > div.with-panel > pw-panel.flush > div.scroll scrolls sideways (407px in 350px)',
  'engines@900': 'div.page > pw-engine-card > section.engine > div.surface scrolls sideways (813px in 598px)',
  'engines@1024': 'div.page > pw-engine-card > section.engine > div.surface scrolls sideways (813px in 722px)',
  'apps@900': 'pw-root > div.shell > main#main scrolls sideways (870px in 664px) | pw-apps-page > div.page > header.page-head > div.actions is 100px in a 600px container (by 48px) | pw-panel > header > div.actions > div.controls is 361px in a 254px container (by 107px) | div.detail > pw-app-usage > pw-panel.flush ',
  'apps@1024': 'pw-root > div.shell > main#main scrolls sideways (870px in 788px) | div.detail > pw-app-usage > pw-panel.flush > table.table.recent is 525px in a 412px container (by 114px)',
  'apps-detail@900': 'pw-root > div.shell > main#main scrolls sideways (976px in 664px) | pw-apps-page > div.page > header.page-head > div.actions is 100px in a 600px container (by 48px) | div.hero-top > div.htitle > p > span.faint is 52px in a 158px container (by 20px) · 600/min · 20k req/day · 5.0M/day · 120M/mo · ×2 |',
  'apps-detail@1024': 'pw-root > div.shell > main#main scrolls sideways (976px in 788px) | div.detail > pw-app-usage > pw-panel.flush > table.table.recent is 631px in a 412px container (by 220px) | fieldset.group > ul.models > li > label.check is 220px in a 168px container (by 52px) | fieldset.group > ul.models > li > lab',
  'apps-new-dialog@900': 'pw-root > div.shell > main#main scrolls sideways (870px in 664px) | pw-apps-page > div.page > header.page-head > div.actions is 100px in a 600px container (by 48px) | pw-panel > header > div.actions > div.controls is 361px in a 254px container (by 107px) | div.detail > pw-app-usage > pw-panel.flush ',
  'apps-new-dialog@1024': 'pw-root > div.shell > main#main scrolls sideways (870px in 788px) | div.detail > pw-app-usage > pw-panel.flush > table.table.recent is 525px in a 412px container (by 114px)',
  'apps-key-secret@900': 'pw-root > div.shell > main#main scrolls sideways (722px in 664px) | pw-apps-page > div.page > header.page-head > div.actions is 100px in a 600px container (by 48px) | div.hero-top > div.htitle > p > span.faint is 39px in a 158px container (by 26px) · 60/min · ×1 | pw-panel > header > div.actions > d',
  'requests@900': 'page scrolls sideways by 28px | div.page > div.with-panel.fill > pw-panel.table-panel.flush > div.tbl scrolls sideways (662px in 598px) | pw-panel.table-panel.flush > div.tbl > div.sheet > cdk-virtual-scroll-viewport.cdk-virtual-scroll-viewport.vp scrolls sideways (988px in 598px) | div.page > div.f',
  'requests@1024': 'pw-panel.table-panel.flush > div.tbl > div.sheet > cdk-virtual-scroll-viewport.cdk-virtual-scroll-viewport.vp scrolls sideways (988px in 722px) | div.page > div.filters > pw-select.select > button#pw-req-app is 160px in a 140px container (by 33px)',
  'requests@1280': 'div.page > div.with-panel.fill > pw-panel.table-panel.flush > div.tbl scrolls sideways (980px in 978px) | pw-panel.table-panel.flush > div.tbl > div.sheet > cdk-virtual-scroll-viewport.cdk-virtual-scroll-viewport.vp scrolls sideways (988px in 980px)',
  'requests-detail@900': 'div.page > div.with-panel.fill > pw-panel.table-panel.flush > div.tbl scrolls sideways (348px in 226px) | pw-panel.table-panel.flush > div.tbl > div.sheet > cdk-virtual-scroll-viewport.cdk-virtual-scroll-viewport.vp scrolls sideways (988px in 226px) | div.page > div.filters > pw-select.select > butt',
  'requests-detail@1024': 'pw-panel.table-panel.flush > div.tbl > div.sheet > cdk-virtual-scroll-viewport.cdk-virtual-scroll-viewport.vp scrolls sideways (988px in 350px) | div.page > div.filters > pw-select.select > button#pw-req-app is 160px in a 140px container (by 33px)',
  'requests-detail@1280': 'div.page > div.with-panel.fill > pw-panel.table-panel.flush > div.tbl scrolls sideways (662px in 606px) | pw-panel.table-panel.flush > div.tbl > div.sheet > cdk-virtual-scroll-viewport.cdk-virtual-scroll-viewport.vp scrolls sideways (988px in 606px) | div.tbl > div.sheet > div.row.head > span.c-stat',
  'playground@900': 'section.pane > header.pane-head > pw-select.model > button#pg-model-0 is 160px in a 146px container (by 14px)',
  'benchmarks@900': 'pw-root > div.shell > main#main scrolls sideways (695px in 664px) | pw-benchmarks-page > div.page > pw-panel.flush > table.table is 662px in a 600px container (by 63px)',
  'settings@900': 'pw-panel#set-cache > div.row > div.cap > div.seg is 156px in a 134px container (by 23px)',
  'consent-dialog@900': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 598px)',
  'consent-dialog@1280': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 565px) | section.tile > div.trio > div > span.k is 72px in a 62px container (by 10px) Prompt | section.tile > div.trio > div > span.u is 28px in a 62px container (by 10px) tok/s',
  'quit-dialog@900': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 598px)',
  'quit-dialog@1280': 'div.page > div.grid > pw-panel.c7 > pw-apps-card scrolls sideways (662px in 565px) | section.tile > div.trio > div > span.k is 72px in a 62px container (by 10px) Prompt | section.tile > div.trio > div > span.u is 28px in a 62px container (by 10px) tok/s',
};

interface Report {
  readonly spill: readonly string[];
  readonly scroll: readonly string[];
  readonly clipped: readonly string[];
}

/** Runs in the page: the three kinds of layout problem, each as a short readable line. */
function measure(): { spill: string[]; scroll: string[]; clipped: string[] } {
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
  const pageScroll = document.documentElement.scrollWidth - innerWidth;
  if (pageScroll > 0) out.scroll.push(`page scrolls sideways by ${pageScroll}px`);
  for (const e of Array.from(document.body.querySelectorAll('*'))) {
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

async function problems(page: Page): Promise<Report> {
  await page.evaluate(() => document.fonts.ready);
  await page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))));
  return page.evaluate(measure);
}

test.describe('layout at the supported window widths', () => {
  for (const state of STATES) {
    for (const width of WIDTHS) {
      // The tray popover has one size; measure it once.
      if (state.width && width !== WIDTHS[0]) continue;
      const key = `${state.id}@${state.width ?? width}`;
      test(`${key}: nothing spills, scrolls sideways or is clipped`, async ({ page }) => {
        test.fail(key in KNOWN_FAILURES, KNOWN_FAILURES[key] ?? '');
        await page.setViewportSize({ width: state.width ?? width, height: state.height ?? 780 });
        await openApp(page, state.hash, `mock-fast&mock-static${state.query ? `&${state.query}` : ''}`);
        await page.waitForLoadState('networkidle');
        await state.act?.(page);
        await page.waitForLoadState('networkidle');
        const found = await problems(page);
        expect(found, key).toEqual({ spill: [], scroll: [], clipped: [] });
      });
    }
  }
});
