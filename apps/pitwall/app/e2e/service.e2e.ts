import { expect, test } from '@playwright/test';
import { openApp } from './app';

const QUESTION = /Run Pitwall's server in the background\?/;

test.describe('service, start at login and updates', () => {
  test('asks the first-run question once, installs the service on yes', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-consent');
    const dialog = page.getByRole('dialog', { name: QUESTION });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Run in the background' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByText('Pitwall now runs as a service.')).toBeVisible();
    await page.getByRole('link', { name: 'Settings' }).click();
    await expect(page.getByTestId('service-pill')).toHaveText(/Running/);
  });

  test('"No thanks" closes the question and Settings still offers the service', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-consent');
    const dialog = page.getByRole('dialog', { name: QUESTION });
    await dialog.getByRole('button', { name: 'No thanks' }).click();
    await expect(dialog).toHaveCount(0);
    await page.getByRole('link', { name: 'Settings' }).click();
    await expect(page.getByTestId('service-pill')).toHaveText(/Not installed/);
    await page.getByRole('button', { name: 'Install service' }).click();
    await expect(page.getByTestId('service-pill')).toHaveText(/Running/);
  });

  test('does not ask when the service is already registered', async ({ page }) => {
    await openApp(page);
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('turns start at login on', async ({ page }) => {
    await openApp(page, '/settings');
    const toggle = page.getByRole('switch', { name: 'Start at login' });
    await expect(toggle).not.toBeChecked();
    await toggle.click();
    await expect(toggle).toBeChecked();
  });

  test('checks for an update only on request and installs it', async ({ page }) => {
    await openApp(page, '/settings', 'mock-fast&mock-static&mock-update');
    await expect(page.getByText(/only looks for updates when you ask/)).toBeVisible();
    await page.getByRole('button', { name: 'Check for updates' }).click();
    await expect(page.getByText(/Version 0\.2\.0 is ready to install/)).toBeVisible();
    await page.getByRole('button', { name: 'Install and restart' }).click();
    await expect(page.getByRole('button', { name: 'Install and restart' })).toBeEnabled();
  });

  test('says updates are not configured in a build without a key', async ({ page }) => {
    await openApp(page, '/settings');
    await expect(page.getByText('Not configured', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Check for updates' })).toHaveCount(0);
  });
});
