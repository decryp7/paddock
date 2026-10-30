import { expect, test } from '@playwright/test';
import { openApp, openSettings, openSettingsWindow, settingsDialog } from './app';

test.describe('settings', () => {
  test('opens from Mod+, as the Settings window, not as a page of the main window', async ({ page }) => {
    await openApp(page, '/settings');
    // The old route is gone: it lands on the first page.
    await expect(page.getByRole('heading', { name: 'Dashboard', level: 1 })).toBeVisible();
    await page.keyboard.press('ControlOrMeta+Comma');
    const settings = settingsDialog(page);
    await expect(settings.getByRole('heading', { name: 'Pitwall' })).toBeVisible();
    for (const name of ['Pitwall', 'Appearance']) await expect(settings.getByText(name, { exact: true }).first()).toBeVisible();
  });

  test('turns offline mode on, saves, and the switch stays on', async ({ page }) => {
    await openSettings(page);
    const settings = settingsDialog(page);
    await settings.getByRole('switch', { name: 'Offline mode' }).click();
    await expect(settings.getByText('You have unsaved changes.')).toBeVisible();
    await settings.getByRole('button', { name: 'Save changes' }).click();
    await expect(settings.getByText('You have unsaved changes.')).toHaveCount(0);
    await expect(settings.getByRole('switch', { name: 'Offline mode' })).toBeChecked();
  });

  test('switches the theme on the shell’s Appearance page', async ({ page }) => {
    await openSettingsWindow(page);
    await page.getByText('Appearance', { exact: true }).first().click();
    for (const theme of ['Dark', 'Light']) {
      await page.getByRole('combobox', { name: /Theme/ }).click();
      await page.getByRole('option', { name: theme }).click();
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme.toLowerCase());
    }
  });

  test('in its own window, a saved change is confirmed in that window', async ({ page }) => {
    await openSettingsWindow(page);
    await page.getByRole('switch', { name: 'Accept local requests without a key' }).click();
    await page.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Settings saved.')).toBeVisible();
  });
});
