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

/** The ESLint results of the last `lint` call (messages with their severity). */
let lintResults = [];
const dir = mkdtempSync(join(tmpdir(), 'paddock-eslint-e2e-'));
after(() => rmSync(dir, { recursive: true, force: true }));

/** Lints `files` ({ path: text }) with the e2e block over the TypeScript parser (and `extra` config blocks); returns "path:line rule". */
async function lint(files, extra = [], ready = undefined, transient = 'warn', controlOrMeta = undefined) {
  for (const [p, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, p)), { recursive: true });
    writeFileSync(join(dir, p), text);
  }
  const eslint = new ESLint({
    cwd: dir,
    overrideConfigFile: true,
    overrideConfig: defineConfig({ files: ['**/*.ts'], languageOptions: { parser: tseslint.parser } }, ...extra, ...e2ePinnedClock({ ready, transient, controlOrMeta })),
  });
  const results = await eslint.lintFiles(Object.keys(files));
  lintResults = results;
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

test('e2e-ready-reads: a ready helper or any other step between, a click and a read in a function are fine', async () => {
  const found = await lint({
    'e2e/ok.spec.ts': [
      "import { expect, test } from './test';",
      "test('a', async ({ page }) => {",
      '  await expect(options.first()).toBeVisible();',
      '  await expectListFiltered(options);',
      '  const texts = await options.allInnerTexts();',
      '  await expect(options.first()).toBeVisible();',
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

test('e2e-ready-reads is an error by default (the lint commands run with --max-warnings 0, so every app change is blocked)', async () => {
  await lint({ 'e2e/err.spec.ts': "test('a', async () => {\n  await expect(x).toBeVisible();\n  const t = await x.allInnerTexts();\n});\n" });
  const m = lintResults[0].messages.find((x) => x.ruleId === READY);
  assert.ok(m, 'the read is reported');
  assert.equal(m.severity, 2, 'severity 2 is an error');
});

test('e2e-ready-reads: the rule can be switched off by its option, which the wiring test then catches in every real config', async () => {
  const found = await lint({ 'e2e/off.spec.ts': "test('a', async () => {\n  await expect(x).toBeVisible();\n  const t = await x.allInnerTexts();\n});\n" }, [], 'off');
  assert.deepEqual(readLines(found, 'e2e/off.spec.ts'), []);
});

/** A spec whose unready read is preceded by `between` (an inline-config comment, as lines) right after the visibility wait. */
const specWith = (between) => ["test('a', async () => {", '  await expect(x).toBeVisible();', ...between, '  const t = await x.allInnerTexts();', '});', ''].join('\n');

// ESLint's own grammar decides what an inline comment means, and we do not parse it: the e2e block sets `noInlineConfig`, so every inline
// config comment is ignored (and reported), and the read is still an error. One test per variant that switched the rule off before.
const INLINE = {
  'a disable-next-line directive with a description': ['  // eslint-disable-next-line paddock/e2e-ready-reads -- immediate by design'],
  'an inline config comment that turns the rule off': ['  /* eslint paddock/e2e-ready-reads: "off" */'],
  'an inline config comment with severity 0': ['  /* eslint paddock/e2e-ready-reads: 0 */'],
  'a multi-line block comment with the directive on its second line': ['  /*', '     eslint-disable paddock/e2e-ready-reads', '  */'],
  'a tab before the dashes of the description': ['  // eslint-disable-next-line paddock/e2e-ready-reads\t-- why'],
  'three dashes': ['  // eslint-disable-next-line paddock/e2e-ready-reads --- why'],
  'dashes then a tab': ['  // eslint-disable-next-line paddock/e2e-ready-reads --\twhy'],
  'a block disable of the whole file': ['  /* eslint-disable */'],
};
for (const [name, before] of Object.entries(INLINE)) {
  test(`e2e-ready-reads cannot be switched off inline: ${name}`, async () => {
    const found = await lint({ 'e2e/inline.spec.ts': specWith(before) });
    assert.deepEqual(readLines(found, 'e2e/inline.spec.ts').length, 1, `${name}: the unready read is still reported: ${found.join(' | ')}`);
    const m = lintResults[0].messages.find((x) => x.ruleId === READY);
    assert.equal(m.severity, 2);
  });
}

test('noInlineConfig is for the e2e files only: a disable comment outside them still works', async () => {
  const found = await lint({ 'src/app.ts': '/* eslint-disable no-debugger */\ndebugger;\n', 'e2e/dbg.spec.ts': '/* eslint-disable no-debugger */\ndebugger;\n' }, [{ files: ['**/*.ts'], rules: { 'no-debugger': 'error' } }]);
  assert.deepEqual(found.filter((m) => m.startsWith('src/app.ts:') && m.endsWith('no-debugger')), []);
  assert.equal(found.filter((m) => m.startsWith('e2e/dbg.spec.ts:') && m.endsWith('no-debugger')).length, 1, 'in an e2e spec the disable is ignored and the rule fires');
});

test('e2e-ready-reads: the directive text inside a string literal is not a directive and not a finding of its own (no false positive)', async () => {
  const found = await lint({
    'e2e/literal.spec.ts': ["test('a', async () => {", "  const s = '// eslint-disable-next-line paddock/e2e-ready-reads -- not a directive';", '  await expectNotMoving(x);', '  const t = await x.allInnerTexts();', '});', ''].join('\n'),
  });
  assert.deepEqual(found, [], found.join(' | '));
});

test('e2e-ready-reads: evaluate and evaluateAll are reads only when their result is used', async () => {
  const found = await lint({
    'e2e/eval.spec.ts': [
      "import { expect, test } from './test';",
      "test('a', async ({ page }) => {",
      '  await expect(x).toBeVisible();',
      '  await page.evaluate(() => window.__ready);',
      '  await expect(x).toBeVisible();',
      '  await x.evaluate((e) => e.click());',
      '  await expect(x).toBeVisible();',
      '  await page.evaluateHandle(() => 1);',
      '  await expect(x).toBeVisible();',
      '  const n = await x.evaluate((e) => e.scrollTop);',
      '  await expect(x).toBeVisible();',
      '  expect(await x.evaluateAll((e) => e.length)).toBe(2);',
      '  await expect(x).toBeVisible();',
      '  return page.evaluate(() => 1);',
      '});',
      '',
    ].join('\n'),
  });
  assert.deepEqual(readLines(found, 'e2e/eval.spec.ts'), [10, 12, 14], found.join(' | '));
});

const MENU = 'paddock/e2e-transient-wait';
const menuLines = (found, file) => found.filter((m) => m.startsWith(`${file}:`) && m.endsWith(MENU)).map((m) => Number(m.split(' ')[0].split(':')[1]));

test('e2e-transient-wait: a spec that finds a menu or a toast itself is reported; the helper file of that kind may (runs 8297, 8300)', async () => {
  const found = await lint({
    'e2e/menus.spec.ts': [
      "import { expect, test } from './test';",
      "test('a', async ({ page }) => {",
      "  await page.getByRole('menu', { name: 'Actions' }).getByRole('menuitem').first().click();",
      "  const m = page.getByRole('menu');",
      "  const ok = page.getByRole('menubar');",
      "  const item = page.getByRole('menuitem', { name: 'Copy' });",
      "  await page.getByTestId('notification').filter({ hasText: 'Done' }).click();",
      "  const n = page.getByTestId('notification-action-primary');",
      "  const c = page.locator('[data-testid=notification][data-cause=\"x\"]');",
      "  const m = page.locator('[role=menu] [role=menuitem]');",
      "  const d = page.locator('[data-testid=notification-details]');",
      '});',
      '',
    ].join('\n'),
    'e2e/menu-support.ts': "export const menuNamed = (page) => page.getByRole('menu');\nexport const t = (page) => page.getByTestId('notification');\n",
    'e2e/toast-support.ts': "export const toastNamed = (page) => page.getByTestId('notification');\n",
  });
  assert.deepEqual(menuLines(found, 'e2e/menus.spec.ts'), [3, 4, 7, 9, 10], found.join(' | '));
  assert.deepEqual(menuLines(found, 'e2e/menu-support.ts'), [2], found.join(' | '));
  assert.deepEqual(menuLines(found, 'e2e/toast-support.ts'), [], found.join(' | '));
});

test('e2e-transient-wait is off by default (report-only, ER-41)', async () => {
  const found = await lint({ 'e2e/menu-off.spec.ts': "test('a', async ({ page }) => {\n  page.getByRole('menu');\n});\n" }, [], null, null);
  assert.deepEqual(menuLines(found, 'e2e/menu-off.spec.ts'), []);
});

const HOST_KEY = 'paddock/e2e-no-control-or-meta';
const hostKeyLines = (found, file) => found.filter((m) => m.startsWith(`${file}:`) && m.endsWith(HOST_KEY)).map((m) => Number(m.split(' ')[0].split(':')[1]));

test('e2e-no-control-or-meta: the host-following key is reported in a press, a click modifier and a template; pressMod and pageModKey are fine (positive control)', async () => {
  const found = await lint({
    'e2e/keys.spec.ts': [
      "await page.keyboard.press('ControlOrMeta+k');", // 1
      "await row.click({ modifiers: ['ControlOrMeta'] });", // 2
      'await input.press(`ControlOrMeta+Enter`);', // 3
      "await pressMod(page, 'Mod+k');", // 4 ok
      'await input.press(`${await pageModKey(page)}+Enter`);', // 5 ok
      "await page.keyboard.press('Control+k');", // 6 ok: a literal key is the spec's choice
      '',
    ].join('\n'),
  });
  assert.deepEqual(hostKeyLines(found, 'e2e/keys.spec.ts'), [1, 2, 3]);
  assert.ok(lintResults[0].messages.every((m) => m.severity === 2), 'an error, not a warning');
});

test('e2e-no-control-or-meta: the severity can be lowered by the config (report-only use in an app that is not clean yet)', async () => {
  const found = await lint({ 'e2e/off.spec.ts': "await page.keyboard.press('ControlOrMeta+k');\n" }, [], undefined, 'warn', 'off');
  assert.deepEqual(hostKeyLines(found, 'e2e/off.spec.ts'), []);
});
