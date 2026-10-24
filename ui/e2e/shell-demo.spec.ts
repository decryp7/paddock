import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
// The pure lint (no Angular imports), straight from the source: Playwright runs in Node.
import { checkRegistrationSource } from '../projects/paddock-ui/shell/testing/src/registration-lint';
import { uiTimeoutProblems } from '../projects/paddock-ui/shell/jobs/src/ui-timeouts';

/** The notification host's toasts (plan S4). */
const toasts = (page: Page) => page.getByRole('region', { name: 'Notifications', exact: true });

// The shell demo is the reference consumer of @decryp7/paddock-ui/shell (ADR 0012 §7); each work package adds its checks.
for (const colorScheme of ['dark', 'light'] as const) {
  test.describe(`shell demo (${colorScheme})`, () => {
    test.use({ colorScheme });

    test('boots past the splash and axe finds no violations', async ({ page }) => {
      const errors: string[] = [];
      page.on('pageerror', (e) => errors.push(e.message));
      await page.goto('/');
      await expect(page.getByRole('heading', { level: 1, name: 'Paddock shell demo' })).toBeVisible();
      await expect(page.locator('.pd-splash')).toHaveCount(0);
      const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
      expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
      expect(errors).toEqual([]);
    });
  });
}

test('no horizontal scroll at 320 px (rule 18)', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test.describe('contributions (plan S2)', () => {
  test('a contributed command runs with the lazily loaded app context', async ({ page }) => {
    await page.goto('/');
    // The app's part of the context is loaded when the window is idle, not in the first-paint bundle.
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    const reset = page.getByRole('button', { name: 'Reset greetings', exact: true });
    await expect(reset).toBeDisabled();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(toasts(page).getByText('Hello from the shell demo (1)')).toBeVisible();
    await expect(reset).toBeEnabled();
    await reset.click();
    await expect(page.getByText('Greetings reset')).toBeVisible();
    await expect(reset).toBeDisabled();
  });

  test('a failing command is shown as a notice and the app keeps working', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto('/');
    await page.getByRole('button', { name: 'Fail on purpose' }).click();
    await expect(page.getByText('Fail on purpose failed: this command always fails')).toBeVisible();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(page.getByText('Hello from the shell demo (1)')).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('every contribution kind is registered', async ({ page }) => {
    await page.goto('/');
    const rows = page.getByRole('table', { name: 'Registered contributions' }).getByRole('row');
    await expect(rows).toHaveCount(11);
    for (const kind of ['commands', 'keybindings', 'menus', 'pages', 'statusItems', 'settingsPages', 'dialogs', 'healthServices', 'notificationKinds', 'submenus']) {
      await expect(rows.filter({ has: page.getByRole('cell', { name: kind, exact: true }) })).toHaveText(new RegExp(`^${kind}[1-9]\\d*$`));
    }
  });

  test('the registration file imports no store, service, command or component (lazy by construction)', () => {
    const file = 'projects/shell-demo/src/app/demo.contributions.ts';
    expect(checkRegistrationSource(file, readFileSync(file, 'utf8'))).toEqual([]);
  });
});

/** Every non-spec TypeScript file under `dir`. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return path.endsWith('.ts') && !path.endsWith('.spec.ts') ? [path] : [];
  });
}

test.describe('jobs (plan S9)', () => {
  test('a job reports its phases in the status item and the list, and stops on Cancel', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto('/');
    const status = page.getByRole('contentinfo', { name: 'Status bar' }).getByRole('button').first();
    await expect(status).toHaveText('Jobs');
    await page.getByRole('button', { name: 'Start sample job' }).click();
    await expect(status).toHaveText(/^1 running: (Finding files|Reading)$/);

    await status.click();
    await expect(status).toHaveAttribute('aria-expanded', 'true');
    const job = page.getByRole('region', { name: 'Jobs' }).getByRole('listitem');
    await expect(job).toHaveCount(1);
    await expect(job).toContainText('Importing logs');
    // Each phase has its own scale: discovery counts up, then reading has a determinate bar.
    await expect(job.getByRole('progressbar')).toHaveAttribute('aria-valuenow', /\d+/);
    await expect(job).toContainText('Reading');
    await job.getByRole('button', { name: 'Cancel' }).click();
    await expect(job).toContainText('Cancelled');
    await expect(job).toContainText('Stopped at your request.');
    await expect(status).toHaveText('Jobs');
    await expect(toasts(page).getByTestId('notification')).toHaveCount(0);

    await page.keyboard.press('Escape');
    await expect(page.getByRole('region', { name: 'Jobs' })).toHaveCount(0);
    await expect(status).toBeFocused();
    expect(errors).toEqual([]);
  });

  test('a job that finishes is announced; the step that cannot be stopped says so', async ({ page }) => {
    await page.clock.install();
    await page.goto('/');
    await page.getByRole('button', { name: 'Start sample job' }).click();
    const status = page.getByRole('contentinfo', { name: 'Status bar' }).getByRole('button').first();
    await status.click();
    const job = page.getByRole('region', { name: 'Jobs' }).getByRole('listitem');
    // Finding files (2 s) and reading (5 s) pass; writing the index cannot be interrupted.
    await page.clock.runFor(7_100);
    await expect(job).toContainText('Writing the index');
    await expect(job.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    await page.clock.runFor(1_000);
    await expect(job).toContainText('Done');
    await expect(toasts(page)).toContainText('Importing logs: done');
  });

  test('a quiet job shows "Still working" and is never timed out', async ({ page }) => {
    await page.clock.install();
    await page.goto('/');
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    const status = page.getByRole('contentinfo', { name: 'Status bar' }).getByRole('button').first();
    await expect(status).toHaveText('1 running: Waiting for the device');
    await page.clock.runFor(1_600);
    await expect(status).toHaveText('1 running: Still working');
    // Ten minutes without an event: still running, still "Still working", never failed.
    await page.clock.fastForward('10:00');
    await expect(status).toHaveText('1 running: Still working');
    await status.click();
    const job = page.getByRole('region', { name: 'Jobs' }).getByRole('listitem');
    await expect(job).toContainText('Still working. This step is taking longer than usual.');
    await job.getByRole('button', { name: 'Cancel' }).click();
    await page.clock.runFor(200);
    await expect(job).toContainText('Cancelled');
    await expect(status).toHaveText('Jobs');
  });

  test('the jobs popover has no axe violations', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await page.getByRole('contentinfo', { name: 'Status bar' }).getByRole('button').first().click();
    await expect(page.getByRole('region', { name: 'Jobs' }).getByRole('listitem')).toHaveCount(1);
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
  });

  test('no UI timeout anywhere in the shell or its reference app (grep check, rule 12)', () => {
    const files = [...sources('projects/paddock-ui/shell'), ...sources('projects/shell-demo/src')];
    expect(files.length).toBeGreaterThan(20);
    expect(files.flatMap((f) => uiTimeoutProblems(f, readFileSync(f, 'utf8')))).toEqual([]);
  });
});

test.describe('menus and keys (plan S13)', () => {
  const isMac = (page: Page) => page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform));
  const menuBar = (page: Page) => page.getByRole('menubar', { name: 'Application menu' });
  const openMenu = async (page: Page, name: string) => {
    await menuBar(page).getByRole('menuitem', { name, exact: true }).click();
    return page.getByRole('menu', { name });
  };

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  });

  test('the menu bar shows the contributed menus with keys, states and a submenu, and runs items', async ({ page }) => {
    const mac = await isMac(page);
    // The app menu (named after the app) holds Settings… (plan S10); on macOS it is the native app menu.
    await expect(menuBar(page).getByRole('menuitem')).toHaveText(['Shell demo', 'File', 'View', 'Help']);
    const help = await openMenu(page, 'Help');
    const greet = help.getByRole('menuitem', { name: 'Say hello' });
    await expect(greet).toContainText(mac ? '⇧⌘H' : 'Ctrl+Shift+H');
    await expect(greet).toHaveAttribute('aria-keyshortcuts', mac ? 'Shift+Meta+H' : 'Control+Shift+H');
    await expect(help.getByRole('menuitem', { name: 'Reset greetings' })).toHaveAttribute('aria-disabled', 'true');
    await greet.click();
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 1');
    await expect(page.getByRole('menu')).toHaveCount(0);
    // The enabled state follows the command; the submenu holds its own items.
    const again = await openMenu(page, 'Help');
    await expect(again.getByRole('menuitem', { name: 'Reset greetings' })).not.toHaveAttribute('aria-disabled', 'true');
    await again.getByRole('menuitem', { name: 'More demos' }).click();
    await page.getByRole('menu', { name: 'More demos' }).getByRole('menuitem', { name: 'Fail on purpose' }).click();
    await expect(toasts(page)).toContainText('Fail on purpose failed: this command always fails');
    // A toggle shows its check mark.
    const view = await openMenu(page, 'View');
    const compact = view.getByRole('menuitemcheckbox', { name: 'Compact layout' });
    await expect(compact).toHaveAttribute('aria-checked', 'false');
    await compact.click();
    await expect(page.locator('main.page')).toHaveClass(/compact/);
    await expect((await openMenu(page, 'View')).getByRole('menuitemcheckbox', { name: 'Compact layout' })).toHaveAttribute('aria-checked', 'true');
    await page.keyboard.press('Escape');
  });

  test('the menu bar is keyboard operable and an open menu has no axe violations', async ({ page }) => {
    await menuBar(page).getByRole('menuitem', { name: 'File', exact: true }).focus();
    await page.keyboard.press('ArrowDown');
    const file = page.getByRole('menu', { name: 'File' });
    await expect(file.getByRole('menuitem', { name: 'Start sample job' })).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('menu', { name: 'View' })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
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
    await expect(page.locator('main.page')).toHaveClass(/compact/);
  });

  test('a changed key is kept across a reload, and a taken key is refused with its owner', async ({ page }) => {
    const mac = await isMac(page);
    const row = (command: string) => page.locator(`tr[data-command="${command}"]`);
    await row('sd.greet').getByRole('button', { name: 'Change key' }).click();
    await expect(row('sd.greet').getByTestId('shortcut-key')).toHaveText('Press a key…');
    // A key pressed while recording runs nothing.
    await page.keyboard.press('ControlOrMeta+Alt+G');
    await expect(page.getByTestId('shortcut-message')).toHaveText(`Say hello is now ${mac ? '⌥⌘G' : 'Ctrl+Alt+G'}.`);
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 0');
    // The menu shows the new key.
    await expect((await openMenu(page, 'Help')).getByRole('menuitem', { name: 'Say hello' })).toContainText(mac ? '⌥⌘G' : 'Ctrl+Alt+G');
    await page.keyboard.press('Escape');
    // Taken: refused, with the command that has it.
    await row('sd.reset').getByRole('button', { name: 'Change key' }).click();
    await page.keyboard.press('ControlOrMeta+Alt+G');
    await expect(page.getByTestId('shortcut-message')).toContainText('is already used by Say hello; Reset greetings keeps its key.');

    await page.reload();
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await page.keyboard.press('ControlOrMeta+Alt+G');
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 1');
    await row('sd.greet').getByRole('button', { name: 'Reset key' }).click();
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await expect(page.getByTestId('greetings')).toHaveText('Greetings: 2');
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
  const statusBar = (page: Page) => page.getByRole('contentinfo', { name: 'Status bar' });
  const chip = (page: Page) => page.getByTestId('service-health-chip');
  const centre = (page: Page) => page.getByRole('region', { name: 'Notifications centre' });

  test('three identical failures are one chip, one notification with count 3, and no toast; Repair brings the panel back', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto('/');
    await expect(page.getByTestId('engine-data')).toHaveText('Engine data loaded (1 load)');
    await expect(chip(page)).toHaveCount(0);
    await page.getByRole('button', { name: 'Break the demo engine' }).click();

    await expect(chip(page)).toHaveCount(1);
    await expect(chip(page)).toHaveText('Demo engine: stopped');
    // The chip states the outage: no toast for a health cause.
    await expect(toasts(page).getByTestId('notification')).toHaveCount(0);
    // The panel waits calmly instead of showing an error of its own.
    await expect(page.getByTestId('waiting-for-sd.engine')).toContainText('Waiting for the demo engine');
    await expect(page.getByText('Could not load the engine data.')).toHaveCount(0);

    // One message in the centre, count 3.
    await statusBar(page).getByTestId('notifications-status').click();
    const outage = centre(page).locator('[data-cause="health:sd.engine"]');
    await expect(outage).toHaveCount(1);
    await expect(outage).toContainText('The demo engine stopped unexpectedly. (×3)');
    await page.keyboard.press('Escape');

    // "See what happened" opens the chip; Repair is served by the shell.
    await page.getByTestId('waiting-for-details').click();
    const details = page.getByRole('region', { name: 'Services' });
    await expect(details).toContainText('The demo engine stopped unexpectedly.');
    await expect(details).toContainText('Choose Restart.');
    await details.getByText('Technical details').click();
    await expect(details).toContainText('demo-engine exited (signal: 6)');
    await details.getByRole('button', { name: 'Restart' }).click();

    await expect(chip(page)).toHaveCount(0);
    // The panel loaded again by itself, once (load 2 was the one that met the failure).
    await expect(page.getByTestId('engine-data')).toHaveText('Engine data loaded (3 loads)');
    // The outage's message resolved with it.
    await statusBar(page).getByTestId('notifications-status').click();
    await expect(centre(page).locator('[data-cause="health:sd.engine"]')).toHaveCount(0);
    expect(errors).toEqual([]);
  });

  test('a repair that fails keeps the chip with the new cause and a working button', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Break the search index' }).click();
    await expect(chip(page)).toHaveText('Search index: stopped');
    await chip(page).click();
    const details = page.getByRole('region', { name: 'Services' });
    await expect(details).toContainText('The search index is damaged.');
    await details.getByRole('button', { name: 'Try again' }).click();
    await expect(details).toContainText('The search index could not be rebuilt.');
    await expect(details).toContainText('Free some disk space, then try again.');
    await expect(details.getByRole('button', { name: 'Try again' })).toBeEnabled();
    await expect(chip(page)).toHaveText('Search index: stopped');
    // Two services down: one chip with a count.
    await page.keyboard.press('Escape');
    await expect(chip(page)).toBeFocused();
    await page.getByRole('button', { name: 'Break the demo engine' }).click();
    await expect(chip(page)).toHaveText('2 services need attention');
    await expect(toasts(page).getByTestId('notification')).toHaveCount(0);
  });

  test('the same notice three times is one toast with a count, which times out and stays in the centre', async ({ page }) => {
    await page.clock.install();
    await page.goto('/');
    await page.getByRole('button', { name: 'Notify three times' }).click();
    const toast = toasts(page).getByTestId('notification');
    await expect(toast).toHaveCount(1);
    await expect(toast).toContainText('The disk is almost full');
    await expect(toast.getByTestId('notification-count')).toHaveText(/×3/);
    await page.clock.runFor(8_500);
    await expect(toast).toHaveCount(0);
    await statusBar(page).getByTestId('notifications-status').click();
    await expect(centre(page).getByRole('heading', { name: 'Greeting' })).toBeVisible();
    await expect(centre(page)).toContainText('The disk is almost full (×3)');
  });

  test('a toast’s "Show in Notifications" opens the centre at its message', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Fail on purpose' }).click();
    const toast = toasts(page).getByTestId('notification');
    await expect(toast).toHaveAttribute('role', 'alert');
    await toast.getByRole('button', { name: 'Show in Notifications' }).click();
    await expect(toast).toHaveCount(0);
    await expect(centre(page)).toContainText('Fail on purpose failed: this command always fails');
  });

  for (const colorScheme of ['dark', 'light'] as const) {
    test(`toasts, the chip details and the centre have no axe violations (${colorScheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme });
      await page.goto('/');
      await page.getByRole('button', { name: 'Break the search index' }).click();
      await page.getByRole('button', { name: 'Notify three times' }).click();
      await page.getByRole('button', { name: 'Fail on purpose' }).click();
      await chip(page).click();
      await expect(page.getByRole('region', { name: 'Services' })).toBeVisible();
      const axe = () => new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
      let results = await axe();
      expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
      await page.keyboard.press('Escape');
      await statusBar(page).getByTestId('notifications-status').click();
      await expect(centre(page)).toBeVisible();
      results = await axe();
      expect(results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
    });
  }
});

// Dialogs as windows (plan S6, ADR 0009). The browser build opens a dialog window's route by URL, so every dialog of the
// contract file (generated from the registry, not a hand-kept list) boots with only its window's providers and is checked
// per kind; the main window shows the same dialogs in the page.
interface DialogContract {
  kind: 'decision' | 'form' | 'viewer' | 'tool';
  modality: 'modal' | 'modeless';
  requiredChoice: boolean;
  minSize?: { width: number; height: number };
}
const CONTRACTS = (JSON.parse(readFileSync('projects/shell-demo/dialog-contracts.json', 'utf8')) as { dialogs: Record<string, DialogContract> }).dialogs;
const RESULT_KEY = 'paddock.dialog.browserResult';
const CLOSE = /^(close|cancel|not now|dismiss|done|later|got it)\b/i;
const dialogUrl = (id: string, input: unknown = {}) =>
  `/#/dialog?id=${id}&host=main&label=d-main-1&title=${encodeURIComponent(id)}&dialogInput=${encodeURIComponent(JSON.stringify(input))}`;

test.describe('dialogs (plan S6)', () => {
  test('the contract file has every kind', () => {
    expect([...new Set(Object.values(CONTRACTS).map((c) => c.kind))].sort()).toEqual(['decision', 'form', 'tool', 'viewer']);
  });

  for (const [id, contract] of Object.entries(CONTRACTS)) {
    test(`the ${id} (${contract.kind}) window boots, has a working Close and Escape ${contract.requiredChoice ? 'does nothing' : 'closes it'}`, async ({ page }) => {
      const errors: string[] = [];
      page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
      page.on('pageerror', (e) => errors.push(e.message));
      await page.goto(dialogUrl(id));
      const shell = page.locator('pd-dialog-shell');
      await expect(shell).toHaveAttribute('data-kind', contract.kind);
      await expect(shell.getByTestId('dialog-loading')).toHaveCount(0);
      await expect(shell.getByTestId('dialog-error')).toHaveCount(0);
      expect(errors).toEqual([]);
      if (contract.requiredChoice) {
        await page.keyboard.press('Escape');
        await expect(shell.getByRole('button', { name: 'Formal' })).toBeVisible();
        expect(await page.evaluate((k) => localStorage.getItem(k), RESULT_KEY)).toBeNull();
        await shell.getByRole('button', { name: 'Formal' }).click();
        await expect.poll(() => page.evaluate((k) => localStorage.getItem(k), RESULT_KEY)).toBe('{"result":"formal"}');
        return;
      }
      await expect(shell.locator('[data-dialog-close]').or(shell.getByRole('button', { name: CLOSE })).first()).toBeVisible();
      await page.keyboard.press('Escape');
      await expect.poll(() => page.evaluate((k) => localStorage.getItem(k), RESULT_KEY)).toBe('{"result":null}');
    });
  }

  for (const [id] of Object.entries(CONTRACTS).filter(([, c]) => c.kind === 'decision')) {
    test(`the ${id} decision is fitted once to its content: no scrollbar and no gap`, async ({ page }) => {
      await page.setViewportSize({ width: 480, height: 700 });
      await page.goto(dialogUrl(id));
      const shell = page.locator('pd-dialog-shell');
      await expect(shell.getByTestId('dialog-loading')).toHaveCount(0);
      await expect.poll(() => page.evaluate(() => document.documentElement.dataset['fitHistory'] ?? '')).not.toBe('');
      const history = (await page.evaluate(() => document.documentElement.dataset['fitHistory'] ?? '')).split(',');
      expect(history, `fitted more than once: ${history.join(' -> ')}`).toHaveLength(1);
      await page.setViewportSize({ width: 480, height: Number(history[0]) });
      const m = await shell.locator('main').evaluate((el) => ({ scroll: el.scrollHeight, client: el.clientHeight }));
      expect(m.scroll).toBeLessThanOrEqual(m.client);
    });
  }

  test('a viewer opens at 680x560 with the shell’s Close in view; axe finds nothing in a dialog window', async ({ page }) => {
    expect(CONTRACTS['sd.about']?.minSize).toEqual({ width: 680, height: 560 });
    await page.setViewportSize({ width: 680, height: 560 });
    await page.goto(dialogUrl('sd.about'));
    const close = page.getByTestId('dialog-shell-close');
    await expect(close).toHaveText('Close');
    const box = (await close.boundingBox())!;
    expect(box.y + box.height).toBeLessThanOrEqual(560);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag22aa']).analyze();
    expect(results.violations.map((v) => v.id)).toEqual([]);
  });

  test('the tool window loads its own providers and asks the engine', async ({ page }) => {
    await page.goto(dialogUrl('sd.inspector'));
    await expect(page.getByTestId('engine-status')).toContainText('running');
  });

  test('a form takes its input and answers on Enter', async ({ page }) => {
    await page.goto(dialogUrl('sd.rename', { current: 'Old name' }));
    const field = page.getByRole('textbox', { name: 'Name' });
    await expect(field).toHaveValue('Old name');
    await field.fill('New name');
    await field.press('Enter');
    await expect.poll(() => page.evaluate((k) => localStorage.getItem(k), RESULT_KEY)).toBe('{"result":"New name"}');
  });

  test('in the page: a modal blocks behind a backdrop, a modeless one leaves the page usable and opens once', async ({ page }) => {
    await page.goto('/');
    const open = (id: string) => page.locator(`[data-dialog-open="${id}"]`).click();
    await open('sd.confirm-reset');
    const modal = page.getByRole('dialog', { name: 'Reset greetings?' });
    await expect(modal).toHaveAttribute('aria-modal', 'true');
    await expect(page.getByTestId('dialog-backdrop')).toBeVisible();
    await modal.getByRole('button', { name: 'Reset' }).click();
    await expect(page.getByTestId('dialog-answer')).toHaveText('Reset greetings?: true');
    await expect(page.getByTestId('dialog-backdrop')).toHaveCount(0);

    await open('sd.about');
    const viewer = page.getByRole('dialog', { name: 'About the shell demo' });
    await expect(viewer).toHaveAttribute('aria-modal', 'false');
    await expect(page.getByTestId('dialog-backdrop')).toHaveCount(0);
    await open('sd.about');
    await expect(page.getByRole('dialog', { name: 'About the shell demo' })).toHaveCount(1);
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(page.getByText('Hello from the shell demo (1)')).toBeVisible();
    await viewer.getByRole('button', { name: 'Close' }).click();
    await expect(page.getByTestId('dialog-answer')).toHaveText('About the shell demo: dismissed');

    await open('sd.choose-style');
    const choice = page.getByRole('dialog', { name: 'Choose a greeting style' });
    await page.keyboard.press('Escape');
    await expect(choice).toBeVisible();
    await choice.getByRole('button', { name: 'Casual' }).click();
    await expect(page.getByTestId('dialog-answer')).toHaveText('Choose a greeting style: "casual"');

    await open('sd.rename');
    await page.getByRole('dialog', { name: 'Rename' }).getByRole('textbox', { name: 'Name' }).fill('Paddock');
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('dialog-answer')).toHaveText('Rename: "Paddock"');

    await open('sd.inspector');
    await expect(page.getByRole('dialog', { name: 'Engine inspector' }).getByTestId('engine-status')).toContainText('running');
    // A modeless dialog answers Escape from inside it only: from the page it stays open.
    await page.getByRole('heading', { level: 1 }).click();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'Engine inspector' })).toBeVisible();
    await page.getByRole('dialog', { name: 'Engine inspector' }).getByRole('button', { name: 'Refresh' }).press('Escape');
    await expect(page.getByRole('dialog', { name: 'Engine inspector' })).toHaveCount(0);
  });
});
