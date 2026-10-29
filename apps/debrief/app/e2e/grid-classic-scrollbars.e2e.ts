import { expect, Page, test } from '@playwright/test';
import { openApp } from './app';
import { GRID, MESSAGE_COLUMN_MIN } from '../src/app/grid/message-column';

// Classic, always-visible scroll bars (Chromium's headless default hides them): the 8 px bar takes room from the rows, and the
// themes add chrome around the dock groups (the Console theme's cards): the message column must still keep its minimum.
test.use({ launchOptions: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

const THEMES = ['calm', 'console', 'precision', 'contrast'] as const;
const row = (page: Page) => page.locator('debrief-events-grid [role=row].row').first();

for (const theme of THEMES) {
  test(`${theme} theme at 1440 px: the message column keeps its minimum and the rows fit`, async ({ page, browserName }) => {
    test.skip(browserName !== 'chromium', 'the launch option that shows classic scroll bars is Chromium-only');
    await page.addInitScript((t) => localStorage.setItem('debrief.prefs.v1', JSON.stringify({ theme: t })), theme);
    await page.setViewportSize({ width: 1440, height: 900 });
    await openApp(page);
    const grid = page.locator('debrief-events-grid');
    // The bar is really there.
    await expect.poll(() => grid.locator('[role=grid]').evaluate((el) => el.offsetWidth - el.clientWidth)).toBeGreaterThan(0);
    const msg = row(page).locator('.msg');
    await expect.poll(async () => (await msg.boundingBox())!.width).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
    await expect.poll(() => row(page).evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    // The row keeps its right padding: the message column ends a full padding inside the row (a squeezed row eats into it).
    await expect
      .poll(() => row(page).evaluate((r, pad) => r.getBoundingClientRect().right - (r.querySelector('.msg') as HTMLElement).getBoundingClientRect().right >= pad - 1, GRID.padX))
      .toBe(true);
    await expect.poll(() => grid.locator('.header').evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
    // The time text ends before the level mark.
    await expect
      .poll(() =>
        row(page).evaluate((r) => {
          const time = r.querySelector('.time') as HTMLElement;
          const range = document.createRange();
          range.selectNodeContents(time);
          return time.scrollWidth <= time.clientWidth && range.getBoundingClientRect().right <= (r.querySelector('.lvl .dot') as HTMLElement).getBoundingClientRect().left;
        }),
      )
      .toBe(true);
  });
}

// No dock group is ever narrower than the minimum its view declares, whatever the theme puts around the groups (the Console theme's cards
// take 8 px from each): Timeline 560, Registry 420 (src/app/builtin).
for (const theme of THEMES) {
  for (const width of [1440, 1920]) {
    test(`${theme} theme at ${width} px: no dock group is narrower than its declared minimum`, async ({ page, browserName }) => {
      test.skip(browserName !== 'chromium', 'the launch option that shows classic scroll bars is Chromium-only');
      await page.addInitScript((t) => localStorage.setItem('debrief.prefs.v1', JSON.stringify({ theme: t })), theme);
      await page.setViewportSize({ width, height: 900 });
      await openApp(page);
      const widthOf = (name: string) =>
        page
          .locator('.dv-groupview')
          .filter({ has: page.getByRole('tab', { name, exact: true }) })
          .evaluate((g) => g.getBoundingClientRect().width);
      await expect.poll(() => widthOf('Registry')).toBeGreaterThanOrEqual(420);
      await expect.poll(() => widthOf('Timeline')).toBeGreaterThanOrEqual(560);
    });
  }
}
