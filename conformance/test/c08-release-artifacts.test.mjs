import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { findingsOf, makeApp, messages, TAURI_CONF } from './helpers.mjs';

const shared = (n) => readFileSync(new URL(`../release/${n}`, import.meta.url), 'utf8');
const base = { 'app/src-tauri/tauri.conf.json': TAURI_CONF('1.2.3') };

test('C08 good: app copies equal the shared scripts (comment headers may differ)', async () => {
  const withHeader = (n) => shared(n).replace(/^\/\/.*$/m, "// Demo's own header comment");
  const f = await findingsOf('C08', makeApp({ ...base, 'scripts/release/publish.mjs': withHeader('publish.mjs'), 'scripts/release/latest-json.mjs': withHeader('latest-json.mjs') }));
  assert.deepEqual(f, []);
});

test('C08 bad: a drifted copy', async () => {
  const f = await findingsOf('C08', makeApp({ ...base, 'scripts/release/latest-json.mjs': `${shared('latest-json.mjs')}\nconsole.log('local tweak');\n` }));
  assert.match(messages(f), /scripts\/release\/latest-json\.mjs differs from the shared latest-json\.mjs/);
});

const good = {
  'Demo_1.2.3_aarch64.dmg': 'x',
  'Demo_1.2.3_x64-setup.exe': 'x',
  'RELEASE_NOTES.md': '# Demo 1.2.3\n\n## First launch\n\nRight-click Open.\n',
  'latest.json': JSON.stringify({ version: '1.2.3', platforms: { 'windows-x86_64': { url: 'https://h/o/r/Demo_1.2.3_x64-setup.exe', signature: 'c2ln' } } }),
};

test('C08 artifacts good', async () => {
  const dir = makeApp(good);
  assert.deepEqual(await findingsOf('C08', makeApp(base), { options: { artifacts: dir } }), []);
});

test('C08 artifacts bad: stale installer, no First launch, http url, unsigned', async () => {
  const dir = makeApp({
    ...good,
    'Demo_1.2.2_x64-setup.exe': 'old',
    'RELEASE_NOTES.md': '# Demo\n',
    'latest.json': JSON.stringify({ version: '1.2.2', platforms: { 'windows-x86_64': { url: 'http://h/x.exe', signature: '' } } }),
  });
  const m = messages(await findingsOf('C08', makeApp(base), { options: { artifacts: dir } }));
  assert.match(m, /Demo_1\.2\.2_x64-setup\.exe is version 1\.2\.2/);
  assert.match(m, /no "First launch" section/);
  assert.match(m, /latest\.json says 1\.2\.2/);
  assert.match(m, /not https/);
  assert.match(m, /no signature/);
});

test('C08 artifacts fail closed: empty or missing folder', async () => {
  assert.match(messages(await findingsOf('C08', makeApp(base), { options: { artifacts: makeApp({ 'x/.keep': '' }).concat('/none') } })), /cannot read the artifacts folder/);
});
