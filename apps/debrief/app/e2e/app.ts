import { expect, Locator, Page } from '@playwright/test';
import { expectDockPlanned } from '../../../../ui/projects/paddock-e2e/src/ready.ts';

/**
 * How long the first load of the demo workspace may take, from navigation until the grid shows its first rows (it is slower while
 * the whole suite runs in parallel). One budget for these waits (`loadWait`). The dock's planning, the load's last phase, gets what
 * is left of it as its first wait and then waits on while the dock takes steps (`expectDockPlanned`; the budget alone ran out on loaded
 * WebKit runners, debrief.yml runs 8559 and 8636). The planning grows with CPU contention, in WebKit most: debrief.yml run 8243 (2 workers on a loaded runner) planned 3.8 to 5.2 s
 * after the status bar showed the events and failed twice at the default 5 s; in the CI image with 4 CPUs, 1 worker took 0.05 to 1.9 s
 * and 4 workers 3.9 to 6.5 s, the whole load 7.1 to 10.5 s (8 loads each; docs/lessons.md, 2026-11-18).
 */
export const APP_LOAD_TIMEOUT = 15_000;

/**
 * The waits of one app load: each may use what is left of {@link APP_LOAD_TIMEOUT} from the moment this is called (just before
 * navigating), and never less than the project's expect timeout it had before.
 */
export function loadWait(): { timeout: () => number } {
  const deadline = Date.now() + APP_LOAD_TIMEOUT;
  return { timeout: () => Math.max(EXPECT_TIMEOUT, deadline - Date.now()) };
}

/** Playwright's own expect timeout, which Debrief's config keeps (it sets no `expect.timeout`). */
const EXPECT_TIMEOUT = 5_000;

/**
 * How long an error snapshot may take to appear: html-to-image clones and styles the whole window, which takes
 * about 2 s alone and several seconds while the suite runs in parallel, more than the default 5 s expect timeout.
 */
export const CAPTURE_TIMEOUT = 15_000;

/**
 * Loaded rows currently in the DOM (never more than a screenful plus overscan, whatever the total). A row whose page is
 * still loading is a `.placeholder` with the same role: it has no event, so clicking it selects nothing (jobs.e2e.ts
 * clicked one under load and the AI command stayed hidden).
 */
export const gridRows = (page: Page) => page.locator('debrief-events-grid [role=row].row:not(.placeholder)');

/** Opens the app in the browser build (MockBackend) and waits for the demo workspace to load. */
export async function openApp(page: Page): Promise<void> {
  const load = loadWait();
  await page.goto('/');
  await expect(page.getByRole('grid', { name: 'Events' })).toBeVisible({ timeout: load.timeout() });
  await expect(page.locator('debrief-status-bar')).toContainText('events', { timeout: load.timeout() });
  await expect(page.locator('debrief-timeline-panel .lane').first()).toBeVisible({ timeout: load.timeout() });
  await expect(gridRows(page).first()).toBeVisible({ timeout: load.timeout() });
  // The dock's planning gets what is left of the budget as its first wait (at least the expect timeout), then waits on as long as it
  // takes steps (expectDockPlanned): on loaded runners the budget alone ran out in runs 8559 and 8636.
  await expectDockPlanned(page, { firstTimeout: load.timeout() });
}

/**
 * Pauses the page's clock (installed with `page.clock.install()` before the page loaded) 5 s ahead of the page's own time.
 * Time keeps running until the pause lands, so the target must lie ahead; 5 s is a margin for a slow round trip on a loaded
 * runner, not a wait: the clock jumps there at once, and the timers due on the way run once each (a toast's alert, a status
 * item's tick), before anything is measured or clicked. From then on only `runFor` or `resume` moves time.
 */
export async function pauseAhead(page: Page): Promise<void> {
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 5000));
}

/**
 * Clicks `target` with the page's clock paused (`pauseAhead`), then lets time run again, also when the click fails.
 * Controls that move when a timer adds something next to them (a status item to the right of a right-anchored control: a
 * job shows 2 s after it was queued, the problems badge after its scan; a toast stack that reorders on a new alert) can move
 * between mousedown and mouseup, which then hit different elements, and the browser fires no click (debrief.yml run 8136, the
 * AI browsing chip's Stop; runs 8004 and 7941, the toasts; lessons.md). Paused, no timer can move them during the click.
 * This is the tests' guard only: the product fix that will replace it is ADR 0061 extended to the status bar and the toast
 * stack (approved by the user, 2026-11-15), so that a control does not move when an item appears beside it.
 */
export async function clickWithTimePaused(page: Page, target: Locator): Promise<void> {
  await pauseAhead(page);
  try {
    await target.click();
  } finally {
    await page.clock.resume();
  }
}

/** Parses a count like "40,123" or "40.1k" shown in the UI into a number. */
export function parseCount(text: string): number {
  return Number(text.replace(/[^\d]/g, ''));
}

/** Reads the unfiltered total once the demo workspace is loaded (the demo data may change). */
export async function initialTotal(page: Page): Promise<string> {
  const total = gridTotal(page);
  // The loaded total, not the "0" shown while the first import is still running (a slow WebKit run read that).
  await expect(total).toHaveText(/^[1-9][\d,]*$/);
  return total.innerText();
}

/** Total of matching events as shown in the grid bar. */
export function gridTotal(page: Page) {
  return page.locator('debrief-events-grid .bar .total');
}

/**
 * Resolves once the app has rendered what is already scheduled: after a frame and then a task (Angular renders on
 * whichever comes first). The sync point for asserting that something did NOT happen, instead of a fixed sleep.
 */
export async function nextRender(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0))));
}

/**
 * Docks the inspector (by default it is an auto-hidden tool window on the right edge that only slides out while an
 * event is selected): slides it out from its side-bar tab and pins it. Does nothing when it is docked already.
 */
export async function ensureInspector(page: Page): Promise<void> {
  const tab = page.locator('.debrief-ah-tab[data-ah-id="inspector"]');
  if ((await tab.count()) === 0) {
    await expect(page.locator('debrief-inspector')).toBeAttached();
    return;
  }
  const pin = page.locator('#debrief-ah-flyout').getByRole('button', { name: 'Pin Inspector' });
  if (!(await pin.isVisible())) await tab.click();
  await pin.click();
  await expect(tab).toHaveCount(0);
  await expect(page.locator('debrief-inspector')).toBeVisible();
}

/** Views that are not in the default layout, by id, with the activity-bar button that opens them. */
const VIEW_BUTTONS: Readonly<Record<string, string>> = { metrics: 'Metrics' };

/** Opens a view that is not part of the minimal default layout (e.g. Metrics) and waits for its tab. */
export async function openView(page: Page, id: keyof typeof VIEW_BUTTONS | string): Promise<void> {
  const name = VIEW_BUTTONS[id] ?? id;
  const tab = page.locator('.dv-tab', { has: page.locator(`.debrief-tab[data-panel-id="${id}"]`) });
  if ((await tab.count()) === 0) await page.getByRole('button', { name, exact: true }).click();
  await expect(tab).toBeVisible();
}

/**
 * Slides the auto-hidden inspector back in after a selection slid it out, so it does not cover the panels under it.
 * Does nothing when nothing is slid out.
 */
export async function hideInspectorFlyout(page: Page): Promise<void> {
  await nextRender(page);
  const flyout = page.locator('#debrief-ah-flyout');
  if (!(await flyout.isVisible())) return;
  await flyout.getByRole('button', { name: /^Hide / }).click();
  await expect(flyout).toHaveCount(0);
}

/** Opens the filter bar's View options (group by, Marked only, Always show start / stop). */
export async function openViewOptions(page: Page) {
  const button = page.getByRole('button', { name: 'View options' });
  if ((await button.getAttribute('aria-expanded')) !== 'true') await button.click();
  const dialog = page.getByRole('dialog', { name: 'View options' });
  await expect(dialog).toBeVisible();
  return dialog;
}

/** Expands the sidebar's Fields section (it starts collapsed). */
export async function expandFields(page: Page): Promise<void> {
  const toggle = page.getByTestId('fields-toggle');
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
}

/** The status bar's Details popover (sources, events, group-by, importers): opened, returned, and closed by `closeStatusDetails`. */
export async function openStatusDetails(page: Page) {
  await page.getByTestId('status-details').click();
  const popover = page.getByTestId('status-details-popover');
  await expect(popover).toBeVisible();
  return popover;
}

export async function closeStatusDetails(page: Page): Promise<void> {
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('status-details-popover')).toHaveCount(0);
}
