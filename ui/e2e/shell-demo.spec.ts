import { axeViolations, dialogContracts, expect, expectNoHorizontalScroll, test, THEMES } from '@decryp7/paddock-e2e';

// The shell demo is the reference consumer of @decryp7/paddock-ui/shell (ADR 0012 §7); each work package adds its checks.
// It uses only the test kit (@decryp7/paddock-e2e, plan S16); the static source checks are in shell/testing's specs.

for (const colorScheme of THEMES) {
  test.describe(`shell demo (${colorScheme})`, () => {
    test.use({ colorScheme });

    test('boots past the splash and axe finds no violations', async ({ app, page, pageErrors }) => {
      await page.goto('/');
      await expect(page.getByRole('heading', { level: 1, name: 'Overview' })).toBeVisible();
      await expect(app.frame.splash).toHaveCount(0);
      expect(await axeViolations(page)).toEqual([]);
      expect(pageErrors).toEqual([]);
    });
  });
}

test('no horizontal scroll at 320 px (rule 18)', async ({ app, page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await app.frame.open();
  await expectNoHorizontalScroll(page);
});

test.describe('contributions (plan S2)', () => {
  test('a contributed command runs with the lazily loaded app context', async ({ app, page }) => {
    await page.goto('/');
    // The app's part of the context is loaded when the window is idle, not in the first-paint bundle.
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    const reset = page.getByRole('button', { name: 'Reset greetings', exact: true });
    await expect(reset).toBeDisabled();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(app.notifications.toasts.getByText('Hello from the shell demo (1)')).toBeVisible();
    await expect(reset).toBeEnabled();
    await reset.click();
    await expect(page.getByText('Greetings reset')).toBeVisible();
    await expect(reset).toBeDisabled();
  });

  test('a failing command is shown as a notice and the app keeps working', async ({ page, pageErrors }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Fail on purpose' }).click();
    await expect(page.getByText('Fail on purpose failed: this command always fails')).toBeVisible();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(page.getByText('Hello from the shell demo (1)')).toBeVisible();
    expect(pageErrors).toEqual([]);
  });

  test('every contribution kind is registered', async ({ page }) => {
    await page.goto('/#/about');
    const rows = page.getByRole('table', { name: 'Registered contributions' }).getByRole('row');
    await expect(rows).toHaveCount(12);
    for (const kind of ['commands', 'keybindings', 'menus', 'pages', 'statusItems', 'settingsPages', 'dialogs', 'healthServices', 'notificationKinds', 'themes', 'submenus']) {
      await expect(rows.filter({ has: page.getByRole('cell', { name: kind, exact: true }) })).toHaveText(new RegExp(`^${kind}[1-9]\\d*$`));
    }
  });
});

test.describe('jobs (plan S9)', () => {
  test('a job reports its phases in the status item and the list, and stops on Cancel', async ({ app, page, pageErrors }) => {
    await page.goto('/');
    const status = app.statusBar.jobs;
    await expect(status).toHaveText('Jobs');
    await page.getByRole('button', { name: 'Start sample job' }).click();
    // The item names the lead job; its phase is in the tooltip.
    // The lead's phase and value follow its name (rule 11).
    await expect(status).toHaveText(/^1 running: Importing logs · (Finding files [\d,]+ so far|Reading \d+%)$/);
    await expect(status).toHaveAttribute('title', /^Importing logs: (Finding files|Reading)$/);

    await status.click();
    await expect(status).toHaveAttribute('aria-expanded', 'true');
    const job = app.jobs.items;
    await expect(job).toHaveCount(1);
    await expect(job).toContainText('Importing logs');
    // Each phase has its own scale: discovery counts up, then reading has a determinate bar.
    await expect(job.getByRole('progressbar')).toHaveAttribute('aria-valuenow', /\d+/);
    await expect(job).toContainText('Reading');
    await job.getByRole('button', { name: 'Cancel' }).click();
    await expect(job).toContainText('Cancelled');
    await expect(job).toContainText('Stopped at your request.');
    await expect(status).toHaveText('Jobs');
    await expect(app.notifications.toastItems).toHaveCount(0);

    await page.keyboard.press('Escape');
    await expect(app.jobs.list).toHaveCount(0);
    await expect(status).toBeFocused();
    expect(pageErrors).toEqual([]);
  });

  test('a job that finishes is announced; the step that cannot be stopped says so', async ({ app, page }) => {
    await page.clock.install();
    await page.goto('/');
    await page.getByRole('button', { name: 'Start sample job' }).click();
    await app.statusBar.jobs.click();
    const job = app.jobs.items;
    // Finding files (2 s) and reading (5 s) pass; writing the index cannot be interrupted.
    await page.clock.runFor(7_100);
    await expect(job).toContainText('Writing the index');
    await expect(job.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    await page.clock.runFor(1_000);
    await expect(job).toContainText('Done');
    await expect(app.notifications.toasts).toContainText('Importing logs: done');
  });

  test('a quiet job shows "Still working" and is never timed out', async ({ app, page }) => {
    await page.clock.install();
    await page.goto('/');
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    const status = app.statusBar.jobs;
    await expect(status).toHaveText(/^1 running: Update runner · Waiting for the device/);
    await expect(status).toHaveAttribute('title', 'Update runner: Waiting for the device');
    await page.clock.runFor(1_600);
    await expect(status).toHaveAttribute('title', 'Update runner: still working, this step is taking longer than usual');
    // Ten minutes without an event: still running, still "Still working", never failed.
    await page.clock.fastForward('10:00');
    await expect(status).toHaveAttribute('data-stalled', 'true');
    await status.click();
    const job = app.jobs.items;
    await expect(job).toContainText('Still working. This step is taking longer than usual.');
    await job.getByRole('button', { name: 'Cancel' }).click();
    await page.clock.runFor(200);
    await expect(job).toContainText('Cancelled');
    await expect(status).toHaveText('Jobs');
  });

  test('the jobs popover has no axe violations', async ({ app, page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await app.statusBar.jobs.click();
    await expect(app.jobs.items).toHaveCount(1);
    expect(await axeViolations(page)).toEqual([]);
  });
});

test.describe('menus and keys (plan S13)', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  });

  test('the menu bar shows the contributed menus with keys, states and a submenu, and runs items', async ({ app, page }) => {
    const mac = await app.menus.isMac();
    // In the window the app menu's entries (Settings…, plan S10) are at the end of File: the title bar names the app; on
    // macOS they are in the native app menu.
    await expect(app.menus.bar.getByRole('menuitem')).toHaveText(['File', 'View', 'Help']);
    const help = await app.menus.open('Help');
    const greet = help.getByRole('menuitem', { name: 'Say hello' });
    await expect(greet).toContainText(mac ? '⇧⌘H' : 'Ctrl+Shift+H');
    await expect(greet).toHaveAttribute('aria-keyshortcuts', mac ? 'Shift+Meta+H' : 'Control+Shift+H');
    await expect(help.getByRole('menuitem', { name: 'Reset greetings' })).toHaveAttribute('aria-disabled', 'true');
    await greet.click();
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 1');
    await expect(page.getByRole('menu')).toHaveCount(0);
    // The enabled state follows the command; the submenu holds its own items.
    const again = await app.menus.open('Help');
    await expect(again.getByRole('menuitem', { name: 'Reset greetings' })).not.toHaveAttribute('aria-disabled', 'true');
    await again.getByRole('menuitem', { name: 'More demos' }).click();
    await app.menus.menu('More demos').getByRole('menuitem', { name: 'Fail on purpose' }).click();
    await expect(app.notifications.toasts).toContainText('Fail on purpose failed: this command always fails');
    // A toggle shows its check mark.
    const view = await app.menus.open('View');
    const compact = view.getByRole('menuitemcheckbox', { name: 'Compact layout' });
    await expect(compact).toHaveAttribute('aria-checked', 'false');
    await compact.click();
    await expect(page.locator('sd-page')).toHaveClass(/compact/);
    await expect((await app.menus.open('View')).getByRole('menuitemcheckbox', { name: 'Compact layout' })).toHaveAttribute('aria-checked', 'true');
    await page.keyboard.press('Escape');
  });

  test('the menu bar is keyboard operable and an open menu has no axe violations', async ({ app, page }) => {
    await app.menus.bar.getByRole('menuitem', { name: 'File', exact: true }).focus();
    await page.keyboard.press('ArrowDown');
    await expect(app.menus.menu('File').getByRole('menuitem', { name: 'Start sample job' })).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(app.menus.menu('View')).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
    await page.keyboard.press('Escape');
    await expect(page.getByRole('menu')).toHaveCount(0);
  });

  test('keys run their commands, also from a text field, but typing there does not', async ({ page }) => {
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 1');
    const note = page.getByTestId('demo-note');
    await note.click();
    await note.pressSequentially('hk');
    await expect(note).toHaveValue('hk');
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 2');
    await page.keyboard.press('ControlOrMeta+Alt+K');
    await expect(page.locator('sd-page')).toHaveClass(/compact/);
  });

  test('a changed key is kept across a reload, and a taken key is refused with its owner', async ({ app, page }) => {
    const mac = await app.menus.isMac();
    const toasts = app.notifications.toasts;
    await page.getByRole('link', { name: 'Keyboard' }).click();
    const row = (command: string) => page.locator(`tr[data-command="${command}"]`);
    await row('sd.greet').getByRole('button', { name: 'Change key' }).click();
    await expect(row('sd.greet').getByTestId('shortcut-key')).toHaveText('Press a key…');
    // A key pressed while recording runs nothing.
    await page.keyboard.press('ControlOrMeta+Alt+G');
    await expect(page.getByTestId('shortcut-message')).toHaveText(`Say hello is now ${mac ? '⌥⌘G' : 'Ctrl+Alt+G'}.`);
    await expect(toasts.getByText(/Hello from the shell demo/)).toHaveCount(0);
    // The menu shows the new key.
    await expect(await app.menus.item('Help', 'Say hello')).toContainText(mac ? '⌥⌘G' : 'Ctrl+Alt+G');
    await page.keyboard.press('Escape');
    // Taken: refused, with the command that has it.
    await row('sd.reset').getByRole('button', { name: 'Change key' }).click();
    await page.keyboard.press('ControlOrMeta+Alt+G');
    await expect(page.getByTestId('shortcut-message')).toContainText('is already used by Say hello; Reset greetings keeps its key.');

    await page.reload();
    await expect(row('sd.greet')).toBeVisible();
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await page.keyboard.press('ControlOrMeta+Alt+G');
    await expect(toasts.getByText('Hello from the shell demo (1)')).toBeVisible();
    await row('sd.greet').getByRole('button', { name: 'Reset key' }).click();
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await expect(toasts.getByText('Hello from the shell demo (2)')).toBeVisible();
  });

  test('the greetings count has a context menu from the same model', async ({ page }) => {
    await page.getByRole('button', { name: 'Say hello', exact: true }).click();
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 1');
    await page.getByTestId('greetings').click({ button: 'right' });
    const menu = page.getByRole('menu', { name: 'Greeting actions' });
    await expect(menu.getByRole('menuitem')).toHaveText([/Say hello/, /Reset greetings/]);
    await menu.getByRole('menuitem', { name: 'Reset greetings' }).click();
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 0');
  });
});

test.describe('notifications and service health (plan S4)', () => {
  test('three identical failures are one chip, one notification with count 3, and no toast; Repair brings the panel back', async ({ app, page, pageErrors }) => {
    const { chip } = app.statusBar;
    const n = app.notifications;
    await page.goto('/');
    await expect(page.getByTestId('engine-data')).toHaveText('Engine data loaded (1 load)');
    await expect(chip).toHaveCount(0);
    await page.getByRole('button', { name: 'Break the demo engine' }).click();

    await expect(chip).toHaveCount(1);
    await expect(chip).toHaveText('Demo engine: stopped');
    // The chip states the outage: no toast for a health cause.
    await expect(n.toastItems).toHaveCount(0);
    // The panel waits calmly instead of showing an error of its own.
    await expect(page.getByTestId('waiting-for-sd.engine')).toContainText('Waiting for the demo engine');
    await expect(page.getByText('Could not load the engine data.')).toHaveCount(0);

    // One message in the centre, count 3.
    await n.openCentre();
    const outage = n.notice('health:sd.engine');
    await expect(outage).toHaveCount(1);
    await expect(outage).toContainText('The demo engine stopped unexpectedly. (×3)');
    await page.keyboard.press('Escape');

    // "See what happened" opens the chip; Repair is served by the shell.
    await page.getByTestId('waiting-for-details').click();
    const details = n.services;
    await expect(details).toContainText('The demo engine stopped unexpectedly.');
    await expect(details).toContainText('Restarting it brings the engine data back.');
    await details.getByText('Technical details').click();
    await expect(details).toContainText('demo-engine exited (signal: 6)');
    await details.getByRole('button', { name: 'Restart' }).click();

    await expect(chip).toHaveCount(0);
    // The panel loaded again by itself, once (load 2 was the one that met the failure).
    await expect(page.getByTestId('engine-data')).toHaveText('Engine data loaded (3 loads)');
    // The outage's message resolved with it.
    await n.openCentre();
    await expect(n.notice('health:sd.engine')).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  });

  test('a repair that fails keeps the chip with the new cause and a working button', async ({ app, page }) => {
    const { chip } = app.statusBar;
    await page.goto('/');
    await page.getByRole('button', { name: 'Break the search index' }).click();
    await expect(chip).toHaveText('Search index: stopped');
    await chip.click();
    const details = app.notifications.services;
    await expect(details).toContainText('The search index stopped: a segment is damaged.');
    // The repair is named for what it does (rule 19).
    await details.getByRole('button', { name: 'Reset search index' }).click();
    await expect(details).toContainText('The search index could not be rebuilt.');
    await expect(details).toContainText('Free some disk space, then try again.');
    await expect(details.getByRole('button', { name: 'Reset search index' })).toBeEnabled();
    await expect(chip).toHaveText('Search index: stopped');
    // Two services down: one chip with a count.
    await page.keyboard.press('Escape');
    await expect(chip).toBeFocused();
    await page.getByRole('button', { name: 'Break the demo engine' }).click();
    await expect(chip).toHaveText('2 services need attention');
    await expect(app.notifications.toastItems).toHaveCount(0);
  });

  test('the same notice three times is one toast with a count, which times out and stays in the centre', async ({ app, page }) => {
    await page.clock.install();
    await page.goto('/');
    await page.getByRole('button', { name: 'Notify three times' }).click();
    const toast = app.notifications.toastItems;
    await expect(toast).toHaveCount(1);
    await expect(toast).toContainText('The disk is almost full');
    await expect(toast.getByTestId('notification-count')).toHaveText(/×3/);
    await page.clock.runFor(8_500);
    await expect(toast).toHaveCount(0);
    const centre = await app.notifications.openCentre();
    await expect(centre.getByRole('heading', { name: 'Greeting' })).toBeVisible();
    await expect(centre).toContainText('The disk is almost full (×3)');
  });

  test('a toast’s "Show in Notifications" opens the centre at its message', async ({ app, page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Fail on purpose' }).click();
    const toast = app.notifications.toastItems;
    await expect(toast).toHaveAttribute('role', 'alert');
    await toast.getByRole('button', { name: 'Show in Notifications' }).click();
    await expect(toast).toHaveCount(0);
    await expect(app.notifications.centre).toContainText('Fail on purpose failed: this command always fails');
  });

  for (const colorScheme of THEMES) {
    test(`toasts, the chip details and the centre have no axe violations (${colorScheme})`, async ({ app, page }) => {
      await page.emulateMedia({ colorScheme });
      await page.goto('/');
      await page.getByRole('button', { name: 'Break the search index' }).click();
      await page.getByRole('button', { name: 'Notify three times' }).click();
      await page.getByRole('button', { name: 'Fail on purpose' }).click();
      await app.statusBar.chip.click();
      await expect(app.notifications.services).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
      await page.keyboard.press('Escape');
      await app.notifications.openCentre();
      expect(await axeViolations(page)).toEqual([]);
    });
  }
});

// Dialogs as windows (plan S6, ADR 0009). The browser build opens a dialog window's route by URL, so every dialog of the
// contract file (generated from the registry, not a hand-kept list) boots with only its window's providers and is checked
// per kind; the main window shows the same dialogs in the page.
const CONTRACTS = dialogContracts('projects/shell-demo/dialog-contracts.json');

test.describe('dialogs (plan S6)', () => {
  test('the contract file has every kind', () => {
    expect([...new Set(Object.values(CONTRACTS).map((c) => c.kind))].sort()).toEqual(['decision', 'form', 'tool', 'viewer']);
  });

  for (const [id, contract] of Object.entries(CONTRACTS)) {
    test.describe(() => {
      test.use({ consoleErrors: true });
      test(`the ${id} (${contract.kind}) window boots, has a working Close and Escape ${contract.requiredChoice ? 'does nothing' : 'closes it'}`, async ({ app, page, pageErrors }) => {
        const d = app.dialogs;
        await d.openWindow(id);
        await expect(d.shell).toHaveAttribute('data-kind', contract.kind);
        await d.loaded();
        expect(pageErrors).toEqual([]);
        if (contract.requiredChoice) {
          await page.keyboard.press('Escape');
          await expect(d.shell.getByRole('button', { name: 'Formal' })).toBeVisible();
          expect(await d.result()).toBeNull();
          await d.shell.getByRole('button', { name: 'Formal' }).click();
          await d.expectResult('{"result":"formal"}');
          return;
        }
        await expect(d.wayOut).toBeVisible();
        await page.keyboard.press('Escape');
        await d.expectResult('{"result":null}');
      });
    });
  }

  for (const [id] of Object.entries(CONTRACTS).filter(([, c]) => c.kind === 'decision')) {
    test(`the ${id} decision is fitted once to its content: no scrollbar and no gap`, async ({ app, page }) => {
      const d = app.dialogs;
      await page.setViewportSize({ width: 480, height: 700 });
      await d.openWindow(id);
      await expect(d.shell.getByTestId('dialog-loading')).toHaveCount(0);
      await expect.poll(() => d.fitHistory()).not.toEqual([]);
      const history = await d.fitHistory();
      expect(history, `fitted more than once: ${history.join(' -> ')}`).toHaveLength(1);
      await page.setViewportSize({ width: 480, height: history[0] as number });
      const m = await d.shell.locator('main').evaluate((el) => ({ scroll: el.scrollHeight, client: el.clientHeight }));
      expect(m.scroll).toBeLessThanOrEqual(m.client);
    });
  }

  test('a viewer opens at 680x560 with the shell’s Close in view; axe finds nothing in a dialog window', async ({ app, page }) => {
    expect(CONTRACTS['sd.about']?.minSize).toEqual({ width: 680, height: 560 });
    await page.setViewportSize({ width: 680, height: 560 });
    await app.dialogs.openWindow('sd.about');
    const close = app.dialogs.shellClose;
    await expect(close).toHaveText('Close');
    const box = (await close.boundingBox())!;
    expect(box.y + box.height).toBeLessThanOrEqual(560);
    await expectNoHorizontalScroll(page);
    expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
  });

  test('the tool window loads its own providers and asks the engine', async ({ app, page }) => {
    await app.dialogs.openWindow('sd.inspector');
    await expect(page.getByTestId('engine-status')).toContainText('running');
  });

  test('a form takes its input and answers on Enter', async ({ app, page }) => {
    await app.dialogs.openWindow('sd.rename', { current: 'Old name' });
    const field = page.getByRole('textbox', { name: 'Name' });
    await expect(field).toHaveValue('Old name');
    await field.fill('New name');
    await field.press('Enter');
    await app.dialogs.expectResult('{"result":"New name"}');
  });

  test('in the page: a modal blocks behind a backdrop, a modeless one leaves the page usable and opens once', async ({ app, page }) => {
    const d = app.dialogs;
    await page.goto('/#/dialogs');
    // A modeless dialog may sit over the buttons (it opens in the middle of the window): the button is clicked as itself.
    const open = (id: string) => page.locator(`[data-dialog-open="${id}"]`).dispatchEvent('click');
    await open('sd.confirm-reset');
    const modal = d.inPage('Reset greetings?');
    await expect(modal).toHaveAttribute('aria-modal', 'true');
    await expect(d.backdrop).toBeVisible();
    await modal.getByRole('button', { name: 'Reset' }).click();
    await expect(page.getByTestId('dialog-answer')).toHaveText('Reset greetings?: true');
    await expect(d.backdrop).toHaveCount(0);

    await open('sd.about');
    const viewer = d.inPage('About the shell demo');
    await expect(viewer).toHaveAttribute('aria-modal', 'false');
    await expect(d.backdrop).toHaveCount(0);
    await open('sd.about');
    await expect(d.inPage('About the shell demo')).toHaveCount(1);
    // The window stays usable: the page's controls and the keys work beside it.
    await page.locator('[data-dialog-open="sd.rename"]').focus();
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await expect(page.getByText('Hello from the shell demo (1)')).toBeVisible();
    await viewer.getByRole('button', { name: 'Close' }).click();
    await expect(page.getByTestId('dialog-answer')).toHaveText('About the shell demo: dismissed');

    await open('sd.choose-style');
    const choice = d.inPage('Choose a greeting style');
    await page.keyboard.press('Escape');
    await expect(choice).toBeVisible();
    await choice.getByRole('button', { name: 'Casual' }).click();
    await expect(page.getByTestId('dialog-answer')).toHaveText('Choose a greeting style: "casual"');

    await open('sd.rename');
    await d.inPage('Rename').getByRole('textbox', { name: 'Name' }).fill('Paddock');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('dialog-answer')).toHaveText('Rename: "Paddock"');

    await open('sd.inspector');
    const inspector = d.inPage('Engine inspector');
    await expect(inspector.getByTestId('engine-status')).toContainText('running');
    // A modeless dialog answers Escape from inside it only: from the page it stays open.
    await page.locator('[data-dialog-open="sd.rename"]').focus();
    await page.keyboard.press('Escape');
    await expect(inspector).toBeVisible();
    await inspector.getByRole('button', { name: 'Refresh' }).press('Escape');
    await expect(inspector).toHaveCount(0);
  });
});
