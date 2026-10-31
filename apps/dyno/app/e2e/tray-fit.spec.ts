import { expect, test } from '@decryp7/paddock-e2e';

/**
 * The default Dyno popup fits the family's popover cap (`--pd-tray-max-height`, 520 px) without scrolling, in its ordinary state and
 * in the states that make its widgets taller (an error with its cause, the agent stopped, stale readings, the empty rows). A popup that
 * scrolls on first open hides its third card, so the check measures the real component against the real token (Spotter's popup is held
 * to the same cap). Waits are on conditions, never sleeps.
 */
const POPUP = (query: string) => `/?latency=0${query}#/tray`;

const STATES: [string, string][] = [
  ['the default layout', ''],
  ['the agent stopped', '&agent=stopped'],
  ['every widget stale', '&popup=activeAlarms:stale,diskSpace:stale,cpuMemory:stale'],
  ['every widget with an error', '&popup=activeAlarms:error,diskSpace:error,cpuMemory:error'],
  ['every widget empty', '&popup=activeAlarms:empty,diskSpace:empty,cpuMemory:empty'],
];

test.describe('the popup fits its cap', () => {
  for (const [name, query] of STATES) {
    test(`${name}: at most 520 px and nothing to scroll`, async ({ page }) => {
      await page.setViewportSize({ width: 360, height: 900 });
      await page.goto(POPUP(query));
      await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
      await expect(page.locator('pd-widget-frame')).toHaveCount(3);
      const cap = await page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--pd-tray-max-height')));
      expect(cap).toBe(520);
      const popover = page.locator('pd-tray-popover');
      await expect(async () => {
        const box = (await popover.boundingBox())!;
        const overflow = await page.locator('pd-tray-popover [role="region"]').evaluate((el) => el.scrollHeight - el.clientHeight);
        expect(box.height, `the popup is ${box.height} px; its content is ${overflow} px taller than the room it has`).toBeLessThanOrEqual(cap);
        expect(overflow, `the content scrolls by ${overflow} px in a popup of ${box.height} px`).toBeLessThanOrEqual(1);
      }).toPass({ timeout: 10_000 });
    });
  }
});
