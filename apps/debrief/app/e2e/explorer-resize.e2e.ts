import { expect, Page, test } from '@playwright/test';
import { openApp } from './app';

// The Explorer is a view of the layout plan (ADR 0056) with a declared minimum and maximum width; the user resizes it by
// dragging the sash at its inner edge. It was a fixed 272 px column with no sash at all (reported 2026-10-27).
const SHOTS = process.env['EXPLORER_SHOTS'];

const sidebarWidth = async (page: Page): Promise<number> => Math.round((await page.locator('debrief-sidebar').boundingBox())!.width);

/** Drags the Explorer's inner edge by `dx` px, as a user does: press on the edge, move, release. */
async function dragEdge(page: Page, dx: number): Promise<void> {
  const box = (await page.locator('debrief-sidebar').boundingBox())!;
  const x = box.x + box.width - 1;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx, y, { steps: 8 });
  await page.mouse.up();
}

test('the Explorer is resized by dragging its sash, keeps the width across a window resize, and stops at its declared limits', async ({ page }) => {
  await openApp(page);
  const before = await sidebarWidth(page);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${process.env['EXPLORER_PHASE'] ?? 'run'}-1-before-drag.png` });
  await dragEdge(page, 80);
  await expect.poll(() => sidebarWidth(page)).toBe(before + 80);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${process.env['EXPLORER_PHASE'] ?? 'run'}-2-after-drag.png` });

  // A window resize and the re-plan it causes keep what the user dragged.
  await page.setViewportSize({ width: 1300, height: 900 });
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(page.getByRole('grid', { name: 'Events' })).toBeVisible();
  await expect.poll(() => sidebarWidth(page)).toBe(before + 80);

  // The declared minimum holds, and the Explorer never grows past its maximum.
  await dragEdge(page, -2000);
  const min = Number(await page.getByRole('separator', { name: 'Resize the Explorer' }).getAttribute('aria-valuemin'));
  await expect.poll(() => sidebarWidth(page)).toBe(min);
  await dragEdge(page, 2000);
  const max = Number(await page.getByRole('separator', { name: 'Resize the Explorer' }).getAttribute('aria-valuemax'));
  await expect.poll(() => sidebarWidth(page)).toBeLessThanOrEqual(max);
  expect(await sidebarWidth(page)).toBeGreaterThan(min);
});

test('the Explorer width is kept for the next session', async ({ page }) => {
  await openApp(page);
  const before = await sidebarWidth(page);
  await dragEdge(page, 60);
  await expect.poll(() => sidebarWidth(page)).toBe(before + 60);
  await page.reload();
  await expect(page.getByRole('grid', { name: 'Events' })).toBeVisible();
  await expect.poll(() => sidebarWidth(page)).toBe(before + 60);
});

/** Counts how often the Explorer is added to or removed from the page while `body` runs (a layout that fights itself flips it over and over). */
async function flipsDuring(page: Page, body: () => Promise<void>): Promise<number> {
  await page.evaluate(() => {
    const w = window as unknown as { __flips: number };
    w.__flips = 0;
    new MutationObserver((records) => {
      for (const r of records) for (const n of [...r.addedNodes, ...r.removedNodes]) if ((n as Element).nodeName === 'DEBRIEF-SIDEBAR') w.__flips++;
    }).observe(document.querySelector('.body')!, { childList: true });
  });
  await body();
  return page.evaluate(() => (window as unknown as { __flips: number }).__flips);
}

/** Observation window for "it stays put": long enough for many debounced re-plans (150 ms each); it asserts an absence, so there is no condition to wait for. */
const OBSERVE_MS = 2000;

test('a stored wide Explorer in a narrow window settles at once instead of opening and closing forever', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('debrief.prefs.v1', JSON.stringify({ explorerWidth: 640 })));
  await page.setViewportSize({ width: 1100, height: 900 });
  await openApp(page);
  const flips = await flipsDuring(page, () => page.waitForTimeout(OBSERVE_MS));
  expect(flips).toBeLessThanOrEqual(0);
});

test('dragging the Explorer wider than the window allows stops at the room the plan has, and never closes it mid-drag', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 900 });
  await openApp(page);
  const sash = page.getByRole('separator', { name: 'Resize the Explorer' });
  const flips = await flipsDuring(page, async () => {
    const box = (await page.locator('debrief-sidebar').boundingBox())!;
    const x = box.x + box.width - 1;
    const y = box.y + box.height / 2;
    await page.mouse.move(x, y);
    await page.mouse.down();
    const max = Number(await sash.getAttribute('aria-valuemax'));
    expect(max).toBeLessThan(640);
    await page.mouse.move(x + 600, y, { steps: 20 });
    await page.waitForTimeout(OBSERVE_MS);
    await expect(page.locator('debrief-sidebar')).toBeVisible();
    await page.mouse.up();
    // Released: judged once. It stays open at the cap or, if the dock re-fitted, within it; never wider than the room found at the start.
    await expect.poll(() => sidebarWidth(page)).toBeLessThanOrEqual(max);
    expect(await sidebarWidth(page)).toBeGreaterThanOrEqual(Number(await sash.getAttribute('aria-valuemin')));
  });
  expect(flips).toBe(0);
  await expect(page.locator('debrief-sidebar')).toBeVisible();
});

test('the sash shows keyboard focus with the focus ring, not the hover strip', async ({ page }) => {
  await openApp(page);
  const sash = page.getByRole('separator', { name: 'Resize the Explorer' });
  await page.mouse.move(0, 0);
  await sash.focus();
  await page.keyboard.press('Shift');
  const style = await sash.evaluate((el) => {
    const c = getComputedStyle(el);
    return { outline: c.outlineStyle, width: c.outlineWidth, color: c.outlineColor, opacity: c.opacity, ring: getComputedStyle(document.documentElement).getPropertyValue('--focus-ring').trim() };
  });
  expect(style.outline).not.toBe('none');
  expect(style.opacity).toBe('1');
  expect(parseFloat(style.width)).toBeGreaterThanOrEqual(2);
});
