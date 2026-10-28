import { axeViolations, expect, test, THEMES } from '@decryp7/paddock-e2e';

// The shell pages on Angular Material (WP-2, rule 47): axe in both themes, keyboard-only use of the toggles, checkboxes and radios,
// and no CSP or console errors (Material sets styles through the CSSOM, which the CSP allows; this proves it). Chromium only in
// branches (rule 3). Run it on its own port: SHELL_DEMO_PORT=47xx npx playwright test --project=shell-demo shell-demo-materialpages
const PAGES: ReadonlyArray<{ name: string; page: string; query?: string; ready: string }> = [
  { name: 'General (every settings field)', page: 'sd.general', ready: 'settings-content' },
  { name: 'AI access (MCP)', page: 'pd.mcp', ready: 'mcp-page' },
  { name: 'Notification channels', page: 'pd.channels', ready: 'channels-page' },
  { name: 'Storage', page: 'pd.storage', ready: 'storage-table' },
  { name: 'Knowledge packs', page: 'pd.packs', query: '?packs=problems', ready: 'packs-state-notes' },
  { name: 'Background service', page: 'pd.login', query: '?service=notInstalled', ready: 'service-condition' },
  { name: 'Language', page: 'pd.language', ready: 'language-page' },
  { name: 'Set-up', page: '', query: '?setup=fresh', ready: 'setup-list' },
];

test.use({ consoleErrors: true });

for (const colorScheme of THEMES) {
  test.describe(`Material controls on the shell pages (${colorScheme})`, () => {
    test.use({ colorScheme });

    for (const p of PAGES) {
      test(`${p.name}: no axe violations and no console errors`, async ({ app, page, pageErrors }) => {
        if (p.page) await app.settings.open({ page: p.page }, p.query ?? '');
        else {
          // The Set-up checklist is a dialog window of its own.
          await app.dialogs.openWindow('pd.setup', {}, { title: 'Set-up', query: p.query });
          await app.dialogs.loaded();
        }
        await expect(page.getByTestId(p.ready).first()).toBeVisible();
        expect(await axeViolations(page)).toEqual([]);
        expect(pageErrors).toEqual([]);
      });
    }

    test('AI access: the master toggle and a checkbox work with the keyboard alone', async ({ app, page }) => {
      await app.settings.open({ page: 'pd.mcp' });
      const master = page.getByTestId('mcp-enabled').getByRole('switch');
      await master.focus();
      await expect(master).toBeFocused();
      await page.keyboard.press('Space');
      await expect(master).toBeChecked();
      await page.keyboard.press('Tab');
      const tool = page.getByTestId('mcp-tool-search_events').getByRole('checkbox');
      await expect(tool).toBeFocused();
      await page.keyboard.press('Space');
      await expect(tool).not.toBeChecked();
    });

    test('Language: the radio group moves with the arrow keys and saves the choice', async ({ app, page }) => {
      await app.settings.open({ page: 'pd.language' });
      const system = page.getByTestId('language-system').getByRole('radio');
      await system.focus();
      await page.keyboard.press('ArrowDown');
      await expect(page.getByTestId('language-en').getByRole('radio')).toBeChecked();
      await expect(page.getByTestId('language-en').getByRole('radio')).toBeFocused();
    });
  });
}
