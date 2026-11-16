import type { Browser, BrowserContext, BrowserContextOptions, PlaywrightTestArgs, PlaywrightTestOptions, PlaywrightWorkerArgs, PlaywrightWorkerOptions, TestType } from '@playwright/test';

/**
 * The default instant of every pinned page, and of the documentation screenshots (`FROZEN_AT` in doc-screens.ts is this value): a
 * Thursday, 14:30 UTC, so "today" holds a mock's items at any wall-clock time.
 */
export const PINNED_AT = Date.UTC(2026, 9, 22, 14, 30, 0);
/** {@link PINNED_AT} as an ISO instant. */
export const PINNED_NOW = new Date(PINNED_AT).toISOString();

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
 * Runs in the page after the clock is installed. Playwright's fake clock catches an exception a timer callback throws (its
 * `_updateRealTimeTimer` ends in `.catch(console.error)`, and `_runTo` keeps only the first error of a batch; clock.ts of
 * playwright-core 1.64.1, read 2026-11-16), so a bug thrown from `setTimeout`, `setInterval`, `requestAnimationFrame` or
 * `requestIdleCallback` reached neither `window` 'error' nor `pageerror`. This wraps the four so each callback runs in its own
 * try/catch and reports through `reportError`, the way the browser reports a thrown timer callback. Ids, string callbacks and
 * the other arguments pass through unchanged. Serialised into the page: it must not use anything from this module.
 */
export function reportTimerErrors(): void {
  const w = window as unknown as Record<string, unknown> & { reportError(e: unknown): void };
  const wrap = (callback: unknown) =>
    typeof callback !== 'function'
      ? callback
      : function (this: unknown, ...args: unknown[]) {
          try {
            return (callback as (...a: unknown[]) => unknown).apply(this, args);
          } catch (e) {
            w.reportError(e);
            return undefined;
          }
        };
  for (const name of ['setTimeout', 'setInterval', 'requestAnimationFrame', 'requestIdleCallback']) {
    const original = w[name];
    if (typeof original !== 'function') continue;
    w[name] = function (this: unknown, callback: unknown, ...rest: unknown[]) {
      return (original as (...a: unknown[]) => unknown).call(this, wrap(callback), ...rest);
    };
  }
}

/**
 * `base` with the page clock installed before the first navigation of every context: the same fixed instant for a mock
 * backend's stamps and for the app's day grouping (CI run 8154: a run just after midnight had no "Today"). The clock
 * ticks on from the instant, so timers and animation frames keep working; a spec that calls `page.clock.install` or
 * `setFixedTime` itself still can (the later call wins, Playwright 1.64). A throw from a timer callback is reported as in a real browser ({@link reportTimerErrors}). `PADDOCK_E2E_NOW` moves the pin for a sweep.
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
      if (pinnedClock) {
        await context.clock.install({ time: pinnedInstant(pinnedClock) });
        await context.addInitScript(reportTimerErrors);
      }
      await use(context);
    },
  });
  return extended as unknown as TestType<T & PinnedClockFixtures, W>;
}

/**
 * A context of its own (a window size, a user agent, a colour scheme) with the same pin as the fixture's: the clock installed at
 * {@link PINNED_NOW} (or `pin.now`, or `PADDOCK_E2E_NOW`), the zone and locale of the wrapper unless `options` names its own, and
 * {@link reportTimerErrors}. `browser.newContext()` makes none of that; a spec that needs a context beyond the fixtures' calls this.
 * `pin: false` returns the context as `browser.newContext` makes it (a spec that needs the real time, with its reason on the line above).
 */
export async function newPinnedContext(
  browser: Browser,
  options: BrowserContextOptions & { readonly pin?: PinnedClockSetting } = {},
): Promise<BrowserContext> {
  const { pin = {}, ...contextOptions } = options;
  if (pin === false) return browser.newContext(contextOptions);
  const context = await browser.newContext({ timezoneId: pin.timezoneId ?? PINNED_ZONE, locale: pin.locale ?? PINNED_LOCALE, ...contextOptions });
  await context.clock.install({ time: pinnedInstant(pin) });
  await context.addInitScript(reportTimerErrors);
  return context;
}
