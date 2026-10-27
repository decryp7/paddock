import { expect, test } from '@playwright/test';

// The browser build with the mock backend; `simulate=0` keeps the demo still (no pushed toasts mid-test).
const APP = '/?simulate=0#';

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

    const nav = page.getByRole('navigation', { name: 'Main' });
    for (const [link, heading] of [
      ['Runners', 'Runners'],
      ['Activity', 'Activity'],
      ['Logs', 'Logs'],
      ['Updates', 'Updates'],
      ['Settings', 'Settings'],
      ['Dashboard', 'Dashboard'],
    ] as const) {
      await nav.getByRole('link', { name: link }).click();
      await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
    }
  });

  test('the sidebar is keyboard navigable with arrow keys', async ({ page }) => {
    await page.goto(`${APP}/`);
    const nav = page.getByRole('navigation', { name: 'Main' });
    await nav.getByRole('link', { name: 'Dashboard' }).focus();
    await page.keyboard.press('ArrowDown');
    await expect(nav.getByRole('link', { name: /Runners/ })).toBeFocused();
    await page.keyboard.press('End');
    await expect(nav.getByRole('link', { name: 'Settings' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
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

  test('switches the theme and remembers it', async ({ page }) => {
    await page.goto(`${APP}/settings`);
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('radio', { name: 'Light' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.getByRole('radio', { name: 'Follow system' }).click();
    await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/);
  });

  test('the registration wizard checks each step and registers a runner', async ({ page }) => {
    await page.goto(`${APP}/runners`);
    await page.getByRole('button', { name: 'Register runner' }).first().click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByRole('heading', { name: 'Register a runner' })).toBeVisible();

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

    await expect(dialog).toBeHidden();
    await expect(page.getByRole('link', { name: 'nightly', exact: true })).toBeVisible();
  });

  test('shows the banner and a way out when the service is not installed', async ({ page }) => {
    await page.goto('/?simulate=0&mock=notInstalled#/');
    const banner = page.getByRole('alert').filter({ hasText: 'not installed' });
    await expect(banner).toBeVisible();
    await banner.getByRole('button', { name: 'Install service' }).click();
    await expect(banner).toBeHidden();
    await expect(page.getByRole('link', { name: 'build-win' }).first()).toBeVisible();
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

  test('edits the runner configuration, reviews the diff and saves', async ({ page }) => {
    await page.goto(`${APP}/runners/build-win?tab=config`);
    await expect(page.getByRole('heading', { name: 'Runner configuration' })).toBeVisible();
    await expect(page.getByTestId('label-check')).toContainText('Labels verified after restart');
    await expect(page.getByRole('button', { name: 'Review changes' })).toBeDisabled();

    await page.getByLabel('Capacity').fill('4');
    const review = page.getByRole('button', { name: 'Review changes' });
    await expect(review).toBeEnabled();
    await review.click();

    const dialog = page.getByRole('dialog', { name: 'Review changes' });
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
    const dialog = page.getByRole('dialog', { name: 'Review changes' });
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
    const dialog = page.getByRole('dialog', { name: 'Stop sleep.exe?' });
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
    const dialog = page.getByRole('dialog', { name: 'Stop the runner process?' });
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
    await page.goto(`${APP}/settings?section=defaults`);
    await expect(page.getByRole('heading', { name: 'Leftover processes' })).toBeVisible();
    await page.getByRole('button', { name: 'Add rule' }).last().click();
    await page.getByLabel('Pattern of rule 6').fill('*chrome*');
    const save = page.locator('pc-process-policy-editor').getByRole('button', { name: 'Save', exact: true });
    await save.click();
    await expect(page.getByText('Process defaults saved')).toBeVisible();
  });

  test('with the service down after an update, Settings still shows the service and Repair brings it back', async ({ page }) => {
    await page.goto('/?simulate=0&mock-down&mock-startfailed#/settings?section=service');
    await expect(page.getByTestId('service-start-failed').first()).toContainText('could not start after the update');
    await expect(page.getByText('Installed, not running')).toBeVisible();
    await page.getByRole('button', { name: 'Repair service' }).click();
    await expect(page.getByText('Installed and running')).toBeVisible();
    await expect(page.getByTestId('service-start-failed')).toHaveCount(0);
  });

  test('the global notice repairs the service when it could not start after the update', async ({ page }) => {
    await page.goto('/?simulate=0&mock-down&mock-startfailed#/');
    const notice = page.getByTestId('service-start-failed');
    await expect(notice).toContainText('could not start after the update');
    await notice.getByRole('button', { name: 'Repair' }).click();
    await expect(notice).toHaveCount(0);
    await expect(page.getByRole('alert').filter({ hasText: 'cannot reach its service' })).toHaveCount(0);
  });
});
