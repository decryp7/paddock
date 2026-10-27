import { expect, type Page } from '@playwright/test';

/** Opens a screen against the mock backend with no simulated latency; `?mock-static` stops the live ticker. */
export async function openApp(page: Page, hash = '/', query = 'mock-fast&mock-static'): Promise<void> {
  await page.goto(`/?${query}#${hash}`);
  await expect(page.locator('pw-root')).toBeVisible();
  await expect(page.locator('h1').first()).toBeVisible();
}
