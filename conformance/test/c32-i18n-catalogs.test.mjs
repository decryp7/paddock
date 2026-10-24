import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { runPaddockI18n } from '../src/checks/C32-i18n-catalogs.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const BIN = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'ui', 'projects', 'paddock-ui', 'bin');
const CONFIG = JSON.stringify({
  catalogs: [{ out: 'locale/messages.xlf', prefix: 'ap.', src: ['src'] }],
  locales: [{ locale: 'en-XA', out: 'locale/messages.en-XA.json', inputs: ['locale/messages.xlf'] }],
});

/** An app with an installed copy of the real paddock-i18n (the files of this checkout), its sources and config. */
function appWith(extra = {}) {
  const root = makeApp({ 'app/paddock-i18n.json': CONFIG, 'app/src/a.ts': "export const A = msg('ap.menu', 'Menu');\n", ...extra });
  const bin = join(root, 'app/node_modules/@decryp7/paddock-ui/bin');
  mkdirSync(bin, { recursive: true });
  for (const f of ['paddock-i18n.mjs', 'i18n-build.mjs', 'i18n-messages.mjs']) copyFileSync(join(BIN, f), join(bin, f));
  return root;
}

const build = (root) => runPaddockI18n({ root, configFile: join(root, 'app/paddock-i18n.json'), appDir: 'app' });
const generate = (root) => {
  const r = runPaddockI18nWrite(root);
  assert.equal(r.status, 0, r.output);
};
function runPaddockI18nWrite(root) {
  const r = spawnSync(process.execPath, [join(root, 'app/node_modules/@decryp7/paddock-ui/bin/paddock-i18n.mjs'), 'build', '--no-extract', '--config', join(root, 'app/paddock-i18n.json')], { encoding: 'utf8' });
  return { status: r.status, output: `${r.stdout}${r.stderr}` };
}

test('C32 good: freshly generated catalogs pass, through the real paddock-i18n', async () => {
  const root = appWith();
  generate(root);
  assert.equal(build(root).status, 0);
  assert.deepEqual(await findingsOf('C32', root), []);
});

test('C32 bad: a string added to the sources without regenerating, and a hand-edited runtime file, are errors', async () => {
  const root = appWith();
  generate(root);
  writeFileSync(join(root, 'app/src/b.ts'), "export const B = msg('ap.new', 'New');\n");
  let f = await findingsOf('C32', root);
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.match(messages(f), /messages\.xlf is out of date/);
  generate(root);
  writeFileSync(join(root, 'app/locale/messages.en-XA.json'), '{}\n');
  f = await findingsOf('C32', root);
  assert.match(messages(f), /messages\.en-XA\.json is out of date/);
});

test('C32 warns, never fails, when the app declares no build or its tool is not installed', async () => {
  const none = await findingsOf('C32', makeApp({ 'app/package.json': '{}' }));
  assert.equal(none.length, 1);
  assert.equal(none[0].severity, 'warn');
  assert.match(messages(none), /no paddock-i18n\.json/);
  const bare = makeApp({ 'app/paddock-i18n.json': CONFIG });
  const f = await findingsOf('C32', bare);
  assert.equal(f[0].severity, 'warn');
  assert.match(messages(f), /not installed/);
});

test('C32 takes an injected runner and a configured config path', async () => {
  const root = makeApp({ 'web/i18n.json': CONFIG });
  const calls = [];
  const options = { i18n: (a) => (calls.push(a.configFile), { status: 1, output: 'paddock-i18n: x.xlf is out of date: regenerate it' }) };
  const f = await findingsOf('C32', root, { options });
  assert.match(messages(f), /no paddock-i18n\.json/);
  assert.equal(calls.length, 0);
  writeFileSync(join(root, 'paddock.conformance.json'), JSON.stringify({ i18n: { config: 'web/i18n.json' } }));
  const g = await findingsOf('C32', root, { options });
  assert.equal(g[0].severity, 'error');
  assert.match(g[0].message, /x\.xlf is out of date/);
  assert.equal(calls.length, 1);
});
