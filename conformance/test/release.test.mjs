import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { buildManifest } from '../release/latest-json.mjs';
import { exportLines, stripKey, strippedEnv } from '../release/strip-keys.mjs';
import { buildManifest as pitwallBuild } from '../fixtures/pitwall-latest-json.mjs';
import { makeApp } from './helpers.mjs';

const SHARED = fileURLToPath(new URL('../release/latest-json.mjs', import.meta.url));
const PITWALL = fileURLToPath(new URL('../fixtures/pitwall-latest-json.mjs', import.meta.url));
const STRIP = fileURLToPath(new URL('../release/strip-keys.mjs', import.meta.url));

function artifacts() {
  const dir = makeApp({
    'Demo_1.2.0_x64-setup.exe': 'exe',
    'Demo_1.2.0_x64-setup.exe.sig': 'c2lnLWV4ZQ==\n',
    'Demo_1.2.0_amd64.deb': 'deb',
    'Demo_1.2.0_amd64.deb.sig': 'c2lnLWRlYg==',
    'Demo.app.tar.gz': 'app',
    'Demo.app.tar.gz.sig': 'c2lnLWFwcA==',
    'Demo_1.2.0_x64_en-US.msi': 'msi',
    'RELEASE_NOTES.md': '# Notes\r\n- a\u0007\n',
    'Demo with space_1.2.0_amd64.AppImage': 'ai',
    'Demo with space_1.2.0_amd64.AppImage.sig': 'c2ln',
  });
  mkdirSync(join(dir, 'sub'));
  writeFileSync(join(dir, 'sub', 'Demo_1.2.0_aarch64-setup.exe'), 'arm');
  writeFileSync(join(dir, 'sub', 'Demo_1.2.0_aarch64-setup.exe.sig'), 'c2ln');
  return dir;
}

test('latest-json: the manifest equals Pitwall\'s for the same input, byte for byte', () => {
  const input = {
    version: 'v1.2.0', notes: 'Fixes\r\n- grid\u0007', pubDate: '2026-10-17T12:00:00Z', baseUrl: 'https://h/o/r/releases/download/v1.2.0/', macArch: 'x86_64',
    files: ['Pitwall_1.2.0_x64-setup.exe', 'Pitwall_1.2.0_amd64.deb', 'Pitwall.app.tar.gz', 'Pitwall 1.2.0_arm64-setup.exe', 'Pitwall_1.2.0_x64_en-US.msi'].map((name) => ({ name, size: 7, signature: ' c2ln\n' })),
  };
  assert.equal(JSON.stringify(buildManifest(input), null, 2), JSON.stringify(pitwallBuild(input), null, 2));
});

test('latest-json CLI: the written latest.json equals Pitwall\'s (pub_date aside)', () => {
  const dir = artifacts();
  const run = (script, out) =>
    execFileSync(process.execPath, [script, '--dir', dir, '--version', '1.2.0', '--base-url', 'https://h/o/r/releases/download/v1.2.0', '--notes', join(dir, 'RELEASE_NOTES.md'), '--out', out], { encoding: 'utf8' });
  const mine = join(makeApp({ x: '' }), 'mine.json');
  const theirs = join(makeApp({ x: '' }), 'theirs.json');
  assert.equal(run(SHARED, mine).replace(mine, 'OUT'), run(PITWALL, theirs).replace(theirs, 'OUT'));
  const strip = (p) => readFileSync(p, 'utf8').replace(/"pub_date": "[^"]+"/, '"pub_date": "T"');
  assert.equal(strip(mine), strip(theirs));
  assert.ok(Object.keys(JSON.parse(readFileSync(mine, 'utf8')).platforms).length >= 5);
});

test('strip-keys: whitespace removed, only the two key names, unset names skipped', () => {
  assert.equal(stripKey('  ab c\r\nd\te\n'), 'abcde');
  const env = { TAURI_SIGNING_PRIVATE_KEY: 'AA\nBB\n', TAURI_UPDATER_PUBKEY: ' CC ', TAURI_SIGNING_PRIVATE_KEY_PASSWORD: 'a b', OTHER: 'x' };
  assert.deepEqual(strippedEnv(env), { TAURI_SIGNING_PRIVATE_KEY: 'AABB', TAURI_UPDATER_PUBKEY: 'CC' });
  assert.deepEqual(strippedEnv({ TAURI_UPDATER_PUBKEY: '\n' }), {});
  assert.deepEqual(exportLines({ A: 'x' }), ["export A='x'"]);
  assert.throws(() => exportLines({ A: "x'y" }), /quote/);
});

test('strip-keys CLI: eval form and --append-to form', () => {
  const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: 'AA\nBB\n', TAURI_UPDATER_PUBKEY: 'CC DD' };
  assert.equal(execFileSync(process.execPath, [STRIP], { env, encoding: 'utf8' }), "export TAURI_SIGNING_PRIVATE_KEY='AABB'\nexport TAURI_UPDATER_PUBKEY='CCDD'\n");
  const file = join(makeApp({ x: '' }), 'env');
  execFileSync(process.execPath, [STRIP, '--append-to', file], { env });
  assert.equal(readFileSync(file, 'utf8'), 'TAURI_SIGNING_PRIVATE_KEY=AABB\nTAURI_UPDATER_PUBKEY=CCDD\n');
  const sh = execFileSync('bash', ['-c', `eval "$(node ${STRIP})"; printf '%s' "$TAURI_UPDATER_PUBKEY"`], { env, encoding: 'utf8' });
  assert.equal(sh, 'CCDD');
});
