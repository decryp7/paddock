import { expect, test } from '@playwright/test';
import { openApp } from './app';
import { expectNoSeriousViolations } from './axe';
import { showProfileEditor } from './menu-support';

test('a drop step added in the pipeline flow removes events and shows its effect', async ({ page }, testInfo) => {
  await openApp(page);
  await showProfileEditor(page);
  await page.getByRole('button', { name: /^Pipeline/ }).first().click();
  await expect(page.getByText('No steps yet')).toBeVisible();

  await page.getByRole('group', { name: 'Add an activity' }).getByRole('button', { name: 'Drop' }).click();
  await expect(page.locator('.card .summary')).toHaveText('Drop events where @message contains');

  const card = page.locator('debrief-pipeline-editor .card').first();
  await card.getByRole('combobox', { name: 'Field', exact: true }).fill('@level');
  await card.getByRole('textbox', { name: 'Value', exact: true }).fill('Debug');
  await expect(card.locator('.summary')).toHaveText('Drop events where @level contains Debug');
  await expect(card.locator('.stat')).toHaveText('not run yet');
  await expectNoSeriousViolations(page, testInfo, 'pipeline-flow');

  await page.getByRole('button', { name: 'Save & apply' }).click();
  await expect(page.getByText('● Unsaved')).toHaveCount(0);

  await page.getByTestId('status-details').click();
  await expect(page.getByTestId('status-details-popover')).toContainText('Dropped by pipeline');
  await page.keyboard.press('Escape');
  await expect(card.locator('.stat')).toContainText(/in → .* out \(−/);
  await expect(page.locator('debrief-sidebar .src[title*="dropped by pipeline"]').first()).toBeVisible();
  await expect(page.locator('debrief-flow-overview')).toContainText('1 step');
});

test('steps can be reordered with the keyboard-accessible buttons and disabled', async ({ page }) => {
  await openApp(page);
  await showProfileEditor(page);
  await page.getByRole('button', { name: /^Pipeline/ }).first().click();
  const palette = page.getByRole('group', { name: 'Add an activity' });
  await palette.getByRole('button', { name: 'Drop' }).click();
  await palette.getByRole('button', { name: 'Set level' }).click();
  await page.getByRole('button', { name: 'Move step 2 up' }).click();
  await expect(page.locator('debrief-pipeline-editor .card').first()).toHaveAttribute('data-step', 'set_level');
  await page.getByLabel('Disable step 1').check();
  await expect(page.locator('debrief-pipeline-editor .card').first().locator('.stat')).toHaveText('disabled');
});
