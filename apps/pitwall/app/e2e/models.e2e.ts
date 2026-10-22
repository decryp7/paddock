import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('models', () => {
  test('lists the library and loads a model', async ({ page }) => {
    await openApp(page, '/models');
    await expect(page.getByRole('table').first()).toBeVisible();
    await page.getByRole('button', { name: /^Load / }).first().click();
    await expect(page.getByText('Loaded', { exact: true }).first()).toBeVisible();
  });

  test('searches Hugging Face, shows fit verdicts and downloads a variant', async ({ page }) => {
    await openApp(page, '/models');
    await page.getByRole('tab', { name: /Discover/ }).click();
    await page.getByLabel('Search Hugging Face for GGUF models').fill('gemma');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await page.locator('pw-discover-tab .hit').first().click();
    await expect(page.getByText('Will it fit?')).toBeVisible();
    await expect(page.getByText("Won't fit").first()).toBeVisible();
    await page.getByRole('button', { name: /^Download gemma-3-12b-it-Q2_K/ }).click();
    await page.getByRole('tab', { name: /Downloads/ }).click();
    await expect(page.locator('pw-downloads-list')).toContainText('gemma-3-12b-it-Q2_K');
  });

  test('recommends models for this machine, filters by tag and downloads in one click', async ({ page }) => {
    await openApp(page, '/models');
    await page.getByRole('tab', { name: /Discover/ }).click();
    await expect(page.getByText('Recommended for this machine')).toBeVisible();
    await page.getByRole('button', { name: 'code', exact: true }).click();
    await expect(page.getByRole('list', { name: 'Recommended models' }).getByRole('listitem')).toHaveCount(2);
    await page.getByRole('button', { name: /^Download Qwen2.5 Coder/ }).click();
    await page.getByRole('tab', { name: /Downloads/ }).click();
    await expect(page.locator('pw-downloads-list')).toContainText('Qwen2.5-Coder');
  });
});
