import { expect, expectNothingCovers, test } from '@decryp7/paddock-e2e';

// ER-21: nothing covers an interactive element in the shell frame (title bar, sidebar, status bar) of the shell demo.
test('the shell frame: nothing covers an interactive element', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await expect(page.getByTestId('app-context')).toContainText('loaded');
  await expectNothingCovers(page.locator('body'));
});
