import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { executableFrom, parseArgs, setupEnv, stage } from './stage-paddock-setup.mjs';

const conf = { productName: 'Dyno', identifier: 'net.decryptology.dyno', version: '1.1.0', mainBinaryName: 'dyno-app' };

test('the build environment names the app by its Tauri configuration and carries the updater key', () => {
  assert.deepEqual(setupEnv(conf, 'KEY=='), {
    PADDOCK_SETUP_IDENTIFIER: 'net.decryptology.dyno',
    PADDOCK_SETUP_PRODUCT: 'Dyno',
    PADDOCK_SETUP_MAIN: 'dyno-app.exe',
    PADDOCK_SETUP_VERSION: '1.1.0',
    PADDOCK_SETUP_UPDATE_KEY: 'KEY==',
  });
  assert.equal(setupEnv(conf, '').PADDOCK_SETUP_UPDATE_KEY, undefined, 'no key: a build that refuses apply-update, not an empty key');
  assert.throws(() => setupEnv({ ...conf, identifier: '' }, ''), /identifier/);
  assert.throws(() => setupEnv({ ...conf, mainBinaryName: undefined }, ''), /mainBinaryName/);
});

test('the executable is the bin artifact of paddock-setup in cargo json messages', () => {
  const lines = [
    '{"reason":"compiler-artifact","target":{"name":"paddock-workspace-hack","kind":["lib"]},"executable":null}',
    'not json at all',
    '{"reason":"compiler-artifact","target":{"name":"paddock-setup","kind":["lib"]},"executable":null}',
    '{"reason":"compiler-artifact","target":{"name":"paddock-setup","kind":["bin"]},"executable":"C:\\\\t\\\\release\\\\paddock-setup.exe"}',
    '{"reason":"build-finished","success":true}',
  ].join('\n');
  assert.equal(executableFrom(lines), 'C:\\t\\release\\paddock-setup.exe');
  assert.throws(() => executableFrom('{"reason":"build-finished","success":true}'), /no paddock-setup/);
});

test('arguments: the app is needed, the target and cargo folder are optional, nothing else is accepted', () => {
  assert.deepEqual(parseArgs(['--app', 'dyno']), { app: 'dyno', target: undefined, root: undefined });
  assert.deepEqual(parseArgs(['--app', 'dyno', '--target', 'x86_64-pc-windows-msvc']).target, 'x86_64-pc-windows-msvc');
  assert.throws(() => parseArgs([]), /--app/);
  assert.throws(() => parseArgs(['--app', 'dyno', '--bogus']), /unknown/);
  assert.throws(() => parseArgs(['--app']), /needs a value/);
});

test('staging fails closed without the paddock-setup crate, copies the built program next to the hooks otherwise', () => {
  const root = mkdtempSync(join(tmpdir(), 'stage-'));
  const src = join(root, 'apps/dyno/app/src-tauri');
  mkdirSync(join(src, 'windows'), { recursive: true });
  writeFileSync(join(src, 'tauri.conf.json'), JSON.stringify(conf));
  const logs = [];
  const run = (cmd, args, opts) => {
    logs.push({ cmd, args, env: opts.env, cwd: opts.cwd });
    writeFileSync(join(root, 'built.exe'), 'MZ');
    return `{"reason":"compiler-artifact","target":{"name":"paddock-setup","kind":["bin"]},"executable":${JSON.stringify(join(root, 'built.exe'))}}\n`;
  };
  assert.throws(() => stage({ app: 'dyno', root, run, env: {} }), /crates\/paddock-setup is not in this tree/);
  mkdirSync(join(root, 'crates/paddock-setup'), { recursive: true });
  writeFileSync(join(root, 'crates/paddock-setup/Cargo.toml'), '[package]\n');
  const out = stage({ app: 'dyno', root, run, env: { TAURI_UPDATER_PUBKEY: 'PUB' } });
  assert.equal(out, join(src, 'windows/paddock-setup.exe'));
  assert.equal(readFileSync(out, 'utf8'), 'MZ');
  assert.equal(logs[0].cmd, 'cargo');
  assert.deepEqual(logs[0].args.slice(0, 5), ['build', '-p', 'paddock-setup', '--features', 'proc-stop']);
  assert.ok(logs[0].args.includes('--release') && logs[0].args.includes('--message-format=json-render-diagnostics'));
  assert.equal(logs[0].cwd, join(root, 'crates'));
  assert.equal(logs[0].env.PADDOCK_SETUP_UPDATE_KEY, 'PUB');
  assert.equal(logs[0].env.PADDOCK_SETUP_IDENTIFIER, 'net.decryptology.dyno');
  const t = stage({ app: 'dyno', root, run, env: {}, target: 'x86_64-pc-windows-msvc' });
  assert.ok(t);
  assert.deepEqual(logs[1].args.slice(-2), ['--target', 'x86_64-pc-windows-msvc']);
  assert.throws(() => stage({ app: 'nope', root, run, env: {} }), /no such app/);
});
