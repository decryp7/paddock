import assert from 'node:assert/strict';
import { test } from 'node:test';
import C137, { kitImportLine } from '../src/checks/C137-e2e-kit-import.mjs';

const ctxOf = (files) => ({ files: () => Object.keys(files), read: (f) => files[f] ?? null });
const SPEC = 'apps/debrief/app/e2e/dock.e2e.ts';
const IMPORT = "import { settledLayout } from '@decryp7/paddock-e2e';\n";
const wf = (run) => `jobs:\n  e2e:\n    steps:\n      - name: x\n        run: ${run}\n`;
const own = { 'apps/debrief/paddock.ci.json': '{"workflow":"debrief.yml"}' };

test('C137 bad: an app with its own workflow that never builds the kit', () => {
  const found = C137.run(ctxOf({ [SPEC]: IMPORT, ...own, '.gitea/workflows/debrief.yml': wf('npm run build -w ui'), '.gitea/workflows/ci.yml': wf('npm run build:e2e -w ui') }));
  assert.equal(found.length, 1, 'the generator-checks job of ci.yml builds it, but it is not this app');
  assert.equal(found[0].file, SPEC);
  assert.equal(found[0].line, 1);
});

test('C137 good: the own workflow builds the kit, the import is relative, or it is a type import', () => {
  assert.deepEqual(C137.run(ctxOf({ [SPEC]: IMPORT, ...own, '.gitea/workflows/debrief.yml': wf('npm run build:e2e -w ui') })), []);
  assert.deepEqual(C137.run(ctxOf({ [SPEC]: "import { a } from '../../../../ui/projects/paddock-e2e/src/settled-layout.ts';\n" })), []);
  assert.deepEqual(C137.run(ctxOf({ [SPEC]: "import type { Page } from '@decryp7/paddock-e2e';\n" })), []);
});

test('C137 a comment-only mention of build:e2e is not a build step', () => {
  const run = '|\n          # npm run build:e2e -w ui\n          npm run build -w ui';
  assert.equal(C137.run(ctxOf({ [SPEC]: IMPORT, ...own, '.gitea/workflows/debrief.yml': wf(run) })).length, 1);
});

test('C137 a leg of ci.yml: each e2e command list of its contract must run the step (e2e and e2eWebkit)', () => {
  const dyno = 'apps/dyno/app/e2e/a.e2e.ts';
  const withStep = { 'apps/dyno/paddock.ci.json': '{"e2e":["npm run build -w ui","npm run build:e2e -w ui"],"e2eWebkit":["npm run build:e2e -w ui"]}', '.gitea/workflows/ci.yml': wf('npm run build:e2e -w ui') };
  const without = { 'apps/dyno/paddock.ci.json': '{"e2e":["npm run build -w ui","npm run build:e2e -w ui"],"e2eWebkit":["npm run build -w ui"]}', '.gitea/workflows/ci.yml': wf('npm run build:e2e -w ui') };
  assert.deepEqual(C137.run(ctxOf({ [dyno]: IMPORT, ...withStep })), []);
  assert.equal(C137.run(ctxOf({ [dyno]: IMPORT, ...without })).length, 1, 'the WebKit leg lacks it, and a build:e2e elsewhere in ci.yml does not count');
});

test('C137 reports an app whose contract cannot be read, and reads app specs only', () => {
  const found = C137.run(ctxOf({ [SPEC]: IMPORT }));
  assert.equal(found.length, 1);
  assert.match(found[0].message, /cannot be read/);
  assert.deepEqual(C137.run(ctxOf({ 'ui/e2e/a.spec.ts': IMPORT })), []);
});

test('C137 finds the line of the import, multi-line imports included', () => {
  assert.equal(kitImportLine("// x\nimport {\n  a,\n} from '@decryp7/paddock-e2e';\n"), 2);
  assert.equal(kitImportLine("import { a } from './x';\n"), 0);
});
