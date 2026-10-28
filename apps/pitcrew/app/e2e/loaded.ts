import { expect, type Page } from '@playwright/test';
import { waitForStableFrame } from '@decryp7/paddock-e2e';

/** Waits for the screen to be what a user sees: no skeleton or splash, fonts and images decoded, the layout settled. */
export async function loaded(page: Page): Promise<void> {
  await expect(page.locator('pd-splash')).toHaveCount(0);
  await expect(page.locator('pd-skeleton')).toHaveCount(0);
  await page.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all([...document.images].map((img) => img.decode().catch(() => undefined)));
  });
  await waitForStableFrame(page);
}
