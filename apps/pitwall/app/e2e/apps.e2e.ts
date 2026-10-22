import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('apps and keys', () => {
  test('creates an app and shows its key once', async ({ page }) => {
    await openApp(page, '/apps');
    await page.getByRole('button', { name: 'New app' }).first().click();
    await page.getByLabel('Name').fill('My script');
    await page.getByRole('button', { name: 'Create app' }).click();
    const dialog = page.locator('pw-key-secret-dialog');
    await expect(dialog).toContainText('pw_');
    await dialog.getByRole('button', { name: "I've saved it" }).click();
    await expect(page.getByText('My script').first()).toBeVisible();
  });

  test('approves a pairing request with its permissions', async ({ page }) => {
    await openApp(page, '/');
    const card = page.getByRole('region', { name: /Pairing request from/ });
    const name = (await card.getAttribute('aria-label'))!.replace('Pairing request from ', '');
    await card.getByRole('button', { name: /Approve|Allow|Pair/ }).first().click();
    await page.getByRole('link', { name: /Apps/ }).click();
    await expect(page.getByText(name).first()).toBeVisible();
  });
});
