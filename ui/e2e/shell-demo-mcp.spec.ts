import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// Settings > AI access (MCP) (plan X22, ADR 0007) in the shell demo: the page over the demo's in-memory engine, which plays
// `paddock-mcp` (off by default, a key returned once, an audit log). `?mcp=calls` starts with recorded calls, `?mcp=error` fails the
// first ask once (both only where the page is opened with the query, `app.settings.open`; the dialogs flow runs from the main window). Chromium only in branches (rule 3). Only the test kit. Run it on its own port:
//   SHELL_DEMO_PORT=47xx npx playwright test --project=shell-demo shell-demo-mcp


/** From the main window: the Settings window and the dialogs it opens both open in the page (a dialog window of the browser build cannot open another one). */
async function openPage(page: import('@playwright/test').Page, app: { settings: { nav: import('@playwright/test').Locator } }, query: string): Promise<void> {
  await page.goto(`/${query}`);
  await page.getByRole('button', { name: 'Settings…', exact: true }).click();
  await app.settings.nav.getByRole('button', { name: 'AI access (MCP)' }).click();
  await expect(page.getByTestId('mcp-page')).toBeVisible();
}

test.describe('AI access (MCP) (plan X22)', () => {
  test('turn it on, allow an action, add a client (the key is shown once), revoke it, read and clear the log', async ({ app, page, pageErrors }) => {
    await openPage(page, app, '');
    await expect(page.getByTestId('mcp-enabled')).not.toBeChecked();
    await expect(page.getByText('AI access is off')).toBeVisible();
    // Tools are read-only; the action is a separate, off-by-default permission with a warning.
    await expect(page.getByTestId('mcp-tools')).not.toContainText('Export a report');
    await expect(page.getByTestId('mcp-actions-warning')).toContainText('change something');
    await expect(page.getByTestId('mcp-action-export_report')).not.toBeChecked();
    await expect(page.getByTestId('mcp-no-clients')).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);

    await page.getByTestId('mcp-enabled').check();
    await expect(page.getByText('AI access is on')).toBeVisible();
    await page.getByTestId('mcp-tool-errors_around').uncheck();
    await expect(page.getByTestId('mcp-tool-errors_around')).not.toBeChecked();
    await page.getByTestId('mcp-action-export_report').check();
    await expect(page.getByTestId('mcp-action-export_report')).toBeChecked();

    // The key is created and shown in the key dialog's window, once.
    await page.getByTestId('mcp-add-client').click();
    const dialog = page.getByRole('dialog', { name: 'Add a client' });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByTestId('dialog-action-create')).toBeDisabled();
    await dialog.getByTestId('mcp-key-client').fill('Pitwall');
    await dialog.getByTestId('mcp-key-client').press('Enter');
    await expect(dialog.getByTestId('mcp-key-shown')).toContainText('only time the key is shown');
    const key = await dialog.getByTestId('mcp-key-value').inputValue();
    expect(key).toMatch(/^pmk_[0-9a-f]{64}$/);
    expect(await axeViolations(page)).toEqual([]);
    await dialog.getByTestId('dialog-action-done').click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByTestId('mcp-keys')).toContainText('Pitwall');
    await expect(page.getByTestId('mcp-keys')).toContainText('Active');
    // The key is nowhere in the page after the dialog closed.
    await expect(page.locator('body')).not.toContainText(key);

    // Revoke asks first; Keep is the default and Escape keeps the key.
    await page.getByTestId('mcp-revoke').click();
    const confirm = page.getByRole('dialog', { name: 'Revoke the key of "Pitwall"?' });
    await expect(confirm.getByRole('button', { name: 'Keep' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(confirm).toHaveCount(0);
    await expect(page.getByTestId('mcp-keys')).toContainText('Active');
    await page.getByTestId('mcp-revoke').click();
    await page.getByRole('dialog', { name: 'Revoke the key of "Pitwall"?' }).getByRole('button', { name: 'Revoke key', exact: true }).click();
    await expect(page.getByTestId('mcp-keys')).toContainText('Revoked');
    await expect(page.getByTestId('mcp-revoke')).toHaveCount(0);

    // The log: a client calls tools (played by the demo's engine), redacted arguments, then Clear.
    await page.evaluate(() => {
      const engine = (globalThis as { __pdDemoMcp?: { recordCall(call: object): void } }).__pdDemoMcp;
      engine?.recordCall({ client: 'Pitwall', tool: 'search_events', arguments: { query: '<omitted: 17 characters>', limit: 20 }, resultBytes: 4120, durationMs: 38, outcome: 'ok' });
      engine?.recordCall({ client: 'script', tool: 'export_report', arguments: {}, resultBytes: 0, durationMs: 0, outcome: 'refused' });
    });
    const log = page.getByTestId('mcp-audit');
    await expect(log).toContainText('search_events');
    await expect(log).toContainText('export_report');
    await expect(log).toContainText('Refused');
    await expect(log).not.toContainText('rm -rf');
    expect(await axeViolations(page)).toEqual([]);
    await page.getByTestId('mcp-audit-clear').click();
    await expect(log).toContainText('No client has called a tool yet.');
    await expect(page.getByTestId('mcp-audit-clear')).toBeDisabled();
    expect(pageErrors).toEqual([]);
  });

  test('a load error is inline with Retry', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.mcp' }, '?mcp=error');
    const error = page.getByTestId('mcp-error');
    await expect(error).toHaveAttribute('role', 'alert');
    await error.getByRole('button', { name: 'Retry' }).click();
    await expect(page.getByTestId('mcp-enabled')).toBeVisible();
  });

  test('320 px: no horizontal scroll, the table and the log stay usable, axe is clean', async ({ app, page }) => {
    await openPage(page, app, '');
    await page.evaluate(() => {
      const engine = (globalThis as { __pdDemoMcp?: { recordCall(call: object): void } }).__pdDemoMcp;
      engine?.recordCall({ client: 'Pitwall', tool: 'search_events', arguments: { query: '<omitted: 17 characters>', limit: 20 }, resultBytes: 4120, durationMs: 38, outcome: 'ok' });
    });
    await page.getByTestId('mcp-add-client').click();
    const dialog = page.getByRole('dialog', { name: 'Add a client' });
    await dialog.getByTestId('mcp-key-client').fill('A rather long client name that must wrap, not push sideways');
    await dialog.getByTestId('mcp-key-client').press('Enter');
    await dialog.getByTestId('dialog-action-done').click();
    await page.setViewportSize({ width: 320, height: 800 });
    await expectNoHorizontalScroll(page);
    await expect(page.getByTestId('mcp-keys')).toBeVisible();
    await expect(page.getByTestId('mcp-audit')).toContainText('search_events');
    expect(await axeViolations(page)).toEqual([]);
  });
});
