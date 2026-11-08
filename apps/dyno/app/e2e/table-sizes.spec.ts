import { expect, test, type Locator, type Page } from '@decryp7/paddock-e2e';

/**
 * Tables size to the window (pd-table `height="window"`): on a page that scrolls once, each table is as tall as its rows up to the
 * height of the visible page area, follows the window live, never needs more than the window, and leaves the wheel to the page at the
 * end of its rows. Three window sizes, one page of each kind: an Extras tab and Alarms (single-table pages: the table fills the window down to its least height, then the page scrolls once; Firewall, Network and the rest are in page-layout.spec.ts).
 * Everything waits on a condition, never on a fixed time (ER-12).
 */
const SIZES = [
  { width: 900, height: 600 },
  { width: 1280, height: 800 },
  { width: 1920, height: 1200 },
] as const;

const PAGES = [
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
    // The page area keeps whatever scroll offset the last resize left (WebKit and Chromium leave different ones): bring the whole
    // table into view first, as a person would, then scroll its rows to their end. The table fits the area, so all of it can show.
    await grid.evaluate((g) => g.scrollIntoView({ block: 'nearest' }));
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

test('Extras inventory: at the edge of the table the wheel scrolls the page, not a second scrollbar', async ({ page }) => {
  await page.setViewportSize(SIZES[0]);
  await page.goto(url('/extras?tab=inventory'));
  const grid = page.getByRole('grid', { name: 'Installed software' });
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

for (const open of [false, true]) {
  test(`Processes at 900x600 with the details panel ${open ? 'open' : 'closed'}: the last row is reachable and not clipped`, async ({ page }) => {
    await page.setViewportSize(SIZES[0]);
    await page.goto(url('/processes'));
    await page.getByRole('radio', { name: 'All', exact: true }).click();
    const grid = page.getByRole('treegrid', { name: 'Processes' });
    await expect(grid).toBeVisible();
    const rows = grid.locator('.pd-body [role=row][data-index]');
    await expect(rows.first()).toBeVisible();
    if (open) {
      await rows.first().focus();
      await page.keyboard.press('Enter');
      await expect(page.getByRole('complementary', { name: 'Process details' })).toBeVisible();
    }
    // The body keeps its minimum and the pane around it grows to hold it (the page scrolls), so nothing is cut by the pane.
    // One measure per poll (the rows are drawn again when the panel narrows the columns): scroll to the end, then ask where the last row is.
    await expect
      .poll(async () =>
        grid.evaluate((g) => {
          const body = g.querySelector<HTMLElement>('.pd-body')!;
          body.scrollTop = body.scrollHeight;
          // The virtual body draws only the rows near the scroll position: the last row is the one whose index is the grid's row count.
          const row = body.querySelector<HTMLElement>(`[role=row][aria-rowindex="${g.getAttribute('aria-rowcount')}"]`);
          if (!row) return 'last row not drawn yet';
          const pane = g.closest('section.table')!.getBoundingClientRect();
          const main = document.querySelector('#pd-main')!;
          const main_ = main.getBoundingClientRect();
          const r = row.getBoundingClientRect();
          if (body.clientHeight < 240) return `body ${body.clientHeight} px, below its minimum`;
          // Cut by the pane (overflow hidden), or beyond the page area with no way to scroll to it.
          if (r.bottom > pane.bottom + 1) return 'cut by the table pane';
          if (r.bottom > main_.bottom + 1 && main.scrollHeight - main.scrollTop - main.clientHeight < 1) return 'beyond the page';
          return 'ok';
        }),
      )
      .toBe('ok');
  });
}
