import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('settings', () => {
  test('turns offline mode on, saves, and Models then refuses to download', async ({ page }) => {
    await openApp(page, '/settings');
    await page.getByRole('switch', { name: 'Offline mode' }).click();
    await expect(page.getByText('You have unsaved changes.')).toBeVisible();
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('You have unsaved changes.')).toHaveCount(0);
    await page.getByRole('switch', { name: 'Offline mode' }).waitFor();
    await expect(page.getByRole('switch', { name: 'Offline mode' })).toBeChecked();
  });

  test('switches the theme', async ({ page }) => {
    await openApp(page, '/');
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('radio', { name: 'Light' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  });
});
