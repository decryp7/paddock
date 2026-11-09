import type { DialogDef } from '@decryp7/paddock-ui/shell';
import { assertDialogBoots } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { provideIcons } from '../shared/icons';
import { DIALOGS } from './contributions';
import { DIALOG_BASE_COMMANDS, dialogBaseProviders } from './dialog-base';

// Every Pitwall dialog through the boot-every-dialog harness (ADR 0009 §4, ADR 0026 §6): it boots with only its window's
// providers, sends only what its Rust contract declares, draws no footer of its own, reports ready and closes on Escape
// (a required choice ignores it).
const ALL = DIALOGS as readonly DialogDef[];

/** What each dialog is opened with (the e2e fixtures use the same shapes). */
export const INPUTS = {
  'pd.confirm': { heading: 'Delete Qwen3.5 9B?', message: 'qwen3.5-9b-q4_k_m.gguf (5.6 GB) is removed from disk. You can download it again later.', confirm: { label: 'Delete model', destructive: true } },
  'pw.key-secret': { appName: 'Raycast', secret: 'pw_live_8f3a9c2e7b1d4f6a0c5e9b2d7f1a3c8e', prefix: 'pw_live_8f3a' },
  'pw.service-consent': { platform: 'macos' },
  'pw.mcp-key': { app: 'Dyno' },
};

describe('Pitwall dialogs boot as their windows do', () => {
  it('are the shared confirmation, the Settings window, the five own dialogs and the shell’s About, Updates, What’s New and Report a problem', () => {
    expect(ALL.map((d) => d.id).sort()).toEqual(['pd.about', 'pd.confirm', 'pd.report-problem', 'pd.settings', 'pd.updates', 'pd.whats-new', 'pw.create-app', 'pw.key-secret', 'pw.mcp-key', 'pw.quit', 'pw.service-consent']);
  });

  for (const def of ALL) {
    it(`${def.id} (${def.kind})`, async () => {
      await assertDialogBoots(def, {
        dialogs: ALL,
        providers: () => [provideIcons(), { provide: BACKEND, useFactory: () => new MockBackend({ latencyMs: 0, now: () => 0 }) }, ...dialogBaseProviders()],
        baseCommands: DIALOG_BASE_COMMANDS,
        answers: { window_ready: { recovered: null, ui: null } },
        inputs: INPUTS,
        ignoreErrors: [],
      });
    });
  }
});
