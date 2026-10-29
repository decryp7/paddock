import type { DialogDef } from '@decryp7/paddock-ui/shell';
import { assertDialogBoots } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { provideIcons } from '../shared/icons';
import { DIALOGS } from './contributions';
import { DIALOG_BASE_COMMANDS, dialogBaseProviders } from './dialog-base';
import { dialogJobsProviders } from './dialog-window';

// Every Dyno dialog through the boot-every-dialog harness (ADR 0009 §4, ADR 0026 §6): it boots with only its window's
// providers, sends only what its Rust contract declares, draws no footer of its own, reports ready and closes on Escape.
const ALL = DIALOGS as readonly DialogDef[];

const INPUTS = {
  'pd.confirm': { heading: 'Delete “GPU limit”?', message: 'The rule stops raising alarms.', confirm: { label: 'Delete rule', destructive: true } },
  'dy.alert-rule': { rule: null, takenIds: [] },
  'dy.web-search': { text: 'Slack crashed', subject: 'Slack' },
  'dy.file-hash': { source: { path: '/Applications/Slack.app/Contents/MacOS/Slack' }, name: 'Slack' },
};

describe('Dyno dialogs boot as their windows do', () => {
  it('are the shared confirmation, the Settings window, the four own dialogs and the shell’s About, Updates and What’s New', () => {
    expect(ALL.map((d) => d.id).sort()).toEqual(['dy.alert-rule', 'dy.file-hash', 'dy.lockers', 'dy.web-search', 'pd.about', 'pd.confirm', 'pd.settings', 'pd.updates', 'pd.whats-new']);
  });

  for (const def of ALL) {
    it(`${def.id} (${def.kind})`, async () => {
      await assertDialogBoots(def, {
        dialogs: ALL,
        providers: () => [provideIcons(), { provide: BACKEND, useFactory: () => new MockBackend({ latencyMs: 0, now: () => 0 }) }, ...dialogJobsProviders(def.id), ...dialogBaseProviders()],
        baseCommands: DIALOG_BASE_COMMANDS,
        answers: { window_ready: { recovered: null, ui: null } },
        inputs: INPUTS,
        ignoreErrors: [],
      });
    });
  }
});
