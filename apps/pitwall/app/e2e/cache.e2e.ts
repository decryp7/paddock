import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('caching', () => {
  test('settings: sets an expiry, sees the figures and clears a cache after confirming', async ({ page }) => {
    await openApp(page, '/settings');
    const card = page.locator('#set-cache');
    await expect(card).toContainText('never sent anywhere');
    await expect(card.locator('.cache-stats').first()).toContainText('90 days since last use');
    await card.getByRole('combobox', { name: 'Cached answers expiry' }).click();
    await page.getByRole('option', { name: '30 days after it is stored' }).click();
    await expect(page.getByText('You have unsaved changes.')).toBeVisible();
    await page.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('You have unsaved changes.')).toHaveCount(0);
    await expect(card.getByRole('combobox', { name: 'Cached answers expiry' })).toContainText('30 days after it is stored');

    await card.getByRole('button', { name: 'Clear now' }).nth(1).click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Clear now' }).click();
    await expect(card.locator('.cache-stats').nth(1)).toContainText('none');
  });

  test('apps: turns Cache answers on for an app and clears its cached answers', async ({ page }) => {
    await openApp(page, '/apps');
    await page.getByRole('button', { name: /Pitcrew/ }).first().click();
    const toggle = page.getByRole('switch', { name: 'Cache answers' });
    await expect(toggle).not.toBeChecked();
    await toggle.click();
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByRole('switch', { name: 'Cache answers' })).toBeChecked();
    await expect(page.getByText('on for this app')).toBeVisible();
    await page.getByRole('button', { name: "Clear this app's cached answers" }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Clear cached answers' }).click();
    await expect(page.getByText(/Removed 40 cached answers/)).toBeVisible();
  });
});
