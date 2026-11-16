import { expect, test as base } from '@playwright/test';
import { pinnedClockTest } from '../../../../ui/projects/paddock-e2e/src/pinned-clock.ts';

/**
 * The app's `test`: Playwright's with the page clock pinned to one fixed instant, zone and locale (`pinnedClockTest`), so a
 * mock backend's relative stamps and the app's day grouping agree at every wall-clock time (CI run 8154). Specs import `test`
 * and `expect` from here, never from `@playwright/test` (conformance C133). A spec that needs the real time says why:
 * `test.use({ pinnedClock: false })`. `PADDOCK_E2E_NOW` moves the pin.
 */
export const test = pinnedClockTest(base);
export { expect };
