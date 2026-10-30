import { expect, test } from '@playwright/test';
import { expectEscapeReturnsFocus, expectReachableByKeyboard } from '../../../../ui/projects/paddock-e2e/src/keyboard.ts';
import { openApp } from './app';

// The keyboard-only flow (C33, WCAG 2.1.1, 2.1.2, 2.4.3): the sidebar and a page's primary action are reachable with Tab,
// and a dialog opened from the keyboard closes with Escape and gives the focus back to what opened it.
test.describe('keyboard only', () => {
  test('the pages and the Apps page’s New app are reachable with Tab', async ({ page }) => {
    await openApp(page, '/apps', 'mock-fast&mock-static&mock-empty');
    await expectReachableByKeyboard(page, {
      dashboard: page.locator('pd-pages-sidebar a', { hasText: 'Dashboard' }),
      models: page.locator('pd-pages-sidebar a', { hasText: 'Models' }),
      'new app': page.getByRole('button', { name: 'New app' }).first(),
    });
  });

  test('New app opens from the keyboard and Escape gives the focus back to it', async ({ page }) => {
    await openApp(page, '/apps', 'mock-fast&mock-static&mock-empty');
    await expectEscapeReturnsFocus(page, page.getByRole('button', { name: 'New app' }).first(), page.getByRole('dialog', { name: 'New app' }));
  });

  test('a confirmation opened from a row menu returns the focus to the menu’s button', async ({ page }) => {
    await openApp(page, '/apps');
    const more = page.getByRole('button', { name: 'More actions for Debrief' });
    await more.focus();
    await page.keyboard.press('Enter');
    await page.getByRole('menuitem', { name: /Delete app/ }).click();
    const confirm = page.getByRole('alertdialog');
    await expect(confirm).toContainText('Delete Debrief?');
    await expect(confirm.getByRole('button', { name: 'Cancel' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(confirm).toBeHidden();
  });
});
