import type { DialogDef } from '@decryp7/paddock-ui/shell';
import { assertDialogBoots } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { provideIcons } from '../shared/icons';
import { deleteWatchPrompt } from '../shared/ui/confirm';
import { DIALOGS } from './contributions';
import { DIALOG_BASE_COMMANDS, dialogBaseProviders } from './dialog-base';

// Every Spotter dialog through the boot-every-dialog harness (ADR 0009 §4, ADR 0026 §6): it boots with only its window's
// providers, sends only what its Rust contract declares, draws no footer of its own, reports ready and closes on Escape.
const ALL = DIALOGS as readonly DialogDef[];

describe('Spotter dialogs boot as their windows do', () => {
  it('are the shared confirmation, the Settings window and the shell’s About, Updates, What’s New and Report a problem', () => {
    expect(ALL.map((d) => d.id).sort()).toEqual(['pd.about', 'pd.confirm', 'pd.report-problem', 'pd.settings', 'pd.updates', 'pd.whats-new']);
  });

  for (const def of ALL) {
    it(`${def.id} (${def.kind})`, async () => {
      await assertDialogBoots(def, {
        dialogs: ALL,
        providers: () => [provideIcons(), { provide: BACKEND, useFactory: () => new MockBackend({ now: () => 0 }) }, ...dialogBaseProviders()],
        baseCommands: DIALOG_BASE_COMMANDS,
        answers: { window_ready: { recovered: null, ui: null } },
        inputs: { 'pd.confirm': { ...deleteWatchPrompt('Acme') } },
        ignoreErrors: [],
      });
    });
  }
});
