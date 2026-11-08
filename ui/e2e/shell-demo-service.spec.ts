import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// Settings > Background service (plan X4) in the shell demo: the page over the demo's in-memory service, which plays
// `paddock-service` (the same statuses, live phases and refusals). `?service=<condition>` picks where it starts and
// `?service-outcome=fail|hang|...` how a repair or install ends. Chromium only in branches (rule 3). Only the test kit.

test.describe('background service (plan X4)', () => {
  test('an install walks its phases and ends running with nothing left to press', async ({ app, page, pageErrors }) => {
    await app.settings.open({ page: 'pd.login' }, '?service=notInstalled&service-step=manual');
    await expect(page.getByTestId('service-condition')).toContainText('Not installed');
    const install = page.getByRole('button', { name: 'Install' });
    await expect(install).toHaveClass(/mat-mdc-unelevated-button/);
    await expect(page.getByRole('button', { name: 'Repair' })).toHaveCount(0);
    // The login switch is an ordinary setting beneath the status.
    await expect(app.settings.field('launchAtLogin').getByRole('switch', { name: 'Open at login' })).not.toBeChecked();
    expect(await axeViolations(page)).toEqual([]);
    await expectNoHorizontalScroll(page);

    // The demo service's phases wait for the test (`?service-step=manual`): a poll can miss a phase that lasts as long as a 500 ms timer.
    const step = async () => {
      await page.waitForFunction(() => '__sdServiceStep' in window);
      await page.evaluate(() => (window as unknown as { __sdServiceStep: () => void }).__sdServiceStep());
    };
    await install.click();
    await step();
    await expect(page.getByTestId('service-working')).toContainText('Installing the service…');
    await expect(page.getByTestId('service-step')).toHaveText('Removing the old registration');
    await step();
    await expect(page.getByTestId('service-step')).toHaveText('Registering the service');
    await step();
    await expect(page.getByTestId('service-step')).toHaveText('Checking that it answers');
    await step();
    await expect(page.getByTestId('service-outcome')).toContainText('installed and running');
    await expect(page.getByTestId('service-condition')).toContainText('Running');
    await expect(page.getByTestId('service-working')).toHaveCount(0);
    await expect(page.locator('.pd-service__actions')).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });

  test('a repair shows its phase and "Still working" for as long as it runs', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.login' }, '?service=notRunning&service-outcome=hang&service-step=manual');
    await expect(page.getByTestId('service-condition')).toContainText('Not running');
    await page.getByRole('button', { name: 'Repair' }).click();
    const step = async () => {
      await page.waitForFunction(() => '__sdServiceStep' in window);
      await page.evaluate(() => (window as unknown as { __sdServiceStep: () => void }).__sdServiceStep());
    };
    await step();
    await step();
    await step();
    await expect(page.getByTestId('service-step')).toHaveText('Checking that it answers');
    await expect(page.getByTestId('service-still-working')).toContainText('Still working');
    await expect(page.getByTestId('service-condition')).toContainText('Repairing');
    await expect(page.getByRole('button', { name: 'Repair' })).toHaveCount(0);
    await expect(page.locator('mat-progress-bar')).toHaveAccessibleName('Working…');
    // Nothing gives up on it: no step is left to run, and the page still says so.
    await expect(page.getByTestId('service-still-working')).toBeVisible();
  });

  test('a failed repair says why inline and Retry runs it again', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.login' }, '?service=notRunning&service-outcome=fail');
    await page.getByRole('button', { name: 'Repair' }).click();
    const failure = page.getByTestId('service-failure');
    await expect(failure).toHaveAttribute('role', 'alert');
    await expect(failure).toContainText('The repair did not work.');
    await expect(failure).toContainText('Close the other copy of the app');
    await expect(page.getByRole('button', { name: 'Repair' })).toBeEnabled();
    await failure.getByRole('button', { name: 'Retry' }).click();
    await expect(page.getByTestId('service-working')).toBeVisible();
    await expect(failure).toContainText('The repair did not work.');
    await expect(page.getByTestId('service-working')).toHaveCount(0);
  });

  test('switched off in Login Items: the only button opens Login Items, and the app never flips the switch', async ({ app, page }) => {
    await app.settings.open({ page: 'pd.login' }, '?service=switchedOff');
    await expect(page.getByTestId('service-condition')).toContainText('Switched off');
    await expect(page.getByTestId('service-message')).toContainText('switched off in Login Items');
    await expect(page.locator('.pd-service__actions button')).toHaveText(['Open Login Items…']);
    await expect(page.getByRole('button', { name: 'Open Login Items…' })).toHaveClass(/mat-mdc-unelevated-button/);
    await page.getByRole('button', { name: 'Open Login Items…' }).click();
    await expect(page.getByTestId('service-failure')).toHaveCount(0);
    await expect(page.getByTestId('service-condition')).toContainText('Switched off');
  });
});
