import { expect, Page, test } from '@playwright/test';
import { APP_LOAD_TIMEOUT } from './app';
import { expectNoSeriousViolations } from './axe';
import { hotspots, openProfiles, runCommand, showCaptures } from './profiling-support';

/** Clicks the timeline's chip with this name, or its entry in the "+N" menu when it did not fit in the strip. */
async function openFromStrip(page: Page, name: string): Promise<void> {
  const timeline = page.locator('debrief-timeline-panel');
  const chip = timeline.getByRole('button', { name, exact: true });
  if (await chip.count()) {
    await chip.click();
    return;
  }
  await timeline.getByTestId('chip-more').click();
  await page.getByRole('menuitem', { name, exact: true }).click();
}

test.describe('Profiling with the logs', () => {
  test('capture spans on the log timeline open the profile', async ({ page }, testInfo) => {
    await openProfiles(page);
    // Opening a capture switched to the Profiling perspective; the log timeline is in Investigate.
    await page.keyboard.press('ControlOrMeta+1');
    await page.getByRole('tab', { name: /Timeline/ }).click();
    await runCommand(page, 'Profiling: show captures');
    // Two rows of chips in the header strip; the rest of the five captures are in its "+N" menu.
    const timeline = page.locator('debrief-timeline-panel');
    await expect(timeline.getByTestId('profile-span').or(timeline.getByTestId('chip-more'))).not.toHaveCount(0);
    expect((await timeline.getByTestId('profile-span').count()) + Number((await timeline.getByTestId('chip-more').textContent())?.replace('+', '') ?? 0)).toBe(5);
    await expectNoSeriousViolations(page, testInfo, 'profiling-captures', 'debrief-captures-view');

    // Make the other profile active, then open the first from the timeline.
    await (await showCaptures(page)).getByRole('button', { name: 'orders-regression.speedscope.json', exact: true }).click();
    await page.getByRole('tab', { name: /Timeline/ }).click();
    await openFromStrip(page, 'Open capture orders.speedscope.json');
    await expect(hotspots(page).getByRole('combobox', { name: 'Active profile' }).locator('option:checked')).toHaveText('orders.speedscope.json');

    await page.getByRole('tab', { name: /Timeline/ }).click();
    // Nothing (ADR 0027: the sticky header, its tick labels) may paint over a chip or a lane marker: the point under the
    // centre of every capture span, dump chip, note flag and lane marker or run below the header is that element.
    // Inert covers (pointer-events: none) are made live for the probe, so a label that only looks harmless still fails it.
    await page.mouse.move(0, 0);
    const probe = await page.addStyleTag({ content: 'debrief-timeline-panel :not(.overlay, .overlay *) { pointer-events: auto !important }' });
    await expect(timeline.locator('[data-testid=profile-span], [data-testid=dump-marker], [data-testid=note-mark]')).not.toHaveCount(0);
    await expect(timeline.locator('debrief-timeline-lanes .marker')).not.toHaveCount(0);
    await expect
      .poll(() =>
        timeline.evaluate((panel) => {
          const header = panel.querySelector('.sticky')!.getBoundingClientRect();
          const scroller = panel.getBoundingClientRect();
          const covered: string[] = [];
          const check = (el: Element, name: string, belowHeader: boolean) => {
            const r = el.getBoundingClientRect();
            const [cx, cy] = [r.x + r.width / 2, r.y + r.height / 2];
            if (belowHeader && (cy < header.bottom || cy > scroller.bottom || r.width === 0)) return;
            if (!el.contains(document.elementFromPoint(cx, cy))) covered.push(name);
          };
          panel.querySelectorAll('[data-testid=profile-span], [data-testid=dump-marker], [data-testid=note-mark]').forEach((el) => check(el, el.getAttribute('aria-label') ?? 'chip', false));
          panel.querySelectorAll('debrief-timeline-lanes .marker, debrief-timeline-lanes debrief-run-track .run').forEach((el) => check(el, el.getAttribute('aria-label') ?? 'lane item', true));
          return covered;
        }),
      )
      .toEqual([]);
    await probe.evaluate((el) => el.remove());
    await openFromStrip(page, 'Open heap snapshot heap-2.heapsnapshot');
    await expect(page.locator('debrief-heap-view').getByRole('combobox', { name: 'Heap snapshot' }).locator('option:checked')).toHaveText('heap-2.heapsnapshot');
  });

  test('the Moment view lists profile samples in the window and opens the profile on that range', async ({ page }) => {
    await openProfiles(page);
    await runCommand(page, 'Analyse this moment');
    const moment = page.locator('debrief-moment-view');
    const section = moment.locator('debrief-profile-moment');
    await expect(section).toContainText('Profile samples in this window');
    await expect(section.getByRole('region', { name: 'Profile orders.speedscope.json' })).toContainText('Shop.Orders.OrderService.Place');
    await section.getByRole('region', { name: 'Profile orders.speedscope.json' }).getByRole('button', { name: 'Open capture on this range' }).click();
    await expect(hotspots(page).getByTestId('range-pill')).toBeVisible();
  });

  test('a failed query shows an inline error with Retry', async ({ page }) => {
    await openProfilesWithFailure(page);
    const view = hotspots(page);
    await expect(view.getByRole('alert')).toContainText('hotspots failed');
    await view.getByRole('button', { name: 'Retry' }).click();
    await expect(view.locator('tbody td.name button').first()).toBeVisible();
  });
});

async function openProfilesWithFailure(page: import('@playwright/test').Page): Promise<void> {
  await page.goto('/?mockProfilingFail=hotspots:1');
  await expect(page.locator('debrief-status-bar')).toBeVisible({ timeout: APP_LOAD_TIMEOUT });
  await expect(page.locator('debrief-status-bar')).toContainText('events');
  await runCommand(page, 'Open performance capture');
  await expect(hotspots(page)).toBeVisible();
}
