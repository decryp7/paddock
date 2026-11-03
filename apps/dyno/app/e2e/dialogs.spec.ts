import { FIXTURES } from './dialog-fixtures';
import { dialogContracts, dialogFitProblems, dialogWindowUrl, expect, expectLoopSettles, SIZE_SCALES, test } from '@decryp7/paddock-e2e';

// Dyno's dialogs as windows opened by URL (what Rust loads): the app's own contract (src-tauri/src/dialogs.rs) and the
// shell's, which Dyno registers (About, What's new, Updates, the shared confirmation). ADR 0026 §6.
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts('src/app/generated/dialog-contracts.ts', ...['about', 'confirm', 'diagnostics'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`));

test.describe('Dyno dialogs', () => {
  test('are the ten Dyno registers', () => {
    expect(Object.keys(CONTRACTS).sort()).toEqual(['dy.alert-rule', 'dy.file-hash', 'dy.lockers', 'dy.web-search', 'pd.about', 'pd.confirm', 'pd.report-problem', 'pd.settings', 'pd.updates', 'pd.whats-new']);
  });

  test('every dialog fits at 1, 1.25 and 1.5 on 1920x1080 and 1366x768: its footer in view, on top, no gap (ADR 0026 §6)', async ({ browser, baseURL }) => {
    test.setTimeout(900_000);
    const problems: string[] = [];
    for (const [id, contract] of Object.entries(CONTRACTS)) problems.push(...(await dialogFitProblems(browser, id, contract, { baseURL: baseURL!, inputs: FIXTURES[id] ?? [{}] })));
    expect(problems).toEqual([]);
  });

  test('every dialog sizes to its content through one loop: quiet within three measures at 1, 1.25 and 1.5, a new epoch on reload (ADR 0043)', async ({ browser, baseURL }) => {
    test.setTimeout(900_000);
    for (const [id, contract] of Object.entries(CONTRACTS)) {
      for (const scale of SIZE_SCALES) await expectLoopSettles(browser, baseURL!, id, contract, scale, (FIXTURES[id] ?? [{}])[0]);
    }
  });

  test('Updates fits its content when the engine answers after the first measure (no scrollbar beside one line)', async ({ page }) => {
    await page.addInitScript(() => ((globalThis as { __pdHoldUpdatesStatus?: boolean }).__pdHoldUpdatesStatus = true));
    await page.setViewportSize({ width: 560, height: 700 });
    await page.goto(dialogWindowUrl('pd.updates', {}, { title: 'Updates' }));
    // The window was sized for "asking" before any status arrived (the held status is a pending task, so it is not settled yet); now the status comes.
    await page.locator('html[data-dialog-ready]').waitFor();
    const before = Number(await page.evaluate(() => document.documentElement.dataset['fittedHeight']));
    await page.evaluate(() => (globalThis as { __pdReleaseUpdatesStatus?: () => void }).__pdReleaseUpdatesStatus?.());
    await expect(page.getByTestId('updates-current')).toBeVisible();
    await page.locator('html[data-fit-settled]').waitFor();
    // The shell refits to the real content, and in a window of that height the body shows all of it.
    await expect.poll(() => page.evaluate(() => Number(document.documentElement.dataset['fittedHeight']))).toBeGreaterThan(before);
    const fitted = Number(await page.evaluate(() => document.documentElement.dataset['fittedHeight']));
    await page.setViewportSize({ width: 560, height: fitted });
    const body = await page.getByTestId('dialog-body').evaluate((el) => ({ scroll: el.scrollHeight, client: el.clientHeight }));
    expect(body.scroll).toBeLessThanOrEqual(body.client + 1);
  });
});
