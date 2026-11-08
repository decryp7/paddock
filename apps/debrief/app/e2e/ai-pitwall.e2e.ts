import { expect, Page, test } from '@playwright/test';
import { expectNoSeriousViolations } from './axe';
import { openAppWith } from './mock-url';

// Pitwall as the only AI engine (ADR 0059, ADR 0070) against the scripted AI mock (?mockPitwall=...).

async function runCommand(page: Page, text: string): Promise<void> {
  await page.keyboard.press('ControlOrMeta+k');
  const input = page.getByRole('combobox', { name: 'Type a command' });
  await input.fill(text);
  await expect(page.locator('debrief-command-palette').getByRole('option').first()).toContainText(text);
  await page.keyboard.press('Enter');
}

async function openAiSettings(page: Page) {
  await page.keyboard.press('ControlOrMeta+Comma');
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await settings.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: /AI assistant/ }).click();
  return settings;
}

test('"Connect to Pitwall" opens Settings ▸ AI, shows the code and ends in Pitwall', async ({ page }, testInfo) => {
  await openAppWith(page, '?mockPitwall=running&mockAi=setup&mockAiStepMs=40');
  await runCommand(page, 'Connect to Pitwall');
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await expect(settings.getByTestId('ai-engine-status')).toContainText('Engine: Pitwall');
  await expect(settings.getByTestId('ai-engine-status')).toContainText('cannot serve Debrief right now');
  await expectNoSeriousViolations(page, testInfo, 'ai-settings-connect', '[role=dialog]');
  await settings.getByTestId('ai-pitwall-connect').click();
  await expect(settings.getByTestId('ai-pitwall-code')).toHaveText('482 913');
  await expect(settings.getByTestId('ai-pitwall-cancel')).toBeVisible();
  await expect(settings.getByTestId('ai-engine-status')).toContainText('Engine: Pitwall 1.0.4 · model qwen3-8b-q4km', { timeout: 10_000 });
  await expect(settings.getByTestId('ai-pitwall-forget')).toBeVisible();
});

test('Debrief offers no model download of its own: Pitwall is the only engine', async ({ page }) => {
  await openAppWith(page, '?mockPitwall=running&mockAi=setup&mockAiStepMs=5');
  const settings = await openAiSettings(page);
  await expect(settings.getByTestId('ai-engine-status')).toBeVisible();
  const text = (await settings.innerText()).toLowerCase();
  for (const word of ['manage models', 'download a model', 'built-in engine', 'inference runtime']) expect(text).not.toContain(word);
  await expect(page.getByRole('dialog', { name: /AI models|Set up the AI assistant/ })).toHaveCount(0);
});

test('a denied pairing shows the problem and can be tried again', async ({ page }) => {
  await openAppWith(page, '?mockPitwall=deny&mockAi=setup&mockAiStepMs=5');
  const settings = await openAiSettings(page);
  await settings.getByTestId('ai-pitwall-connect').click();
  await expect(settings.getByTestId('ai-settings-status')).toBeVisible();
  await expect(settings.getByText('Pitwall denied the request.')).toBeVisible({ timeout: 10_000 });
  await expect(settings.getByTestId('ai-pitwall-connect')).toBeVisible();
  await settings.getByTestId('ai-pitwall-connect').click();
  await expect(settings.getByText('Pitwall denied the request.')).toBeVisible({ timeout: 10_000 });
});

test('Settings → AI offers to connect to a running Pitwall', async ({ page }, testInfo) => {
  await openAppWith(page, '?mockPitwall=running&mockAiStepMs=5');
  const settings = await openAiSettings(page);
  await expect(settings.getByText('Pitwall is running on this computer')).toBeVisible();
  await expect(settings.getByTestId('ai-pitwall-connect')).toBeVisible();
  await expectNoSeriousViolations(page, testInfo, 'ai-settings-pitwall', '[role=dialog]');
  await settings.getByTestId('ai-pitwall-connect').click();
  await expect(settings.getByTestId('ai-engine-status')).toContainText('Engine: Pitwall 1.0.4', { timeout: 10_000 });
});

test('with no Pitwall at all, every AI entry point lands on Settings ▸ AI and none is a dead button', async ({ page }) => {
  await openAppWith(page, '?mockPitwall=absent&mockAi=setup&mockAiStepMs=5');
  // The activity bar offers the connect step, not the chat.
  await expect(page.locator('debrief-activity-bar [data-activity-id="ai-setup"]')).toBeVisible();
  await expect(page.locator('debrief-activity-bar [data-activity-id="ai"]')).toHaveCount(0);
  // Ask AI on the query bar is not a dead button: it leads to the connect path.
  await page.keyboard.press('ControlOrMeta+f');
  await page.getByTestId('ai-ask-query').click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  await expect(page.getByRole('dialog', { name: 'Ask AI to write a query' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  // The palette entry opens Settings at the AI section, which says Pitwall is not found and offers no connect button.
  await runCommand(page, 'Connect to Pitwall');
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await expect(settings).toBeVisible();
  await expect(settings.getByTestId('ai-settings-status')).toBeVisible();
  await expect(settings.getByTestId('ai-pitwall-connect')).toHaveCount(0);
  // The welcome page offers the same path.
  await page.keyboard.press('Escape');
  await runCommand(page, 'Close workspace');
  await expect(page.getByTestId('welcome-ai-offer')).toContainText('Pitwall');
  await page.getByTestId('welcome-ai-setup').click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
});
