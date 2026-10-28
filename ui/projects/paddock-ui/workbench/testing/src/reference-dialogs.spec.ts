import { DIALOG_BASE_COMMANDS, SHELL_COMMANDS, type DialogDef } from '@decryp7/paddock-ui/shell';
import { assertDialogBoots, dialogTableProblems } from '@decryp7/paddock-ui/shell/testing';
import { WORKBENCH_COMMANDS } from '@decryp7/paddock-ui/workbench';
import { dockLayoutDialogs } from '@decryp7/paddock-ui/workbench/dock';

// The workbench demo's dialogs (the dock's Save layout and Manage layouts) through the boot-every-dialog harness and its
// command table (ADR 0009 §4); their contracts are the workbench's Rust table (ADR 0026 §3).
const DIALOGS = dockLayoutDialogs({ storageKey: 'wd' }) as readonly DialogDef[];
/** Every dialog window of the workbench publishes its display state to its host (`provideDialogUiSync`). */
const BASE = [...DIALOG_BASE_COMMANDS, 'ui_sync_publish'];

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
});
