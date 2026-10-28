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

test('C44 flags a LISTEN on a jobs backend, or inject(JobsBackend) outside provider wiring (rule 42)', async () => {
  const listen = "import { TauriJobsBackend, JobsBackend } from '@decryp7/paddock-ui/shell/jobs';\nexport class S {\n  private readonly backend = inject(JobsBackend);\n  start() { this.backend.listen((e) => toast(e)); }\n}\n";
  const bad = await findingsOf('C44', app({ 'app/src/app/page.ts': listen }));
  assert.deepEqual(bad.map((f) => f.line), [3, 4]);
  assert.equal(bad[0].rule, 42);
  assert.match(messages(bad), /listens to the shell’s jobs backend/);
  const built = "import { TauriJobsBackend as T } from '@decryp7/paddock-ui/shell/jobs';\nconst b = new T(ipc);\nb.listen(f);\nnew T(ipc).listen(g);\n";
  assert.deepEqual((await findingsOf('C44', app({ 'app/src/app/x.ts': built }))).map((f) => f.line), [3, 4]);
  // inject in provider wiring is the shell's own pattern
  assert.deepEqual(await findingsOf('C44', app({ 'app/src/app/provide-app.ts': "import { JobsBackend } from '@decryp7/paddock-ui/shell/jobs';\nconst b = inject(JobsBackend);\nb.listen(f);\n" })).then((f) => f.map((x) => x.line)), [3]);
});

test('C44 does not flag provider wiring shaped like the pending Spotter and Pitcrew moves', async () => {
  const engines = "import { JobsBackend, MemoryJobsBackend, TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';\nexport function jobsBackend(engine: Engine): JobsBackend {\n  return engine === 'tauri' ? new TauriJobsBackend(ipc) : new MemoryJobsBackend();\n}\nexport const browser = { jobs: new MemoryJobsBackend() };\n";
  const main = "import { TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';\nexport const opts = { jobs: { backend: () => new TauriJobsBackend(ipc) } };\n";
  const mock = "import { MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';\nexport class MockBackend {\n  readonly jobs = new MemoryJobsBackend();\n  seed() { this.jobs.add(1); }\n}\n";
  const f = await findingsOf('C44', app({ 'app/src/app/shell/engines.ts': engines, 'app/src/app/shell/main-window.ts': main, 'app/src/app/core/mock-backend.ts': mock }));
  assert.deepEqual(f, []);
});
