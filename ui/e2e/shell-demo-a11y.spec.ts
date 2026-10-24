import { axeEveryDialog, dialogContracts, expect, expectEscapeReturnsFocus, expectReachableByKeyboard, expectTabOrder, test, THEMES } from '@decryp7/paddock-e2e';

// Accessibility of every shell window (plan X49, WCAG 2.2 AA): axe over the window of every dialog the demo declares, in
// both themes, and keyboard-only flows. The dialog list is the generated contract file, so a new dialog is covered the
// moment it exists. The main window, its popovers and the Settings pages are checked in their own specs.
const CONTRACTS = 'projects/shell-demo/dialog-contracts.json';

test('every dialog window passes axe in both themes (the list is the contract file)', async ({ page }) => {
  expect(Object.keys(dialogContracts(CONTRACTS)).length).toBeGreaterThanOrEqual(10);
  // A dialog that needs an input to open gets a valid one (its window shows an inline error without it, which the unit tests cover).
  const inputs = { 'pd.channels.secret': { channelId: 'slack', label: 'Team Slack', kind: 'token' }, 'pd.channels.remove': { channelId: 'slack', name: 'Team Slack' } };
  expect(await axeEveryDialog(page, CONTRACTS, { inputs })).toEqual({});
});

for (const colorScheme of THEMES) {
  test.describe(`keyboard only (${colorScheme})`, () => {
    test.use({ colorScheme });

    test('the main window: the menu bar, the sidebar toggle, the jobs and notification items are reachable with Tab', async ({ app, page }) => {
      await page.goto('/');
      await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
      await expectReachableByKeyboard(
        page,
        {
          'File menu': app.menus.bar.getByRole('menuitem', { name: 'File', exact: true }),
          'sidebar toggle': app.frame.toggle,
          'notifications': app.statusBar.bell,
        },
        200,
      );
    });

    test('a menu opened with the keyboard closes with Escape and gives the focus back', async ({ app, page }) => {
      await page.goto('/');
      await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
      await expectEscapeReturnsFocus(page, app.menus.bar.getByRole('menuitem', { name: 'File', exact: true }), app.menus.menu('File'), 'ArrowDown');
    });

    test('a dialog window keeps the focus inside: Tab cycles, never traps, and every stop shows the focus', async ({ app, page }) => {
      await app.dialogs.openWindow('sd.rename');
      await app.dialogs.loaded();
      await expectTabOrder(page, { label: 'sd.rename', max: 40 });
    });
  });
}
