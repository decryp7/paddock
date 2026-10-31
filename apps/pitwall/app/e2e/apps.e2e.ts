import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('apps and keys', () => {
  test('creates an app and shows its key once', async ({ page }) => {
    await openApp(page, '/apps');
    await page.getByRole('button', { name: 'New app' }).first().click();
    const form = page.getByRole('dialog', { name: 'New app' });
    await expect(form.getByTestId('dialog-action-create')).toBeDisabled();
    await form.getByLabel('Name').fill('My script');
    await form.getByTestId('dialog-action-create').click();
    const secret = page.getByRole('alertdialog', { name: /New key for My script/ });
    await expect(secret.locator('#pw-key-secret')).toHaveText(/^pw_/);
    // A required choice: Escape does not lose the key.
    await page.keyboard.press('Escape');
    await expect(secret).toBeVisible();
    await secret.getByRole('button', { name: "I've saved it" }).click();
    await expect(secret).toHaveCount(0);
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

  test('the app detail shows usage over time, limits with progress and recent requests', async ({ page }) => {
    await openApp(page, '/apps');
    await page.getByRole('button', { name: /Spotter/ }).first().click();
    const usage = page.locator('pw-app-usage');
    await expect(usage.getByRole('heading', { name: 'Usage over time' })).toBeVisible();
    await expect(usage.locator('pw-echart canvas').first()).toBeVisible();
    await expect(usage.getByRole('list', { name: 'Limits and how much of each is used' })).toContainText('Tokens per day');
    await expect(usage.getByRole('progressbar', { name: 'Tokens per day' })).toBeVisible();
    await expect(usage.locator('table.recent tbody tr').first()).toBeVisible();

    await usage.getByRole('radio', { name: '30d' }).click();
    await expect(usage.getByRole('radio', { name: '30d' })).toHaveAttribute('aria-checked', 'true');
    await expect(usage.locator('pw-echart')).toHaveAttribute('aria-label', /the last 30 days/);
    await usage.getByRole('radio', { name: 'Peak memory' }).click();
    await expect(usage.locator('pw-echart')).toHaveAttribute('aria-label', /Peak memory of Spotter/);
  });

  test('saving a memory share limit shows it in the limits list', async ({ page }) => {
    await openApp(page, '/apps');
    await page.getByRole('button', { name: /Pitcrew/ }).first().click();
    await expect(page.locator('pw-app-usage')).toContainText('No limits set');
    const save = page.getByRole('button', { name: 'Save permissions' });
    // The editor reloads its fields from the selected app a moment after it appears; type again until it keeps the edit.
    await expect(async () => {
      await page.getByLabel('Memory share of RAM').fill('30');
      await expect(save).toBeEnabled({ timeout: 500 });
    }).toPass();
    await save.click();
    await expect(page.locator('pw-app-usage').getByRole('progressbar', { name: 'Memory share' })).toBeVisible();
  });

  test('the dashboard usage legend opens the app detail', async ({ page }) => {
    await openApp(page, '/');
    const card = page.locator('pw-usage-card');
    await expect(card.getByRole('heading', { name: 'Usage by app' })).toBeVisible();
    await expect(card.locator('pw-echart canvas')).toBeVisible();
    await card.getByRole('radio', { name: '7d' }).click();
    await card.getByRole('link', { name: 'Spotter' }).click();
    await expect(page.getByRole('heading', { name: 'Spotter', level: 2 })).toBeVisible();
    await expect(page.locator('pw-app-usage')).toBeVisible();
  });
});
