import { DEMO_DIALOGS } from '../../../../shell-demo/src/app/demo.contributions';
import { DIALOG_BASE_COMMANDS, dialogBaseProviders } from '../../../../shell-demo/src/app/dialog-base';
import { APP_COMMANDS } from '../../../../shell-demo/src/app/generated/app-commands';
import { DIALOG_BASE } from '../../../../shell-demo/src/app/generated/dialog-contracts';
import { assertDialogBoots, dialogTableProblems } from './dialog-harness';
import { SHELL_COMMANDS, type DialogDef } from '@decryp7/paddock-ui/shell';
import { CHANNELS_COMMANDS } from '@decryp7/paddock-ui/shell/channels';
import { MCP_COMMANDS } from '@decryp7/paddock-ui/shell/mcp';

// The shell demo's dialogs (one per kind) through the boot-every-dialog harness and its command table: what every app runs
// over its own registry (ADR 0009 §4). The contracts are Rust's (ADR 0026 §3): the registry reads the generated ones.
const DIALOGS = DEMO_DIALOGS as readonly DialogDef[];

describe('shell-demo dialogs', () => {
  it('has one of each kind, a modal form and a required choice', () => {
    expect([...new Set(DIALOGS.map((d) => d.kind))].sort()).toEqual(['decision', 'form', 'tool', 'viewer']);
    expect(DIALOGS.some((d) => d.kind === 'form' && d.modality === 'modal')).toBe(true);
    expect(DIALOGS.some((d) => d.requiredChoice)).toBe(true);
  });

  for (const def of DIALOGS) {
    it(`${def.id} (${def.kind}) boots with only its window's providers`, async () => {
      const report = await assertDialogBoots(def, {
        dialogs: DIALOGS,
        // Called with the IPC recorder in place: the window takes its Tauri backends, as in the desktop build.
        providers: dialogBaseProviders,
        baseCommands: DIALOG_BASE_COMMANDS,
        answers: { window_ready: { recovered: null, ui: null } },
        inputs: { 'sd.rename': { current: 'Demo' }, 'pd.channels.secret': { channelId: 'slack', label: 'Slack', kind: 'token' }, 'pd.channels.remove': { channelId: 'slack', name: 'Slack' }, 'pd.mcp.revoke': { keyId: 1, client: 'Pitwall' } },
      });
      expect(report.sent).toContain('window_ready');
      if (def.id === 'sd.inspector') expect(report.sent).toContain('demo_status');
      if (def.id === 'pd.settings') expect(report.sent).toContain('settings_overview');
      if (def.id === 'pd.setup') expect(report.sent).toContain('setup_state');
    });
  }

  it('declares only commands of its table that dialogs may call', () => {
    // The app's own table and the shell plugin's (every shell crate's commands, plan S17).
    expect(dialogTableProblems(DIALOGS, { ...APP_COMMANDS, ...SHELL_COMMANDS, ...CHANNELS_COMMANDS, ...MCP_COMMANDS }, DIALOG_BASE_COMMANDS)).toEqual([]);
  });

  it('takes the base commands from the Rust table', () => {
    expect([...DIALOG_BASE]).toEqual([...DIALOG_BASE_COMMANDS].sort());
  });
});
