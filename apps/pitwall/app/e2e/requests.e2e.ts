import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('requests inspector', () => {
  test('opens a request, shows its timing and keeps content hidden until asked', async ({ page }) => {
    await openApp(page, '/requests');
    await page.getByRole('row').nth(1).click();
    const detail = page.getByRole('complementary');
    await expect(detail).toContainText('Timing');
    await expect(detail).toContainText('Routing');
    await expect(page.getByRole('button', { name: 'Show content' })).toBeVisible();
  });

  test('shows the preset an app asked for and the model it resolved to', async ({ page }) => {
    await openApp(page, '/requests');
    await page.getByRole('row').filter({ hasText: 'debrief-explain' }).first().click();
    await expect(page.locator('pw-request-detail')).toContainText('Preset asked for');
    await expect(page.locator('pw-request-detail')).toContainText('Resolved to');
  });
});

test.describe('requests log', () => {
  test('is calm by default: errors alone carry a status, and the repeated columns are off', async ({ page }) => {
    await openApp(page, '/requests');
    await expect(page.getByRole('columnheader', { name: 'API', exact: true })).toHaveCount(0);
    await expect(page.getByRole('columnheader', { name: 'TTFT ms' })).toBeVisible();
    await page.getByRole('radio', { name: /Errors/ }).click();
    await expect(page.locator('.c-status .status').first()).toContainText(/^\d{3}/);
    await page.getByRole('button', { name: 'Columns' }).click();
    await page.getByRole('menuitemcheckbox', { name: 'API' }).click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('columnheader', { name: 'API', exact: true })).toBeVisible();
  });

  test('keeps the panel closed on Escape and returns focus to the row', async ({ page }) => {
    await openApp(page, '/requests');
    const row = page.getByRole('row').nth(1);
    await row.focus();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('complementary')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('complementary')).toHaveCount(0);
    await expect(row).toBeFocused();
  });
});
