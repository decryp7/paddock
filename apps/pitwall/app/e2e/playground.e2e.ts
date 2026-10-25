import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('playground', () => {
  test('streams an answer with stats', async ({ page }) => {
    await openApp(page, '/playground');
    await page.getByLabel('Message').fill('Hello there');
    await page.getByRole('button', { name: 'Send' }).click();
    await expect(page.locator('.turn.bot .text').first()).not.toBeEmpty({ timeout: 10_000 });
    await expect(page.locator('.stats').first()).toContainText('tok/s', { timeout: 15_000 });
  });
});
