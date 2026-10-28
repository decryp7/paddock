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
