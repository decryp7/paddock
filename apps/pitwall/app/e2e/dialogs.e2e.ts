import { expect, test, type Page } from '@playwright/test';
import { dialogContracts, dialogFitProblems, dialogWindowUrl, ShellDialogs } from '../../../../ui/projects/paddock-e2e/src/index.ts';
import { openApp } from './app';
import { CONTRACT_FILES, FIXTURES } from './dialog-fixtures';

// Pitwall's dialogs as windows opened by URL (what Rust loads): its own contracts (src-tauri/src/dialogs.rs) and the shell's,
// which Pitwall registers (About, What's new, Updates, the shared confirmation). ADR 0026 §6.
const CONTRACTS = dialogContracts(...CONTRACT_FILES);

const open = async (page: Page, id: string, input: unknown) => {
  await page.goto(dialogWindowUrl(id, input, { title: id, query: '?mock-fast&mock-static' }));
  const dialogs = new ShellDialogs(page);
  await dialogs.loaded();
  return dialogs;
};

test.describe('Pitwall dialog windows', () => {
  test('are the nine Pitwall registers', () => {
    expect(Object.keys(CONTRACTS).sort()).toEqual(['pd.about', 'pd.confirm', 'pd.settings', 'pd.updates', 'pd.whats-new', 'pw.create-app', 'pw.key-secret', 'pw.quit', 'pw.service-consent']);
  });

  test('every dialog fits at device scale 1 and 1.5 on 1920x1080 and 1366x768: its footer in view, on top, no gap (ADR 0026 §6)', async ({ browser, baseURL }) => {
    test.setTimeout(900_000);
    const problems: string[] = [];
    for (const [id, contract] of Object.entries(CONTRACTS)) {
      problems.push(...(await dialogFitProblems(browser, id, contract, { baseURL: baseURL!, inputs: FIXTURES[id] ?? [{}], scales: [1, 1.5] })));
    }
    expect(problems).toEqual([]);
  });

  test('New app: Create app waits for a name, and answers it', async ({ page }) => {
    const d = await open(page, 'pw.create-app', {});
    await expect(d.action('create')).toBeDisabled();
    await expect(page.getByLabel('Name')).toBeFocused();
    await page.getByLabel('Name').fill('  Obsidian ');
    await page.keyboard.press('Enter');
    await expect.poll(() => d.result()).toContain('"Obsidian"');
  });

  test('the new key: no way out but "I\'ve saved it"; Escape does nothing', async ({ page }) => {
    const d = await open(page, 'pw.key-secret', FIXTURES['pw.key-secret']![0]);
    await expect(d.shell.locator('[data-testid=dialog-footer] [data-role=dismiss]')).toHaveCount(0);
    await page.keyboard.press('Escape');
    expect(await d.result()).toBeNull();
    await d.action('saved').click();
    await expect.poll(() => d.result()).toContain('true');
  });

  test('Quit Pitwall?: Cancel answers nothing; "Don\'t ask again" is part of the answer', async ({ page }) => {
    const d = await open(page, 'pw.quit', {});
    await expect(d.action('keep')).toBeFocused();
    await page.getByRole('checkbox', { name: "Don't ask again" }).check();
    await d.action('stop').click();
    await expect.poll(() => d.result()).toContain('"stopServing":true,"remember":true');
  });

  test('the service question: Run in the background answers install', async ({ page }) => {
    const d = await open(page, 'pw.service-consent', { platform: 'windows' });
    await expect(page.getByText(/administrator prompt/)).toBeVisible();
    await d.action('install').click();
    await expect.poll(() => d.result()).toContain('"install":true');
  });
});

test.describe('quitting from the main window', () => {
  test('a quit asked for with "Ask each time" shows the question in the page; Cancel leaves Pitwall running', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-quit');
    const question = page.getByRole('alertdialog');
    await expect(question).toContainText('Quit Pitwall?');
    await question.getByRole('button', { name: 'Cancel' }).click();
    await expect(question).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Dashboard', level: 1 })).toBeVisible();
  });
});
