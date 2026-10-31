import assert from 'node:assert/strict';
import { test } from 'node:test';
import { grantProblems } from '../src/checks/C49-provider-grants.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const JOBS = `export const JOBS_COMMANDS = {\n  job_cancel: { kind: 'mutation' },\n  jobs_snapshot: { kind: 'query' },\n} as const;\n`;
const SETTINGS = `export const SETTINGS_COMMANDS = {\n  settings_get: { kind: 'query' },\n  settings_set: { kind: 'mutation' },\n} as const;\n`;
const contracts = (commands) => `export const DIALOG_CONTRACTS = {\n  'pd.settings': {\n    id: 'pd.settings',\n    kind: 'tool',\n    commands: [${commands.map((c) => `'${c}'`).join(', ')}],\n  },\n} as const;\n`;
const CONTRIB = `export const D = defineDialog(DIALOG_CONTRACTS['pd.settings'], {\n  providers: () => import('./settings-window').then((m) => m.W),\n});\n`;
const WINDOW = `export const W = [provideShellJobs({ backend: jobsBackend }), ...settingsWindowProviders({})];\n`;
const required = { provideShellJobs: ['job_cancel', 'jobs_snapshot'], settingsWindowProviders: ['settings_get', 'settings_set'] };

test('grantProblems names the commands a dialog window\'s providers use that its contract does not grant', () => {
  const ok = grantProblems({ contracts: contracts(['settings_*', 'jobs_snapshot', 'job_cancel']), sources: { 'c.ts': CONTRIB, 'settings-window.ts': WINDOW }, required });
  assert.deepEqual(ok, []);
  const bad = grantProblems({ contracts: contracts(['settings_*']), sources: { 'c.ts': CONTRIB, 'settings-window.ts': WINDOW }, required });
  assert.equal(bad.length, 1);
  assert.match(bad[0], /pd\.settings/);
  assert.match(bad[0], /provideShellJobs/);
  assert.match(bad[0], /job_cancel, jobs_snapshot/);
});

test('C49 bad: the Settings window loads shell jobs but its contract does not grant them; good once it does', async () => {
  const files = (cmds) => ({
    'apps/a/src-tauri/Cargo.toml': '[dependencies]\npaddock-shell = { path = "x" }\n',
    'apps/a/app/src/app/generated/dialog-contracts.ts': contracts(cmds),
    'apps/a/app/src/app/shell/contributions.ts': CONTRIB,
    'apps/a/app/src/app/shell/settings-window.ts': WINDOW,
    'ui/projects/paddock-ui/shell/jobs/generated/jobs-commands.ts': JOBS,
    'ui/projects/paddock-ui/shell/settings/generated/settings-commands.ts': SETTINGS,
  });
  const f = await findingsOf('C49', makeApp(files(['settings_*'])));
  assert.equal(f.length, 1);
  assert.match(messages(f), /jobs_snapshot/);
  assert.deepEqual(await findingsOf('C49', makeApp(files(['settings_*', 'job_cancel', 'jobs_snapshot']))), []);
});

test('C49 skips an app whose checkout has no shared provider metadata', async () => {
  const f = await findingsOf(
    'C49',
    makeApp({
      'apps/a/src-tauri/Cargo.toml': '[dependencies]\npaddock-shell = { path = "x" }\n',
      'apps/a/app/src/app/generated/dialog-contracts.ts': contracts([]),
      'apps/a/app/src/app/shell/contributions.ts': CONTRIB,
      'apps/a/app/src/app/shell/settings-window.ts': WINDOW,
    }),
  );
  assert.deepEqual(f, []);
});
