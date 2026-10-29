import { expect, Page, test } from '@playwright/test';
import { openApp } from './app';
import { MESSAGE_COLUMN_MIN } from '../src/app/grid/message-column';

// The events grid's columns never overflow or overlap: below the full date-and-time width the time column shows the compact
// time (full date and time in its tooltip), and extra columns are part of the Events view's declared minimum width.

const row = (page: Page) => page.locator('debrief-events-grid [role=row].row').first();

/** The first row's time text must end before the level dot starts, and the time cell must hold its content. */
async function expectTimeClear(page: Page): Promise<void> {
  await expect
    .poll(() =>
      row(page).evaluate((r) => {
        const time = r.querySelector('.time') as HTMLElement;
        const dot = r.querySelector('.lvl .dot') as HTMLElement;
        const range = document.createRange();
        range.selectNodeContents(time);
        const text = range.getBoundingClientRect();
        return { fits: time.scrollWidth <= time.clientWidth, clear: text.right <= dot.getBoundingClientRect().left };
      }),
    )
    .toEqual({ fits: true, clear: true });
}

for (const style of ['readable', 'iso'] as const) {
  test.describe(`${style} dates`, () => {
    test.beforeEach(async ({ page }) => {
      await page.addInitScript((s) => localStorage.setItem('debrief.settings', JSON.stringify({ version: 1, settings: { dateFormat: s } })), style);
    });

    for (const width of [1024, 1440, 1920]) {
      test(`${width} px: the time never overflows its column or touches the level mark`, async ({ page }) => {
        await page.setViewportSize({ width, height: 900 });
        await openApp(page);
        await expectTimeClear(page);
        // Compact (time only) with the full date and time in the tooltip, or the full format when the column is wide enough.
        const time = row(page).locator('.time');
        const text = ((await time.textContent()) ?? '').trim();
        if (/^\d{2}:\d{2}:\d{2}\.\d{3}$/.test(text)) await expect(time).toHaveAttribute('title', /\d{4} \d{2}:\d{2}:\d{2}\.\d{3}|\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}/);
        else expect(text).toMatch(style === 'iso' ? /^\d{4}-\d{2}-\d{2} \d{2}:/ : /^\d+ \w{3} \d{2}:/);
      });
    }
  });
}

test('the full date and time is shown where the column is wide enough', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  await openApp(page);
  await expect(row(page).locator('.time')).toHaveText(/^\d+ \w{3} \d{2}:\d{2}:\d{2}\.\d{3}$/);
});

for (const width of [1024, 1440]) {
  test(`${width} px with two extra columns: nothing overflows and the message column keeps its minimum`, async ({ page }) => {
    await page.addInitScript(() => localStorage.setItem('debrief.prefs.v1', JSON.stringify({ extraColumns: ['@line', '@template'] })));
    await page.setViewportSize({ width, height: 900 });
    await openApp(page);
    const grid = page.locator('debrief-events-grid');
    const msg = row(page).locator('.msg');
    await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
    await expect.poll(() => grid.locator('.header').evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    await expect.poll(() => row(page).evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
    // The grid group ends inside the window.
    await expect.poll(async () => { const g = (await grid.boundingBox())!; return g.x + g.width; }).toBeLessThanOrEqual(width + 1);
    await expectTimeClear(page);
  });
}
