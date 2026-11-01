import { expect, type Page } from '@playwright/test';
import { dialogWindowUrl, pressMod } from '../../../../ui/projects/paddock-e2e/src/index.ts';

/** Opens a screen of the main window against the mock backend with no simulated latency; `?mock-static` stops the live ticker. */
export async function openApp(page: Page, hash = '/', query = 'mock-fast&mock-static'): Promise<void> {
  // A new document even when only the hash differs (a dialog window's URL is the same page with another hash).
  if (page.url() !== 'about:blank') await page.goto('about:blank');
  await page.goto(`/?${query}#${hash}`);
  await expect(page.locator('pw-root')).toBeVisible();
  await expect(page.locator('h1').first()).toBeVisible();
}

/**
 * Opens Settings from the main window as the user does (Mod+,): in the browser build the Settings window opens in the page,
 * over the main window, which stays usable (it is a modeless tool). Resolves once Pitwall's page shows its settings.
 */
export async function openSettings(page: Page, query = 'mock-fast&mock-static'): Promise<void> {
  await openApp(page, '/', query);
  await pressMod(page, 'Mod+Comma');
  await expect(settingsDialog(page)).toBeVisible();
}

/** The Settings window as the page shows it. */
export function settingsDialog(page: Page) {
  return page.getByRole('dialog', { name: 'Settings' });
}

/** Opens the Settings window by its URL, as Rust loads it in its own OS window. */
export async function openSettingsWindow(page: Page, query = 'mock-fast&mock-static'): Promise<void> {
  await page.goto(dialogWindowUrl('pd.settings', {}, { title: 'Settings', query: `?${query}` }));
  await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
}
