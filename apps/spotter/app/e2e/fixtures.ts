import { expect, test as base } from '@playwright/test';

/**
 * The instant every Spotter e2e page starts at: mid-day UTC. The mock backend stamps its items relative to the page clock
 * and the feed groups them by local calendar day, so a run just after midnight had no item from "Today" (CI run 8154,
 * 00:19 UTC). One fixed instant and zone make the grouping the same at every wall-clock time. `SPOTTER_E2E_NOW` (an ISO
 * instant) overrides it to reproduce another time of day; a value just after local midnight makes flows and today-clock fail on
 * purpose (nothing is from today then), and an unreadable one throws.
 */
const DEFAULT_NOW = '2026-11-16T12:00:00Z';
const raw = process.env['SPOTTER_E2E_NOW'] || DEFAULT_NOW;
export const E2E_NOW = new Date(raw);
if (Number.isNaN(E2E_NOW.getTime())) {
  throw new Error(`SPOTTER_E2E_NOW: "${raw}" is not a date. Use an ISO instant such as ${DEFAULT_NOW}.`);
}

/** The zone and locale of every e2e page, the screenshots project's too. */
export const E2E_ZONE = 'UTC';
export const E2E_LOCALE = 'en-US';

/** `test` with the page clock installed before the first navigation, ticking on from {@link E2E_NOW} (timers keep working). */
export const test = base.extend({
  timezoneId: E2E_ZONE,
  locale: E2E_LOCALE,
  page: async ({ page }, use) => {
    await page.clock.install({ time: E2E_NOW });
    await use(page);
  },
});

export { expect };
