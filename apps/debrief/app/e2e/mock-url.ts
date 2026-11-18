import { expectDockPlanned } from '../../../../ui/projects/paddock-e2e/src/ready.ts';
import { expect, Page } from '@playwright/test';
import { loadWait } from './app';

/**
 * Opens the app with the mock backend's URL switches (see src/app/core/mock/mock-options.ts), e.g.
 * `?mockTotal=50000000`. Only waits for the shell and the events grid, since some switches make
 * other panels fail on purpose.
 */
export async function openAppWith(page: Page, search: string, options: { grid?: boolean } = {}): Promise<void> {
  // The first load builds the demo workspace; under a full parallel run it takes longer than one assertion. Its waits share
  // one budget (APP_LOAD_TIMEOUT, measured there), the dock's planning included.
  const load = loadWait();
  await page.goto(`/${search}`);
  await expect(page.locator('debrief-status-bar')).toContainText('events', { timeout: load.timeout() });
  if (options.grid ?? true) {
    await expect(page.locator('debrief-events-grid')).toBeVisible({ timeout: load.timeout() });
    await expectDockPlanned(page, { timeout: load.timeout() });
  }
}

/** The events grid's scroll container. */
export const viewport = (page: Page) => page.locator('debrief-events-grid .viewport');

export { gridRows } from './app';

/** Parses the id out of "Synthetic event 123". */
export function eventId(text: string | null): number {
  const m = /Synthetic event (\d+)/.exec(text ?? '');
  if (!m) throw new Error(`not a synthetic row: ${text}`);
  return Number(m[1]);
}
