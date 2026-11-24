import type { PlaywrightTestConfig } from '@playwright/test';

type Trace = NonNullable<NonNullable<PlaywrightTestConfig['use']>['trace']>;

/**
 * The `use.trace` of a project running in `browserName` on `platform`: traces of failed tests, as every Paddock config keeps them.
 * WebKit on macOS keeps them without screenshots (and with DOM snapshots): a trace screenshot taken while the display sleeps
 * fills the WindowServer until it aborts (Playwright #43066; ADR 0120 H2, WP-0). Every other engine and OS is unchanged.
 */
export function paddockTrace(browserName: string, platform: string = process.platform): Trace {
  if (browserName === 'webkit' && platform === 'darwin') return { mode: 'retain-on-failure', screenshots: false, snapshots: true };
  return 'retain-on-failure';
}
