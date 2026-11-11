import { expect, test } from '@decryp7/paddock-e2e';

/**
 * Dyno's buttons are the family's height: 32 px at the default density and 28 px at Compact (WP-14, decision 2 of the 1.1.1 plan). Dyno
 * used to override `--pd-button-height` with its own control height (28 and 24 px); the override is gone, so the one family token decides.
 * Icon buttons are square and sized by their own token, so only text buttons are measured.
 */
const PAGES = ['/alarms', '/network', '/firewall', '/lists', '/power'];
const DENSITIES = [
  { density: 'standard', height: 32 },
  { density: 'compact', height: 28 },
] as const;

for (const d of DENSITIES) {
  for (const hash of PAGES) {
    test(`${hash} at ${d.density} density: text buttons are ${d.height} px tall`, async ({ page }) => {
      await page.goto(`/?latency=0#${hash}`);
      await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
      await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]'), undefined, { timeout: 30_000 });
      await page.evaluate((v) => document.documentElement.setAttribute('data-density', v), d.density);
      const heights = await page.evaluate(() =>
        [...document.querySelectorAll<HTMLElement>('#pd-main .mat-mdc-button-base:not(.mat-mdc-icon-button)')]
          .filter((b) => b.getBoundingClientRect().height > 0)
          .map((b) => ({ text: (b.textContent ?? '').trim().slice(0, 30), height: Math.round(b.getBoundingClientRect().height) })),
      );
      expect(heights.length, `${hash} shows at least one text button`).toBeGreaterThan(0);
      for (const h of heights) expect(h.height, `button "${h.text}"`).toBe(d.height);
    });
  }
}
