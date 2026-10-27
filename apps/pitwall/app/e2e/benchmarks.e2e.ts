import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('benchmarks', () => {
  test('runs one model from its row and has a single Run all; units are in the headers', async ({ page }) => {
    await openApp(page, '/benchmarks');
    await expect(page.getByRole('columnheader', { name: 'Generation tok/s' })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'TTFT ms' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Run all' })).toBeVisible();
    const rows = page.locator('tbody tr');
    const before = await rows.count();
    await page.getByRole('button', { name: 'Run benchmark for Gemma 4 12B' }).click();
    await expect(rows).toHaveCount(before + 1);
  });
});
