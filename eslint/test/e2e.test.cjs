'use strict';
const assert = require('node:assert/strict');
const { mkdirSync, mkdtempSync, writeFileSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { dirname, join } = require('node:path');
const { test, after } = require('node:test');
const { ESLint } = require('eslint');
const { defineConfig } = require('eslint/config');
const tseslint = require('typescript-eslint');
const { e2ePinnedClock } = require('../src/index.cjs');

const dir = mkdtempSync(join(tmpdir(), 'paddock-eslint-e2e-'));
after(() => rmSync(dir, { recursive: true, force: true }));

/** Lints `files` ({ path: text }) with the e2e block over the TypeScript parser (and `extra` config blocks); returns "path:line rule". */
async function lint(files, extra = [], ready = 'warn') {
  for (const [p, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, p)), { recursive: true });
    writeFileSync(join(dir, p), text);
  }
  const eslint = new ESLint({
    cwd: dir,
    overrideConfigFile: true,
    overrideConfig: defineConfig({ files: ['**/*.ts'], languageOptions: { parser: tseslint.parser } }, ...extra, ...e2ePinnedClock({ ready })),
  });
  const results = await eslint.lintFiles(Object.keys(files));
  return results.flatMap((r) => r.messages.map((m) => `${r.filePath.slice(dir.length + 1)}:${m.line} ${m.ruleId ?? 'parse'}`));
}
const RULE = 'paddock/e2e-pinned-clock';
const lines = (found, file) => found.filter((m) => m.startsWith(`${file}:`) && m.endsWith(RULE)).map((m) => Number(m.split(' ')[0].split(':')[1]));

test('a spec reaches test other than through the wrapper: value, alias, default, namespace, subpath and re-export forms', async () => {
  const found = await lint({
    'e2e/value.spec.ts': "import { expect, test } from '@playwright/test';\n",
    'e2e/alias.spec.ts': "import { test as base } from '@decryp7/paddock-e2e';\n",
    'e2e/default.spec.ts': "import pw from '@playwright/test';\n",
    'e2e/namespace.spec.ts': "import * as pw from '@playwright/test';\n",
    'e2e/subpath.spec.ts': "import { test } from '@playwright/test/lib/index';\n",
    'e2e/reexport.spec.ts': "export { test } from '@playwright/test';\n",
    'e2e/exportall.spec.ts': "export * from '@decryp7/paddock-e2e';\n",
  });
  for (const f of ['value', 'alias', 'default', 'namespace', 'subpath', 'reexport', 'exportall']) assert.deepEqual(lines(found, `e2e/${f}.spec.ts`), [1], `${f}: ${found.join(' | ')}`);
});

test('types, other names, the wrapper itself and a type position import() are fine', async () => {
  const found = await lint({
    'e2e/ok.spec.ts':
      "import type { Page, test as t } from '@playwright/test';\nimport { expect, type test as u } from '@playwright/test';\nimport { pressMod } from '@decryp7/paddock-e2e';\nimport { test } from './test';\nexport type P = import('@playwright/test').Page;\nexport type { test as v } from '@playwright/test';\nfunction f(p: import('@decryp7/paddock-e2e').Locator) { return p; }\n",
    'e2e/test.ts': "import { expect, test as base } from '@playwright/test';\nexport const test = base;\nexport { expect };\n",
  });
  assert.deepEqual(found, []);
});

test('a dynamic import() or require(), also by template literal or variable', async () => {
  const found = await lint({
    'e2e/dyn.spec.ts': [
      "const a = await import('@playwright/test');",
      "const b = require('@decryp7/paddock-e2e');",
      'const c = await import(`@playwright/test`);',
      'const d = await import(`@playwright/${x}`);',
      'const e = await import(name);',
      "const f = await import('@playwright/test/lib/x');",
      "const ok = await import('./other');",
      "const ok2 = require('node:fs');",
      '',
    ].join('\n'),
  });
  assert.deepEqual(lines(found, 'e2e/dyn.spec.ts'), [1, 2, 3, 4, 5, 6]);
});

test('clock.install with no time in any form, and newContext / newPage on a browser', async () => {
  const found = await lint({
    'e2e/pages.spec.ts': [
      'await page.clock.install();', // 1
      'await page.clock.install({});', // 2
      'await context.clock.install(undefined);', // 3
      'await page.clock.install(opts);', // 4
      'await page.clock.install({ time: undefined });', // 5
      'await page.clock.install({ time: T });', // 6 ok
      'await page.clock.install({ time: T, ...rest });', // 7 ok
      'await browser.newContext({});', // 8
      'await pwBrowser.newPage();', // 9
      'await helper.newContext({});', // 10
      'await ctx.newPage();', // 11 ok: a pinned context
      '',
    ].join('\n'),
  });
  assert.deepEqual(lines(found, 'e2e/pages.spec.ts'), [1, 2, 3, 4, 5, 8, 9, 10]);
});

test("an app's own no-restricted-syntax in the same files survives (rule options do not merge, the rule is its own)", async () => {
  const extra = [{ files: ['e2e/**/*.ts'], rules: { 'no-restricted-syntax': ['error', { selector: "Literal[value='raw-row']", message: 'an app rule' }] } }];
  const found = await lint({ 'e2e/both.spec.ts': "import { test } from '@playwright/test';\nconst s = 'raw-row';\n" }, extra);
  assert.ok(found.includes(`e2e/both.spec.ts:1 ${RULE}`), found.join(' | '));
  assert.ok(found.includes('e2e/both.spec.ts:2 no-restricted-syntax'), found.join(' | '));
});

const READY = 'paddock/e2e-ready-reads';
const readLines = (found, file) => found.filter((m) => m.startsWith(`${file}:`) && m.endsWith(READY)).map((m) => Number(m.split(' ')[0].split(':')[1]));

test('e2e-ready-reads: a read right after a bare visibility wait is reported (the sweep of 2026-11-17)', async () => {
  const found = await lint({
    'e2e/bad.spec.ts': [
      "import { expect, test } from './test';",
      "test('a', async ({ page }) => {",
      "  const options = page.getByRole('option');",
      '  await expect(options.first()).toBeVisible();',
      '  const texts = await options.allInnerTexts();',
      "  await page.locator('#a').waitFor();",
      "  const box = await page.locator('#a').boundingBox();",
      "  await page.locator('#b').waitFor({ state: 'visible' });",
      '  const n = await page.evaluate(() => 1);',
      '});',
      '',
    ].join('\n'),
  });
  assert.deepEqual(readLines(found, 'e2e/bad.spec.ts'), [5, 7, 9], found.join(' | '));
});

test('e2e-ready-reads: a ready helper or any other step between, a waiver, a click and a read in a function are fine', async () => {
  const found = await lint({
    'e2e/ok.spec.ts': [
      "import { expect, test } from './test';",
      "test('a', async ({ page }) => {",
      '  await expect(options.first()).toBeVisible();',
      '  await expectListFiltered(options);',
      '  const texts = await options.allInnerTexts();',
      '  await expect(options.first()).toBeVisible();',
      '  // conformance: e2e-ready — the list is static here, read once',
      '  const t2 = await options.allInnerTexts();',
      '  await expect(options.first()).toBeVisible();',
      '  await options.first().click();',
      '  await expect(options.first()).not.toBeVisible();',
      '  const t3 = await options.allInnerTexts();',
      '  await expect(options.first()).toBeVisible();',
      '  const f = async () => options.allInnerTexts();',
      '});',
      '',
    ].join('\n'),
  });
  assert.deepEqual(readLines(found, 'e2e/ok.spec.ts'), [], found.join(' | '));
});

test('e2e-ready-reads is off by default (report-only: the lint commands run with --max-warnings 0)', async () => {
  const found = await lint({ 'e2e/off.spec.ts': "test('a', async () => {\n  await expect(x).toBeVisible();\n  const t = await x.allInnerTexts();\n});\n" }, [], null);
  assert.deepEqual(readLines(found, 'e2e/off.spec.ts'), []);
});
