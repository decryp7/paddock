import { test as base, expect } from '@playwright/test';
import { pinnedClockTest } from '../src/index.ts';

const test = pinnedClockTest(base, { now: '2001-09-09T01:46:40Z', timezoneId: 'Asia/Singapore', locale: 'en-GB' });

test('a page starts at the pinned instant in the pinned zone and locale, and the clock ticks', async ({ page }) => {
  await page.goto('data:text/html,<p>x</p>');
  const read = () => page.evaluate(() => ({ now: new Date().toISOString(), tz: Intl.DateTimeFormat().resolvedOptions().timeZone, lang: navigator.language }));
  const first = await read();
  expect(first.now.startsWith('2001-09-09T01:46:')).toBe(true);
  expect(first.tz).toBe('Asia/Singapore');
  expect(first.lang).toBe('en-GB');
  await page.clock.runFor(60_000);
  expect((await read()).now >= '2001-09-09T01:47:').toBe(true);
});

test('a page opened from the context is pinned too', async ({ context }) => {
  const other = await context.newPage();
  expect(await other.evaluate(() => new Date().toISOString().slice(0, 13))).toBe('2001-09-09T01');
});

test.describe('opted out', () => {
  // The spec needs the real time.
  test.use({ pinnedClock: false });
  test('keeps the real clock', async ({ page }) => {
    expect(Math.abs((await page.evaluate(() => Date.now())) - Date.now())).toBeLessThan(60_000);
  });
});

test('a second install in the spec wins over the pin', async ({ page }) => {
  await page.clock.install({ time: new Date('2026-01-01T00:00:00Z') });
  expect(await page.evaluate(() => new Date().getUTCFullYear())).toBe(2026);
  expect(await page.evaluate(() => new Date().toISOString().slice(0, 10))).toBe('2026-01-01');
});
