import assert from 'node:assert/strict';
import { test } from 'node:test';
import C133, { directTestImports, mockClockReads } from '../src/checks/C133-e2e-pinned-clock.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

test('C133 rule 1: a value import of test from Playwright or the kit in a spec', () => {
  assert.deepEqual(directTestImports("import { expect, test } from '@playwright/test';\n"), [
    { line: 1, what: "imports `test` from @playwright/test instead of the app's e2e/test.ts (the page clock is not pinned there)" },
  ]);
  assert.equal(directTestImports("import { expect, test as base, type Page } from '@decryp7/paddock-e2e';\n").length, 1);
  assert.equal(directTestImports("import {\n  dialogWindowUrl,\n  test,\n} from '@decryp7/paddock-e2e';\n")[0].line, 1);
});

test('C133 rule 1: types, other names, the wrapper import and a waiver are fine', () => {
  assert.deepEqual(directTestImports("import type { Page } from '@playwright/test';\nimport { expect } from '@playwright/test';\n"), []);
  assert.deepEqual(directTestImports("import { expect, test } from './test';\n"), []);
  assert.deepEqual(directTestImports("import { pressMod } from '@decryp7/paddock-e2e';\n"), []);
  assert.deepEqual(directTestImports("// conformance: e2e-test — a probe of the raw Playwright clock\nimport { test } from '@playwright/test';\n"), []);
  assert.deepEqual(directTestImports("// import { test } from '@playwright/test';\n"), [], 'a comment is not an import');
});

test('C133 rule 2: Date.now() and new Date() with no argument in a mock', () => {
  assert.deepEqual(mockClockReads('const a = Date.now();\nconst b = new Date();\nconst c = new Date( );\n').map((s) => s.line), [1, 2, 3]);
  assert.deepEqual(mockClockReads('const a = new Date(ms).toISOString();\nconst d = new Date(2026, 9, 1);\n'), [], 'a date made from a value is not a clock read');
  assert.deepEqual(mockClockReads('// Date.now()\nconst s = "Date.now()";\n'), []);
  assert.deepEqual(mockClockReads('// conformance: mock-clock — the real wall clock of a download header\nconst a = Date.now();\n'), []);
});

// The hermetic control: a tree in memory with both defects planted, and the check sees both (and nothing in the good files).
test('C133 sees a planted direct import and a planted Date.now() in a mock, and only those', async () => {
  const root = makeApp({
    'apps/demo/app/e2e/flows.spec.ts': "import { expect, test } from '@playwright/test';\n",
    'apps/demo/app/e2e/good.spec.ts': "import { expect, test } from './test';\nimport type { Page } from '@playwright/test';\n",
    'apps/demo/app/e2e/test.ts': "import { expect, test as base } from '@playwright/test';\nexport const test = base;\n",
    'apps/demo/app/playwright.config.ts': "import { defineConfig, test } from '@playwright/test';\n",
    'apps/demo/app/src/app/core/mock-backend.ts': 'export const stamp = () => Date.now();\n',
    'apps/demo/app/src/app/core/mock/clocked.ts': 'export const stamp = (now: () => number) => now();\n',
    'apps/demo/app/src/app/core/mock/mock-backend.spec.ts': 'const x = Date.now();\n',
    'apps/debrief/app/src/app/core/mock/mock-clock.ts': 'export const now = () => Date.now();\n',
  });
  const found = await findingsOf('C133', root);
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]).sort(), [
    ['apps/demo/app/e2e/flows.spec.ts', 1, 'warn'],
    ['apps/demo/app/src/app/core/mock-backend.ts', 1, 'warn'],
  ]);
  assert.match(found.map((f) => f.message).join('\n'), /rule 1[\s\S]*rule 2|rule 2[\s\S]*rule 1/);
});

test('C133 is report-only: its findings are warnings', () => {
  assert.equal(C133.mode, 'report-only');
});
