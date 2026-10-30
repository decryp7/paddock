import { dialogContracts, dialogFitProblems, expect, test } from '@decryp7/paddock-e2e';

// Spotter's dialogs as windows opened by URL (what Rust loads): the app's own contract (src-tauri/src/dialogs.rs) and the
// shell's, which Spotter registers (About, What's new, Updates, the shared confirmation). ADR 0026 §6.
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts('src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`));

test.describe('Spotter dialogs', () => {
  test('are the five Spotter registers', () => {
    expect(Object.keys(CONTRACTS).sort()).toEqual(['pd.about', 'pd.confirm', 'pd.settings', 'pd.updates', 'pd.whats-new']);
  });

  test('every dialog fits at 1, 1.25 and 1.5 on 1920x1080 and 1366x768: its footer in view, on top, no gap (ADR 0026 §6)', async ({ browser, baseURL }) => {
    test.setTimeout(600_000);
    const fixtures: Record<string, unknown[]> = {
      'pd.confirm': [
        { heading: 'Delete "Lumen CLI"?', message: 'Spotter stops watching it and forgets its history. Downloaded files stay where they are.', confirm: { label: 'Delete watch', destructive: true } },
        { heading: 'Delete "A watch with a very long name that the heading must wrap"?', message: 'Spotter stops watching it and forgets its history. '.repeat(10), confirm: { label: 'Delete watch', destructive: true } },
      ],
    };
    const problems: string[] = [];
    for (const [id, contract] of Object.entries(CONTRACTS)) problems.push(...(await dialogFitProblems(browser, id, contract, { baseURL: baseURL!, inputs: fixtures[id] ?? [{}] })));
    expect(problems).toEqual([]);
  });
});
