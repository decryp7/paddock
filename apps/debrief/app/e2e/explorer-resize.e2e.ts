import { expect, Page, test } from '@playwright/test';
import { openApp } from './app';
import { MESSAGE_COLUMN_MIN } from '../src/app/grid/message-column';

// The Explorer is a view of the layout plan (ADR 0056) with a declared minimum and maximum width; the user resizes it by
// dragging the sash at its inner edge. It was a fixed 272 px column with no sash at all (reported 2026-10-27).
const SHOTS = process.env['EXPLORER_SHOTS'];

// Registry keeps its own column and the Explorer gives way to it, so a drag has room only in a window wider than 1440 px.
test.use({ viewport: { width: 1900, height: 900 } });

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

/** How many window plans the dock has run (`data-plan-runs` on the dock's element, set by the app after every plan). */
const planRuns = async (page: Page): Promise<number> =>
  Number((await page.locator('[data-plan-runs]').first().getAttribute('data-plan-runs').catch(() => null)) ?? 0);

/** Whether a re-plan is scheduled and has not run yet (`data-plan-pending` on the dock's element: the debounce and dockview's settle retries). */
const planPending = async (page: Page): Promise<boolean> => (await page.locator('[data-plan-runs]').first().getAttribute('data-plan-pending').catch(() => null)) === '1';

/**
 * Resolves once the dock is quiet: no re-plan is scheduled, and the count of plans run is the same on two reads a frame or more
 * apart (the polling interval). A layout that fights itself always has a re-plan pending or a count that moves, so it never gets there.
 */
async function planIdle(page: Page): Promise<void> {
  let last = -1;
  await expect
    .poll(
      async () => {
        const pending = await planPending(page);
        const now = await planRuns(page);
        const quiet = !pending && now > 0 && now === last;
        last = pending ? -1 : now;
        return quiet;
      },
      { intervals: [100], timeout: 15_000, message: 'the dock keeps re-planning' },
    )
    .toBe(true);
}

test('a stored wide Explorer in a narrow window settles at once instead of opening and closing forever', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('debrief.prefs.v1', JSON.stringify({ explorerWidth: 640 })));
  // Open wide, then shrink to a window where 640 px no longer fits beside the dock: the plan must settle, not open and close forever.
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page);
  await planIdle(page);
  const flips = await flipsDuring(page, async () => {
    await page.setViewportSize({ width: 1100, height: 900 });
    await planIdle(page);
  });
  expect(flips).toBe(0);
  await expect(page.locator('debrief-sidebar')).toBeVisible();
});

test('dragging the Explorer wider than the window allows stops at the room the plan has, and never closes it mid-drag', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 900 });
  await openApp(page);
  await planIdle(page);
  const sash = page.getByRole('separator', { name: 'Resize the Explorer' });
  const flips = await flipsDuring(page, async () => {
    const box = (await page.locator('debrief-sidebar').boundingBox())!;
    const x = box.x + box.width - 1;
    const y = box.y + box.height / 2;
    await page.mouse.move(x, y);
    await page.mouse.down();
    const max = Number(await sash.getAttribute('aria-valuemax'));
    expect(max).toBeLessThan(640);
    const plans = await planRuns(page);
    await page.mouse.move(x + 600, y, { steps: 20 });
    // The dock re-plans while the Explorer grows; it must not judge the Explorer in the middle of the drag.
    await expect.poll(() => planRuns(page)).toBeGreaterThan(plans);
    await expect(page.locator('debrief-sidebar')).toBeVisible();
    await page.mouse.up();
    // Released: judged once. It stays open at the cap or, if the dock re-fitted, within it; never wider than the room found at the start.
    await planIdle(page);
    expect(await sidebarWidth(page)).toBeLessThanOrEqual(max);
    expect(await sidebarWidth(page)).toBeGreaterThanOrEqual(Number(await sash.getAttribute('aria-valuemin')));
  });
  expect(flips).toBe(0);
  await expect(page.locator('debrief-sidebar')).toBeVisible();
});

/** The Registry group's text: it holds "Events" when Registry has been tabbed into the Events group. */
const registryTabbed = async (page: Page): Promise<boolean> =>
  (await page.locator('.dv-groupview').filter({ has: page.getByRole('tab', { name: 'Registry', exact: true }) }).innerText()).includes('Events');

const messageWidth = async (page: Page): Promise<number> =>
  (await page.locator('debrief-events-grid [role=row].row').first().locator('.msg').boundingBox())!.width;

test('at 1440 px an explicit resize wins: dragging to 500 px tabs Registry, keeps the message column, and the width survives a re-plan and a reload', async ({ page }) => {
  await page.addInitScript(() => { if (!sessionStorage.getItem('seeded')) { localStorage.clear(); sessionStorage.setItem('seeded', '1'); } });
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page);
  await planIdle(page);
  // By itself the layout prefers Registry's column: the Explorer is small.
  expect(await registryTabbed(page)).toBe(false);
  const start = await sidebarWidth(page);
  expect(start).toBeLessThan(300);
  const flips = await flipsDuring(page, async () => {
    await dragEdge(page, 500 - start);
    await planIdle(page);
  });
  await expect.poll(() => sidebarWidth(page)).toBe(500);
  await expect.poll(() => registryTabbed(page)).toBe(true);
  expect(await messageWidth(page)).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
  // A re-plan at the same window size keeps it.
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.setViewportSize({ width: 1440, height: 900 });
  await planIdle(page);
  expect(await sidebarWidth(page)).toBe(500);
  expect(await registryTabbed(page)).toBe(true);
  expect(flips).toBe(0);
  // And a reload.
  await page.reload();
  await expect(page.getByRole('grid', { name: 'Events' })).toBeVisible();
  await planIdle(page);
  await expect.poll(() => sidebarWidth(page)).toBe(500);
  expect(await registryTabbed(page)).toBe(true);
  expect(await messageWidth(page)).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
});

test('at 1440 px the keyboard resize goes as far as the message column allows, and only a narrower window makes the Explorer give way', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page);
  await planIdle(page);
  const sash = page.getByRole('separator', { name: 'Resize the Explorer' });
  await sash.focus();
  await page.keyboard.press('End');
  await planIdle(page);
  const max = Number(await sash.getAttribute('aria-valuemax'));
  expect(max).toBeGreaterThan(400);
  await expect.poll(() => sidebarWidth(page)).toBe(max);
  expect(await messageWidth(page)).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
  // Narrower: the Explorer gives way (the message column keeps its minimum); wider again: back at the chosen width.
  await page.setViewportSize({ width: 1200, height: 900 });
  await planIdle(page);
  expect(await sidebarWidth(page)).toBeLessThan(max);
  expect(await messageWidth(page)).toBeGreaterThanOrEqual(MESSAGE_COLUMN_MIN);
  await page.setViewportSize({ width: 1440, height: 900 });
  await planIdle(page);
  await expect.poll(() => sidebarWidth(page)).toBe(max);
});

test('an Explorer closed in the middle of a drag does not leave the plan held: the window can still narrow it away', async ({ page }) => {
  await page.setViewportSize({ width: 1300, height: 900 });
  await openApp(page);
  await planIdle(page);
  const box = (await page.locator('debrief-sidebar').boundingBox())!;
  await page.mouse.move(box.x + box.width - 1, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width + 20, box.y + box.height / 2, { steps: 4 });
  await page.keyboard.press('ControlOrMeta+b');
  await expect(page.locator('debrief-sidebar')).toHaveCount(0);
  await page.mouse.up();
  await page.keyboard.press('ControlOrMeta+b');
  await expect(page.locator('debrief-sidebar')).toBeVisible();
  await page.setViewportSize({ width: 700, height: 900 });
  await expect(page.locator('debrief-sidebar')).toHaveCount(0);
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
