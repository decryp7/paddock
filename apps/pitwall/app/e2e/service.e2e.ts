import { expect, test } from '@playwright/test';
import { openApp, openSettings, openSettingsWindow, settingsDialog } from './app';

const QUESTION = /Run Pitwall's server in the background\?/;

test.describe('service and start at login', () => {
  test('asks the first-run question once, installs the service on yes', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-consent');
    const dialog = page.getByRole('alertdialog');
    await expect(dialog).toContainText(QUESTION);
    await dialog.getByRole('button', { name: 'Run in the background' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByText('Pitwall now runs as a service.')).toBeVisible();
    await page.keyboard.press('ControlOrMeta+Comma');
    await expect(settingsDialog(page).getByTestId('service-pill').first()).toHaveText(/Running/);
  });

  test('"Not now" closes the question and Settings still offers the service', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-consent');
    const dialog = page.getByRole('alertdialog');
    await dialog.getByRole('button', { name: 'Not now' }).click();
    await expect(dialog).toHaveCount(0);
    await page.keyboard.press('ControlOrMeta+Comma');
    const settings = settingsDialog(page);
    await expect(settings.getByTestId('service-pill').first()).toHaveText(/Not installed/);
    await settings.getByRole('button', { name: 'Install service' }).first().click();
    await expect(settings.getByTestId('service-pill').first()).toHaveText(/Running/);
  });

  test('"Don\'t ask again" with Not now never asks again', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-consent');
    const dialog = page.getByRole('alertdialog');
    await dialog.getByRole('checkbox', { name: "Don't ask again" }).check();
    await dialog.getByRole('button', { name: 'Not now' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
  });

  test('does not ask when the service is already registered', async ({ page }) => {
    await openApp(page);
    await expect(page.getByRole('alertdialog')).toHaveCount(0);
  });

  test('turns start at login on', async ({ page }) => {
    await openSettings(page);
    const toggle = settingsDialog(page).getByRole('switch', { name: 'Start at login' });
    await expect(toggle).not.toBeChecked();
    await toggle.click();
    await expect(toggle).toBeChecked();
  });

  test('app updates are the shell’s Updates window (Help ▸ Check for updates…), not a section of Pitwall’s settings', async ({ page }) => {
    await openSettingsWindow(page);
    await expect(page.locator('#set-general')).toBeVisible();
    await expect(page.locator('#set-updates')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Install and restart' })).toHaveCount(0);
    await openApp(page);
    await page.getByText('Help', { exact: true }).click();
    await page.getByRole('menuitem', { name: /Check for updates/ }).click();
    await expect(page.getByRole('dialog', { name: 'Updates' })).toBeVisible();
  });

  test('with the server down, Settings still shows the service and Repair brings it back', async ({ page }) => {
    await openSettingsWindow(page, 'mock-fast&mock-static&mock-down&mock-startfailed');
    await expect(page.getByTestId('service-pill').first()).toHaveText(/Could not start/);
    await expect(page.getByText(/could not start after the update/).first()).toBeVisible();
    await page.locator('#set-service').getByRole('button', { name: 'Repair service' }).click();
    await expect(page.getByTestId('service-pill').first()).toHaveText(/Running/);
    await expect(page.getByText('Registration repaired')).toBeVisible();
  });

  test('the dashboard banner repairs the service when it could not start after the update', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-down&mock-startfailed');
    const banner = page.getByTestId('service-start-failed');
    await expect(banner).toContainText('could not start after the update');
    await banner.getByRole('button', { name: 'Repair' }).click();
    await expect(banner).toHaveCount(0);
  });
});
