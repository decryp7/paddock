import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages, TAURI_CONF } from './helpers.mjs';

const wf = (run, env = '') => `jobs:\n  t:\n    steps:\n      - name: Tests\n${env}        run: ${run}\n`;
const W = '.gitea/workflows/build.yml';

test('C15 good/bad: pinned browser vs installed Chrome in specs and scripts', async () => {
  assert.deepEqual(await findingsOf('C15', makeApp({ 'app/e2e/a.spec.ts': "const p = process.env.DEMO_TEST_BROWSER;\n", 'app/src/p.spec.ts': "expect(name).toBe('chrome.exe');\n" })), []);
  const f = await findingsOf('C15', makeApp({
    'app/e2e/a.spec.ts': "launch({ channel: 'chrome' });\n",
    'scripts/run.sh': 'open "/Applications/Google Chrome.app"\n',
    'app/src/product.ts': "const c = '/Applications/Google Chrome.app';\n",
  }));
  assert.deepEqual(f.map((x) => x.file).sort(), ['app/e2e/a.spec.ts', 'scripts/run.sh']);
});

test('C15 warns when Playwright runs without REQUIRE_TEST_BROWSER', async () => {
  const f = await findingsOf('C15', makeApp({ 'app/playwright.config.ts': 'export default {}', [W]: wf('npx playwright test') }));
  assert.equal(f[0].severity, 'warn');
  assert.deepEqual(await findingsOf('C15', makeApp({ 'app/playwright.config.ts': 'export default {}', [W]: wf('npx playwright test', '        env:\n          DEMO_REQUIRE_TEST_BROWSER: "1"\n') })), []);
});

const spec = (line) => ({ 'app/src-tauri/tauri.conf.json': TAURI_CONF('0.4.2'), 'app/src/app/settings.spec.ts': `${line}\n` });

test('C16 good: the version is read from its source', async () => {
  assert.deepEqual(await findingsOf('C16', makeApp(spec("expect(about.textContent).toMatch(/Version \\d+\\.\\d+\\.\\d+/); const v = APP_VERSION;"))), []);
  // Another thing that merely has the same number is not the app version.
  assert.deepEqual(await findingsOf('C16', makeApp(spec("const plugin = { id: 'x', version: '0.4.2' };"))), []);
});

test('C16 bad: spec asserts the literal; mock assigns it', async () => {
  const f = await findingsOf('C16', makeApp(spec("expect(el.textContent).toContain('Version 0.4.2');")));
  assert.equal(f.length, 1);
  assert.equal(f[0].file, 'app/src/app/settings.spec.ts');
  const m = await findingsOf('C16', makeApp({ ...spec(''), 'app/src/app/core/mock-backend.ts': "reply('appInfo', () => ({ version: '0.4.2', os: 'mac' }));\n" }));
  assert.equal(m.length, 1);
  assert.match(m[0].message, /hard-codes the app version 0\.4\.2/);
});

test('C16 fails closed when the version cannot be read', async () => {
  assert.match(messages(await findingsOf('C16', makeApp({ 'a.txt': 'x' }))), /cannot read the app version/);
});

