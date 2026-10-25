import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('tray popover', () => {
  test.use({ viewport: { width: 360, height: 520 } });

  test('shows memory, hot models and the key actions, and pauses serving', async ({ page }) => {
    await openApp(page, '/tray');
    await expect(page.getByRole('heading', { name: 'Pitwall' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Memory' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Loaded models' })).toContainText('Qwen3');
    await page.getByRole('button', { name: 'Pause serving' }).click();
    await expect(page.getByText(/Paused on/)).toBeVisible();
    await page.getByRole('button', { name: 'Resume' }).click();
    await expect(page.getByText(/Serving on/)).toBeVisible();
    await expect(page.getByRole('button', { name: /Review|pair/ }).first()).toBeVisible();
  });
});
