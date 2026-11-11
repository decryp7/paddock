import type { DialogDef } from '@decryp7/paddock-ui/shell/contributions';
import { assertDialogBoots } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { jobsProviders, mockWithJobs } from '../../testing/job-test-support';
import { provideIcons } from '../shared/icons';
import { DIALOGS } from './contributions';
import { DIALOG_BASE_COMMANDS, dialogBaseProviders } from './dialog-base';
import { JOB_DIALOGS, dialogJobsProviders } from './dialog-window';
import { TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { DIALOG_CONTRACTS } from '../generated/dialog-contracts';
import { dynoJobsIpc } from './engines';

// Every Dyno dialog through the boot-every-dialog harness (ADR 0009 §4, ADR 0026 §6): it boots with only its window's
// providers, sends only what its Rust contract declares, draws no footer of its own, reports ready and closes on Escape.
const ALL = DIALOGS as readonly DialogDef[];

const INPUTS = {
  'pd.confirm': { heading: 'Delete “GPU load”?', message: 'The rule stops raising alarms.', confirm: { label: 'Delete rule', destructive: true } },
  'dy.alert-rule': { rule: null, takenIds: [] },
  'dy.web-search': { text: 'Slack crashed', subject: 'Slack' },
  'dy.file-hash': { source: { path: '/Applications/Slack.app/Contents/MacOS/Slack' }, name: 'Slack' },
};

describe('Dyno dialogs boot as their windows do', () => {
  it('are the shared confirmation, the Settings window, the four own dialogs, the AI access dialogs and the shell’s About, Updates, What’s New and Report a problem', () => {
    expect(ALL.map((d) => d.id).sort()).toEqual(['dy.alert-rule', 'dy.file-hash', 'dy.lockers', 'dy.web-search', 'pd.about', 'pd.confirm', 'pd.mcp.key', 'pd.mcp.revoke', 'pd.report-problem', 'pd.settings', 'pd.updates', 'pd.whats-new']);
  });

  for (const def of ALL) {
    it(`${def.id} (${def.kind})`, async () => {
      await assertDialogBoots(def, {
        dialogs: ALL,
        // A dialog that follows a job gets the in-memory jobs backend the mock pushes into: the file read ends, and the window settles after it.
        providers: () => {
          const { mock, jobs } = mockWithJobs();
          return [provideIcons(), { provide: BACKEND, useValue: mock }, ...(dialogJobsProviders(def.id).length ? [jobsProviders(jobs)] : []), ...dialogBaseProviders()];
        },
        baseCommands: DIALOG_BASE_COMMANDS,
        answers: { window_ready: { recovered: null, ui: null } },
        inputs: INPUTS,
        ignoreErrors: [],
      });
    });
  }
});

// The in-memory jobs backend of the harness above sends nothing, so it cannot see a command the real jobs path calls that the
// contract does not declare (Dyno 1.1.0 VM round: "The pd.settings dialog called jobs_snapshot, which its contract does not
// declare"). This drives the real jobs port of each job-following dialog and checks every command it invokes.
describe('the jobs path of a job-following dialog calls only what its contract declares', () => {
  const permitted = (id: keyof typeof DIALOG_CONTRACTS, command: string): boolean =>
    [...DIALOG_CONTRACTS[id].commands, ...DIALOG_BASE_COMMANDS].some((c: string) => (c.endsWith('*') ? command.startsWith(c.slice(0, -1)) : c === command));

  for (const id of JOB_DIALOGS) {
    it(`${id}: list and cancel`, async () => {
      const called: string[] = [];
      const ipc = {
        invoke: async <T>(command: string): Promise<T> => {
          called.push(command);
          return (command === 'job_cancel' ? false : []) as T;
        },
        listen: async () => () => undefined,
      };
      const backend = new TauriJobsBackend(dynoJobsIpc(ipc));
      await backend.snapshot();
      await backend.cancel(1);
      expect(called).toContain('jobs_snapshot');
      expect(called.filter((c) => !permitted(id as keyof typeof DIALOG_CONTRACTS, c))).toEqual([]);
    });
  }
});
