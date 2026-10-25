import { DIALOG_BASE_COMMANDS, SHELL_COMMANDS, type DialogDef } from '@decryp7/paddock-ui/shell';
import { dialogContractFile, serializeDialogContracts } from '@decryp7/paddock-ui/shell/dialogs';
import { assertDialogBoots, dialogTableProblems } from '@decryp7/paddock-ui/shell/testing';
import { WORKBENCH_COMMANDS } from '@decryp7/paddock-ui/workbench';
import { dockLayoutDialogs } from '@decryp7/paddock-ui/workbench/dock';

// The workbench demo's dialogs (the dock's Save layout and Manage layouts) through the boot-every-dialog harness, its
// command table and its contract file, which the desktop app's shell plugin reads (ADR 0009 §4).
const DIALOGS = dockLayoutDialogs({ storageKey: 'wd' }) as readonly DialogDef[];
/** Every dialog window of the workbench publishes its display state to its host (`provideDialogUiSync`). */
const BASE = [...DIALOG_BASE_COMMANDS, 'ui_sync_publish'];
const FILE = 'projects/workbench-demo/dialog-contracts.json';

interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  writeFileSync(path: string, data: string): void;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;
const env = (name: string): string | undefined => (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env[name];

describe('workbench-demo dialogs', () => {
  for (const def of DIALOGS) {
    it(`${def.id} (${def.kind}) boots with only its window's providers`, async () => {
      await assertDialogBoots(def, {
        dialogs: DIALOGS,
        baseCommands: BASE,
        answers: { window_ready: { recovered: null, ui: null } },
        inputs: { 'pd.wb.dock.layout-save': { names: ['Review'], suggested: 'Layout 2' } },
      });
    });
  }

  it('declares only commands the shell and workbench plugins open to dialogs', () => {
    expect(dialogTableProblems(DIALOGS, { ...SHELL_COMMANDS, ...WORKBENCH_COMMANDS }, BASE)).toEqual([]);
  });

  // Regenerate with PADDOCK_WRITE_CONTRACTS=1 npx ng test paddock-ui --watch=false --include=**/workbench/testing/src/reference-dialogs.spec.ts
  it('keeps the demo dialog-contracts.json equal to the registry', async () => {
    const fs = await nodeFs();
    const want = serializeDialogContracts(dialogContractFile(DIALOGS, { base: BASE }));
    if (env('PADDOCK_WRITE_CONTRACTS') === '1') fs.writeFileSync(FILE, want);
    expect(fs.readFileSync(FILE, 'utf8')).toBe(want);
  });
});
