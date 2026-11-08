import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { assertNoRustflagsEnv, cleanKey, executableFrom, isRelease, parseArgs, remapConfig, setupEnv, stage } from './stage-paddock-setup.mjs';

const conf = { productName: 'Dyno', identifier: 'net.decryptology.dyno', version: '1.1.0', mainBinaryName: 'dyno-app' };

test('the build environment names the app by its Tauri configuration and carries the updater key', () => {
  assert.deepEqual(setupEnv(conf, 'KEY=='), {
    PADDOCK_SETUP_IDENTIFIER: 'net.decryptology.dyno',
    PADDOCK_SETUP_PRODUCT: 'Dyno',
    PADDOCK_SETUP_MAIN: 'dyno-app.exe',
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
  assert.deepEqual(parseArgs(['--app', 'dyno']), { app: 'dyno', target: undefined, root: undefined, requireKey: false });
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
  // No `--features`: the stopper is on by the build.rs cfg `has_proc_installed`; a feature that does not exist fails the build.
  assert.deepEqual(logs[0].args.slice(0, 5), ['build', '-p', 'paddock-setup', '--release', '--message-format=json-render-diagnostics']);
  assert.equal(logs[0].args[5], '--config');
  assert.match(logs[0].args[6], /^target\.'cfg\(windows\)'\.rustflags=\['--remap-path-prefix=.*=\/home', '--remap-path-prefix=.*\.cargo=\/cargo', '--remap-path-prefix=.*=\/paddock'\]$/);
  assert.equal(logs[0].cwd, join(root, 'crates'));
  assert.equal(logs[0].env.PADDOCK_SETUP_UPDATE_KEY, 'PUB');
  assert.equal(logs[0].env.PADDOCK_SETUP_IDENTIFIER, 'net.decryptology.dyno');
  const t = stage({ app: 'dyno', root, run, env: {}, target: 'x86_64-pc-windows-msvc' });
  assert.ok(t);
  assert.deepEqual(logs[1].args.slice(0, 7), ['build', '-p', 'paddock-setup', '--release', '--message-format=json-render-diagnostics', '--target', 'x86_64-pc-windows-msvc']);
  assert.throws(() => stage({ app: 'nope', root, run, env: {} }), /no such app/);
});

test('the key is stripped of every whitespace like the bundle action does, and a release needs one', () => {
  assert.equal(cleanKey(' AB \r\nCD\t'), 'ABCD');
  assert.equal(cleanKey(undefined), '');
  assert.equal(isRelease({ GITHUB_REF: 'refs/tags/dyno-v1.1.0' }, false), true);
  assert.equal(isRelease({ GITEA_REF: 'refs/tags/x' }, false), true);
  assert.equal(isRelease({ GITHUB_REF: 'refs/heads/main' }, false), false);
  assert.equal(isRelease({}, true), true, '--require-key');
  assert.deepEqual(parseArgs(['--app', 'dyno', '--require-key']).requireKey, true);
  const root = mkdtempSync(join(tmpdir(), 'stage-'));
  const src = join(root, 'apps/dyno/app/src-tauri');
  mkdirSync(join(src, 'windows'), { recursive: true });
  mkdirSync(join(root, 'crates/paddock-setup'), { recursive: true });
  writeFileSync(join(root, 'crates/paddock-setup/Cargo.toml'), '[package]\n');
  writeFileSync(join(src, 'tauri.conf.json'), JSON.stringify(conf));
  let seen;
  const run = (c, a, o) => {
    seen = o.env.PADDOCK_SETUP_UPDATE_KEY;
    writeFileSync(join(root, 'b.exe'), 'MZ');
    return `{"reason":"compiler-artifact","target":{"name":"paddock-setup","kind":["bin"]},"executable":${JSON.stringify(join(root, 'b.exe'))}}\n`;
  };
  stage({ app: 'dyno', root, run, env: { TAURI_UPDATER_PUBKEY: 'AB\nCD ' } });
  assert.equal(seen, 'ABCD');
  assert.throws(() => stage({ app: 'dyno', root, run, env: { GITHUB_REF: 'refs/tags/dyno-v1.1.0' } }), /no updater public key.*TAURI_UPDATER_PUBKEY/s);
  assert.throws(() => stage({ app: 'dyno', root, run, env: {}, requireKey: true }), /no updater public key/);
  assert.doesNotThrow(() => stage({ app: 'dyno', root, run, env: { GITHUB_REF: 'refs/tags/x', TAURI_UPDATER_PUBKEY: 'K' } }));
});

test("the program's bytes do not depend on the app release: no app version in its build, no test override, no incremental state", () => {
  assert.equal('PADDOCK_SETUP_VERSION' in setupEnv(conf, 'K'), false);
  assert.deepEqual(setupEnv({ ...conf, version: '1.1.1' }, 'K'), setupEnv({ ...conf, version: '9.9.9' }, 'K'));
  const root = mkdtempSync(join(tmpdir(), 'stage-'));
  const src = join(root, 'apps/dyno/app/src-tauri');
  mkdirSync(join(src, 'windows'), { recursive: true });
  mkdirSync(join(root, 'crates/paddock-setup'), { recursive: true });
  writeFileSync(join(root, 'crates/paddock-setup/Cargo.toml'), '[package]\n');
  const envs = [];
  const run = (c, a, o) => {
    envs.push(o.env);
    writeFileSync(join(root, 'b.exe'), 'MZ');
    return `{"reason":"compiler-artifact","target":{"name":"paddock-setup","kind":["bin"]},"executable":${JSON.stringify(join(root, 'b.exe'))}}\n`;
  };
  const stagedWith = (version) => {
    writeFileSync(join(src, 'tauri.conf.json'), JSON.stringify({ ...conf, version }));
    return stage({ app: 'dyno', root, run, env: { TAURI_UPDATER_PUBKEY: 'K', PADDOCK_SETUP_TEST_VERSION: '9.9.9' } });
  };
  stagedWith('1.1.1');
  stagedWith('1.2.0');
  // The two builds are asked for with the same environment (the version of the app is not in it) ...
  assert.deepEqual(envs[0], envs[1]);
  // ... without the VM tests' version override ...
  assert.equal('PADDOCK_SETUP_TEST_VERSION' in envs[0], false);
  assert.equal(envs[0].CARGO_INCREMENTAL, '0');
});

test('path remapping: home, cargo home and the checkout, narrowest last, as one cfg(windows) config', () => {
  const c = remapConfig({ root: 'C:\\Users\\decryp7\\paddock', cargoHome: 'C:\\Users\\decryp7\\.cargo', home: 'C:\\Users\\decryp7' });
  assert.equal(c, "target.'cfg(windows)'.rustflags=['--remap-path-prefix=C:\\Users\\decryp7=/home', '--remap-path-prefix=C:\\Users\\decryp7\\.cargo=/cargo', '--remap-path-prefix=C:\\Users\\decryp7\\paddock=/paddock']");
  assert.throws(() => remapConfig({ root: "/a'b", cargoHome: '/c', home: '/h' }), /quote/);
});

test('RUSTFLAGS in the environment is refused: cargo would drop +crt-static and the remapping', () => {
  assert.throws(() => assertNoRustflagsEnv({ RUSTFLAGS: '-D warnings' }), /RUSTFLAGS is set.*crt-static/);
  assert.throws(() => assertNoRustflagsEnv({ CARGO_ENCODED_RUSTFLAGS: 'x' }), /CARGO_ENCODED_RUSTFLAGS/);
  assert.doesNotThrow(() => assertNoRustflagsEnv({}));
});
