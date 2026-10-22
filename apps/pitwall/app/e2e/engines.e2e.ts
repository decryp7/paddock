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

  test('pins a build for all models, reads what it supports, and rolls back with the target named', async ({ page }) => {
    await openApp(page, '/engines');
    // Capabilities in words, with the upgrade hint for a build that is too old.
    await page.getByRole('button', { name: 'What CPU (AVX2) b4300 supports' }).click();
    const features = page.getByRole('list', { name: 'Features of CPU (AVX2) b4300' });
    await expect(features).toContainText('Needs b5600');
    await expect(features).toContainText('Install llama.cpp b5600 or newer in Engines');
    // Pin: accent and words, and the page says automatic choice is off.
    await page.getByRole('button', { name: 'Pin Vulkan b11669 for all models' }).click();
    const pinned = page.getByRole('button', { name: 'Unpin Vulkan b11669 for all models' });
    await expect(pinned).toHaveAttribute('aria-pressed', 'true');
    await expect(pinned).toHaveClass(/\bon\b/);
    await expect(page.getByRole('status').filter({ hasText: 'Pinned to Vulkan b11669 for all models' })).toBeVisible();
    // Roll back names its target and asks first.
    await page.getByRole('button', { name: /^Roll back to CUDA 13/ }).click();
    await expect(page.getByRole('dialog')).toContainText('Roll back to CUDA 13 (NVIDIA) b11669?');
    await expect(page.getByRole('dialog')).toContainText('pinned for all models');
    await page.getByRole('dialog').getByRole('button', { name: 'Roll back' }).click();
    await expect(page.getByRole('button', { name: 'Unpin CUDA 13 (NVIDIA) b11669 for all models' })).toBeVisible();
  });

  test('pins a model to a build from the library', async ({ page }) => {
    await openApp(page, '/models');
    await page.getByRole('tab', { name: /Library/ }).click();
    await page.getByRole('row', { name: /Qwen3 8B Instruct/ }).click();
    const menu = page.locator('select[id^="pin-"]').first();
    await menu.selectOption('vulkan');
    await expect(page.getByText(/Pinned to vulkan b11669/).first()).toBeVisible();
  });
});
