import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const app = (files) => makeApp({ ...files, 'paddock.conformance.json': JSON.stringify({ app: 'demo' }) });

test('C40 good: an app that waits through the shell job follow', async () => {
  const f = await findingsOf('C40', app({ 'app/src/app/import.ts': "import { JobFollow } from '@decryp7/paddock-ui/shell/jobs';\nexport const x = 1;\n" }));
  assert.deepEqual(f, []);
});

test('C40 bad: a direct listener on paddock://jobs, or the JOBS_EVENT constant', async () => {
  const literal = await findingsOf('C40', app({ 'app/src/app/core/jobs.ts': "listen('paddock://jobs', (e) => toast(e));\n" }));
  assert.equal(literal.length, 1);
  assert.equal(literal[0].severity, 'error');
  assert.equal(literal[0].file, 'app/src/app/core/jobs.ts');
  assert.equal(literal[0].line, 1);
  assert.match(messages(literal), /listens to the shell’s jobs event/);
  const constant = await findingsOf('C40', app({ 'app/src/app/a.ts': "import { JOBS_EVENT } from '@decryp7/paddock-ui/shell/jobs';\nlisten(JOBS_EVENT, f);\n" }));
  assert.equal(constant.length, 2);
});

test('C40 ignores comments, specs and files outside the app', async () => {
  const f = await findingsOf('C40', app({ 'app/src/a.ts': "// the shell listens to paddock://jobs for us\n/** paddock://jobs */\n", 'app/src/a.spec.ts': "emit('paddock://jobs', {});\n", 'crates/x/a.ts': "listen('paddock://jobs');\n" }));
  assert.deepEqual(f, []);
});
