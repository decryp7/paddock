import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
// The pure lint (no Angular imports), straight from the source: Playwright runs in Node.
import { checkRegistrationSource } from '../projects/paddock-ui/shell/testing/src/registration-lint';
import { uiTimeoutProblems } from '../projects/paddock-ui/shell/jobs/src/ui-timeouts';

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
    const reset = page.getByRole('button', { name: 'Reset greetings' });
    await expect(reset).toBeDisabled();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await expect(page.getByRole('list', { name: 'Notices' }).getByText('Hello from the shell demo (1)')).toBeVisible();
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
    await expect(page.getByRole('list', { name: 'Notices' }).getByRole('listitem')).toHaveCount(0);

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
    await expect(page.getByRole('list', { name: 'Notices' })).toContainText('Importing logs: done');
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
  const isMac = (page: import('@playwright/test').Page) => page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform));
  const menuBar = (page: import('@playwright/test').Page) => page.getByRole('menubar', { name: 'Application menu' });
  const openMenu = async (page: import('@playwright/test').Page, name: string) => {
    await menuBar(page).getByRole('menuitem', { name, exact: true }).click();
    return page.getByRole('menu', { name });
  };

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
  });

  test('the menu bar shows the contributed menus with keys, states and a submenu, and runs items', async ({ page }) => {
    const mac = await isMac(page);
    await expect(menuBar(page).getByRole('menuitem')).toHaveText(['File', 'View', 'Help']);
    const help = await openMenu(page, 'Help');
    const greet = help.getByRole('menuitem', { name: 'Say hello' });
    await expect(greet).toContainText(mac ? '⇧⌘H' : 'Ctrl+Shift+H');
    await expect(greet).toHaveAttribute('aria-keyshortcuts', mac ? 'Shift+Meta+H' : 'Control+Shift+H');
    await expect(help.getByRole('menuitem', { name: 'Reset greetings' })).toHaveAttribute('aria-disabled', 'true');
    await greet.click();
    await expect(page.getByRole('list', { name: 'Notices' }).getByText('Hello from the shell demo (1)')).toBeVisible();
    await expect(page.getByRole('menu')).toHaveCount(0);
    // The enabled state follows the command; the submenu holds its own items.
    const again = await openMenu(page, 'Help');
    await expect(again.getByRole('menuitem', { name: 'Reset greetings' })).not.toHaveAttribute('aria-disabled', 'true');
    await again.getByRole('menuitem', { name: 'More demos' }).click();
    await page.getByRole('menu', { name: 'More demos' }).getByRole('menuitem', { name: 'Fail on purpose' }).click();
    await expect(page.getByText('Fail on purpose failed: this command always fails')).toBeVisible();
    // A toggle shows its check mark.
    const view = await openMenu(page, 'View');
    const compact = view.getByRole('menuitemcheckbox', { name: 'Compact notices' });
    await expect(compact).toHaveAttribute('aria-checked', 'false');
    await compact.click();
    await expect(page.getByRole('list', { name: 'Notices' })).toHaveClass(/compact/);
    await expect((await openMenu(page, 'View')).getByRole('menuitemcheckbox', { name: 'Compact notices' })).toHaveAttribute('aria-checked', 'true');
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
    await expect(page.getByText('Hello from the shell demo (1)')).toBeVisible();
    const note = page.getByTestId('demo-note');
    await note.click();
    await note.pressSequentially('hk');
    await expect(note).toHaveValue('hk');
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await expect(page.getByText('Hello from the shell demo (2)')).toBeVisible();
    await page.keyboard.press('ControlOrMeta+Alt+K');
    await expect(page.getByRole('list', { name: 'Notices' })).toHaveClass(/compact/);
  });

  test('a changed key is kept across a reload, and a taken key is refused with its owner', async ({ page }) => {
    const mac = await isMac(page);
    const row = (command: string) => page.locator(`tr[data-command="${command}"]`);
    await row('sd.greet').getByRole('button', { name: 'Change key' }).click();
    await expect(row('sd.greet').getByTestId('shortcut-key')).toHaveText('Press a key…');
    // A key pressed while recording runs nothing.
    await page.keyboard.press('ControlOrMeta+Alt+G');
    await expect(page.getByTestId('shortcut-message')).toHaveText(`Say hello is now ${mac ? '⌥⌘G' : 'Ctrl+Alt+G'}.`);
    await expect(page.getByRole('list', { name: 'Notices' }).getByRole('listitem')).toHaveCount(0);
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
    await expect(page.getByRole('list', { name: 'Notices' }).getByRole('listitem')).toHaveText(['Hello from the shell demo (1)']);
    await row('sd.greet').getByRole('button', { name: 'Reset key' }).click();
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await expect(page.getByText('Hello from the shell demo (2)')).toBeVisible();
  });

  test('the notices have a context menu from the same model', async ({ page }) => {
    await page.getByRole('button', { name: 'Say hello', exact: true }).click();
    await page.getByRole('list', { name: 'Notices' }).click({ button: 'right' });
    const menu = page.getByRole('menu', { name: 'Notice actions' });
    await expect(menu.getByRole('menuitem')).toHaveText([/Say hello/, /Reset greetings/]);
    await menu.getByRole('menuitem', { name: 'Reset greetings' }).click();
    await expect(page.getByText('Greetings reset')).toBeVisible();
  });
});
