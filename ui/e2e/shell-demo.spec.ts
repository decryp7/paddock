import { readFileSync } from 'node:fs';
import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
// The pure lint (no Angular imports), straight from the source: Playwright runs in Node.
import { checkRegistrationSource } from '../projects/paddock-ui/shell/testing/src/registration-lint';

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
    await expect(rows).toHaveCount(10);
    for (const kind of ['commands', 'keybindings', 'menus', 'pages', 'statusItems', 'settingsPages', 'dialogs', 'healthServices', 'notificationKinds']) {
      await expect(rows.filter({ has: page.getByRole('cell', { name: kind, exact: true }) })).toHaveText(new RegExp(`^${kind}[1-9]\\d*$`));
    }
  });

  test('the registration file imports no store, service, command or component (lazy by construction)', () => {
    const file = 'projects/shell-demo/src/app/demo.contributions.ts';
    expect(checkRegistrationSource(file, readFileSync(file, 'utf8'))).toEqual([]);
  });
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
