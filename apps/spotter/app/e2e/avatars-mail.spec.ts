import { expect, test } from '@playwright/test';

test.describe('avatars, e-mail settings and the upgrade command', () => {
  test('watch rows, the feed and the detail hero show the engine pictures, and the wizard shows them for presets', async ({ page }) => {
    await page.goto('/#/watches');
    await expect(page.locator('tbody tr sp-avatar img').first()).toBeVisible();
    await page.goto('/');
    await expect(page.locator('sp-whats-new sp-avatar img').first()).toBeVisible();
    await page.goto('/#/watches/w-2');
    await expect(page.locator('.head sp-avatar img')).toBeVisible();
    await page.goto('/#/watches/new');
    await expect(page.locator('sp-source-step sp-avatar img, .grid sp-avatar img').first()).toBeVisible();
  });

  test('the e-mail form saves the server, takes a password, and sends a test message', async ({ page }) => {
    await page.goto('/#/settings');
    const mail = page.locator('sp-mail-settings');
    await expect(mail.getByRole('button', { name: 'Send test e-mail' })).toBeDisabled();
    await mail.getByLabel('Server').fill('smtp.example.org');
    await mail.getByLabel('From').fill('spotter@example.org');
    await mail.getByLabel('User name').fill('robot');
    await mail.getByLabel('Password').fill('not-a-real-password');
    await mail.getByRole('button', { name: 'TLS', exact: true }).click();
    await expect(mail.getByLabel('Port')).toHaveValue('465');
    await mail.getByRole('button', { name: 'Save' }).click();
    await expect(mail.getByRole('status')).toHaveText('Mail settings saved.');
    await expect(mail.getByLabel('Password')).toHaveAttribute('placeholder', /Saved/);
    await mail.getByLabel('Send a test message to').fill('nobody-here');
    await mail.getByRole('button', { name: 'Send test e-mail' }).click();
    await expect(mail.getByRole('alert')).toContainText('Enter an e-mail address');
    await mail.getByLabel('Send a test message to').fill('me@example.org');
    await mail.getByRole('button', { name: 'Send test e-mail' }).click();
    await expect(mail.getByRole('status')).toHaveText('Test e-mail sent to me@example.org.');
  });

  test('the upgrade line copies the command for a package watch', async ({ page, context, browserName }) => {
    if (browserName === 'chromium') await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.goto('/#/watches/w-6');
    const copy = page.locator('.upgrade').getByRole('button', { name: /^Copy the upgrade command/ });
    await expect(copy).toBeVisible();
    await copy.click();
    await expect(page.getByText('Upgrade command copied')).toBeVisible();
  });
});
