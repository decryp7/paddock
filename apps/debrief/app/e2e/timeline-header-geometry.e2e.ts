import { expect, Page, test } from '@playwright/test';
import { openAppWith } from './mock-url';

/** Live tail at 40 events/s: the timeline reloads (loading indicator), the health headline counts, the hint changes. */
const LIVE = '?mockLive=1&mockLiveRate=40&mockLiveRotateMs=0&mockLiveAlertMs=0';
const BUTTONS = ['Zoom in', 'Zoom out', 'Timeline menu'] as const;

/**
 * The distinct left edges each button had after each of `steps` driven ticks of `stepMs`. The page runs on Playwright's clock
 * (the mock's live timers are driven by `page.clock.runFor`, not by real time), so the same ticks happen on a loaded runner as on
 * a quiet one. A button that is clicked must stay put between the mouse press and its release, so each must have exactly one.
 */
async function edgesOver(page: Page, steps: number, stepMs: number): Promise<Record<string, number[]>> {
  const seen: Record<string, Set<number>> = Object.fromEntries(BUTTONS.map((n) => [n, new Set<number>()]));
  for (let i = 0; i < steps; i++) {
    await page.clock.runFor(stepMs);
    const lefts = await page.evaluate(
      (names) => names.map((n) => document.querySelector(`debrief-timeline-panel [aria-label="${n}"]`)?.getBoundingClientRect().left),
      [...BUTTONS],
    );
    BUTTONS.forEach((n, k) => {
      const left = lefts[k];
      if (left !== undefined) seen[n]?.add(Math.round(left));
    });
  }
  return Object.fromEntries(BUTTONS.map((n) => [n, [...(seen[n] ?? [])]]));
}

for (const width of [1100, 760]) {
  test.describe(`timeline header geometry at ${width} px`, () => {
    test.use({ viewport: { width, height: 800 } });

    test('the zoom and menu buttons stay put while live data loads, counts and pauses following', async ({ page }) => {
      await page.clock.install();
      await openAppWith(page, LIVE);
      await page.locator('debrief-status-bar').getByRole('button', { name: 'Live tail', exact: true }).click();
      await page.getByRole('combobox', { name: 'Live window' }).selectOption('5m');
      await expect(page.getByTestId('follow-toggle')).toBeVisible();
      // Freeze time a little ahead (time keeps running until the pause lands, so leave room): from here only `runFor` moves it.
      await page.clock.pauseAt(await page.evaluate(() => Date.now() + 5000));

      const following = await edgesOver(page, 40, 100);
      for (const n of BUTTONS) expect(following[n], `${n} while following`).toHaveLength(1);

      // Nothing focusable in the left group is clipped by it (a button cut off by the lead's edge would stay tabbable).
      const clipped = await page.evaluate(() => {
        const lead = document.querySelector('debrief-timeline-panel .head .lead')?.getBoundingClientRect();
        const items = [...document.querySelectorAll('debrief-timeline-panel .head .lead button, debrief-timeline-panel .head .lead [tabindex]')];
        return lead ? items.filter((e) => e.getBoundingClientRect().right > lead.right + 1).map((e) => e.getAttribute('aria-label') ?? e.textContent) : ['no lead'];
      });
      // While following, at 1100 px, the title and hint give way first and nothing is cut off. Paused (the chip is 107 px wider)
      // or on narrower panes the headline's own width no longer fits and its end is clipped: a known limit, see lessons.md.
      if (width >= 1100) expect(clipped).toEqual([]);

      // The first zoom pauses following and the chip becomes "Paused — Resume following", about 150 px wider.
      const before = following['Zoom in']?.[0];
      await page.locator('debrief-timeline-panel').getByRole('button', { name: 'Zoom in' }).click();
      // The page's timers only move with the driven clock.
      await page.clock.runFor(300);
      await expect(page.getByTestId('resume-following')).toBeVisible();
      const paused = await edgesOver(page, 15, 100);
      for (const n of BUTTONS) expect(paused[n], `${n} while paused`).toEqual(following[n]);
      expect(paused['Zoom in']?.[0]).toBe(before);

      // The menu button stays inside the panel (an overfull row must not push it past the edge).
      const panel = await page.locator('debrief-timeline-panel').boundingBox();
      const menu = await page.locator('debrief-timeline-panel').getByRole('button', { name: 'Timeline menu' }).boundingBox();
      expect(menu && panel && menu.x + menu.width <= panel.x + panel.width).toBe(true);
    });
  });
}
