import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const app = (files) => makeApp({ ...files, 'paddock.conformance.json': JSON.stringify({ app: 'demo' }) });

test('C44 good: an app that waits through the shell job follow', async () => {
  const f = await findingsOf('C44', app({ 'app/src/app/import.ts': "import { JobFollow } from '@decryp7/paddock-ui/shell/jobs';\nexport const x = 1;\n" }));
  assert.deepEqual(f, []);
});

test('C44 bad: a direct listener on paddock://jobs, or the JOBS_EVENT constant', async () => {
  const literal = await findingsOf('C44', app({ 'app/src/app/core/jobs.ts': "listen('paddock://jobs', (e) => toast(e));\n" }));
  assert.equal(literal.length, 1);
  assert.equal(literal[0].severity, 'error');
  assert.equal(literal[0].file, 'app/src/app/core/jobs.ts');
  assert.equal(literal[0].line, 1);
  assert.match(messages(literal), /listens to the shell’s jobs event/);
  const constant = await findingsOf('C44', app({ 'app/src/app/a.ts': "import { JOBS_EVENT } from '@decryp7/paddock-ui/shell/jobs';\nlisten(JOBS_EVENT, f);\n" }));
  assert.equal(constant.length, 1);
});

test('C44 ignores comments, specs and files outside the app', async () => {
  const f = await findingsOf('C44', app({ 'app/src/a.ts': "// the shell listens to paddock://jobs for us\n/** paddock://jobs */\n", 'app/src/a.spec.ts': "emit('paddock://jobs', {});\n", 'crates/x/a.ts': "listen('paddock://jobs');\n" }));
  assert.deepEqual(f, []);
});

test('C44 only flags JOBS_EVENT imported from paddock-ui: aliases and namespaces yes, an app’s own constant no', async () => {
  const own = await findingsOf('C44', app({ 'app/src/app/jobs-backend.ts': "export const JOBS_EVENT = 'debrief://jobs';\nlisten(JOBS_EVENT, f);\n" }));
  assert.deepEqual(own, []);
  const alias = await findingsOf('C44', app({ 'app/src/app/a.ts': "import { JOBS_EVENT as E } from '@decryp7/paddock-ui/shell/jobs';\nlisten(E, f);\n" }));
  assert.equal(alias.length, 1);
  assert.equal(alias[0].line, 2);
  const ns = await findingsOf('C44', app({ 'app/src/app/a.ts': "import * as jobs from '@decryp7/paddock-ui/shell/jobs';\nlisten(jobs.JOBS_EVENT, f);\n" }));
  assert.equal(ns.length, 1);
  const multi = await findingsOf('C44', app({ 'app/src/app/a.ts': "import {\n  JobFollow,\n  JOBS_EVENT,\n} from '@decryp7/paddock-ui/shell/jobs';\nlisten(JOBS_EVENT, f);\n" }));
  assert.equal(multi.length, 1);
});

test('C44 flags the jobs backends outside provider setup, with rule 42', async () => {
  const use = "import { TauriJobsBackend, JobsBackend } from '@decryp7/paddock-ui/shell/jobs';\nconst b = inject(JobsBackend);\nconst t = new TauriJobsBackend();\n";
  const bad = await findingsOf('C44', app({ 'app/src/app/page.ts': use }));
  assert.equal(bad.length, 2);
  assert.equal(bad[0].rule, 42);
  assert.match(messages(bad), /jobs backend outside its provider setup/);
  assert.deepEqual(await findingsOf('C44', app({ 'app/src/app/provide-app.ts': use, 'app/src/app/app.config.ts': use, 'app/src/main.ts': use })), []);
});
