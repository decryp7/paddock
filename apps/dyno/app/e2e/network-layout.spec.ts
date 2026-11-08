import { expect, test, type Page } from '@decryp7/paddock-e2e';

/**
 * The Network page's Connections table (grouped by program) fits the window: the page area does not scroll, the table's one scroller
 * ends above the status bar so its last row can be reached, and the sticky header never covers a row. Conditions only (ER-12).
 */
const SIZES = [
  { width: 1920, height: 1080 },
  { width: 1280, height: 800 },
] as const;

async function geometry(page: Page) {
  return page.evaluate(() => {
    const main = document.querySelector('#pd-main') as HTMLElement;
    const status = document.querySelector('[data-pd-region=status]') as HTMLElement;
    const body = document.querySelector('[role=treegrid] .pd-body') as HTMLElement;
    const head = document.querySelector('[role=treegrid] .pd-head-table, [role=treegrid] [role=row]') as HTMLElement;
    const rows = [...body.querySelectorAll<HTMLElement>('[role=row]')].map((r) => r.getBoundingClientRect());
    const b = body.getBoundingClientRect();
    return {
      mainScroll: main.scrollHeight - main.clientHeight,
      bodyBottom: b.bottom,
      bodyTop: b.top,
      headBottom: head.getBoundingClientRect().bottom,
      statusTop: status.getBoundingClientRect().top,
      firstTop: rows.length ? Math.min(...rows.map((r) => r.top)) : 0,
    };
  });
}

for (const size of SIZES) {
  test(`Network connections, grouped by program, fit a ${size.width}x${size.height} window`, async ({ page }) => {
    await page.setViewportSize(size);
    await page.goto('/?latency=0&rows=many#/network');
    await page.getByRole('radio', { name: 'Program' }).click().catch(() => undefined);
    const grid = page.getByRole('treegrid', { name: 'Connections' });
    await expect(grid.locator('.pd-body [role=row]').first()).toBeVisible();
    await page.screenshot({ path: `/private/tmp/claude-501/dyno-network-layout/${process.env['SHOT'] ?? 'run'}-${size.width}x${size.height}.png` });
    const g = await geometry(page);
    expect(g.mainScroll, `the page area scrolls by ${g.mainScroll}px: ${JSON.stringify(g)}`).toBeLessThanOrEqual(1);
    expect(g.bodyBottom, `the table body ends below the status bar top: ${JSON.stringify(g)}`).toBeLessThanOrEqual(g.statusTop + 1);
    expect(g.bodyTop, `the body starts above the header's bottom: ${JSON.stringify(g)}`).toBeGreaterThanOrEqual(g.headBottom - 1);
    expect(g.firstTop, `the first row starts under the header: ${JSON.stringify(g)}`).toBeGreaterThanOrEqual(g.headBottom - 1);
    // Scroll to the end: the last row is fully above the status bar.
    await grid.locator('.pd-body').evaluate((b) => (b.scrollTop = b.scrollHeight));
    const last = grid.locator('.pd-body [role=row]').last();
    await expect(last).toBeInViewport({ ratio: 0.99 });
    const box = (await last.boundingBox())!;
    expect(box.y + box.height, 'the last row is under the status bar').toBeLessThanOrEqual((await geometry(page)).statusTop + 1);
  });
}
