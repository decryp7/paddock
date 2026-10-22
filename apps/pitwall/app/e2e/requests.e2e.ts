import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('requests inspector', () => {
  test('opens a request, shows its timing and keeps content hidden until asked', async ({ page }) => {
    await openApp(page, '/requests');
    await page.getByRole('row').nth(1).click();
    const detail = page.getByRole('complementary').or(page.locator('pw-request-detail')).first();
    await expect(detail).toContainText('Timing');
    await expect(detail).toContainText('Routing');
    await expect(page.getByRole('button', { name: 'Show content' })).toBeVisible();
  });
});
