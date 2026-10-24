import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { test } from 'node:test';
import { runHermetic } from '../src/hermetic.mjs';
import { findingsOf, makeApp, messages, TAURI_CONF } from './helpers.mjs';

const wf = (run, env = '') => `jobs:\n  t:\n    steps:\n      - name: Tests\n${env}        run: ${run}\n`;
const W = '.gitea/workflows/build.yml';

test('C14 static good/bad: wrapper or own HOME/TMPDIR vs bare cargo test', async () => {
  assert.deepEqual(await findingsOf('C14', makeApp({ [W]: wf('npx paddock-conformance run -- cargo test') })), []);
  assert.deepEqual(await findingsOf('C14', makeApp({ [W]: wf('cargo test', '        env:\n          HOME: /x\n          TMPDIR: /y\n') })), []);
  const f = await findingsOf('C14', makeApp({ [W]: wf('cargo test --workspace') }));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
});

const node = process.execPath;
const quiet = { ps: () => '' };

test('C14 hermetic run good: environment redirected, nothing left', async () => {
  const code = `const fs=require('fs');const o=process.env;if(!o.HOME.includes('paddock-hermetic-')||!o.TMPDIR.includes('paddock-hermetic-'))process.exit(9);const d=fs.mkdtempSync(o.TMPDIR+'/t-');fs.rmSync(d,{recursive:true})`;
  const r = await runHermetic(node, ['-e', code], quiet);
  assert.equal(r.status, 0);
  assert.deepEqual(r.findings, []);
  assert.equal(existsSync(r.scratch), false);
});

test('C14 hermetic run bad: temp leak, home write, and the exit status is passed through', async () => {
  const code = `const fs=require('fs');fs.writeFileSync(process.env.TMPDIR+'/leak.txt','x');fs.writeFileSync(process.env.HOME+'/.stray','x');process.exit(4)`;
  const r = await runHermetic(node, ['-e', code], quiet);
  assert.equal(r.status, 4);
  assert.match(messages(r.findings), /temp leak: 1 item/);
  assert.match(messages(r.findings), /wrote into the \(scratch\) home: \.stray/);
  const allowed = await runHermetic(node, ['-e', `require('fs').writeFileSync(process.env.HOME+'/.ok','x')`], { ...quiet, allowHome: ['.ok'] });
  assert.deepEqual(allowed.findings, []);
});

test('C14 hermetic run bad: a process left behind is reported and killed', { skip: process.platform === 'win32' }, async () => {
  const code = `require('child_process').spawn(process.execPath,['-e','setTimeout(()=>{},60000)'],{stdio:'ignore',detached:false}).unref()`;
  const r = await runHermetic(node, ['-e', code], quiet);
  assert.match(messages(r.findings), /still alive: process group/);
});

test('C14 a command that cannot start is a finding', async () => {
  const r = await runHermetic('/nonexistent/binary', [], quiet);
  assert.equal(r.status, 127);
  assert.match(messages(r.findings), /cannot start/);
});

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

