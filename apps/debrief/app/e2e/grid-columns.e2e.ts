import { expect, Page, test } from '@playwright/test';
import { openApp } from './app';
import { eventsViewMinWidth, GRID, MESSAGE_COLUMN_MIN } from '../src/app/grid/message-column';

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

// The themes differ in the monospace font and its size (Contrast uses 13 px): the column is sized from the cell's own font.
const THEMES = ['calm', 'console', 'precision', 'contrast'] as const;

for (const theme of THEMES) {
  for (const style of ['readable', 'iso'] as const) {
    test.describe(`${theme} theme, ${style} dates`, () => {
      test.beforeEach(async ({ page }) => {
        await page.addInitScript(
          ([s, t]) => {
            localStorage.setItem('debrief.settings', JSON.stringify({ version: 1, settings: { dateFormat: s } }));
            localStorage.setItem('debrief.prefs.v1', JSON.stringify({ theme: t }));
          },
          [style, theme],
        );
      });

      for (const width of [1024, 1440, 1920]) {
        test(`${width} px: the time never overflows its column or touches the level mark`, async ({ page }) => {
          await page.setViewportSize({ width, height: 900 });
          await openApp(page);
          await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
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

test('1920 px with two extra columns: dragging the splitter beside the Events view cannot squeeze it under its minimum', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('debrief.prefs.v1', JSON.stringify({ extraColumns: ['@line', '@template'] })));
  await page.setViewportSize({ width: 1920, height: 900 });
  await openApp(page);
  const eventsGroup = page.locator('.dv-groupview').filter({ has: page.getByRole('tab', { name: 'Events', exact: true }) });
  const registry = page.locator('.dv-groupview').filter({ has: page.getByRole('tab', { name: 'Registry', exact: true }) });
  await expect(registry).toBeVisible();
  const events = (await eventsGroup.boundingBox())!;
  // The sash between the two groups sits on the Events group's right edge: drag it far to the left, towards the Events view's minimum.
  const y = events.y + events.height / 2;
  await page.mouse.move(events.x + events.width + 1, y);
  await page.mouse.down();
  await page.mouse.move(events.x + events.width - 600, y, { steps: 12 });
  await page.mouse.up();
  const grid = page.locator('debrief-events-grid');
  const msg = row(page).locator('.msg');
  await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
  await expect.poll(() => grid.locator('.header').evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await expect.poll(() => row(page).evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await expectTimeClear(page);
  // The group is no narrower than the Events view's declared minimum, which counts the extra columns.
  const declared = Number(await eventsGroup.evaluate((el) => (el as HTMLElement).dataset['minWidth']));
  // (Counts both extra columns; the font decides a pixel or two, so compare with the width without them.)
  expect(declared).toBeGreaterThanOrEqual(eventsViewMinWidth(0) + 2 * (GRID.min.extra + GRID.gap) - 8);
  expect((await eventsGroup.boundingBox())!.width).toBeGreaterThanOrEqual(declared - 1);
});
