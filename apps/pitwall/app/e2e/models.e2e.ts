import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('models', () => {
  test('lists the library and loads a model', async ({ page }) => {
    await openApp(page, '/models');
    await expect(page.getByRole('table').first()).toBeVisible();
    await page.getByRole('button', { name: /^Load / }).first().click();
    await expect(page.getByText('Loaded', { exact: true }).first()).toBeVisible();
  });

  test('slots: see what each adds, check the fit table, save, and learn a loaded model keeps its count', async ({ page }) => {
    await openApp(page, '/models');
    // A loaded model: the change is saved but applies on the next load.
    await page.getByRole('button', { name: /^Parallel slots for Qwen3 8B Instruct/ }).click();
    const panel = page.locator('pw-slots-panel');
    await expect(panel).toContainText('Parallel slots · Qwen3 8B Instruct');
    await expect(panel.getByRole('radio', { name: /^4 adds ~/ })).toBeVisible();
    await expect(panel.locator('.fit-table tbody tr')).toHaveCount(4);
    await expect(panel.locator('.sum')).toContainText('requests at once');
    await panel.getByRole('radio', { name: /^8 / }).click();
    await expect(panel.locator('.fit-table tr.sel')).toContainText('8');
    await panel.getByRole('button', { name: 'Save 8 slots' }).click();
    await expect(page.getByText(/8 slots saved\. Qwen3 8B Instruct keeps 2 slots until it loads again/)).toBeVisible();
    await expect(page.getByRole('button', { name: /^Parallel slots for Qwen3 8B Instruct: 8 slots/ })).toBeVisible();
    // Discover shows the concurrency answer next to the verdict.
    await page.getByRole('tab', { name: /Discover/ }).click();
    await page.getByLabel('Search Hugging Face for GGUF models').fill('gemma');
    await page.getByRole('button', { name: 'Search', exact: true }).click();
    await page.locator('pw-discover-tab .hit').first().click();
    await expect(page.locator('pw-discover-tab tbody')).toContainText(/requests at once|one request at a time/);
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

  test('presets: create one with a live preview from the engine, fix a stale one, delete with a confirmation', async ({ page }) => {
    await openApp(page, '/models');
    await page.getByRole('tab', { name: /Presets/ }).click();
    const list = page.locator('pw-presets-tab');
    await expect(list).toContainText('debrief-explain');
    await expect(list).toContainText('Model removed');

    await page.getByRole('button', { name: 'New preset' }).first().click();
    await page.getByLabel('Name apps send as the model').fill('dyno-notes');
    await page.getByLabel('Serves from').fill('auto:chat,small');
    await page.getByLabel('Temperature').fill('0.4');
    const preview = page.getByRole('status', { name: 'What this preset resolves to' });
    await expect(preview).toContainText('Would serve');
    await page.getByLabel('Serves from').fill('no-such-model');
    await expect(preview).toContainText('Cannot be saved');
    await expect(page.getByRole('button', { name: 'Create preset' })).toBeDisabled();
    await page.getByLabel('Serves from').fill('auto:chat,small');
    await page.getByRole('button', { name: 'Create preset' }).click();
    await expect(list.getByRole('row', { name: /dyno-notes/ })).toContainText('temperature 0.4');

    await page.getByRole('button', { name: 'Edit legacy-explain' }).click();
    await page.getByLabel('Serves from').fill('qwen3-8b-instruct-q4_k_m');
    await page.getByRole('button', { name: 'Save preset' }).click();
    await expect(list).not.toContainText('Model removed');

    await page.getByRole('button', { name: 'Delete dyno-notes' }).click();
    await page.getByRole('button', { name: 'Delete preset' }).click();
    await expect(list.getByRole('row', { name: /dyno-notes/ })).toHaveCount(0);
  });
});
