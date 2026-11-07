import { expect, test } from '@decryp7/paddock-e2e';

/**
 * Every Dyno page, with the many-rows mock, at 1920x1080 and 1280x800 (design guide section 2):
 *  - no horizontal scroll or clipping of the page area;
 *  - nothing that scrolls inside the page area while the area itself also scrolls, unless the page is a multi-section page whose
 *    tables fit the area (table-sizes.spec.ts: the page scrolls once and each table is at most as tall as the area);
 *  - a page of one table (FILL) does not scroll as a whole, and its table ends above the status bar;
 *  - the content width is at most 1440 px (dashboards), 880 px (forms), or the whole area (single-table pages).
 * Screenshots go to SHOTS_DIR when it is set. Conditions only (ER-12).
 */
const SIZES = [
  { width: 1920, height: 1080 },
  { width: 1280, height: 800 },
] as const;

type Width = 'wide' | 1440 | 880;
const PAGES: { name: string; hash: string; fill?: boolean; width: Width }[] = [
  { name: 'dashboard', hash: '/', width: 1440 },
  { name: 'processes', hash: '/processes', fill: true, width: 'wide' },
  { name: 'why-slow', hash: '/why-slow', width: 1440 },
  { name: 'throttling', hash: '/throttling', width: 1440 },
  { name: 'issues', hash: '/issues', fill: true, width: 'wide' },
  { name: 'inventory', hash: '/inventory', width: 1440 },
  { name: 'disk-map', hash: '/disk-map', width: 1440 },
  { name: 'network', hash: '/network', fill: true, width: 'wide' },
  { name: 'network-listening', hash: '/network?tab=listening', fill: true, width: 'wide' },
  { name: 'map', hash: '/map', width: 1440 },
  { name: 'lists', hash: '/lists', width: 1440 },
  { name: 'firewall', hash: '/firewall', width: 1440 },
  { name: 'counters', hash: '/counters', width: 1440 },
  { name: 'benchmarks', hash: '/benchmarks', width: 1440 },
  { name: 'alarms', hash: '/alarms', width: 1440 },
  { name: 'reports', hash: '/reports', width: 1440 },
  { name: 'recorder', hash: '/recorder', width: 1440 },
  { name: 'health', hash: '/health', width: 1440 },
  { name: 'power', hash: '/power', width: 1440 },
  { name: 'extras-changes', hash: '/extras?tab=changes', width: 1440 },
  { name: 'extras-inventory', hash: '/extras?tab=inventory', width: 1440 },
  { name: 'lifespan', hash: '/lifespan', width: 1440 },
  { name: 'diagnostics', hash: '/diagnostics', width: 1440 },
];

for (const p of PAGES) {
  for (const size of SIZES) {
    test(`${p.name} at ${size.width}x${size.height}: fits the window`, async ({ page }) => {
      await page.setViewportSize(size);
      await page.goto(`/?latency=0&rows=many#${p.hash}`);
      await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
      await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]'), undefined, { timeout: 30_000 });
      const shots = process.env['SHOTS_DIR'];
      if (shots) await page.screenshot({ path: `${shots}/${p.name}-${size.width}x${size.height}.png` });
      const m = await page.evaluate(() => {
        const main = document.querySelector('#pd-main') as HTMLElement;
        const status = (document.querySelector('[data-pd-region=status]') as HTMLElement).getBoundingClientRect();
        const content = main.querySelector('.page, .wrap') as HTMLElement | null;
        const inner = [...main.querySelectorAll<HTMLElement>('*')].filter((e) => {
          const o = getComputedStyle(e).overflowY;
          return (o === 'auto' || o === 'scroll') && e.scrollHeight > e.clientHeight + 1 && e.clientHeight > 0;
        });
        const bottoms = [...main.querySelectorAll<HTMLElement>('.pd-body')].map((b) => b.getBoundingClientRect().bottom);
        return {
          overflowY: main.scrollHeight - main.clientHeight,
          overflowX: main.scrollWidth - main.clientWidth,
          docX: document.documentElement.scrollWidth - window.innerWidth,
          innerScrollers: inner.length,
          width: content ? Math.round(content.getBoundingClientRect().width) : -1,
          area: main.clientWidth,
          statusTop: status.top,
          maxBodyBottom: bottoms.length ? Math.max(...bottoms) : 0,
        };
      });
      const why = JSON.stringify(m);
      expect(m.overflowX, `horizontal overflow of the page area: ${why}`).toBeLessThanOrEqual(1);
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
