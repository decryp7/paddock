import { expect, pinnedClockTest, test as kit } from '@decryp7/paddock-e2e';

/**
 * Dyno's `test`: the kit's (browser guard, `app`, `pageErrors`) with the page clock pinned to one fixed instant, zone and
 * locale (`pinnedClockTest`), so a mock's relative stamps and the app's day grouping agree at every wall-clock time (CI run
 * 8154). Specs import `test` and `expect` from here, never from `@playwright/test` (conformance C133). A spec that needs the
 * real time says why: `test.use({ pinnedClock: false })`. `PADDOCK_E2E_NOW` moves the pin.
 */
export const test = pinnedClockTest(kit);
export { expect };
