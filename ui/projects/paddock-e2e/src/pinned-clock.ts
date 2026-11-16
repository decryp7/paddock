import type { PlaywrightTestArgs, PlaywrightTestOptions, PlaywrightWorkerArgs, PlaywrightWorkerOptions, TestType } from '@playwright/test';

/** The default instant of every pinned page: a Monday, 12:00 UTC, so "today" holds the mock's items at any wall-clock time. */
export const PINNED_NOW = '2026-11-16T12:00:00Z';

/** The default zone and locale of a pinned page (a spec that sets its own, such as a screenshots project, can reuse them). */
export const PINNED_ZONE = 'UTC';
export const PINNED_LOCALE = 'en-US';

/** The environment variable that moves the pin (an ISO instant), to run a suite at another time of day. */
export const PINNED_NOW_ENV = 'PADDOCK_E2E_NOW';

/** What an app asks of {@link pinnedClockTest}. */
export interface PinnedClockOptions {
  /** The instant the page clock starts at (a `Date`, epoch milliseconds or an ISO string); default {@link PINNED_NOW}. */
  readonly now?: Date | number | string;
  /** The time zone of the pages (default {@link PINNED_ZONE}); the day grouping of a feed depends on it. */
  readonly timezoneId?: string;
  /** The locale of the pages (default {@link PINNED_LOCALE}). */
  readonly locale?: string;
}

/** The value of the `pinnedClock` option: the pin, or `false` for a spec that needs the real time. */
export type PinnedClockSetting = PinnedClockOptions | false;

/** The fixture the wrapper adds. */
export interface PinnedClockFixtures {
  /**
   * `test.use({ pinnedClock: false })` (with a one-line reason next to it) turns the pin off for a file or describe.
   * The zone and locale defaults stay, since Playwright reads them before a fixture can switch them.
   */
  readonly pinnedClock: PinnedClockSetting;
}

/** The instant a pin starts at: the environment (an ISO instant) first, then the option, then {@link PINNED_NOW}. Throws on an unreadable value. */
export function pinnedInstant(options: PinnedClockOptions, env: Readonly<Record<string, string | undefined>> = process.env): Date {
  const fromEnv = env[PINNED_NOW_ENV];
  const raw = fromEnv !== undefined && fromEnv !== '' ? fromEnv : (options.now ?? PINNED_NOW);
  const at = new Date(raw);
  if (Number.isNaN(at.getTime())) throw new Error(`${fromEnv ? PINNED_NOW_ENV : 'pinned clock now'}: "${String(raw)}" is not a date. Use an ISO instant such as ${PINNED_NOW}.`);
  return at;
}

/**
 * `base` with the page clock installed before the first navigation of every context: the same fixed instant for a mock
 * backend's stamps and for the app's day grouping (CI run 8154: a run just after midnight had no "Today"). The clock
 * ticks on from the instant, so timers and animation frames keep working; a spec that calls `page.clock.install` or
 * `setFixedTime` itself still can (the later call wins, Playwright 1.64). `PADDOCK_E2E_NOW` moves the pin for a sweep.
 * Wrap once per app and export the result as the app's `test`; specs import it from there.
 */
export function pinnedClockTest<T extends object, W extends object>(
  base: TestType<T, W>,
  options: PinnedClockOptions = {},
): TestType<T & PinnedClockFixtures, W> {
  // Typed on the plain Playwright test: the base's own fixtures stay, and the result is typed back to them below.
  const extended = (base as unknown as TestType<PlaywrightTestArgs & PlaywrightTestOptions, PlaywrightWorkerArgs & PlaywrightWorkerOptions>).extend<PinnedClockFixtures>({
    pinnedClock: [options, { option: true }],
    timezoneId: options.timezoneId ?? PINNED_ZONE,
    locale: options.locale ?? PINNED_LOCALE,
    context: async ({ context, pinnedClock }, use) => {
      if (pinnedClock) await context.clock.install({ time: pinnedInstant(pinnedClock) });
      await use(context);
    },
  });
  return extended as unknown as TestType<T & PinnedClockFixtures, W>;
}
