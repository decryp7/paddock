import { axeViolations, expect, expectFullyVisible, expectPageFillsHost, test, THEMES } from '@decryp7/paddock-e2e';
import type { Page } from '@playwright/test';

// The browser build with the mock backend; `simulate=0` keeps the demo still (no pushed toasts mid-test).
const APP = '/?simulate=0#';

/** Opens Settings (the shell's window; in the browser build it opens in the page) at a page of its navigation. */
async function openSettings(page: Page, section?: string) {
  await page.getByRole('navigation', { name: 'Pages' }).waitFor();
  await page.keyboard.press('ControlOrMeta+,');
  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(dialog).toBeVisible();
  if (section) await dialog.getByRole('button', { name: section, exact: true }).click();
  return dialog;
}

/** Picks `label` in the themed select named `name` (a combobox button; its list lives in the page's overlay). */
async function choose(page: Page, scope: ReturnType<Page['locator']>, label: string, name = 'Theme') {
  await scope.getByRole('combobox', { name }).click();
  await page.getByRole('option', { name: label, exact: true }).click();
}

test.describe('Pitcrew in the browser (mock backend)', () => {
  test('the dashboard is about the runners, with the machine as context', async ({ page }) => {
    await page.goto(`${APP}/`);
    await expect(page.getByRole('region', { name: /^Runners CPU:/ })).toBeVisible();
    await expect(page.getByRole('region', { name: /^Runners memory:/ })).toBeVisible();
    await expect(page.getByTestId('machine-context')).toContainText('This machine:');
    await expect(page.getByRole('heading', { name: 'CPU', exact: true })).toBeVisible();
    await page.goto(`${APP}/runners/build-win`);
    await expect(page.getByRole('heading', { name: 'CPU and memory' })).toBeVisible();
  });

  test('navigates between the screens with the sidebar', async ({ page }) => {
    await page.goto(`${APP}/`);
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'build-win' }).first()).toBeVisible();

    const nav = page.getByRole('navigation', { name: 'Pages' });
    for (const [link, heading] of [
      ['Runners', 'Runners'],
      ['Activity', 'Activity'],
      ['Insights', 'Insights'],
      ['Storage', 'Storage'],
      ['Logs', 'Logs'],
      ['Updates', 'Updates'],
      ['Dashboard', 'Dashboard'],
    ] as const) {
      await nav.getByRole('link', { name: link }).click();
      await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
    }
  });

  test('the sidebar is keyboard navigable with arrow keys', async ({ page }) => {
    await page.goto(`${APP}/`);
    const nav = page.getByRole('navigation', { name: 'Pages' });
    await nav.getByRole('link', { name: 'Dashboard' }).focus();
    await page.keyboard.press('ArrowDown');
    await expect(nav.getByRole('link', { name: /Runners/ })).toBeFocused();
    await page.keyboard.press('End');
    await expect(nav.getByRole('link', { name: 'Updates' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Updates' })).toBeVisible();
  });

  test('drains a runner from its actions menu and resumes it', async ({ page }) => {
    await page.goto(`${APP}/runners/ui-tests`);
    await expect(page.getByRole('heading', { level: 1, name: 'ui-tests' })).toBeVisible();
    await page.getByRole('button', { name: 'Actions for ui-tests' }).click();
    await page.getByRole('menuitem', { name: 'Drain and pause' }).click();
    await expect(page.getByText('Draining ui-tests')).toBeVisible();
    await expect(page.locator('.title').getByText('Paused')).toBeVisible();

    await page.getByRole('button', { name: 'Actions for ui-tests' }).click();
    await page.getByRole('menuitem', { name: 'Resume' }).click();
    await expect(page.locator('.title').getByText('Idle')).toBeVisible();
  });

  test('updates a runner and shows the progress through the phases', async ({ page }) => {
    await page.goto(`${APP}/updates`);
    const row = page.getByRole('row', { name: /ui-tests/ });
    await row.getByRole('button', { name: 'Update ui-tests to 5.3.0' }).click();
    // The mock walks through the phases in ~11 s and then reports the new version as installed.
    await expect(row.getByRole('cell').nth(1)).toHaveText('5.3.0', { timeout: 20_000 });
  });

  test('switches the theme in Settings > Appearance and remembers it', async ({ page }) => {
    await page.goto(`${APP}/`);
    const settings = await openSettings(page, 'Appearance');
    await choose(page, settings, 'Dark');
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    const again = await openSettings(page, 'Appearance');
    await choose(page, again, 'Light');
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await choose(page, again, 'Same as the system');
    await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/);
  });

  test('the registration wizard checks each step and registers a runner', async ({ page }) => {
    await page.goto(`${APP}/runners`);
    await page.getByRole('button', { name: 'Register runner' }).first().click();
    const dialog = page.getByRole('dialog', { name: 'Register a runner' });
    await expect(dialog.getByText('Pitcrew registers it with your server')).toBeVisible();

    await dialog.getByRole('button', { name: 'Continue' }).click();
    await expect(dialog.getByRole('alert')).toContainText('Enter the server');

    await dialog.getByLabel('Server URL').fill('https://git.example.com');
    await dialog.getByLabel('Registration token').fill('demo-registration-token');
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await dialog.getByLabel('Name in Pitcrew').fill('nightly');
    await dialog.getByLabel('Labels').fill('windows');
    await dialog.getByLabel('Labels').press('Enter');
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await dialog.getByText('Run as the service account').click();
    await expect(dialog.getByText('Jobs would run as LocalSystem, without a desktop')).toBeVisible();
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await expect(dialog.getByText('nightly', { exact: true })).toBeVisible();
    await dialog.getByRole('button', { name: 'Register' }).click();
    // The registration is a job: it shows in the Jobs list and ends by itself.
    await expect(dialog).toBeHidden();
    await expect(page.getByRole('link', { name: 'nightly', exact: true })).toBeVisible();
  });

  test('a service that is not installed is one chip that says so, with its repair (Install), and it brings the runners back', async ({ page, app }) => {
    await page.goto('/?simulate=0&mock=notInstalled#/');
    // The chip says why in the shell's words (a build of ui/dist from before the cause words says "stopped": this fails then).
    await expect(app.statusBar.chip).toContainText('not installed');
    await app.statusBar.chip.click();
    await expect(app.notifications.services).toContainText('is not installed');
    await app.notifications.services.getByRole('button', { name: /^Install/ }).click();
    await expect(app.notifications.services).toBeHidden();
    await expect(page.getByRole('link', { name: 'build-win' }).first()).toBeVisible();
  });

  test('cleaning a disk is a job with progress that ends on its own', async ({ page, app }) => {
    await page.goto(`${APP}/runners/build-win?tab=disk`);
    await page.getByRole('button', { name: /Clean up now|Clean now/ }).first().click();
    await app.statusBar.jobs.click();
    await expect(app.jobs.list).toContainText('Cleaning up the disk');
    await expect(app.jobs.list).toContainText(/Done|done|Finished/, { timeout: 15_000 });
    // A job its page waits for is not announced a second time by the shell.
    await expect(page.locator('pd-notification-host').getByText(/Cleaning up the disk/)).toHaveCount(0);
  });

  test('copying a log line is confirmed on its button, not by a notice', async ({ page }) => {
    // The clipboard of the test browser is a stand-in: what matters is the confirmation, not the system clipboard.
    await page.addInitScript(() => Object.defineProperty(navigator, 'clipboard', { value: { writeText: async () => undefined }, configurable: true }));
    await page.goto(`${APP}/logs`);
    // The viewer follows the end of the log and scrolls: hold on to one line by its number.
    const seq = (await page.locator('[role=log] .line .seq').last().textContent())!.trim();
    const line = page.locator('[role=log] .line').filter({ has: page.locator('.seq', { hasText: new RegExp(`^\\s*${seq}\\s*$`) }) });
    await line.hover();
    await line.getByRole('button', { name: 'Copy this line' }).click();
    await expect(line.getByRole('button', { name: 'Line copied' })).toBeVisible();
    await expect(page.locator('pd-notification-host').getByText(/copied/i)).toHaveCount(0);
  });

  test('About, Updates and What’s New are windows of the shell', async ({ page, app }) => {
    await page.goto(`${APP}/`);
    await (await app.menus.item('Help', 'About')).click();
    await expect(page.getByRole('dialog', { name: /About/ })).toBeVisible();
    await page.keyboard.press('Escape');
    await (await app.menus.item('Help', 'Check for Updates…')).click();
    await expect(page.getByRole('dialog', { name: /Updates/ })).toBeVisible();
  });

  test('has no accessibility violations in either theme', async ({ page }) => {
    for (const colorScheme of THEMES) {
      await page.emulateMedia({ colorScheme });
      await page.goto(`${APP}/runners`);
      await expect(page.getByRole('heading', { level: 1, name: 'Runners' })).toBeVisible();
      // Retried: a button mid-transition or a loading skeleton is not a finding.
      await expect.poll(() => axeViolations(page), { timeout: 10_000 }).toEqual([]);
    }
  });
});

test.describe('native feel (ADR 0008)', () => {
  test('chrome cannot be selected, log text can, browser keys do nothing', async ({ page }) => {
    await page.goto(`${APP}/logs`);
    await expect(page.getByRole('heading', { level: 1, name: 'Logs' })).toBeVisible();
    const selectable = async (loc: ReturnType<typeof page.locator>) => loc.evaluate((e) => getComputedStyle(e).userSelect);
    expect(await selectable(page.getByRole('heading', { level: 1, name: 'Logs' }))).toBe('none');
    const line = page.locator('[role=log] .line .msg').first();
    await expect(line).toBeVisible();
    expect(await selectable(line)).toBe('text');
    expect(await page.evaluate(() => getComputedStyle(document.documentElement).overscrollBehaviorX)).toBe('none');

    // The browser menu is suppressed on chrome.
    const prevented = await page.evaluate(() => {
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
      document.querySelector('h1')!.dispatchEvent(e);
      return e.defaultPrevented;
    });
    expect(prevented).toBe(true);

    // Reload and zoom keys are swallowed: the page is not reloaded.
    await page.evaluate(() => ((window as unknown as { __alive: boolean }).__alive = true));
    for (const key of ['F5', 'Control+r', 'Control+=', 'Control+-', 'Control+0', 'Control+p']) await page.keyboard.press(key);
    await page.mouse.move(400, 400);
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -200);
    await page.keyboard.up('Control');
    await page.waitForTimeout(300);
    expect(await page.evaluate(() => (window as unknown as { __alive?: boolean }).__alive)).toBe(true);
    expect(await page.evaluate(() => window.visualViewport?.scale ?? 1)).toBe(1);
  });

  test('right-clicking selected code opens the shell’s Copy menu, and not the browser’s', async ({ page }) => {
    await page.goto(`${APP}/`);
    await page.getByRole('navigation', { name: 'Pages' }).waitFor();
    // Text is selectable where it is code or a text field (the app chrome is not); a command line is the usual case.
    await page.evaluate(() => {
      const pre = document.createElement('pre');
      pre.id = 'sample-command';
      pre.textContent = 'gitea-runner daemon --config config.yaml';
      pre.style.cssText = 'position:fixed;top:120px;left:400px;z-index:9999;background:white;color:black;padding:8px';
      document.body.append(pre);
    });
    const sample = page.locator('#sample-command');
    await sample.click({ clickCount: 3 });
    await sample.click({ button: 'right' });
    await expect(page.getByRole('menuitem', { name: 'Copy' })).toBeVisible();
  });

  test('the log viewer fills the page: at least 400 px tall with its lines visible, in every way into Logs', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    for (const path of ['/logs', '/logs/build-win', '/logs?runner=build-win']) {
      await page.goto(`${APP}${path}`);
      const viewport = page.locator('cdk-virtual-scroll-viewport[role=log]');
      await expect(viewport).toBeVisible();
      await expect.poll(async () => (await viewport.boundingBox())?.height ?? 0, { message: path }).toBeGreaterThanOrEqual(400);
      await expect(page.locator('[role=log] .line .msg').first()).toBeVisible();
      await expectPageFillsHost(page, { page: 'pc-logs-page' });
    }
  });

  test('the config diff window scrolls its changes inside and keeps Save and apply fully visible', async ({ page, app }) => {
    const lines = Array.from({ length: 60 }, (_, i) => ({
      kind: i % 2 ? 'added' : 'removed',
      text: `  key_${i}: ${'a very long value that does not fit the window '.repeat(6)}`,
      oldNo: i % 2 ? null : i + 1,
      newNo: i % 2 ? i + 1 : null,
    }));
    const preview = { text: '', changed: true, restartNeeded: true, diff: { hunks: [{ lines }], added: 30, removed: 30, truncated: false }, issues: [], changedKeys: ['runner.capacity'], labels: [] };
    await page.setViewportSize({ width: 720, height: 560 });
    await app.dialogs.openWindow('pc.config-diff', { preview, maxBackups: 5 });
    await app.dialogs.loaded();
    const save = page.getByRole('button', { name: 'Save and apply' });
    await expect(save).toBeVisible();
    await expectFullyVisible(save, 'Save and apply');
    // The changes scroll inside their own region, both ways; the window itself does not scroll.
    const diff = page.getByRole('region', { name: 'Changes to config.yaml' });
    expect(await diff.evaluate((e) => e.scrollWidth > e.clientWidth && e.scrollHeight > e.clientHeight)).toBe(true);
    expect(await page.locator('main.pd-body').evaluate((e) => e.scrollHeight <= e.clientHeight && e.scrollWidth <= e.clientWidth)).toBe(true);
    // One title: the window's, not a second heading inside it.
    await expect(page.getByRole('heading', { name: 'Review changes' })).toHaveCount(0);
  });

  test('the register window keeps its buttons fully visible on every step, the fields scroll above them', async ({ page, app }) => {
    await page.setViewportSize({ width: 660, height: 640 });
    await app.dialogs.openWindow('pc.register', {});
    await app.dialogs.loaded();
    const dialog = page.locator('pd-dialog-shell');
    const buttonsInside = async (name: RegExp) => {
      await expectFullyVisible(dialog.getByRole('button', { name }).last(), String(name));
    };
    await dialog.getByLabel('Server URL').fill('https://git.example.com');
    await dialog.getByLabel('Registration token').fill('demo-registration-token');
    await buttonsInside(/Continue/);
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await dialog.getByLabel('Name in Pitcrew').fill('nightly');
    await buttonsInside(/Continue/);
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await dialog.getByText('Run as the service account').click();
    await buttonsInside(/Continue/);
    await dialog.getByRole('button', { name: 'Continue' }).click();
    await buttonsInside(/^Register$/);
    expect(await page.locator('main.pd-body').evaluate((e) => e.scrollHeight <= e.clientHeight)).toBe(true);
  });

  test('every Settings page renders its content in the real Settings window (a window without the main window\'s router)', async ({ page, app }) => {
    const pages: [string, string][] = [
      ['pc.general', 'Start-up'],
      ['pc.service', 'Pitcrew service'],
      ['pc.defaults', 'Leftover processes'],
      ['pc.channels', 'Notification channels'],
      ['pc.tokens', 'Gitea read tokens'],
      ['pc.diagnostics', 'Diagnostics'],
    ];
    await page.setViewportSize({ width: 920, height: 1500 });
    for (const [id, heading] of pages) {
      // A new document each time: a window opens its page once.
      await page.goto('about:blank');
      await app.dialogs.openWindow('pd.settings', { page: id });
      await app.dialogs.loaded();
      await expect(page.getByRole('heading', { name: heading, exact: true }), id).toBeVisible();
    }
  });

  test('edits the runner configuration, reviews the diff and saves', async ({ page }) => {
    await page.goto(`${APP}/runners/build-win?tab=config`);
    await expect(page.getByRole('heading', { name: 'Runner configuration' })).toBeVisible();
    await expect(page.getByTestId('label-check')).toContainText('Labels verified after restart');
    await expect(page.getByRole('button', { name: 'Review changes' })).toBeDisabled();

    await page.getByLabel('Capacity').fill('4');
    const review = page.getByRole('button', { name: 'Review changes' });
    await expect(review).toBeEnabled();
    await review.click();

    const dialog = page.getByRole('dialog', { name: 'Review configuration changes' });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText('capacity: 4')).toBeVisible();
    await expect(dialog.locator('tr.added').first()).toContainText('+');
    await expect(dialog.locator('tr.removed').first()).toContainText('−');
    await expect(dialog).toContainText('The runner restarts after its current job');
    await dialog.getByRole('button', { name: 'Save and apply' }).click();

    await expect(page.getByTestId('saved-banner')).toContainText('The runner restarts after the current job');
    await expect(page.getByTestId('restart-pending')).toBeVisible();
    await expect(page.getByLabel('Capacity')).toHaveValue('4');
  });

  test('an invalid capacity shows the engine’s message and cannot be saved', async ({ page }) => {
    await page.goto(`${APP}/runners/ui-tests?tab=config`);
    await page.getByLabel('Capacity').fill('0');
    await expect(page.getByText('Capacity must be between 1 and 64')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Review changes' })).toBeDisabled();
    await page.getByLabel('Capacity').fill('2');
    await expect(page.getByRole('button', { name: 'Review changes' })).toBeEnabled();
  });

  test('Advanced mode points at the line of a syntax error', async ({ page }) => {
    await page.goto(`${APP}/runners/ui-tests?tab=config`);
    await page.getByRole('radio', { name: 'Advanced (YAML)' }).click();
    const editor = page.getByRole('textbox', { name: 'config.yaml' });
    await expect(page.getByTestId('yaml-status')).toHaveText('Valid YAML');
    const text = await editor.inputValue();
    await editor.fill(text.replace('  capacity: 1\n', '\tcapacity: 1\n'));
    await expect(page.getByTestId('yaml-status')).toHaveText('1 error');
    await expect(page.getByRole('button', { name: /Error on line \d+:/ })).toContainText('tab');
    await expect(page.getByText('You edited the file directly; save or discard to use the form again.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Review changes' })).toBeDisabled();
    await page.getByRole('button', { name: 'Discard changes' }).click();
    await expect(page.getByLabel('Capacity')).toHaveValue('1');
  });

  test('the diff dialog takes focus and Escape closes it', async ({ page }) => {
    await page.goto(`${APP}/runners/ui-tests?tab=config`);
    await page.getByLabel('Capacity').fill('3');
    await page.getByRole('button', { name: 'Review changes' }).click();
    const dialog = page.getByRole('dialog', { name: 'Review configuration changes' });
    await expect(dialog).toBeVisible();
    await expect(dialog.locator(':focus')).toHaveCount(1);
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(page.getByLabel('Capacity')).toHaveValue('3');
  });

  test('lists leftover processes, keeps one longer and stops another after a confirmation', async ({ page }) => {
    await page.goto(`${APP}/runners/build-win?tab=processes`);
    await expect(page.getByTestId('process-summary')).toHaveText('8 processes, 3 leftovers');
    const gradle = page.locator('tr', { hasText: 'java.exe' });
    await expect(gradle.getByText('Leftover')).toBeVisible();
    await expect(gradle.getByTestId('fate')).toContainText('stops in 2 h 14 min');
    const sleep = page.locator('tr', { hasText: 'sleep.exe' });
    await expect(sleep.getByTestId('fate')).toContainText('stops in 7 min');

    await sleep.getByRole('button', { name: 'Keep sleep.exe longer' }).click();
    await page.getByRole('menuitem', { name: '+ 1 hour' }).click();
    await expect(sleep.getByTestId('fate')).toContainText('stops in 1 h 0');

    await sleep.getByRole('button', { name: /^Stop sleep.exe now/ }).click();
    const dialog = page.getByRole('dialog', { name: 'Stop process' });
    await expect(dialog).toContainText('asked to stop first');
    await dialog.getByRole('button', { name: 'Stop now' }).click();
    await expect(page.locator('tr', { hasText: 'sleep.exe' })).toHaveCount(0);
    await expect(page.getByTestId('process-summary')).toHaveText('7 processes, 2 leftovers');
  });

  test('the runner process has no plain Stop button, only a confirmed overflow action', async ({ page }) => {
    await page.goto(`${APP}/runners/ui-tests?tab=processes`);
    const runner = page.locator('tr', { hasText: 'gitea-runner.exe' });
    await expect(runner.getByRole('button', { name: /^Stop / })).toHaveCount(0);
    await runner.getByRole('button', { name: /More actions for/ }).click();
    await page.getByRole('menuitem', { name: 'Stop the runner process…' }).click();
    const dialog = page.getByRole('dialog', { name: 'Stop process' });
    await expect(dialog).toContainText('Pitcrew will restart it');
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toBeHidden();
  });

  test('the dashboard and runners page link leftovers to the Processes tab', async ({ page }) => {
    await page.goto(`${APP}/runners`);
    await page.getByRole('link', { name: '3 leftover processes' }).click();
    await expect(page.getByTestId('process-summary')).toBeVisible();
  });

  test('Settings > Defaults edits the leftover process rules', async ({ page }) => {
    await page.goto(`${APP}/`);
    await openSettings(page, 'Defaults');
    await expect(page.getByRole('heading', { name: 'Leftover processes' })).toBeVisible();
    await page.getByRole('button', { name: 'Add rule' }).last().click();
    await page.getByLabel('Pattern of rule 6').fill('*chrome*');
    const save = page.locator('pc-process-policy-editor').getByRole('button', { name: 'Save', exact: true });
    await save.click();
    await expect(page.getByText('Process defaults saved')).toBeVisible();
  });

  test('with the service down after an update, Settings still shows the service and Repair brings it back', async ({ page }) => {
    await page.goto('/?simulate=0&mock-down&mock-startfailed#/');
    await openSettings(page, 'Service');
    await expect(page.getByTestId('service-start-failed').first()).toContainText('could not start after the update');
    await expect(page.getByText('Installed, not running')).toBeVisible();
    await page.getByRole('button', { name: 'Repair service' }).click();
    await expect(page.getByText('Installed and running')).toBeVisible();
    await expect(page.getByTestId('service-start-failed')).toHaveCount(0);
  });

  test('the service chip repairs the service when it could not start after the update (rule 19: the repair is the shell’s)', async ({ page, app }) => {
    await page.goto('/?simulate=0&mock-down&mock-startfailed#/');
    await expect(app.statusBar.chip).toBeVisible();
    await app.statusBar.chip.click();
    await app.notifications.services.getByRole('button', { name: 'Restart' }).click();
    await expect(app.notifications.services).toBeHidden();
  });
});
