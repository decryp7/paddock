'use strict';
const assert = require('node:assert/strict');
const { mkdirSync, mkdtempSync, writeFileSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { dirname, join } = require('node:path');
const { test, after } = require('node:test');
const { ESLint } = require('eslint');
const tseslint = require('typescript-eslint');
const { e2ePinnedClock } = require('../src/index.cjs');

const dir = mkdtempSync(join(tmpdir(), 'paddock-eslint-e2e-'));
after(() => rmSync(dir, { recursive: true, force: true }));

/** Lints `files` ({ path: text }) with the e2e block over a plain TypeScript parser; returns "path:line rule" per message. */
async function lint(files) {
  for (const [p, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, p)), { recursive: true });
    writeFileSync(join(dir, p), text);
  }
  const eslint = new ESLint({
    cwd: dir,
    overrideConfigFile: true,
    overrideConfig: [{ files: ['**/*.ts'], languageOptions: { parser: tseslint.parser }, plugins: { '@typescript-eslint': tseslint.plugin } }, ...e2ePinnedClock()],
  });
  const results = await eslint.lintFiles(Object.keys(files));
  return results.flatMap((r) => r.messages.map((m) => `${r.filePath.slice(dir.length + 1)}:${m.line} ${m.ruleId ?? 'parse'}`));
}

test('a spec reaches test other than through the wrapper: value, alias, default, namespace and re-export forms', async () => {
  const found = await lint({
    'e2e/value.spec.ts': "import { expect, test } from '@playwright/test';\n",
    'e2e/alias.spec.ts': "import { test as base } from '@decryp7/paddock-e2e';\n",
    'e2e/default.spec.ts': "import pw from '@playwright/test';\n",
    'e2e/namespace.spec.ts': "import * as pw from '@playwright/test';\n",
    'e2e/reexport.spec.ts': "export { test } from '@playwright/test';\n",
    'e2e/exportall.spec.ts': "export * from '@decryp7/paddock-e2e';\n",
  });
  for (const f of ['value', 'alias', 'default', 'namespace', 'reexport', 'exportall']) {
    assert.ok(found.some((m) => m.startsWith(`e2e/${f}.spec.ts:1 @typescript-eslint/no-restricted-imports`)), `${f}: ${found.join(' | ')}`);
  }
});

test('types, other names, the wrapper itself and a type position import() are fine', async () => {
  const found = await lint({
    'e2e/ok.spec.ts': "import type { Page } from '@playwright/test';\nimport { expect } from '@playwright/test';\nimport { pressMod } from '@decryp7/paddock-e2e';\nimport { test } from './test';\nexport type P = import('@playwright/test').Page;\nfunction f(p: import('@decryp7/paddock-e2e').Locator) { return p; }\n",
    'e2e/test.ts': "import { expect, test as base } from '@playwright/test';\nexport const test = base;\nexport { expect };\n",
  });
  assert.deepEqual(found, []);
});

test('a dynamic import(), a require(), a bare or empty clock.install and browser.newContext/newPage', async () => {
  const found = await lint({
    'e2e/syntax.spec.ts': [
      "const a = await import('@playwright/test');",
      "const b = require('@decryp7/paddock-e2e');",
      'await page.clock.install();',
      'await page.clock.install({});',
      'await context.clock.install(undefined);',
      'await browser.newContext({});',
      'await browser.newPage();',
      'await page.clock.install({ time: T });',
      "const ok = await import('./other');",
      '',
    ].join('\n'),
  });
  assert.deepEqual(
    found.map((m) => m.split(' ')[0]),
    ['e2e/syntax.spec.ts:1', 'e2e/syntax.spec.ts:2', 'e2e/syntax.spec.ts:3', 'e2e/syntax.spec.ts:4', 'e2e/syntax.spec.ts:5', 'e2e/syntax.spec.ts:6', 'e2e/syntax.spec.ts:7'],
  );
  assert.ok(found.every((m) => m.endsWith('no-restricted-syntax')));
});
