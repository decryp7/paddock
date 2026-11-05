import { expect, test, type Locator, type Page } from '@decryp7/paddock-e2e';

/**
 * Tables size to the window (pd-table `height="window"`): on a page that scrolls once, each table is as tall as its rows up to the
 * height of the visible page area, follows the window live, never needs more than the window, and leaves the wheel to the page at the
 * end of its rows. Three window sizes, one page of each kind: Firewall (one table per source), an Extras tab, Alarms.
 * Everything waits on a condition, never on a fixed time (ER-12).
 */
const SIZES = [
  { width: 900, height: 600 },
  { width: 1280, height: 800 },
  { width: 1920, height: 1200 },
] as const;

const PAGES = [
  { name: 'Firewall', hash: '/firewall', grid: 'macOS Application Firewall rules' },
  { name: 'Extras: installed software', hash: '/extras?tab=inventory', grid: 'Installed software' },
  { name: 'Alarms', hash: '/alarms', grid: 'Open alarms' },
] as const;

const url = (hash: string) => `/?latency=0&rows=many#${hash}`;

/** The measures of the grid and of the page area (the frame's main region, the one scroller). */
async function measure(page: Page, grid: Locator) {
  const body = grid.locator('.pd-body');
  return {
    body: await body.evaluate((b) => b.clientHeight),
    bodyScroll: await body.evaluate((b) => b.scrollHeight),
    gridHeight: await grid.evaluate((g) => g.getBoundingClientRect().height),
    area: await page.locator('#pd-main').evaluate((p) => p.clientHeight),
    areaScroll: await page.locator('#pd-main').evaluate((p) => p.scrollHeight),
    windowScrolls: await page.evaluate(() => document.documentElement.scrollHeight > window.innerHeight + 1),
  };
}

for (const p of PAGES) {
  test(`${p.name}: the table grows with the window, fits it, and its last row is reachable`, async ({ page }) => {
    await page.setViewportSize(SIZES[0]);
    await page.goto(url(p.hash));
    const grid = page.getByRole('grid', { name: p.grid });
    await expect(grid).toBeVisible();
    const rows = grid.locator('.pd-body [role=row][data-index]');
    await expect(rows.first()).toBeVisible();

    const bodies: number[] = [];
    for (const size of SIZES) {
      await page.setViewportSize(size);
      // The table follows the window: wait until its body is as tall as the page area allows (or its rows need).
      await expect.poll(async () => {
        const m = await measure(page, grid);
        return m.body >= Math.min(m.bodyScroll, 240) && m.gridHeight <= m.area + 1;
      }).toBe(true);
      const m = await measure(page, grid);
      bodies.push(m.body);
      // Only the page scrolls: not the window, and the table never needs more than the visible page area.
      expect(m.windowScrolls, `${size.width}x${size.height}: the window itself scrolls`).toBe(false);
      expect(m.gridHeight, `${size.width}x${size.height}: the table is taller than the page area`).toBeLessThanOrEqual(m.area + 1);
      expect(m.body, `${size.width}x${size.height}: the body is cut below its minimum`).toBeGreaterThanOrEqual(240);
      // Not cut off: scrolled into view, the whole table is inside the page area (2 px for the page's fractional scroll offset).
      await grid.scrollIntoViewIfNeeded();
      const inside = await grid.evaluate((g) => {
        const r = g.getBoundingClientRect();
        const a = document.querySelector('#pd-main')!.getBoundingClientRect();
        return { ok: r.top >= a.top - 2 && r.bottom <= a.bottom + 2, top: r.top, bottom: r.bottom, areaTop: a.top, areaBottom: a.bottom, scrollTop: document.querySelector('#pd-main')!.scrollTop };
      });
      expect(inside.ok, `${size.width}x${size.height}: the table is cut off by the page area ${JSON.stringify(inside)}`).toBe(true);
    }
    // A bigger window gives a taller table, each step (the lists are longer than the largest window's area).
    expect(bodies[1]!).toBeGreaterThan(bodies[0]!);
    expect(bodies[2]!).toBeGreaterThan(bodies[1]!);

    // The last row is reachable in the smallest window.
    await page.setViewportSize(SIZES[0]);
    await expect.poll(async () => (await measure(page, grid)).body).toBe(bodies[0]);
    await grid.locator('.pd-body').evaluate((b) => (b.scrollTop = b.scrollHeight));
    const last = rows.last();
    await expect(last).toBeInViewport({ ratio: 0.99 });
    const within = await last.evaluate((r) => {
      const b = r.closest('.pd-body')!.getBoundingClientRect();
      const x = r.getBoundingClientRect();
      return x.top >= b.top - 1 && x.bottom <= b.bottom + 1;
    });
    expect(within, 'the last row sits inside the body').toBe(true);
  });
}

test('Firewall: at the edge of the table the wheel scrolls the page, not a second scrollbar', async ({ page }) => {
  await page.setViewportSize(SIZES[0]);
  await page.goto(url('/firewall'));
  const grid = page.getByRole('grid', { name: 'macOS Application Firewall rules' });
  await expect(grid).toBeVisible();
  await grid.scrollIntoViewIfNeeded();
  const area = page.locator('#pd-main');
  const body = grid.locator('.pd-body');
  // The table is at its first row and the page is scrolled down to it: a wheel up passes on to the page.
  await body.evaluate((b) => (b.scrollTop = 0));
  await expect.poll(() => area.evaluate((a) => a.scrollTop)).toBeGreaterThan(0);
  const before = await area.evaluate((a) => a.scrollTop);
  const box = (await body.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect.poll(async () => {
    await page.mouse.wheel(0, -120);
    return before - (await area.evaluate((a) => a.scrollTop));
  }).toBeGreaterThan(0);
  expect(await body.evaluate((b) => b.scrollTop)).toBe(0);
});
