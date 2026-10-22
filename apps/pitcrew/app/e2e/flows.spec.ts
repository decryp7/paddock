import { expect, test } from '@playwright/test';

// The browser build with the mock backend; `simulate=0` keeps the demo still (no pushed toasts mid-test).
const APP = '/?simulate=0#';

test.describe('Pitcrew in the browser (mock backend)', () => {
  test('navigates between the screens with the sidebar', async ({ page }) => {
    await page.goto(`${APP}/`);
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'build-win' }).first()).toBeVisible();

    const nav = page.getByRole('navigation', { name: 'Main' });
    for (const [link, heading] of [
      ['Runners', 'Runners'],
      ['Activity', 'Activity'],
      ['Logs', 'Logs'],
      ['Updates', 'Updates'],
      ['Settings', 'Settings'],
      ['Dashboard', 'Dashboard'],
    ] as const) {
      await nav.getByRole('link', { name: link }).click();
      await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
    }
  });

  test('the sidebar is keyboard navigable with arrow keys', async ({ page }) => {
    await page.goto(`${APP}/`);
    const nav = page.getByRole('navigation', { name: 'Main' });
    await nav.getByRole('link', { name: 'Dashboard' }).focus();
    await page.keyboard.press('ArrowDown');
    await expect(nav.getByRole('link', { name: /Runners/ })).toBeFocused();
    await page.keyboard.press('End');
    await expect(nav.getByRole('link', { name: 'Settings' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
  });

  test('drains a runner from its actions menu and resumes it', async ({ page }) => {
    await page.goto(`${APP}/runners/ui-tests`);
    await expect(page.getByRole('heading', { level: 1, name: 'ui-tests' })).toBeVisible();
    await page.getByRole('button', { name: 'Actions for ui-tests' }).click();
    await page.getByRole('menuitem', { name: 'Drain and pause' }).click();
    await expect(page.getByText('Draining ui-tests')).toBeVisible();
    await expect(page.locator('.title').getByText('Paused')).toBeVisible();

    await page.getByRole('button', { name: 'Resume' }).first().click();
    await expect(page.locator('.title').getByText('Idle')).toBeVisible();
  });

  test('updates a runner and shows the progress through the phases', async ({ page }) => {
    await page.goto(`${APP}/updates`);
    const card = page.locator('article', { has: page.getByRole('link', { name: 'ui-tests' }) });
    await card.getByRole('button', { name: 'Update now' }).click();
    await expect(card.getByLabel(/Update progress/)).toBeVisible();
    await expect(card.getByText('5.3.0').first()).toBeVisible();
    // The mock walks through the phases in ~11 s and then reports the new version.
    await expect(card.locator('dd').first()).toHaveText('5.3.0', { timeout: 20_000 });
  });

  test('switches the theme and remembers it', async ({ page }) => {
    await page.goto(`${APP}/settings`);
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('radio', { name: 'Light' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.getByRole('radio', { name: 'Follow system' }).click();
    await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/);
  });

  test('the registration wizard checks each step and registers a runner', async ({ page }) => {
    await page.goto(`${APP}/runners`);
    await page.getByRole('button', { name: 'Register runner' }).first().click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByRole('heading', { name: 'Register a runner' })).toBeVisible();

    await dialog.getByRole('button', { name: 'Continue' }).click();
    await expect(dialog.getByRole('alert')).toContainText('Enter the server');

    await dialog.getByLabel('Server URL').fill('https://git.example.com');
    await dialog.getByLabel('Registration token').fill('demo-registration-token');
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await dialog.getByLabel('Name in Pitcrew').fill('nightly');
    await dialog.getByLabel('Labels').fill('windows');
    await dialog.getByLabel('Labels').press('Enter');
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await dialog.getByText('Run as the service account').click();
    await expect(dialog.getByText('Jobs would run as LocalSystem, without a desktop')).toBeVisible();
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await expect(dialog.getByText('nightly', { exact: true })).toBeVisible();
    await dialog.getByRole('button', { name: 'Register' }).click();

    await expect(dialog).toBeHidden();
    await expect(page.getByRole('link', { name: 'nightly' })).toBeVisible();
  });

  test('shows the banner and a way out when the service is not installed', async ({ page }) => {
    await page.goto('/?simulate=0&mock=notInstalled#/');
    const banner = page.getByRole('alert').filter({ hasText: 'not installed' });
    await expect(banner).toBeVisible();
    await banner.getByRole('button', { name: 'Install service' }).click();
    await expect(banner).toBeHidden();
    await expect(page.getByRole('link', { name: 'build-win' }).first()).toBeVisible();
  });
});
