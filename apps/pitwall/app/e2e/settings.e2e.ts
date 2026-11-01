import { expect, test, type Locator } from '@playwright/test';
import { openApp, openSettings, openSettingsWindow, settingsDialog } from './app';
import { pressMod } from '../../../../ui/projects/paddock-e2e/src/index.ts';

test.describe('settings', () => {
  test('opens from Mod+, as the Settings window, not as a page of the main window', async ({ page }) => {
    await openApp(page, '/settings');
    // The old route is gone: it lands on the first page.
    await expect(page.getByRole('heading', { name: 'Dashboard', level: 1 })).toBeVisible();
    await pressMod(page, 'Mod+Comma');
    const settings = settingsDialog(page);
    await expect(settings.getByRole('heading', { name: 'Pitwall' })).toBeVisible();
    for (const name of ['Pitwall', 'Appearance']) await expect(settings.getByText(name, { exact: true }).first()).toBeVisible();
  });

  test('turns a switch on; it is saved by itself and still on when Settings opens again', async ({ page }) => {
    await openSettings(page);
    const settings = settingsDialog(page);
    await expect(settings.getByText(/when you save|unsaved changes/i)).toHaveCount(0);
    await settings.getByRole('switch', { name: 'Serve other devices on your network' }).click();
    await expect(settings.getByTestId('save-state')).toHaveText('');
    await expect(settings.getByRole('button', { name: /Save changes/ })).toHaveCount(0);
    // Closing right away loses nothing: the page saves what waits when it goes.
    await settings.getByRole('button', { name: 'Close' }).last().click();
    await expect(settings).toHaveCount(0);
    await pressMod(page, 'Mod+Comma');
    await expect(settingsDialog(page).getByRole('switch', { name: 'Serve other devices on your network' })).toBeChecked();
  });

  test('switches the theme on the shell’s Appearance page', async ({ page }) => {
    await openSettingsWindow(page);
    await page.getByText('Appearance', { exact: true }).first().click();
    for (const theme of ['Dark', 'Light']) {
      await page.getByRole('combobox', { name: /Theme/ }).click();
      await page.getByRole('option', { name: theme }).click();
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
      // The list is still closing for a moment, and a click on the field then lands on it: wait until it is gone.
      await expect(page.getByRole('option')).toHaveCount(0);
    }
  });

  test('in its own window, a save that fails names the setting, keeps the change and offers Retry', async ({ page }) => {
    await openSettingsWindow(page, 'mock-fast&mock-static&mock-fail=saveSettings');
    const keyless = page.getByRole('switch', { name: 'Accept local requests without a key' });
    await keyless.click();
    const error = page.getByTestId('save-error');
    await expect(error).toContainText('Accept local requests without a key could not be saved:');
    await expect(error.getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(keyless).toBeChecked();
    await error.getByRole('button', { name: 'Undo accept local requests without a key change' }).click();
    await expect(error).toHaveCount(0);
    await expect(keyless).not.toBeChecked();
  });

  test('a number out of range says what to fix next to it and is not saved', async ({ page }) => {
    await openSettingsWindow(page);
    const port = page.getByRole('spinbutton', { name: 'Port' });
    await port.fill('80');
    await port.press('Tab');
    await expect(port).toHaveAttribute('aria-invalid', 'true');
    await expect(page.locator('#set-server')).toContainText('Enter a number from 1024 to 65535.');
    await port.fill('12000');
    await port.press('Tab');
    await expect(port).toHaveAttribute('aria-invalid', 'false');
    await expect(page.getByTestId('save-error')).toHaveCount(0);
  });

  // VM check 'Settings: a quick close keeps the edit' (testing-manual.md) in the browser build: Mod+W right after the edit.
  for (const [what, edit, check] of [
    [
      'a switch',
      async (d: Locator) => d.getByRole('switch', { name: 'Serve other devices on your network' }).click(),
      async (d: Locator) => expect(d.getByRole('switch', { name: 'Serve other devices on your network' })).toBeChecked(),
    ],
    [
      'a typed number',
      async (d: Locator) => d.getByRole('spinbutton', { name: 'Requests at once' }).fill('7'),
      async (d: Locator) => expect(d.getByRole('spinbutton', { name: 'Requests at once' })).toHaveValue('7'),
    ],
  ] as const) {
    test(`closing Settings at once after ${what} keeps it`, async ({ page }) => {
      await openSettings(page);
      await edit(settingsDialog(page));
      await pressMod(page, 'Mod+KeyW');
      await expect(settingsDialog(page)).toHaveCount(0);
      await pressMod(page, 'Mod+Comma');
      await check(settingsDialog(page));
    });
  }

  test('closing Settings with a refused change says which and why, and closes only when told', async ({ page }) => {
    await openSettings(page, 'mock-fast&mock-static&mock-fail=saveSettings');
    const settings = settingsDialog(page);
    await settings.getByRole('switch', { name: 'Serve other devices on your network' }).click();
    await expect(settings.getByTestId('save-error')).toContainText('Serve other devices on your network could not be saved:');
    await page.keyboard.press('Escape');
    const warning = settings.getByTestId('settings-not-saved');
    await expect(warning).toContainText('Serve other devices on your network was not saved:');
    await warning.getByRole('button', { name: 'Keep editing' }).click();
    await expect(warning).toHaveCount(0);
    await settings.getByRole('button', { name: 'Close' }).last().click();
    await settings.getByTestId('settings-not-saved').getByRole('button', { name: 'Close anyway' }).click();
    await expect(settingsDialog(page)).toHaveCount(0);
  });
});
