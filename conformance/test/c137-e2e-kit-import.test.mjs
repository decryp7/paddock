import assert from 'node:assert/strict';
import { test } from 'node:test';
import C137, { kitImportLine } from '../src/checks/C137-e2e-kit-import.mjs';

const ctxOf = (files) => ({ files: () => Object.keys(files), read: (f) => files[f] ?? null });
const SPEC = 'apps/debrief/app/e2e/dock.e2e.ts';

test('C137 bad: a value import of the kit by package name where the app workflow does not build it', () => {
  const found = C137.run(ctxOf({ [SPEC]: "import { settledLayout } from '@decryp7/paddock-e2e';\n", '.gitea/workflows/debrief.yml': 'jobs:\n  e2e:\n    steps:\n      - run: npm run build -w ui\n' }));
  assert.equal(found.length, 1);
  assert.equal(found[0].file, SPEC);
  assert.equal(found[0].line, 1);
});

test('C137 good: the workflow builds the kit, the import is relative, or it is a type import', () => {
  const built = { '.gitea/workflows/debrief.yml': 'run: npm run build:e2e -w ui\n' };
  assert.deepEqual(C137.run(ctxOf({ [SPEC]: "import { a } from '@decryp7/paddock-e2e';\n", ...built })), []);
  assert.deepEqual(C137.run(ctxOf({ [SPEC]: "import { a } from '../../../../ui/projects/paddock-e2e/src/settled-layout.ts';\n" })), []);
  assert.deepEqual(C137.run(ctxOf({ [SPEC]: "import type { Page } from '@decryp7/paddock-e2e';\n" })), []);
});

test('C137 reads the app workflow, else ci.yml, and only app specs', () => {
  const text = "import { a } from '@decryp7/paddock-e2e/x';\n";
  assert.equal(C137.run(ctxOf({ 'apps/demo/app/e2e/a.ts': text, '.gitea/workflows/ci.yml': 'run: npm run build:e2e -w ui\n' })).length, 0);
  assert.equal(C137.run(ctxOf({ 'apps/demo/app/e2e/a.ts': text, '.gitea/workflows/ci.yml': 'run: true\n' })).length, 1);
  assert.deepEqual(C137.run(ctxOf({ 'ui/e2e/a.spec.ts': text })), []);
});

test('C137 finds the line of the import, multi-line imports included', () => {
  assert.equal(kitImportLine("// x\nimport {\n  a,\n} from '@decryp7/paddock-e2e';\n"), 2);
  assert.equal(kitImportLine("import { a } from './x';\n"), 0);
});
