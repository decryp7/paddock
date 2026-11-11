import { expect, test, type Page } from '@decryp7/paddock-e2e';

/**
 * Every Dyno page, with the many-rows mock, at 1920x1080, 1440x900, 1280x800 and 1024x768, in light and dark (design guide section 2;
 * Processes also at 900x700, where its narrow-table column order applies):
 *  - no horizontal scroll or clipping of the page area;
 *  - nothing that scrolls inside the page area while the area itself also scrolls, unless the page is a multi-section page whose
 *    tables fit the area (table-sizes.spec.ts: the page scrolls once and each table is at most as tall as the area);
 *  - a page of one table (FILL) does not scroll as a whole, and its table ends above the status bar;
 *  - the content width is at most 1440 px (dashboards), 880 px (forms), or the whole area (single-table pages).
 * Screenshots go to SHOTS_DIR when it is set. Conditions only (ER-12).
 */
const SIZES = [
  { width: 1920, height: 1080 },
  { width: 1440, height: 900 },
  { width: 1280, height: 800 },
  { width: 1024, height: 768 },
] as const;
/** Both themes: a page that fits in one can overflow in the other only through a token, but the check is cheap. */
const THEMES = ['light', 'dark'] as const;

type Width = 'wide' | 1440 | 880;
/**
 * `fill`: the page's main list takes the height left (a page of one table, or one table per source, or the Issues list under its
 * summaries); `wide`: that page is one table and uses the whole page area. `open`: what to do before measuring (a tab to switch to).
 */
const PAGES: { name: string; hash: string; fill?: boolean; width: Width; open?: (page: Page) => Promise<void>; nestedWaiver?: string; extraSizes?: { width: number; height: number }[] }[] = [
  { name: 'dashboard', hash: '/', width: 1440, nestedWaiver: 'a grid of fixed-height widgets, some with a scrolling body: card heights are for a later change' },
  { name: 'processes', hash: '/processes', fill: true, width: 'wide', extraSizes: [{ width: 900, height: 700 }] },
  { name: 'why-slow', hash: '/why-slow', width: 1440 },
  { name: 'throttling', hash: '/throttling', width: 1440 },
  { name: 'issues', hash: '/issues', fill: true, width: 1440 },
  { name: 'inventory', hash: '/inventory', width: 1440 },
  { name: 'disk-map', hash: '/disk-map', width: 1440 },
  { name: 'network', hash: '/network', fill: true, width: 'wide' },
  { name: 'network-listening', hash: '/network', fill: true, width: 'wide', open: (page) => page.getByRole('radio', { name: 'Listening' }).click() },
  { name: 'map', hash: '/map', width: 1440 },
  { name: 'lists', hash: '/lists', width: 1440 },
  { name: 'firewall', hash: '/firewall', fill: true, width: 'wide' },
  { name: 'counters', hash: '/counters', width: 1440 },
  { name: 'benchmarks', hash: '/benchmarks', width: 1440 },
  { name: 'alarms', hash: '/alarms', fill: true, width: 'wide' },
  { name: 'alarms-history', hash: '/alarms', fill: true, width: 'wide', open: (page) => page.getByRole('tab', { name: 'History' }).click() },
  { name: 'reports', hash: '/reports', width: 1440 },
  { name: 'recorder', hash: '/recorder', width: 1440 },
  { name: 'health', hash: '/health', width: 1440 },
  { name: 'power', hash: '/power', width: 1440 },
  { name: 'extras-changes', hash: '/extras?tab=changes', width: 1440 },
  { name: 'extras-inventory', hash: '/extras?tab=inventory', fill: true, width: 1440 },
  { name: 'lifespan', hash: '/lifespan', width: 1440 },
  { name: 'diagnostics', hash: '/diagnostics', width: 1440 },
];

/**
 * Every chart has drawn and finished its entry animation: each dy-chart has a canvas, and the painted pixels of all canvases are the same
 * in two checks a frame apart (the first draw loads ECharts on demand and animates, so a capture before that shows empty axes).
 */
async function chartsDrawn(page: Page): Promise<void> {
  await page.waitForFunction(() => {
    const charts = [...document.querySelectorAll('dy-chart')];
    if (charts.length === 0) return true;
    if (charts.some((c) => !c.querySelector('canvas'))) return false;
    const sig = [...document.querySelectorAll('dy-chart canvas')]
      .map((c) => {
        const cv = c as HTMLCanvasElement;
        const d = cv.getContext('2d')!.getImageData(0, 0, cv.width, cv.height).data;
        let n = 0;
        for (let i = 3; i < d.length; i += 4) if (d[i]! > 0) n++;
        return n;
      })
      .join(',');
    const w = window as unknown as { __sig?: string; __same?: number };
    w.__same = w.__sig === sig ? (w.__same ?? 0) + 1 : 0;
    w.__sig = sig;
    return w.__same >= 8;
  }, undefined, { timeout: 30_000, polling: 'raf' });
}

for (const p of PAGES) {
  for (const size of [...SIZES, ...(p.extraSizes ?? [])]) {
    for (const theme of THEMES) {
      test(`${p.name} at ${size.width}x${size.height} (${theme}): fits the window`, async ({ page }) => {
        await page.setViewportSize(size);
        await page.emulateMedia({ colorScheme: theme }); // the default Appearance follows the system
        await page.goto(`/?latency=0&rows=many#${p.hash}`);
        await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
        await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]'), undefined, { timeout: 30_000 });
        if (p.open) await p.open(page);
        await expect(page.locator('#pd-main [role=grid] .pd-body [role=row], #pd-main [role=treegrid] .pd-body [role=row]').first()).toBeVisible().catch(() => undefined);
        await chartsDrawn(page);
        // The theme really applied (a page that fits in one can overflow in the other only through a token): the page's own background is dark or light.
        const lum = await page.evaluate(() => {
          const [r, g, b] = getComputedStyle(document.body).backgroundColor.match(/[\d.]+/g)!.map(Number);
          return (0.2126 * r! + 0.7152 * g! + 0.0722 * b!) / 255;
        });
        if (theme === 'dark') expect(lum, 'the dark theme did not apply').toBeLessThan(0.3);
        else expect(lum, 'the light theme did not apply').toBeGreaterThan(0.7);
        const shots = process.env['SHOTS_DIR'];
        if (shots) await page.screenshot({ path: `${shots}/${p.name}-${size.width}x${size.height}-${theme}.png` });
        const m = await page.evaluate(() => {
          const main = document.querySelector('#pd-main') as HTMLElement;
          const status = (document.querySelector('[data-pd-region=status]') as HTMLElement).getBoundingClientRect();
          const content = main.querySelector('.page, .wrap') as HTMLElement | null;
          const active = (e: HTMLElement) => {
            const o = getComputedStyle(e).overflowY;
            return (o === 'auto' || o === 'scroll') && e.scrollHeight > e.clientHeight + 1 && e.clientHeight > 0;
          };
          const inner = [main, ...main.querySelectorAll<HTMLElement>('*')].filter(active);
          // Two vertical scrollers active at once in one region: an element that scrolls inside an ancestor (below the page area) that scrolls.
          const nestedEls = inner.filter((e) => e !== main && inner.some((o) => o !== e && o.contains(e)));
          const nested = nestedEls.length;
          const bottoms = [...main.querySelectorAll<HTMLElement>('.pd-body')].map((b) => b.getBoundingClientRect().bottom);
          return {
            overflowY: main.scrollHeight - main.clientHeight,
            overflowX: main.scrollWidth - main.clientWidth,
            docX: document.documentElement.scrollWidth - window.innerWidth,
            innerScrollers: inner.length,
            nested,
            nestedWho: nestedEls.map((e) => `${e.tagName.toLowerCase()}.${e.className}`),
            width: content ? Math.round(content.getBoundingClientRect().width) : -1,
            area: main.clientWidth,
            statusTop: status.top,
            maxBodyBottom: bottoms.length ? Math.max(...bottoms) : 0,
          };
        });
        const why = JSON.stringify(m);
        expect(m.overflowX, `horizontal overflow of the page area: ${why}`).toBeLessThanOrEqual(1);
        // A short window is the one place a list's least height and the page's scroll may meet (the page scrolls to the list, the list scrolls its rows).
        if (!p.nestedWaiver && !(p.fill && size.height < 1000)) expect(m.nested, `two vertical scrollers are active in one region: ${why}`).toBe(0);
        expect(m.docX, `the window scrolls horizontally: ${why}`).toBeLessThanOrEqual(1);

        if (p.fill && size.height >= 1000) {
          expect(m.overflowY, `a single-table page scrolls as a whole in a tall window: ${why}`).toBeLessThanOrEqual(1);
          expect(m.maxBodyBottom, `a table ends under the status bar: ${why}`).toBeLessThanOrEqual(m.statusTop + 1);
        } else if (p.fill) {
          // A short window: the table keeps its least height, so the page may scroll, and then the end of the table must be reachable.
          await page.locator('#pd-main').evaluate((e) => (e.scrollTop = e.scrollHeight));
          const end = await page.evaluate(() => Math.max(...[...document.querySelectorAll('#pd-main .pd-body')].map((b) => b.getBoundingClientRect().bottom)));
          expect(end, `the end of the table cannot be scrolled above the status bar: ${why}`).toBeLessThanOrEqual(m.statusTop + 1);
        }
        if (p.width === 'wide') expect(m.width, `a single-table page is narrower than the area: ${why}`).toBeGreaterThanOrEqual(m.area - 1);
        else expect(m.width, `wider than the guide's ${p.width}: ${why}`).toBeLessThanOrEqual(p.width);
      });
    }
  }
}
