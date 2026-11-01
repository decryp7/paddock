import { FIXTURES } from './dialog-fixtures';
import { dialogContracts, dialogFitProblems, expect, test } from '@decryp7/paddock-e2e';

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
});
