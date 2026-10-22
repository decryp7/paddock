import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('engines', () => {
  test('installs a build from the catalog and shows progress', async ({ page }) => {
    await openApp(page, '/engines');
    await page.getByRole('button', { name: /^Install Vulkan/ }).click();
    await expect(page.getByRole('progressbar', { name: /Installing/ })).toBeVisible();
  });

  test('shows skipped packs as an inline notice and installs a pack engine like any other', async ({ page }) => {
    await openApp(page, '/engines');
    await expect(page.getByRole('status', { name: 'Pack problems' })).toContainText('packs/old-vllm.toml');
    await expect(page.getByText('ik_llama.cpp (pack)').first()).toBeVisible();
    await page.getByRole('button', { name: /^Install CPU \(AVX2\) v0\.9\.2/ }).click();
    await expect(page.getByRole('progressbar', { name: /Installing/ })).toBeVisible();
  });
});
