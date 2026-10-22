import { expect, test } from '@playwright/test';

test.describe('Spotter against the mock backend', () => {
  test('the first load shows content, not a blank window', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Dashboard', level: 1 })).toBeVisible();
    await expect(page.getByRole('heading', { name: "What's new" })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Lumen CLI', exact: true })).toBeVisible();
    // Status always has words.
    await expect(page.getByText('Unreachable').first()).toBeVisible();
  });

  test('adds a watch through the wizard with a live preview', async ({ page }) => {
    await page.goto('/#/watches/new');
    await page.getByRole('button', { name: /Node\.js/ }).click();
    await expect(page.getByRole('heading', { name: 'Test this source' })).toBeVisible();
    await expect(page.getByRole('table', { name: 'Releases found' })).toBeVisible();
    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByRole('button', { name: 'Next' }).click();
    await page.getByLabel('Name').fill('Node runtime');
    await page.getByRole('button', { name: 'Add watch' }).click();
    await expect(page.getByRole('heading', { name: 'Node runtime', level: 1 })).toBeVisible();
    await page.getByRole('link', { name: 'All watches' }).click();
    await expect(page.getByRole('link', { name: 'Node runtime' })).toBeVisible();
  });

  test('a wizard that is not ready says what is missing', async ({ page }) => {
    await page.goto('/#/watches/new');
    await page.getByRole('tab', { name: 'Source kinds' }).click();
    await page.getByRole('button', { name: /GitHub/ }).click();
    await expect(page.getByText('Still needed: Repository')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Next' })).toBeDisabled();
  });

  test('pauses, resumes and checks a watch', async ({ page }) => {
    await page.goto('/#/watches');
    await page.getByRole('button', { name: 'Pause Lumen CLI' }).click();
    await expect(page.getByRole('button', { name: 'Resume Lumen CLI' })).toBeVisible();
    await page.getByRole('button', { name: 'Resume Lumen CLI' }).click();
    await expect(page.getByRole('button', { name: 'Pause Lumen CLI' })).toBeVisible();
    await page.getByRole('button', { name: 'Check Lumen CLI now' }).click();
    await expect(page.getByText('Check started')).toBeVisible();
  });

  test('acknowledges a release', async ({ page }) => {
    await page.goto('/#/watches');
    await expect(page.getByRole('button', { name: /as seen$/ }).first()).toBeVisible();
    const before = await page.getByRole('button', { name: /as seen$/ }).count();
    expect(before).toBeGreaterThan(0);
    await page.getByRole('button', { name: /as seen$/ }).first().click();
    await expect(page.getByRole('button', { name: /as seen$/ })).toHaveCount(before - 1);
  });

  test('a settings change applies at once and is still there after navigating', async ({ page }) => {
    await page.goto('/#/settings');
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByLabel('Show notifications').uncheck();
    await page.getByRole('link', { name: 'Watches' }).click();
    await page.getByRole('link', { name: 'Settings' }).click();
    await expect(page.getByRole('radio', { name: 'Dark' })).toBeChecked();
    await expect(page.getByLabel('Show notifications')).not.toBeChecked();
  });

  test('the tray popup lists unseen releases', async ({ page }) => {
    await page.goto('/#/tray');
    await expect(page.getByText(/\d+ unseen/)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open Spotter' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Mark .* as seen/ }).first()).toBeVisible();
  });

  test('a failing backend shows an inline error with Retry', async ({ page }) => {
    await page.goto('/?mock=broken');
    await expect(page.getByRole('button', { name: 'Retry' }).first()).toBeVisible();
    await expect(page.getByText('Spotter cannot reach its engine')).toBeVisible();
  });

  test('an empty install invites you to add the first watch', async ({ page }) => {
    await page.goto('/?mock=empty');
    await expect(page.getByText('Nothing to watch yet')).toBeVisible();
    await expect(page.getByRole('link', { name: 'Add your first watch' })).toBeVisible();
  });
});
