import assert from 'node:assert/strict';
import { test } from 'node:test';
import C133, { bareOptOuts, directTestImports, unpinnedPages } from '../src/checks/C133-e2e-pinned-clock.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

test('C133 rule 1: every way a spec reaches test outside the wrapper', () => {
  const lines = (src) => directTestImports(src).map((s) => s.line);
  assert.deepEqual(lines("import { expect, test } from '@playwright/test';\n"), [1]);
  assert.deepEqual(lines("import { expect, test as base, type Page } from '@decryp7/paddock-e2e';\n"), [1]);
  assert.deepEqual(lines("import {\n  dialogWindowUrl,\n  test,\n} from '@decryp7/paddock-e2e';\n"), [1]);
  assert.deepEqual(lines("import * as pw from '@playwright/test';\n"), [1], 'a namespace import');
  assert.deepEqual(lines("import pw from '@playwright/test';\n"), [1], 'a default import');
  assert.deepEqual(lines("import pw, { expect } from '@playwright/test';\n"), [1], 'a default import beside names');
  assert.deepEqual(lines("export { test } from '@playwright/test';\n"), [1], 'a re-export');
  assert.deepEqual(lines("export { expect, test as t } from '@decryp7/paddock-e2e';\n"), [1]);
  assert.deepEqual(lines("export * from '@playwright/test';\n"), [1], 'export *');
  assert.match(directTestImports("import { test } from '@playwright/test';\n")[0].what, /^a value import of `test` from @playwright\/test instead of the app's e2e\/test\.ts/);
});

test('C133 rule 1: types, other names, the wrapper, a waiver and a comment are fine', () => {
  assert.deepEqual(directTestImports("import type { Page } from '@playwright/test';\nimport { expect } from '@playwright/test';\n"), []);
  assert.deepEqual(directTestImports("import type { test } from '@playwright/test';\nexport type { test } from '@playwright/test';\n"), []);
  assert.deepEqual(directTestImports("import { expect, test } from './test';\n"), []);
  assert.deepEqual(directTestImports("import { pressMod } from '@decryp7/paddock-e2e';\nexport { expect } from '@playwright/test';\n"), []);
  assert.deepEqual(directTestImports("// conformance: e2e-test — a probe of the raw Playwright clock\nimport { test } from '@playwright/test';\n"), []);
  assert.deepEqual(directTestImports("// import { test } from '@playwright/test';\nconst s = \"import { test } from '@playwright/test'\";\n"), []);
});

test('C133 rule 2: pinnedClock false needs a comment on the line above', () => {
  assert.deepEqual(bareOptOuts('test.use({ pinnedClock: false });\n').map((s) => s.line), [1]);
  assert.deepEqual(bareOptOuts('const x = 1;\n\ntest.use({ pinnedClock: false });\n').map((s) => s.line), [3], 'a blank line above is no reason');
  assert.deepEqual(bareOptOuts('// the sampler counts real frames\ntest.use({ pinnedClock: false });\n'), []);
  assert.deepEqual(bareOptOuts('  // Measures real frames: the fake clock replaces rAF.\n  test.use({ pinnedClock: false });\n'), []);
  assert.deepEqual(bareOptOuts('// ok\ntest.use({ pinnedClock: false });\n').map((s) => s.line), [2], 'a two-letter comment says nothing');
});

test('C133 rule 3: a bare clock.install() and browser.newContext(', () => {
  assert.deepEqual(unpinnedPages('await page.clock.install();\nconst c = await browser.newContext({ baseURL });\n').map((s) => s.line), [1, 2]);
  assert.deepEqual(unpinnedPages('await page.clock.install({ time: new Date(T) });\nawait page.clock.setFixedTime(T);\n'), []);
  assert.deepEqual(unpinnedPages('// conformance: e2e-test — a context of its own size, pinned below\nconst c = await browser.newContext();\n'), []);
  assert.deepEqual(unpinnedPages('// await page.clock.install();\nconst s = "browser.newContext(";\n'), []);
});

// The hermetic control: a tree in memory with each defect planted, and the check sees each one and nothing in the good files.
test('C133 sees a planted direct import, opt-out without a reason and bare install in a temporary tree, and only those', async () => {
  const root = makeApp({
    'apps/demo/app/e2e/flows.spec.ts': "import { expect, test } from '@playwright/test';\n",
    'apps/demo/app/e2e/optout.spec.ts': "import { test } from './test';\ntest.use({ pinnedClock: false });\n",
    'apps/demo/app/e2e/clock.spec.ts': "import { test } from './test';\ntest('x', async ({ page }) => {\n  await page.clock.install();\n});\n",
    'apps/demo/app/e2e/good.spec.ts':
      "import { expect, test } from './test';\nimport type { Page } from '@playwright/test';\n// needs the real time\ntest.use({ pinnedClock: false });\n",
    'apps/demo/app/e2e/test.ts': "import { expect, test as base } from '@playwright/test';\nexport const test = base;\n",
    'apps/demo/app/playwright.config.ts': "import { defineConfig, test } from '@playwright/test';\n",
    'apps/demo/app/src/app/core/mock-backend.ts': 'export const stamp = () => Date.now();\n',
  });
  const found = await findingsOf('C133', root);
  assert.deepEqual(
    found.map((f) => [f.file, f.line, f.severity]).sort(),
    [
      ['apps/demo/app/e2e/clock.spec.ts', 3, 'warn'],
      ['apps/demo/app/e2e/flows.spec.ts', 1, 'warn'],
      ['apps/demo/app/e2e/optout.spec.ts', 2, 'warn'],
    ],
    'a mock reading the wall clock is not a finding',
  );
  assert.match(found.map((f) => f.message).join('\n'), /rule 1[\s\S]*rule 2[\s\S]*rule 3|rule 3[\s\S]*rule 1|rule 2[\s\S]*rule 3/);
});

test('C133 is report-only: its findings are warnings', () => {
  assert.equal(C133.mode, 'report-only');
});
