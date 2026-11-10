import { expect, type Page, type Request } from '@playwright/test';
import { dialogWindowUrl, pressMod } from '../../../../ui/projects/paddock-e2e/src/index.ts';

/**
 * Opens a screen of the main window against the mock backend with no simulated latency; `?mock-static` stops the live ticker.
 * A screen that does not render fails with the requests the page was still waiting for (ci.yml run 7854: one module of
 * the lazy Apps page got no answer from the dev server, and the report said only that there was no h1).
 */
export async function openApp(page: Page, hash = '/', query = 'mock-fast&mock-static'): Promise<void> {
  // A new document even when only the hash differs (a dialog window's URL is the same page with another hash).
  if (page.url() !== 'about:blank') await page.goto('about:blank');
  const unanswered = trackUnanswered(page);
  try {
    await page.goto(`/?${query}#${hash}`);
    await expect(page.locator('pw-root')).toBeVisible();
    await expect(page.locator('h1').first()).toBeVisible();
  } catch (e) {
    const open = unanswered.list();
    if (!open.length) throw e;
    throw new Error(`${String(e)}\nRequests with no answer yet (${open.length}): ${open.join(', ')}`, { cause: e });
  } finally {
    unanswered.stop();
  }
}

/** The requests of `page` that started and have neither finished nor failed, by URL without its query. */
function trackUnanswered(page: Page): { list: () => string[]; stop: () => void } {
  const open = new Set<Request>();
  const started = (r: Request) => open.add(r);
  const ended = (r: Request) => open.delete(r);
  page.on('request', started);
  page.on('requestfinished', ended);
  page.on('requestfailed', ended);
  return {
    list: () => [...open].map((r) => r.url().split('?')[0] ?? r.url()),
    stop: () => {
      page.off('request', started);
      page.off('requestfinished', ended);
      page.off('requestfailed', ended);
    },
  };
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
