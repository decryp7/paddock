import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Locator, type Page } from '@playwright/test';

// Settings (plan S10) in the shell demo: the Settings window as a modeless tool dialog over the demo's in-memory engine,
// which serves the sections Rust wrote into the fixtures (every field kind). Chromium only in branches (rule 3).

const RESULT_KEY = 'paddock.dialog.browserResult';
const settingsUrl = (input: unknown = {}, query = '') =>
  `/${query}#/dialog?id=pd.settings&host=main&label=d-main-1&title=Settings&dialogInput=${encodeURIComponent(JSON.stringify(input))}`;
const field = (scope: Page | Locator, key: string) => scope.locator(`pd-setting-field[data-key="${key}"]`);
const nav = (page: Page) => page.getByRole('navigation', { name: 'Settings pages' });

async function axe(page: Page): Promise<string[]> {
  const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
  return results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`);
}

test.describe('settings (plan S10)', () => {
  test('the Settings window opens in the page from its command; a change reaches every window', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto('/');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await page.getByRole('button', { name: 'Settings…', exact: true }).click();
    const win = page.getByRole('dialog', { name: 'Settings' });
    await expect(win).toHaveAttribute('aria-modal', 'false');
    await expect(nav(page).getByRole('button')).toHaveText(['Appearance', 'General', 'Storage']);
    // The Appearance section is the shell's; its change applies to the main window at once.
    await field(win, 'theme').getByRole('combobox').selectOption('dark');
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect(win.getByRole('status').filter({ hasText: 'Theme saved.' })).toBeVisible();
    await field(win, 'theme').getByRole('button', { name: 'Reset to default: Theme' }).click();
    await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/);
    // The keyboard shortcut opens the same (single) window.
    await page.getByRole('button', { name: 'Say hello' }).focus();
    await page.keyboard.press('ControlOrMeta+Comma');
    await expect(page.getByRole('dialog', { name: 'Settings' })).toHaveCount(1);
    // The window's own Close (the page's dialog frame has one too).
    await win.getByText('Close', { exact: true }).click();
    await expect(win).toHaveCount(0);
    expect(errors).toEqual([]);
  });

  test('every field kind is rendered from the schema and its changes are applied', async ({ page }) => {
    await page.goto(settingsUrl({ page: 'sd.general' }));
    await expect(nav(page).locator('[aria-current="page"]')).toHaveText('General');
    await expect(field(page, 'notify').getByRole('switch', { name: 'Show a notification' })).toBeChecked();
    await expect(field(page, 'style').getByRole('combobox', { name: 'Style' })).toHaveValue('casual');
    await expect(field(page, 'greeting').getByRole('textbox', { name: 'Greeting' })).toHaveValue('Hello');
    await expect(field(page, 'repeat').getByRole('spinbutton', { name: 'Repeat' })).toHaveValue('1');
    await expect(field(page, 'volume')).toContainText('Needs restart');

    await field(page, 'notify').getByRole('switch').uncheck();
    await expect(page.getByRole('status').filter({ hasText: 'Show a notification saved.' })).toBeVisible();
    await field(page, 'volume').getByRole('spinbutton').fill('0.8');
    await field(page, 'volume').getByRole('spinbutton').press('Enter');
    await expect(page.getByTestId('settings-restart')).toContainText('1 change takes effect after the app restarts (Volume).');

    await nav(page).getByRole('button', { name: 'Storage' }).click();
    await expect(field(page, 'folder').getByRole('textbox', { name: 'Data folder' })).toHaveValue('');
    await expect(field(page, 'keep')).toContainText('min');
    await expect(field(page, 'cache')).toContainText('MB');
    await expect(field(page, 'ignore')).toHaveCount(0);
    await page.getByText('Show advanced').first().click();
    await expect(field(page, 'ignore').getByRole('textbox', { name: 'Ignored names' })).toHaveValue('*.tmp');
    await field(page, 'keep').getByRole('spinbutton').fill('90');
    await field(page, 'keep').getByRole('spinbutton').press('Tab');
    await expect(page.getByRole('status').filter({ hasText: 'Keep logs for saved.' })).toBeVisible();
    await field(page, 'folder').getByRole('textbox').fill('relative/path');
    await field(page, 'folder').getByRole('textbox').press('Enter');
    await expect(field(page, 'folder').getByRole('alert')).toHaveText('Must be an absolute path (or empty).');
  });

  test('a refused edit stays unsaved, is marked, and is asked about before the window closes', async ({ page }) => {
    await page.goto(settingsUrl({ page: 'sd.general' }));
    const greeting = field(page, 'greeting').getByRole('textbox', { name: 'Greeting' });
    await greeting.fill('Hi there!');
    await greeting.press('Enter');
    await expect(field(page, 'greeting').getByRole('alert')).toHaveText('Does not have the expected format ([A-Za-z ,]+).');
    await expect(greeting).toHaveAttribute('aria-invalid', 'true');
    await expect(nav(page).getByRole('button', { name: /General/ }).getByRole('img', { name: 'unsaved changes' })).toBeVisible();
    // Moving between pages keeps the edit.
    await nav(page).getByRole('button', { name: 'Storage' }).click();
    await nav(page).getByRole('button', { name: /General/ }).click();
    await expect(greeting).toHaveValue('Hi there!');
    // Escape asks; Keep editing keeps the window; Discard closes it.
    await page.keyboard.press('Escape');
    const ask = page.getByRole('alertdialog');
    await expect(ask).toContainText('Some changes are not saved: General.');
    expect(await page.evaluate((k) => localStorage.getItem(k), RESULT_KEY)).toBeNull();
    await ask.getByRole('button', { name: 'Keep editing' }).click();
    await expect(ask).toHaveCount(0);
    await page.getByRole('button', { name: 'Close', exact: true }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Discard changes' }).click();
    await expect.poll(() => page.evaluate((k) => localStorage.getItem(k), RESULT_KEY)).toBe('{"result":null}');
  });

  test('search finds settings however they are written, and the window is keyboard operable', async ({ page }) => {
    await page.goto(settingsUrl());
    const search = page.getByRole('searchbox', { name: 'Search settings' });
    await search.fill('DataFodler');
    await expect(page.locator('pd-setting-field')).toHaveCount(1);
    await expect(field(page, 'folder')).toBeVisible();
    await search.fill('ignored names');
    await expect(field(page, 'ignore')).toBeVisible();
    // Escape clears a filled search without closing the window.
    await search.press('Escape');
    await expect(search).toHaveValue('');
    await expect(page.getByTestId('settings-content').locator('pd-settings-section')).toHaveCount(1);
    // The pages by arrow keys.
    await nav(page).getByRole('button', { name: 'Appearance' }).focus();
    await page.keyboard.press('ArrowDown');
    await expect(nav(page).getByRole('button', { name: 'General' })).toBeFocused();
    await page.keyboard.press('End');
    await expect(nav(page).getByRole('button', { name: 'Storage' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(nav(page).locator('[aria-current="page"]')).toHaveText('Storage');
    // Nothing unsaved: Escape closes.
    await page.keyboard.press('Escape');
    await expect.poll(() => page.evaluate((k) => localStorage.getItem(k), RESULT_KEY)).toBe('{"result":null}');
  });

  test('a damaged settings file gives defaults, one notification and a note in the Settings window', async ({ page }) => {
    await page.goto('/?settings=damaged');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await page.getByRole('contentinfo', { name: 'Status bar' }).getByTestId('notifications-status').click();
    const centre = page.getByRole('region', { name: 'Notifications centre' });
    const notice = centre.locator('[data-cause="settings:file"]');
    await expect(notice).toHaveCount(1);
    await expect(notice).toContainText('Settings were reset to defaults');
    await expect(notice).not.toContainText('×');
    // Its action opens Settings, which says what happened.
    await notice.getByRole('button', { name: 'Open Settings…' }).click();
    const win = page.getByRole('dialog', { name: 'Settings' });
    await expect(win.getByTestId('settings-problem')).toContainText('Defaults are in use; the damaged file was kept as settings.json.corrupt');
    await expect(field(win, 'theme').getByRole('combobox')).toHaveValue('system');
    await win.getByText('Close', { exact: true }).click();
    await expect(win).toHaveCount(0);
  });

  for (const colorScheme of ['dark', 'light'] as const) {
    test(`the Settings window has no axe violations (${colorScheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme });
      await page.goto(settingsUrl({ page: 'sd.general' }, '?settings=damaged'));
      await expect(field(page, 'greeting')).toBeVisible();
      await field(page, 'greeting').getByRole('textbox').fill('Hi!');
      await field(page, 'greeting').getByRole('textbox').press('Enter');
      await expect(field(page, 'greeting').getByRole('alert')).toBeVisible();
      expect(await axe(page)).toEqual([]);
      await page.getByRole('searchbox', { name: 'Search settings' }).fill('size');
      await expect(field(page, 'cache')).toBeVisible();
      expect(await axe(page)).toEqual([]);
    });
  }

  test('no horizontal scroll in the Settings window at 320 px (rule 18)', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await page.goto(settingsUrl({ page: 'sd.storage' }));
    await expect(field(page, 'folder')).toBeVisible();
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });
});
