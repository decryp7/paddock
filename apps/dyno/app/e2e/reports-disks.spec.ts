import { expect, test, type Page } from '@playwright/test';

/**
 * Reports > Disks (user report, Dyno 1.1.0): a long mount path pushed the numbers and the forecast out of the card. At the window
 * minimum (900 px) and a wide window, every row and every cell of the Disks card lies inside the card, the cells do not overlap,
 * and a path that does not fit is shortened with its full text in the tooltip. The cards beside it keep their layout. The mock
 * backend's long paths are invented.
 */
const WIDTHS = [900, 1440];

type Box = { left: number; right: number; top: number; bottom: number };

async function openReports(page: Page, width: number): Promise<void> {
  await page.setViewportSize({ width, height: 900 });
  await page.goto('/?latency=0#/reports');
  await expect(page.getByTestId('report-disks')).toBeVisible({ timeout: 45_000 });
  await page.evaluate(() => document.fonts.ready);
  // The path is cut to the width its box has once laid out; wait until the shown text is the one that fits.
  await page.waitForFunction(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[aria-label="Disks"] [data-testid="path-text"]')).every((b) => {
      const shown = b.querySelector<HTMLElement>('.pd-shown');
      return !b.clientWidth || !shown || shown.scrollWidth <= b.clientWidth + 1;
    }),
  );
}

for (const width of WIDTHS) {
  test(`Disks card: rows and cells stay inside the card at ${width} px`, async ({ page }) => {
    await openReports(page, width);
    const m = await page.evaluate(() => {
      const b = (el: Element): { left: number; right: number; top: number; bottom: number } => {
        const r = el.getBoundingClientRect();
        return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
      };
      const card = document.querySelector<HTMLElement>('section[aria-label="Disks"]')!;
      const rows = Array.from(card.querySelectorAll<HTMLElement>('[data-testid="report-disk"]'));
      return {
        card: b(card),
        cardScroll: { scroll: card.scrollWidth, client: card.clientWidth },
        page: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        rows: rows.map((r) => ({
          row: b(r),
          cells: Array.from(r.children).map((c) => ({ box: b(c), text: (c.textContent ?? '').trim(), over: (c as HTMLElement).scrollWidth - (c as HTMLElement).clientWidth })),
        })),
      };
    });
    expect(m.rows.length).toBeGreaterThanOrEqual(3);
    expect(m.page, 'the page scrolls sideways').toBeLessThanOrEqual(0);
    expect(m.cardScroll.scroll).toBeLessThanOrEqual(m.cardScroll.client + 1);
    expect(m.card.right, 'the card is wider than the window').toBeLessThanOrEqual(width);
    for (const r of m.rows) {
      const inside = (x: Box, within: Box): boolean => x.left >= within.left - 0.5 && x.right <= within.right + 0.5;
      expect(inside(r.row, m.card), `row ${JSON.stringify(r.row)} outside card ${JSON.stringify(m.card)}`).toBe(true);
      for (const c of r.cells) expect(inside(c.box, m.card), `cell "${c.text}" outside the card`).toBe(true);
      for (let i = 1; i < r.cells.length; i++) {
        expect(r.cells[i - 1]!.box.right, `"${r.cells[i - 1]!.text}" overlaps "${r.cells[i]!.text}"`).toBeLessThanOrEqual(r.cells[i]!.box.left + 0.5);
      }
    }
  });

  test(`Disks card: columns line up across rows and the long path is shortened with its full text at ${width} px`, async ({ page }) => {
    await openReports(page, width);
    const lefts = await page.locator('[data-testid="report-disk"]').evaluateAll((rows) =>
      [1, 2].map((i) => rows.map((r) => Math.round((r.children[i] as HTMLElement).getBoundingClientRect().right))),
    );
    for (const col of lefts) expect(new Set(col).size, `column right edges ${col}`).toBe(1);
    const long = page.locator('[data-testid="report-disk"] [data-testid="path-text"]').nth(1);
    await expect(long).toHaveAttribute('data-full', /Example-Archive-Drive-With-A-Long-Name\/Projects-2026\/Quarter-Four\/Backups-Of-Everything-From-Every-Computer-In-The-House-Kept-Here-For-Ten-Years/);
    const cut = await long.evaluate((el) => (el.querySelector('.pd-shown')?.textContent ?? '').length < (el.getAttribute('data-full') ?? '').length);
    if (width === 900) expect(cut, 'at the minimum width the long path is shortened').toBe(true);
  });

  test(`Busiest apps and Health cards keep their layout at ${width} px`, async ({ page }) => {
    await openReports(page, width);
    const cards = await page.evaluate(() =>
      ['Health score trend', 'Busiest apps', 'Disks'].map((n) => {
        const r = document.querySelector(`section[aria-label="${n}"]`)!.getBoundingClientRect();
        return { n, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
      }),
    );
    // The cards share a row or wrap to the next, but never overlap and never leave the window.
    for (const c of cards) expect(c.right, `${c.n} leaves the window`).toBeLessThanOrEqual(width);
    for (let i = 0; i < cards.length; i++) {
      for (let j = i + 1; j < cards.length; j++) {
        const [a, b] = [cards[i]!, cards[j]!];
        const apart = a.right <= b.left + 0.5 || b.right <= a.left + 0.5 || a.bottom <= b.top + 0.5 || b.bottom <= a.top + 0.5;
        expect(apart, `${a.n} overlaps ${b.n}`).toBe(true);
      }
    }
  });
}
